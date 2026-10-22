/* Encrypted UDP sessions with the hub on PROTOCOL_WIFI_UDP_PORT. A Hello from the paired hub gets a
 * Reply and opens a pending session; other Hellos get no answer. A repeated Hello gets the same
 * Reply, since the hub sends it to several addresses and a Reply can be lost. The pending session's
 * first data datagram makes it the active one and ends the old one. Everything goes to the address
 * the hub last sent from. link.c uses this through wifi.h (wifi_hub_alive, wifi_send). */
#pragma once

#include <stdint.h>

#include "wifi.h"

/* Sets the pairing key (NULL for none), opens the socket and starts the UDP task. Call after
 * esp_netif_init(). */
void wifi_udp_init(wifi_rx_fn on_rx, const uint8_t *key);

/* Ends both sessions and sets the pairing key (PROTOCOL_PAIRING_KEY_LEN bytes). With NULL, no Hello
 * is answered. */
void wifi_udp_set_key(const uint8_t *key);
