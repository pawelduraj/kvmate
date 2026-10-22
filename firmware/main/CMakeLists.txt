idf_component_register(
    SRCS "main.c" "link.c" "log_forward.c" "storage.c"
         "io/button.c" "io/led.c"
         "usb/usb_device.c" "usb/usb_hid.c" "usb/usb_audio.c"
         "wifi/wifi.c" "wifi/wifi_udp.c"
    INCLUDE_DIRS "." "io" "usb" "wifi"
    PRIV_REQUIRES protocol esp_app_format esp_driver_uart esp_driver_gpio esp_timer esp_hw_support log freertos
                 esp_wifi esp_netif esp_event nvs_flash lwip
)
