#include "link.h"

#include <string.h>

#include "driver/uart.h"
#include "esp_app_desc.h"
#include "esp_log.h"
#include "esp_mac.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "led.h"
#include "log_forward.h"
#include "soc/uart_channel.h"
#include "usb_audio.h"
#include "usb_device.h"
#include "usb_hid.h"
#include "wifi.h"

#define LINK_UART UART_NUM_0
#define LINK_RX_BUF 4096
#define LINK_TX_BUF 4096

static const char *TAG = "link";

#define SRC_CABLE ((void *)0)
#define SRC_WIFI ((void *)1)

static SemaphoreHandle_t s_tx_lock;
static SemaphoreHandle_t s_rx_lock;
static uint16_t s_tx_seq;
static uint8_t s_tx_buf[PROTOCOL_MAX_ENCODED_LEN];
static volatile int64_t s_last_rx_us;
static volatile int64_t s_last_cable_rx_us;
static volatile bool s_hub_alive;
static volatile bool s_wifi_status_due;
static volatile bool s_session;
static bool s_selftest_ok;
static protocol_decoder_t s_decoder;
static protocol_decoder_t s_wifi_decoder;

static bool send_locked(uint8_t msg_type, const protocol_part_t *parts, size_t n_parts)
{
    size_t n = protocol_encode_frame(msg_type, s_tx_seq++, parts, n_parts, s_tx_buf, sizeof(s_tx_buf));
    if (n == 0) {
        return false;
    }
    bool cable = esp_timer_get_time() - s_last_cable_rx_us < (int64_t)PROTOCOL_LINK_TIMEOUT_MS * 1000;
    if (cable || !wifi_hub_alive() || !wifi_send(s_tx_buf, n)) {
        uart_write_bytes(LINK_UART, s_tx_buf, n);
    }
    return true;
}

bool link_send(uint8_t msg_type, const protocol_part_t *parts, size_t n_parts)
{
    if (xSemaphoreTake(s_tx_lock, pdMS_TO_TICKS(20)) != pdTRUE) {
        return false;
    }
    bool sent = send_locked(msg_type, parts, n_parts);
    xSemaphoreGive(s_tx_lock);
    return sent;
}

bool link_hub_alive(void)
{
    return s_hub_alive;
}

static void send_device_info(void)
{
    protocol_device_info_t info = {
        .firmware_version = {PROTOCOL_APP_VERSION_MAJOR, PROTOCOL_APP_VERSION_MINOR, PROTOCOL_APP_VERSION_PATCH},
        .selftest_passed = s_selftest_ok ? 1 : 0,
    };
    esp_efuse_mac_get_default(info.mac_address);
    memcpy(info.firmware_build_id, esp_app_get_description()->app_elf_sha256, sizeof(info.firmware_build_id));
    protocol_part_t part = {&info, sizeof(info)};
    link_send(PROTOCOL_MSG_TYPE_DEVICE_INFO, &part, 1);
}

static bool read_payload(const protocol_frame_t *frame, void *out, size_t size)
{
    if (frame->header.payload_len < size) {
        return false;
    }
    memcpy(out, frame->payload, size);
    return true;
}

static void on_frame(const protocol_frame_t *frame, void *ctx)
{
    s_last_rx_us = esp_timer_get_time();
    if (ctx == SRC_CABLE) {
        s_last_cable_rx_us = s_last_rx_us;
    }
    s_hub_alive = true;

    union {
        protocol_keyboard_state_t keyboard;
        protocol_mouse_report_t mouse;
        protocol_consumer_state_t consumer;
        protocol_set_led_t led;
        protocol_device_config_t config;
        protocol_usb_names_t usb_names;
        protocol_wifi_config_t wifi;
    } p;
#define READ(field) read_payload(frame, &p.field, sizeof(p.field))
    switch (frame->header.msg_type) {
    case PROTOCOL_MSG_TYPE_HELLO:
        s_session = true;
        send_device_info();
        s_wifi_status_due = true;
        break;
    case PROTOCOL_MSG_TYPE_HEARTBEAT:
        break;
    case PROTOCOL_MSG_TYPE_KEYBOARD_STATE:
        if (READ(keyboard)) {
            usb_hid_set_keyboard(&p.keyboard);
        }
        break;
    case PROTOCOL_MSG_TYPE_MOUSE_REPORT:
        if (READ(mouse)) {
            usb_hid_add_mouse(&p.mouse);
        }
        break;
    case PROTOCOL_MSG_TYPE_CONSUMER_STATE:
        if (READ(consumer)) {
            usb_hid_set_consumer(p.consumer.usage);
        }
        break;
    case PROTOCOL_MSG_TYPE_RELEASE_ALL:
        usb_hid_release_all();
        break;
    case PROTOCOL_MSG_TYPE_SET_LED:
        if (READ(led)) {
            led_set(p.led.red, p.led.green, p.led.blue);
        }
        break;
    case PROTOCOL_MSG_TYPE_DEVICE_CONFIG:
        if (READ(config)) {
            led_configure(p.config.led_pin, p.config.led_brightness);
        }
        break;
    case PROTOCOL_MSG_TYPE_USB_NAMES:
        if (READ(usb_names)) {
            usb_device_set_names(&p.usb_names);
        }
        break;
    case PROTOCOL_MSG_TYPE_AUDIO_CAPTURE:
        if (frame->header.payload_len >= sizeof(protocol_audio_header_t)) {
            size_t head = sizeof(protocol_audio_header_t);
            usb_audio_mic_write(frame->payload + head, frame->header.payload_len - head);
        }
        break;
    case PROTOCOL_MSG_TYPE_WIFI_CONFIG:
        if (ctx != SRC_CABLE) {
            ESP_LOGW(TAG, "Wi-Fi settings are accepted over the cable only");
        } else if (READ(wifi)) {
            wifi_apply(&p.wifi);
            s_wifi_status_due = true;
        }
        break;
    case PROTOCOL_MSG_TYPE_WIFI_SCAN:
        wifi_scan();
        break;
    default:
        ESP_LOGW(TAG, "unknown message 0x%02X", frame->header.msg_type);
        break;
    }
#undef READ
}

