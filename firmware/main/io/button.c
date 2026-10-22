#include "button.h"

#include "driver/gpio.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "link.h"

#define BUTTON_GPIO GPIO_NUM_0
#define POLL_MS 10
#define DEBOUNCE_MS 30
#define LONG_PRESS_MS 1500

static void send_event(uint8_t kind)
{
    protocol_button_event_t ev = {.kind = kind};
    protocol_part_t part = {&ev, sizeof(ev)};
    link_send(PROTOCOL_MSG_TYPE_BUTTON_EVENT, &part, 1);
}

static void button_task(void *arg)
{
    (void)arg;
    int held_ms = 0;
    bool long_sent = false;
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(POLL_MS));
        bool pressed = gpio_get_level(BUTTON_GPIO) == 0;
        if (pressed) {
            held_ms += POLL_MS;
            if (!long_sent && held_ms >= LONG_PRESS_MS) {
                long_sent = true;
                send_event(PROTOCOL_BUTTON_EVENT_LONG);
            }
        } else {
            if (held_ms >= DEBOUNCE_MS && !long_sent) {
                send_event(PROTOCOL_BUTTON_EVENT_SHORT);
            }
            held_ms = 0;
            long_sent = false;
        }
    }
}

void button_init(void)
{
    const gpio_config_t cfg = {
        .pin_bit_mask = 1ULL << BUTTON_GPIO,
        .mode = GPIO_MODE_INPUT,
        .pull_up_en = GPIO_PULLUP_ENABLE,
        .pull_down_en = GPIO_PULLDOWN_DISABLE,
        .intr_type = GPIO_INTR_DISABLE,
    };
    gpio_config(&cfg);
    xTaskCreate(button_task, "button", 2048, NULL, 4, NULL);
}
