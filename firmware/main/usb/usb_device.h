/* The USB device on the board's "USB" port: keyboard, mouse with media keys, headphones and
 * microphone. The keyboard uses the boot protocol, so it also works before the OS starts (UEFI,
 * BitLocker). The audio is UAC1, which needs no drivers on any OS. Every interface carries the
 * product name, which Windows shows for the headphones and the microphone. */
#pragma once

#include "protocol.h"

/* USB interface numbers, in descriptor order. */
enum {
    ITF_KEYBOARD = 0,
    ITF_MOUSE,
    ITF_AUDIO_CONTROL,
    ITF_AUDIO_SPEAKER,
    ITF_AUDIO_MIC,
    ITF_COUNT,
};

/* TinyUSB's HID instance numbers, in descriptor order. */
enum { HID_KEYBOARD = 0, HID_MOUSE = 1 };

/* Report IDs on the mouse interface, which also carries the media keys. The keyboard's interface
 * has none, so it keeps working before the OS starts. After its ID, the mouse report is a
 * protocol_mouse_report_t (16-bit X and Y). */
enum { REPORT_ID_MOUSE = 1, REPORT_ID_CONSUMER = 2 };

/* The USB product ID under Espressif's vendor ID: the one TinyUSB uses for HID devices. A device
 * that only uses standard drivers needs no product ID of its own (github.com/espressif/usb-pids). */
#define KVMATE_USB_PID 0x4004

/* The device version (bcdDevice). Bump it whenever the descriptors change, so computers read them
 * again. */
#define KVMATE_USB_BCD 0x0201

/* Endpoint addresses. */
#define EP_KEYBOARD 0x81
#define EP_MOUSE 0x82
#define EP_AUDIO_OUT 0x03
#define EP_AUDIO_IN 0x83

/* UAC1 entities. Headphones: USB stream (input terminal) -> volume and mute (feature unit) ->
 * headphones (output terminal). Microphone: input terminal -> USB stream (output terminal). */
#define UAC1_ENTITY_SPK_INPUT_TERMINAL 0x01
#define UAC1_ENTITY_SPK_FEATURE_UNIT 0x02
#define UAC1_ENTITY_SPK_OUTPUT_TERMINAL 0x03
#define UAC1_ENTITY_MIC_INPUT_TERMINAL 0x11
#define UAC1_ENTITY_MIC_OUTPUT_TERMINAL 0x13

/* The audio format on USB, the same as on the link to the hub. */
#define AUDIO_SAMPLE_RATE PROTOCOL_AUDIO_RATE_HZ
#define AUDIO_SPK_CHANNELS PROTOCOL_AUDIO_PLAYBACK_CHANNELS
#define AUDIO_MIC_CHANNELS PROTOCOL_AUDIO_CAPTURE_CHANNELS
#define AUDIO_BYTES_PER_SAMPLE 2

/* Starts USB with the names stored in NVS. Call after storage_init() and before link_init(),
 * because a hub that is already waiting sends UsbNames right after the handshake. */
void usb_device_init(void);

/* The product name the controlled computer sees. */
const char *usb_device_name(void);

/* Takes new names from the hub. An empty or invalid name (only printable ASCII is valid) becomes
 * "kvmate". If the names changed, the board stores them and reconnects to the computer after 0.5 s.
 * The serial number is the MAC plus a hash of any custom names, because Windows keeps the old names
 * of a device it already knows (same IDs and serial number). The hub matches only the MAC part. */
void usb_device_set_names(const protocol_usb_names_t *names);
