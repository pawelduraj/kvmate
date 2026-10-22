#include "usb_device.h"

#include <stdio.h>
#include <string.h>

#include "esp_log.h"
#include "esp_mac.h"
#include "esp_timer.h"
#include "storage.h"
#include "tinyusb.h"
#include "tinyusb_default_config.h"

#define DEFAULT_NAME "kvmate"
#define NVS_KEY "usb"
#define LANGID_EN_US "\x09\x04"
#define RECONNECT_US (500 * 1000)

static const char *TAG = "usb";

static const uint8_t s_keyboard_report_desc[] = {
    TUD_HID_REPORT_DESC_KEYBOARD(),
};

static const uint8_t s_mouse_report_desc[] = {
    HID_USAGE_PAGE(HID_USAGE_PAGE_DESKTOP),
    HID_USAGE(HID_USAGE_DESKTOP_MOUSE),
    HID_COLLECTION(HID_COLLECTION_APPLICATION),
        HID_REPORT_ID(REPORT_ID_MOUSE)
        HID_USAGE(HID_USAGE_DESKTOP_POINTER),
        HID_COLLECTION(HID_COLLECTION_PHYSICAL),
            HID_USAGE_PAGE(HID_USAGE_PAGE_BUTTON),
            HID_USAGE_MIN(1),
            HID_USAGE_MAX(5),
            HID_LOGICAL_MIN(0),
            HID_LOGICAL_MAX(1),
            HID_REPORT_COUNT(5),
            HID_REPORT_SIZE(1),
            HID_INPUT(HID_DATA | HID_VARIABLE | HID_ABSOLUTE),
            HID_REPORT_COUNT(1),
            HID_REPORT_SIZE(3),
            HID_INPUT(HID_CONSTANT),
            HID_USAGE_PAGE(HID_USAGE_PAGE_DESKTOP),
            HID_USAGE(HID_USAGE_DESKTOP_X),
            HID_USAGE(HID_USAGE_DESKTOP_Y),
            HID_LOGICAL_MIN_N(0x8001, 2),
            HID_LOGICAL_MAX_N(0x7FFF, 2),
            HID_REPORT_COUNT(2),
            HID_REPORT_SIZE(16),
            HID_INPUT(HID_DATA | HID_VARIABLE | HID_RELATIVE),
            HID_USAGE(HID_USAGE_DESKTOP_WHEEL),
            HID_LOGICAL_MIN(0x81),
            HID_LOGICAL_MAX(0x7F),
            HID_REPORT_COUNT(1),
            HID_REPORT_SIZE(8),
            HID_INPUT(HID_DATA | HID_VARIABLE | HID_RELATIVE),
            HID_USAGE_PAGE(HID_USAGE_PAGE_CONSUMER),
            HID_USAGE_N(HID_USAGE_CONSUMER_AC_PAN, 2),
            HID_LOGICAL_MIN(0x81),
            HID_LOGICAL_MAX(0x7F),
            HID_REPORT_COUNT(1),
            HID_REPORT_SIZE(8),
            HID_INPUT(HID_DATA | HID_VARIABLE | HID_RELATIVE),
        HID_COLLECTION_END,
    HID_COLLECTION_END,
    TUD_HID_REPORT_DESC_CONSUMER(HID_REPORT_ID(REPORT_ID_CONSUMER)),
};

uint8_t const *tud_hid_descriptor_report_cb(uint8_t instance)
{
    return instance == 0 ? s_keyboard_report_desc : s_mouse_report_desc;
}

#define AUDIO_EP_OUT_SIZE TUD_AUDIO_EP_SIZE(false, AUDIO_SAMPLE_RATE, AUDIO_BYTES_PER_SAMPLE, AUDIO_SPK_CHANNELS)
#define AUDIO_EP_IN_SIZE TUD_AUDIO_EP_SIZE(false, AUDIO_SAMPLE_RATE, AUDIO_BYTES_PER_SAMPLE, AUDIO_MIC_CHANNELS)

#define AUDIO_AC_UNITS_LEN                                                                                   \
    (TUD_AUDIO10_DESC_INPUT_TERM_LEN + TUD_AUDIO10_DESC_FEATURE_UNIT_LEN(2) + TUD_AUDIO10_DESC_OUTPUT_TERM_LEN + \
     TUD_AUDIO10_DESC_INPUT_TERM_LEN + TUD_AUDIO10_DESC_OUTPUT_TERM_LEN)

