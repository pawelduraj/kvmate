/* The board's Wi-Fi, set by the hub in WifiConfig and kept in NVS. Station: the board joins a
 * network, retrying every 2 to 30 s. Access point: the board's own WPA2 network for one computer.
 * It gives out no gateway and no DNS, so that computer keeps using its other connection for the
 * internet. Power saving is off for lower latency; the board runs on USB power anyway. */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "protocol.h"

/* Called on the Wi-Fi task with each decrypted datagram, which holds whole frames. */
typedef void (*wifi_rx_fn)(const uint8_t *data, size_t len);

/* Starts Wi-Fi with the stored settings, and the encrypted sessions with the hub. With no settings
 * Wi-Fi stays off, but scanning still works. Call after storage_init(). */
void wifi_init(wifi_rx_fn on_rx);

/* Stores and applies new settings in the background. If they can't be stored, nothing changes. Mode
 * OFF erases the stored settings. Applying ends the hub's Wi-Fi session. */
void wifi_apply(const protocol_wifi_config_t *cfg);

/* Scans in the background and sends WifiScanResult: strongest networks first, one per name. */
void wifi_scan(void);

/* Fills `out`. Returns true if anything other than the signal strength changed since the last call.
 * Call from one task only. */
bool wifi_status(protocol_wifi_status_t *out);

/* True if the hub sent something over Wi-Fi in the last PROTOCOL_LINK_TIMEOUT_MS (in wifi_udp.c). */
bool wifi_hub_alive(void);

/* Sends whole frames to the hub over Wi-Fi. False if no session is active (in wifi_udp.c). */
bool wifi_send(const uint8_t *data, size_t len);

/* True if Wi-Fi is set up but in PROTOCOL_WIFI_STATE_FAILED. */
bool wifi_failed(void);
