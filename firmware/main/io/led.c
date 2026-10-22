#include "led.h"

#include "driver/gpio.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "led_strip.h"
#include "protocol.h"

static const char *TAG = "led";

static SemaphoreHandle_t s_lock;
static led_strip_handle_t s_strip;
static uint8_t s_gpio = LED_GPIO_NONE;
static uint8_t s_brightness = 64;
static uint8_t s_r, s_g, s_b;
static bool s_idle = true;
static volatile bool s_alert;

static void create_strip(uint8_t gpio)
{
    if (s_strip) {
        led_strip_clear(s_strip);
        led_strip_del(s_strip);
        s_strip = NULL;
    }
    s_gpio = gpio;
    int gpio_num = gpio;
    if (gpio == LED_GPIO_NONE || !GPIO_IS_VALID_OUTPUT_GPIO(gpio_num)) {
        return;
    }
    led_strip_config_t strip_cfg = {
        .strip_gpio_num = gpio_num,
        .max_leds = 1,
        .led_model = LED_MODEL_WS2812,
        .color_component_format = LED_STRIP_COLOR_COMPONENT_FMT_GRB,
    };
    led_strip_rmt_config_t rmt_cfg = {
        .clk_src = RMT_CLK_SRC_DEFAULT,
        .resolution_hz = 10 * 1000 * 1000,
    };
    if (led_strip_new_rmt_device(&strip_cfg, &rmt_cfg, &s_strip) != ESP_OK) {
        ESP_LOGW(TAG, "no LED driver on GPIO %d", gpio);
        s_strip = NULL;
    }
}

static void show(uint8_t r, uint8_t g, uint8_t b)
{
    if (!s_strip) {
        return;
    }
    uint32_t k = s_brightness;
    led_strip_set_pixel(s_strip, 0, r * k / 255, g * k / 255, b * k / 255);
    led_strip_refresh(s_strip);
}

static void led_task(void *arg)
{
    (void)arg;
    bool phase = false;
    for (;;) {
        vTaskDelay(pdMS_TO_TICKS(500));
        phase = !phase;
        xSemaphoreTake(s_lock, portMAX_DELAY);
        if (s_idle) {
            if (s_alert) {
                show(phase ? 255 : 0, 0, 0);
            } else {
                show(phase ? 255 : 0, phase ? 120 : 0, 0);
            }
        }
        xSemaphoreGive(s_lock);
    }
}

void led_init(void)
{
    s_lock = xSemaphoreCreateMutex();
    create_strip(LED_DEFAULT_GPIO);
    xTaskCreate(led_task, "led", 2560, NULL, 2, NULL);
}

void led_configure(uint8_t gpio, uint8_t brightness)
{
    xSemaphoreTake(s_lock, portMAX_DELAY);
    s_brightness = brightness;
    if (gpio != s_gpio) {
        create_strip(gpio);
    }
    if (!s_idle) {
        show(s_r, s_g, s_b);
    }
    xSemaphoreGive(s_lock);
}

void led_set(uint8_t r, uint8_t g, uint8_t b)
{
    xSemaphoreTake(s_lock, portMAX_DELAY);
    s_r = r;
    s_g = g;
    s_b = b;
    s_idle = false;
    show(r, g, b);
    xSemaphoreGive(s_lock);
}

void led_idle(void)
{
    xSemaphoreTake(s_lock, portMAX_DELAY);
    s_idle = true;
    xSemaphoreGive(s_lock);
}

void led_idle_alert(bool alert)
{
    s_alert = alert;
}