#define AUDIO_AS_STREAM_LEN                                                                                      \
    (TUD_AUDIO10_DESC_STD_AS_LEN + TUD_AUDIO10_DESC_STD_AS_LEN + TUD_AUDIO10_DESC_CS_AS_INT_LEN +                  \
     TUD_AUDIO10_DESC_TYPE_I_FORMAT_LEN(1) + TUD_AUDIO10_DESC_STD_AS_ISO_EP_LEN + TUD_AUDIO10_DESC_CS_AS_ISO_EP_LEN)

#define AUDIO_DESC_LEN \
    (TUD_AUDIO10_DESC_STD_AC_LEN + TUD_AUDIO10_DESC_CS_AC_LEN(2) + AUDIO_AC_UNITS_LEN + 2 * AUDIO_AS_STREAM_LEN)

#define CONFIG_TOTAL_LEN (TUD_CONFIG_DESC_LEN + 2 * TUD_HID_DESC_LEN + AUDIO_DESC_LEN)

enum { STR_LANG = 0, STR_MANUFACTURER, STR_PRODUCT, STR_SERIAL };

static const uint8_t s_config_desc[] = {
    TUD_CONFIG_DESCRIPTOR(1, ITF_COUNT, 0, CONFIG_TOTAL_LEN, TUSB_DESC_CONFIG_ATT_REMOTE_WAKEUP, 100),

    TUD_HID_DESCRIPTOR(ITF_KEYBOARD, STR_PRODUCT, HID_ITF_PROTOCOL_KEYBOARD, sizeof(s_keyboard_report_desc),
                       EP_KEYBOARD, 8, 1),
    TUD_HID_DESCRIPTOR(ITF_MOUSE, STR_PRODUCT, HID_ITF_PROTOCOL_NONE, sizeof(s_mouse_report_desc), EP_MOUSE, 16, 1),

    TUD_AUDIO10_DESC_STD_AC(ITF_AUDIO_CONTROL, 0x00, STR_PRODUCT),
    TUD_AUDIO10_DESC_CS_AC(0x0100, AUDIO_AC_UNITS_LEN, ITF_AUDIO_SPEAKER, ITF_AUDIO_MIC),
    TUD_AUDIO10_DESC_INPUT_TERM(UAC1_ENTITY_SPK_INPUT_TERMINAL, AUDIO_TERM_TYPE_USB_STREAMING,
                                UAC1_ENTITY_MIC_OUTPUT_TERMINAL, AUDIO_SPK_CHANNELS,
                                AUDIO10_CHANNEL_CONFIG_LEFT_FRONT | AUDIO10_CHANNEL_CONFIG_RIGHT_FRONT, 0x00, 0x00),
    TUD_AUDIO10_DESC_FEATURE_UNIT(UAC1_ENTITY_SPK_FEATURE_UNIT, UAC1_ENTITY_SPK_INPUT_TERMINAL, 0x00,
                                  (AUDIO10_FU_CONTROL_BM_MUTE | AUDIO10_FU_CONTROL_BM_VOLUME), 0x00, 0x00),
    TUD_AUDIO10_DESC_OUTPUT_TERM(UAC1_ENTITY_SPK_OUTPUT_TERMINAL, AUDIO_TERM_TYPE_OUT_HEADPHONES, 0x00,
                                 UAC1_ENTITY_SPK_FEATURE_UNIT, 0x00),
    TUD_AUDIO10_DESC_INPUT_TERM(UAC1_ENTITY_MIC_INPUT_TERMINAL, AUDIO_TERM_TYPE_IN_GENERIC_MIC, 0x00,
                                AUDIO_MIC_CHANNELS, AUDIO10_CHANNEL_CONFIG_NON_PREDEFINED, 0x00, 0x00),
    TUD_AUDIO10_DESC_OUTPUT_TERM(UAC1_ENTITY_MIC_OUTPUT_TERMINAL, AUDIO_TERM_TYPE_USB_STREAMING,
                                 UAC1_ENTITY_SPK_INPUT_TERMINAL, UAC1_ENTITY_MIC_INPUT_TERMINAL, 0x00),

    TUD_AUDIO10_DESC_STD_AS_INT(ITF_AUDIO_SPEAKER, 0x00, 0x00, 0x00),
    TUD_AUDIO10_DESC_STD_AS_INT(ITF_AUDIO_SPEAKER, 0x01, 0x01, 0x00),
    TUD_AUDIO10_DESC_CS_AS_INT(UAC1_ENTITY_SPK_INPUT_TERMINAL, 0x01, AUDIO10_DATA_FORMAT_TYPE_I_PCM),
    TUD_AUDIO10_DESC_TYPE_I_FORMAT(AUDIO_SPK_CHANNELS, AUDIO_BYTES_PER_SAMPLE, 16, AUDIO_SAMPLE_RATE),
    TUD_AUDIO10_DESC_STD_AS_ISO_EP(EP_AUDIO_OUT, (uint8_t)(TUSB_XFER_ISOCHRONOUS | TUSB_ISO_EP_ATT_ADAPTIVE),
                                   AUDIO_EP_OUT_SIZE, 0x01, 0x00),
    TUD_AUDIO10_DESC_CS_AS_ISO_EP(AUDIO10_CS_AS_ISO_DATA_EP_ATT_SAMPLING_FRQ,
                                  AUDIO10_CS_AS_ISO_DATA_EP_LOCK_DELAY_UNIT_MILLISEC, 0x0001),

    TUD_AUDIO10_DESC_STD_AS_INT(ITF_AUDIO_MIC, 0x00, 0x00, 0x00),
    TUD_AUDIO10_DESC_STD_AS_INT(ITF_AUDIO_MIC, 0x01, 0x01, 0x00),
    TUD_AUDIO10_DESC_CS_AS_INT(UAC1_ENTITY_MIC_OUTPUT_TERMINAL, 0x01, AUDIO10_DATA_FORMAT_TYPE_I_PCM),
    TUD_AUDIO10_DESC_TYPE_I_FORMAT(AUDIO_MIC_CHANNELS, AUDIO_BYTES_PER_SAMPLE, 16, AUDIO_SAMPLE_RATE),
    TUD_AUDIO10_DESC_STD_AS_ISO_EP(EP_AUDIO_IN, (uint8_t)(TUSB_XFER_ISOCHRONOUS | TUSB_ISO_EP_ATT_ASYNCHRONOUS),
                                   AUDIO_EP_IN_SIZE, 0x01, 0x00),
    TUD_AUDIO10_DESC_CS_AS_ISO_EP(AUDIO10_CS_AS_ISO_DATA_EP_ATT_SAMPLING_FRQ,
                                  AUDIO10_CS_AS_ISO_DATA_EP_LOCK_DELAY_UNIT_MILLISEC, 0x0001),
};

