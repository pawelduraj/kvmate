#include "usb_audio.h"

#include <stdbool.h>
#include <string.h>

#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "link.h"
#include "tinyusb.h"
#include "usb_device.h"

#define PLAY_FRAME_BYTES (AUDIO_SPK_CHANNELS * AUDIO_BYTES_PER_SAMPLE)
#define MIC_FRAME_BYTES (AUDIO_MIC_CHANNELS * AUDIO_BYTES_PER_SAMPLE)
#define PLAY_CHUNK_FRAMES 192
#define STATUS_INTERVAL_US (20 * 1000)
#define MIC_LOW_WATER_FRAMES 96
#define MIC_START_FRAMES 480

#define VOLUME_MIN (-60 * 256)
#define VOLUME_MAX 0
#define VOLUME_RES 256

static const char *TAG = "usb_audio";

static volatile bool s_spk_active;
static volatile bool s_mic_active;
static int16_t s_volume[AUDIO_SPK_CHANNELS + 1];
static uint8_t s_mute[AUDIO_SPK_CHANNELS + 1];
static uint32_t s_sample_rate = AUDIO_SAMPLE_RATE;
static uint32_t s_play_frame;

static const uint8_t s_silence[MIC_START_FRAMES * MIC_FRAME_BYTES];

static uint16_t mic_fill_frames(void)
{
    return (uint16_t)(tu_fifo_count(tud_audio_get_ep_in_ff()) / MIC_FRAME_BYTES);
}

void usb_audio_mic_write(const uint8_t *pcm, size_t len)
{
    if (!s_mic_active || len == 0) {
        return;
    }
    tud_audio_write(pcm, (uint16_t)(len - len % MIC_FRAME_BYTES));
}

static void send_status(void)
{
    protocol_audio_status_t st = {
        .host_playing = s_spk_active ? 1 : 0,
        .host_recording = s_mic_active ? 1 : 0,
        .mic_queued_frames = mic_fill_frames(),
        .host_volume = s_volume[0],
        .host_muted = s_mute[0],
    };
    protocol_part_t part = {&st, sizeof(st)};
    link_send(PROTOCOL_MSG_TYPE_AUDIO_STATUS, &part, 1);
}

static void audio_task(void *arg)
{
    (void)arg;
    static uint8_t chunk[PLAY_CHUNK_FRAMES * PLAY_FRAME_BYTES];
    int64_t next_status = 0;
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(2));
        if (!tud_mounted()) {
            continue;
        }

        while (s_spk_active && tud_audio_available() >= sizeof(chunk)) {
            uint16_t n = tud_audio_read(chunk, sizeof(chunk));
            protocol_audio_header_t head = {.first_frame_number = s_play_frame};
            s_play_frame += n / PLAY_FRAME_BYTES;
            if (link_hub_alive()) {
                protocol_part_t parts[] = {{&head, sizeof(head)}, {chunk, n}};
                link_send(PROTOCOL_MSG_TYPE_AUDIO_PLAYBACK, parts, 2);
            }
        }

        if (s_mic_active && mic_fill_frames() < MIC_LOW_WATER_FRAMES) {
            tud_audio_write(s_silence, MIC_LOW_WATER_FRAMES * MIC_FRAME_BYTES);
        }

        int64_t now = esp_timer_get_time();
        if (now >= next_status && link_hub_alive()) {
            next_status = now + STATUS_INTERVAL_US;
            send_status();
        }
    }
}

void usb_audio_init(void)
{
    xTaskCreate(audio_task, "usb_audio", 4096, NULL, 8, NULL);
}

static void set_streaming(uint8_t itf, bool active)
{
    if (itf == ITF_AUDIO_SPEAKER && active != s_spk_active) {
        s_spk_active = active;
        if (active) {
            tud_audio_clear_ep_out_ff();
        }
        ESP_LOGI(TAG, "speaker %s", active ? "streaming" : "idle");
    } else if (itf == ITF_AUDIO_MIC && active != s_mic_active) {
        if (active) {
            tud_audio_clear_ep_in_ff();
            tud_audio_write(s_silence, sizeof(s_silence));
        }
        s_mic_active = active;
        ESP_LOGI(TAG, "microphone %s", active ? "streaming" : "idle");
    }
}