static void rx_task(void *arg)
{
    (void)arg;
    static uint8_t chunk[512];
    protocol_decoder_init(&s_decoder);
    for (;;) {
        int n = uart_read_bytes(LINK_UART, chunk, 1, pdMS_TO_TICKS(10));
        if (n <= 0) {
            continue;
        }
        size_t buffered = 0;
        uart_get_buffered_data_len(LINK_UART, &buffered);
        if (buffered > sizeof(chunk) - 1) {
            buffered = sizeof(chunk) - 1;
        }
        if (buffered > 0) {
            int more = uart_read_bytes(LINK_UART, chunk + 1, buffered, 0);
            if (more > 0) {
                n += more;
            }
        }
        xSemaphoreTake(s_rx_lock, portMAX_DELAY);
        protocol_decoder_push(&s_decoder, chunk, (size_t)n, on_frame, SRC_CABLE);
        xSemaphoreGive(s_rx_lock);
    }
}

void link_wifi_rx(const uint8_t *data, size_t len)
{
    xSemaphoreTake(s_rx_lock, portMAX_DELAY);
    protocol_decoder_push(&s_wifi_decoder, data, len, on_frame, SRC_WIFI);
    xSemaphoreGive(s_rx_lock);
}

static void send_heartbeat(void)
{
    if (xSemaphoreTake(s_tx_lock, pdMS_TO_TICKS(20)) != pdTRUE) {
        return;
    }
    uint8_t flags = 0;
    if (s_session) {
        flags |= PROTOCOL_HEARTBEAT_FLAG_HELLO_ANSWERED;
    }
    if (usb_hid_mounted()) {
        flags |= PROTOCOL_HEARTBEAT_FLAG_USB_MOUNTED;
    }
    if (usb_hid_suspended()) {
        flags |= PROTOCOL_HEARTBEAT_FLAG_USB_SUSPENDED;
    }
    protocol_heartbeat_t hb = {.flags = flags};
    protocol_part_t part = {&hb, sizeof(hb)};
    send_locked(PROTOCOL_MSG_TYPE_HEARTBEAT, &part, 1);
    xSemaphoreGive(s_tx_lock);
}

static void heartbeat_task(void *arg)
{
    (void)arg;
    const int status_every = PROTOCOL_WIFI_STATUS_INTERVAL_MS / PROTOCOL_HEARTBEAT_INTERVAL_MS;
    int ticks = 0;
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(PROTOCOL_HEARTBEAT_INTERVAL_MS));
        if (s_hub_alive && esp_timer_get_time() - s_last_rx_us > (int64_t)PROTOCOL_LINK_TIMEOUT_MS * 1000) {
            s_hub_alive = false;
            usb_hid_release_all();
            led_idle();
        }
        led_idle_alert(wifi_failed());
        send_heartbeat();

        protocol_wifi_status_t st;
        bool changed = wifi_status(&st);
        if (s_hub_alive && (changed || s_wifi_status_due || ++ticks >= status_every)) {
            ticks = 0;
            s_wifi_status_due = false;
            protocol_part_t part = {&st, sizeof(st)};
            link_send(PROTOCOL_MSG_TYPE_WIFI_STATUS, &part, 1);
        }
    }
}

void link_init(bool selftest_ok)
{
    s_selftest_ok = selftest_ok;
    s_tx_lock = xSemaphoreCreateMutex();
    s_rx_lock = xSemaphoreCreateMutex();
    protocol_decoder_init(&s_wifi_decoder);

    const uart_config_t cfg = {
        .baud_rate = PROTOCOL_DEFAULT_BAUD,
        .data_bits = UART_DATA_8_BITS,
        .parity = UART_PARITY_DISABLE,
        .stop_bits = UART_STOP_BITS_1,
        .flow_ctrl = UART_HW_FLOWCTRL_DISABLE,
        .source_clk = UART_SCLK_DEFAULT,
    };
    ESP_ERROR_CHECK(uart_driver_install(LINK_UART, LINK_RX_BUF, LINK_TX_BUF, 0, NULL, 0));
    ESP_ERROR_CHECK(uart_param_config(LINK_UART, &cfg));
    ESP_ERROR_CHECK(uart_set_pin(LINK_UART, UART_NUM_0_TXD_DIRECT_GPIO_NUM, UART_NUM_0_RXD_DIRECT_GPIO_NUM,
                                 UART_PIN_NO_CHANGE, UART_PIN_NO_CHANGE));

    log_forward_init();

    xTaskCreate(rx_task, "link_rx", 4096, NULL, 12, NULL);
    xTaskCreate(heartbeat_task, "link_hb", 3072, NULL, 5, NULL);
}