_Static_assert(sizeof(s_config_desc) == CONFIG_TOTAL_LEN, "configuration descriptor length");
_Static_assert(AUDIO_EP_OUT_SIZE <= CFG_TUD_AUDIO_FUNC_1_EP_OUT_SZ_MAX, "CFG_TUD_AUDIO_FUNC_1_EP_OUT_SZ_MAX too small");
_Static_assert(AUDIO_EP_IN_SIZE <= CFG_TUD_AUDIO_FUNC_1_EP_IN_SZ_MAX, "CFG_TUD_AUDIO_FUNC_1_EP_IN_SZ_MAX too small");

static const tusb_desc_device_t s_device_desc = {
    .bLength = sizeof(tusb_desc_device_t),
    .bDescriptorType = TUSB_DESC_DEVICE,
    .bcdUSB = 0x0200,
    .bDeviceClass = 0x00,
    .bDeviceSubClass = 0x00,
    .bDeviceProtocol = 0x00,
    .bMaxPacketSize0 = CFG_TUD_ENDPOINT0_SIZE,
    .idVendor = TINYUSB_ESPRESSIF_VID,
    .idProduct = KVMATE_USB_PID,
    .bcdDevice = KVMATE_USB_BCD,
    .iManufacturer = STR_MANUFACTURER,
    .iProduct = STR_PRODUCT,
    .iSerialNumber = STR_SERIAL,
    .bNumConfigurations = 1,
};