bool tud_audio_set_itf_cb(uint8_t rhport, tusb_control_request_t const *p_request)
{
    (void)rhport;
    set_streaming(tu_u16_low(p_request->wIndex), tu_u16_low(p_request->wValue) != 0);
    return true;
}

bool tud_audio_set_itf_close_ep_cb(uint8_t rhport, tusb_control_request_t const *p_request)
{
    (void)rhport;
    if (tu_u16_low(p_request->wValue) == 0) {
        set_streaming(tu_u16_low(p_request->wIndex), false);
    }
    return true;
}

bool tud_audio_set_req_ep_cb(uint8_t rhport, tusb_control_request_t const *p_request, uint8_t *buf)
{
    (void)rhport;
    if (TU_U16_HIGH(p_request->wValue) == AUDIO10_EP_CTRL_SAMPLING_FREQ &&
        p_request->bRequest == AUDIO10_CS_REQ_SET_CUR && p_request->wLength == 3) {
        s_sample_rate = tu_unaligned_read32(buf) & 0x00FFFFFF;
        if (s_sample_rate != AUDIO_SAMPLE_RATE) {
            ESP_LOGW(TAG, "host asked for %lu Hz, only %d Hz is supported", (unsigned long)s_sample_rate,
                     AUDIO_SAMPLE_RATE);
        }
        return true;
    }
    return false;
}

bool tud_audio_get_req_ep_cb(uint8_t rhport, tusb_control_request_t const *p_request)
{
    if (TU_U16_HIGH(p_request->wValue) == AUDIO10_EP_CTRL_SAMPLING_FREQ &&
        p_request->bRequest == AUDIO10_CS_REQ_GET_CUR) {
        uint8_t freq[3] = {(uint8_t)s_sample_rate, (uint8_t)(s_sample_rate >> 8), (uint8_t)(s_sample_rate >> 16)};
        return tud_audio_buffer_and_schedule_control_xfer(rhport, p_request, freq, sizeof(freq));
    }
    return false;
}

bool tud_audio_set_req_entity_cb(uint8_t rhport, tusb_control_request_t const *p_request, uint8_t *buf)
{
    (void)rhport;
    uint8_t channel = TU_U16_LOW(p_request->wValue);
    uint8_t selector = TU_U16_HIGH(p_request->wValue);
    if (TU_U16_HIGH(p_request->wIndex) != UAC1_ENTITY_SPK_FEATURE_UNIT || channel > AUDIO_SPK_CHANNELS ||
        p_request->bRequest != AUDIO10_CS_REQ_SET_CUR) {
        return false;
    }
    if (selector == AUDIO10_FU_CTRL_MUTE && p_request->wLength == 1) {
        s_mute[channel] = buf[0];
        return true;
    }
    if (selector == AUDIO10_FU_CTRL_VOLUME && p_request->wLength == 2) {
        int16_t v = (int16_t)tu_unaligned_read16(buf);
        s_volume[channel] = v < VOLUME_MIN ? VOLUME_MIN : (v > VOLUME_MAX ? VOLUME_MAX : v);
        return true;
    }
    return false;
}

bool tud_audio_get_req_entity_cb(uint8_t rhport, tusb_control_request_t const *p_request)
{
    uint8_t channel = TU_U16_LOW(p_request->wValue);
    uint8_t selector = TU_U16_HIGH(p_request->wValue);
    if (TU_U16_HIGH(p_request->wIndex) != UAC1_ENTITY_SPK_FEATURE_UNIT || channel > AUDIO_SPK_CHANNELS) {
        return false;
    }
    if (selector == AUDIO10_FU_CTRL_MUTE && p_request->bRequest == AUDIO10_CS_REQ_GET_CUR) {
        return tud_audio_buffer_and_schedule_control_xfer(rhport, p_request, &s_mute[channel], 1);
    }
    if (selector == AUDIO10_FU_CTRL_VOLUME) {
        int16_t v;
        switch (p_request->bRequest) {
        case AUDIO10_CS_REQ_GET_CUR: v = s_volume[channel]; break;
        case AUDIO10_CS_REQ_GET_MIN: v = VOLUME_MIN; break;
        case AUDIO10_CS_REQ_GET_MAX: v = VOLUME_MAX; break;
        case AUDIO10_CS_REQ_GET_RES: v = VOLUME_RES; break;
        default: return false;
        }
        return tud_audio_buffer_and_schedule_control_xfer(rhport, p_request, &v, sizeof(v));
    }
    return false;
}
