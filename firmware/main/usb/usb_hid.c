#include "usb_hid.h"

#include <string.h>

#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "tinyusb.h"
#include "usb_device.h"

static const char *TAG = "usb_hid";

typedef struct {
    uint8_t modifier;
    uint8_t keys[6];
} kbd_report_t;

static portMUX_TYPE s_lock = portMUX_INITIALIZER_UNLOCKED;
static TaskHandle_t s_task;

static kbd_report_t s_kbd;
static bool s_kbd_dirty;
static bool s_kbd_any_down;

static int32_t s_mouse_dx, s_mouse_dy, s_mouse_wheel, s_mouse_pan;
static uint8_t s_mouse_buttons;
static bool s_mouse_dirty;

static uint16_t s_consumer;
static bool s_consumer_dirty;

static void wake_task(void)
{
    if (s_task) {
        xTaskNotifyGive(s_task);
    }
}

static void to_boot_report(const protocol_keyboard_state_t *st, kbd_report_t *out)
{
    memset(out, 0, sizeof(*out));
    out->modifier = st->usage_bitmap[0xE0 / 8];
    int n = 0;
    for (int usage = 0x04; usage < 0xE0; usage++) {
        if (st->usage_bitmap[usage / 8] & (1u << (usage % 8))) {
            if (n == 6) {
                memset(out->keys, 0x01, sizeof(out->keys));
                return;
            }
            out->keys[n++] = (uint8_t)usage;
        }
    }
}

void usb_hid_set_keyboard(const protocol_keyboard_state_t *state)
{
    kbd_report_t report;
    to_boot_report(state, &report);
    taskENTER_CRITICAL(&s_lock);
    if (memcmp(&report, &s_kbd, sizeof(report)) != 0) {
        s_kbd = report;
        s_kbd_dirty = true;
    }
    s_kbd_any_down = report.modifier != 0 || report.keys[0] != 0;
    taskEXIT_CRITICAL(&s_lock);
    wake_task();
}

void usb_hid_add_mouse(const protocol_mouse_report_t *report)
{
    protocol_mouse_report_t r;
    memcpy(&r, report, sizeof(r));
    taskENTER_CRITICAL(&s_lock);
    s_mouse_dx += r.move_x;
    s_mouse_dy += r.move_y;
    s_mouse_wheel += r.wheel;
    s_mouse_pan += r.wheel_horizontal;
    s_mouse_dirty = true;
    s_mouse_buttons = r.buttons;
    taskEXIT_CRITICAL(&s_lock);
    wake_task();
}

void usb_hid_set_consumer(uint16_t usage)
{
    taskENTER_CRITICAL(&s_lock);
    if (usage != s_consumer) {
        s_consumer = usage;
        s_consumer_dirty = true;
    }
    taskEXIT_CRITICAL(&s_lock);
    wake_task();
}

void usb_hid_release_all(void)
{
    static const kbd_report_t empty;
    taskENTER_CRITICAL(&s_lock);
    if (memcmp(&s_kbd, &empty, sizeof(empty)) != 0) {
        s_kbd = empty;
        s_kbd_dirty = true;
    }
    s_kbd_any_down = false;
    s_mouse_dx = s_mouse_dy = s_mouse_wheel = s_mouse_pan = 0;
    if (s_mouse_buttons) {
        s_mouse_buttons = 0;
        s_mouse_dirty = true;
    }
    if (s_consumer) {
        s_consumer = 0;
        s_consumer_dirty = true;
    }
    taskEXIT_CRITICAL(&s_lock);
    wake_task();
}

bool usb_hid_mounted(void)
{
    return tud_mounted();
}

bool usb_hid_suspended(void)
{
    return tud_suspended();
}

static int32_t take_clamped(int32_t *acc, int32_t lo, int32_t hi)
{
    int32_t v = *acc < lo ? lo : (*acc > hi ? hi : *acc);
    *acc -= v;
    return v;
}

static void hid_task(void *arg)
{
    (void)arg;
    for (;;) {
        ulTaskNotifyTake(pdTRUE, pdMS_TO_TICKS(4));
        if (!tud_mounted()) {
            continue;
        }
        if (tud_suspended()) {
            bool wake;
            taskENTER_CRITICAL(&s_lock);
            wake = s_kbd_any_down;
            taskEXIT_CRITICAL(&s_lock);
            if (wake) {
                tud_remote_wakeup();
            }
            continue;
        }

        if (tud_hid_n_ready(HID_KEYBOARD)) {
            kbd_report_t report;
            bool send = false;
            taskENTER_CRITICAL(&s_lock);
            if (s_kbd_dirty) {
                report = s_kbd;
                s_kbd_dirty = false;
                send = true;
            }
            taskEXIT_CRITICAL(&s_lock);
            if (send) {
                tud_hid_n_keyboard_report(HID_KEYBOARD, 0, report.modifier, report.keys);
            }
        }

        if (tud_hid_n_ready(HID_MOUSE)) {
            protocol_mouse_report_t mouse;
            uint16_t consumer = 0;
            bool send_mouse = false, send_consumer = false;
            taskENTER_CRITICAL(&s_lock);
            if (s_mouse_dirty) {
                mouse.buttons = s_mouse_buttons;
                mouse.move_x = (int16_t)take_clamped(&s_mouse_dx, -32767, 32767);
                mouse.move_y = (int16_t)take_clamped(&s_mouse_dy, -32767, 32767);
                mouse.wheel = (int8_t)take_clamped(&s_mouse_wheel, -127, 127);
                mouse.wheel_horizontal = (int8_t)take_clamped(&s_mouse_pan, -127, 127);
                s_mouse_dirty = s_mouse_dx || s_mouse_dy || s_mouse_wheel || s_mouse_pan;
                send_mouse = true;
            } else if (s_consumer_dirty) {
                consumer = s_consumer;
                s_consumer_dirty = false;
                send_consumer = true;
            }
            taskEXIT_CRITICAL(&s_lock);
            if (send_mouse) {
                tud_hid_n_report(HID_MOUSE, REPORT_ID_MOUSE, &mouse, sizeof(mouse));
            } else if (send_consumer) {
                tud_hid_n_report(HID_MOUSE, REPORT_ID_CONSUMER, &consumer, sizeof(consumer));
            }
        }
    }
}

uint16_t tud_hid_get_report_cb(uint8_t instance, uint8_t report_id, hid_report_type_t report_type, uint8_t *buffer,
                               uint16_t reqlen)
{
    (void)instance;
    (void)report_id;
    (void)report_type;
    (void)buffer;
    (void)reqlen;
    return 0;
}

void tud_hid_set_report_cb(uint8_t instance, uint8_t report_id, hid_report_type_t report_type, uint8_t const *buffer,
                           uint16_t bufsize)
{
    (void)instance;
    (void)report_id;
    (void)report_type;
    (void)buffer;
    (void)bufsize;
}

void tud_hid_report_complete_cb(uint8_t instance, uint8_t const *report, uint16_t len)
{
    (void)instance;
    (void)report;
    (void)len;
    wake_task();
}

void usb_hid_init(void)
{
    xTaskCreate(hid_task, "usb_hid", 3072, NULL, 10, &s_task);
    ESP_LOGI(TAG, "HID ready");
}
