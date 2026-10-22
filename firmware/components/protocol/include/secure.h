/* Encrypted Wi-Fi datagrams: the board's side of crates/protocol/src/secure.rs, with the same
 * HMAC-SHA256, HKDF-SHA256 and AES-256-GCM. Each session's keys come from the pairing key and a nonce
 * from each side. A data datagram is accepted only once. protocol_secure_selftest() checks this code
 * against datagrams made by Rust. */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "mbedtls/gcm.h"
#include "protocol.h"

/* One session with the hub. All zeroes means no session. */
typedef struct {
    bool valid;
    /* The id in protocol_dgram_data_header_t.session_id. */
    uint32_t id;
    uint8_t hub_nonce[PROTOCOL_DGRAM_NONCE_LEN];
    uint8_t board_nonce[PROTOCOL_DGRAM_NONCE_LEN];
    /* Board -> hub. */
    mbedtls_gcm_context tx;
    /* Hub -> board. */
    mbedtls_gcm_context rx;
    uint64_t tx_counter;
    /* Replay window: the highest counter received, plus 1; 0 before the first datagram. */
    uint64_t rx_next;
    /* Replay window: which counters below rx_next were received. Bit 0 is the highest, bit 1 the
     * one before it, and so on. */
    uint64_t rx_seen;
} protocol_secure_session_t;

/* The pairing key's id, sent in every Hello: the first 8 bytes of HMAC-SHA256(key, "kvmate/key-id"). */
void protocol_secure_key_id(const uint8_t key[PROTOCOL_PAIRING_KEY_LEN], uint8_t out[8]);

/* The MAC of a Reply: HMAC-SHA256(key, "kvmate/reply" followed by hub_nonce and board_nonce). */
void protocol_secure_reply_mac(const uint8_t key[PROTOCOL_PAIRING_KEY_LEN], const uint8_t hub_nonce[16],
                               const uint8_t board_nonce[16], uint8_t out[32]);

/* Starts a session with keys made from the pairing key and both nonces. Ends whatever session `s`
 * held before. */
void protocol_secure_session_start(protocol_secure_session_t *s, const uint8_t key[PROTOCOL_PAIRING_KEY_LEN],
                                   const uint8_t hub_nonce[16], const uint8_t board_nonce[16]);

/* Frees the session's keys and zeroes it. Does nothing to a zeroed session. */
void protocol_secure_session_end(protocol_secure_session_t *s);

/* Encrypts `len` bytes of frames (at most PROTOCOL_DGRAM_MAX_PLAIN) into a data datagram for the hub.
 * Returns the datagram's length, or 0 if there is no session, `len` is too long or `out_size` is
 * too small. */
size_t protocol_secure_seal(protocol_secure_session_t *s, const uint8_t *plain, size_t len, uint8_t *out,
                            size_t out_size);

/* The session id of a data datagram, 0 if `dgram` is not one. */
uint32_t protocol_secure_data_session(const uint8_t *dgram, size_t len);

/* Decrypts a data datagram from the hub into `plain`, which must hold PROTOCOL_DGRAM_MAX_PLAIN bytes.
 * A longer datagram is refused before anything is written there. Returns the plaintext's length, or
 * -1 if the datagram is for another session, too long, forged, damaged or already received. */
int protocol_secure_open(protocol_secure_session_t *s, const uint8_t *dgram, size_t len, uint8_t *plain);

/* Compares `len` bytes in constant time. */
bool protocol_secure_equal(const uint8_t *a, const uint8_t *b, size_t len);

/* Checks the functions above against test_vectors.h, made by the Rust code. Also checks that a
 * datagram received twice, with one byte changed, or too long is refused. */
bool protocol_secure_selftest(void);
