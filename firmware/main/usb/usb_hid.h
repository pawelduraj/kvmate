/* Keyboard, mouse and media keys for the controlled computer. Every function only updates the state
 * and is safe from any task; the HID task sends the reports. Pressing a key while the computer
 * sleeps wakes it. */
#pragma once

#include <stdbool.h>
#include <stdint.h>

#include "protocol.h"

/* Starts the HID task. */
void usb_hid_init(void);

/* Sets the whole keyboard state, sent as a boot-protocol report. The computer sees up to 6 keys at
 * once, plus Ctrl, Shift, Alt and Win. With more held it gets a "too many keys" report and keeps
 * what it had. */
void usb_hid_set_keyboard(const protocol_keyboard_state_t *state);

/* Sets the buttons and adds the movement. Movement too large for one report goes out in the next
 * ones. An empty report is sent too: the hub uses it to keep the computer awake. */
void usb_hid_add_mouse(const protocol_mouse_report_t *report);

/* Sets the media key held (its HID consumer usage), 0 for none. */
void usb_hid_set_consumer(uint16_t usage);

/* Releases every key and button and drops the movement not sent yet. */
void usb_hid_release_all(void);

/* True once the controlled computer has set up the USB device. */
bool usb_hid_mounted(void);

/* True while the controlled computer sleeps (it has suspended the bus). */
bool usb_hid_suspended(void);
