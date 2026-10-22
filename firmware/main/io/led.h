/* The board's LED. It shows the colour the hub picks; with no hub it blinks amber (red when
 * Wi-Fi can't connect). */
#pragma once

#include <stdbool.h>
#include <stdint.h>

/* Default LED pin, until the hub sends its own. */
#define LED_DEFAULT_GPIO 48

/* No LED. */
#define LED_GPIO_NONE 0xFF

/* Starts the LED on LED_DEFAULT_GPIO, blinking at brightness 64 of 255. */
void led_init(void);

/* Moves the LED to `gpio` and sets the brightness that scales every colour. LED_GPIO_NONE, or a pin
 * that can't drive the LED, means no LED. */
void led_configure(uint8_t gpio, uint8_t brightness);

/* Shows the colour until led_idle(). */
void led_set(uint8_t r, uint8_t g, uint8_t b);

/* Goes back to blinking; call when the hub is gone. */
void led_idle(void);

/* Blinks red instead of amber while `alert` is true. */
void led_idle_alert(bool alert);