static char s_manufacturer[PROTOCOL_USB_NAME_MAX + 1];
static char s_product[PROTOCOL_USB_NAME_MAX + 1];
static char s_serial[17];
static const char *s_strings[] = {
    [STR_LANG] = LANGID_EN_US,
    [STR_MANUFACTURER] = s_manufacturer,
    [STR_PRODUCT] = s_product,
    [STR_SERIAL] = s_serial,
};

static uint8_t s_mac[6];
static esp_timer_handle_t s_reconnect_timer;

static bool name_from(char *out, const uint8_t *in, uint8_t len)
{
    if (len == 0 || len > PROTOCOL_USB_NAME_MAX) {
        return false;
    }
    for (uint8_t i = 0; i < len; i++) {
        if (in[i] < 0x20 || in[i] > 0x7E) {
            return false;
        }
    }
    memcpy(out, in, len);
    out[len] = 0;
    return true;
}

static void names_from(const protocol_usb_names_t *n, char *manufacturer, char *product)
{
    if (!n || !name_from(manufacturer, n->manufacturer, n->manufacturer_len)) {
        strcpy(manufacturer, DEFAULT_NAME);
    }
    if (!n || !name_from(product, n->product, n->product_len)) {
        strcpy(product, DEFAULT_NAME);
    }
}

static uint32_t fnv1a(uint32_t h, const char *s)
{
    for (; *s; s++) {
        h = (h ^ (uint8_t)*s) * 16777619u;
    }
    return h * 16777619u;
}

static void update_serial(void)
{
    int len = snprintf(s_serial, sizeof(s_serial), "%02X%02X%02X%02X%02X%02X", s_mac[0], s_mac[1], s_mac[2],
                       s_mac[3], s_mac[4], s_mac[5]);
    if (strcmp(s_manufacturer, DEFAULT_NAME) != 0 || strcmp(s_product, DEFAULT_NAME) != 0) {
        uint32_t h = fnv1a(fnv1a(2166136261u, s_manufacturer), s_product);
        snprintf(s_serial + len, sizeof(s_serial) - len, "%04X", (unsigned)((h ^ (h >> 16)) & 0xFFFF));
    }
}

static void load_names(void)
{
    protocol_usb_names_t n;
    names_from(storage_load(NVS_KEY, &n, sizeof(n)) ? &n : NULL, s_manufacturer, s_product);
}

static void on_reconnect(void *arg)
{
    tud_connect();
}

void usb_device_set_names(const protocol_usb_names_t *names)
{
    char manufacturer[PROTOCOL_USB_NAME_MAX + 1], product[PROTOCOL_USB_NAME_MAX + 1];
    names_from(names, manufacturer, product);
    if (strcmp(manufacturer, s_manufacturer) == 0 && strcmp(product, s_product) == 0) {
        return;
    }
    bool defaults = strcmp(manufacturer, DEFAULT_NAME) == 0 && strcmp(product, DEFAULT_NAME) == 0;
    esp_err_t err = storage_save(NVS_KEY, defaults ? NULL : names, sizeof(*names));
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "USB names not saved: %s", esp_err_to_name(err));
    }
    tud_disconnect();
    strcpy(s_manufacturer, manufacturer);
    strcpy(s_product, product);
    update_serial();
    esp_timer_start_once(s_reconnect_timer, RECONNECT_US);
    ESP_LOGI(TAG, "USB names: \"%s\" by \"%s\" (serial %s), reconnecting to the computer", s_product,
             s_manufacturer, s_serial);
}

void usb_device_init(void)
{
    esp_efuse_mac_get_default(s_mac);
    load_names();
    update_serial();
    const esp_timer_create_args_t reconnect = {.callback = on_reconnect, .name = "usb_reconnect"};
    esp_timer_create(&reconnect, &s_reconnect_timer);

    esp_log_level_set("tusb_desc", ESP_LOG_WARN);

    tinyusb_config_t cfg = TINYUSB_DEFAULT_CONFIG();
    cfg.descriptor.device = &s_device_desc;
    cfg.descriptor.full_speed_config = s_config_desc;
    cfg.descriptor.string = s_strings;
    cfg.descriptor.string_count = sizeof(s_strings) / sizeof(s_strings[0]);
    ESP_ERROR_CHECK(tinyusb_driver_install(&cfg));
}

const char *usb_device_name(void)
{
    return s_product;
}
