#include "button.h"
#include "esp_log.h"
#include "led.h"
#include "link.h"
#include "secure.h"
#include "storage.h"
#include "usb_audio.h"
#include "usb_device.h"
#include "usb_hid.h"
#include "wifi.h"

static const char *TAG = "kvmate";

void app_main(void)
{
    bool selftest_ok = protocol_selftest() && protocol_secure_selftest();
    led_init();
    storage_init();
    usb_device_init();
    link_init(selftest_ok);
    usb_hid_init();
    usb_audio_init();
    button_init();
    wifi_init(link_wifi_rx);
    ESP_LOGI(TAG, "kvmate firmware %d.%d.%d, protocol %d, self-test %s, USB as \"%s\"", PROTOCOL_APP_VERSION_MAJOR,
             PROTOCOL_APP_VERSION_MINOR, PROTOCOL_APP_VERSION_PATCH, PROTOCOL_VERSION, selftest_ok ? "ok" : "FAILED",
             usb_device_name());
}
