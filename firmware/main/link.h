/* Talking to the hub, by cable (the board's COM port) or over Wi-Fi. Replies go by cable while
 * the hub talks by cable, otherwise over Wi-Fi. When the hub goes quiet for 1.5 s, the board
 * releases every key and the LED blinks again. Messages from the cable and Wi-Fi are handled one
 * at a time. A message longer than expected (from a newer hub) is fine; a shorter one is dropped. */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "codec.h"

/* Starts the cable link and the log forwarding. The board then sends a Heartbeat every
 * PROTOCOL_HEARTBEAT_INTERVAL_MS, and WifiStatus on every change and every
 * PROTOCOL_WIFI_STATUS_INTERVAL_MS. `selftest_ok` goes to the hub in DeviceInfo. */
void link_init(bool selftest_ok);

/* Sends one message whose payload is the `n_parts` parts joined. Safe from any task. Returns false
 * if the message was dropped: the payload is too long, or another task held the link for 20 ms. */
bool link_send(uint8_t msg_type, const protocol_part_t *parts, size_t n_parts);

/* True if the hub was heard, by cable or Wi-Fi, in the last PROTOCOL_LINK_TIMEOUT_MS. */
bool link_hub_alive(void);

/* Handles a decrypted Wi-Fi datagram from the hub (whole frames). */
void link_wifi_rx(const uint8_t *data, size_t len);
