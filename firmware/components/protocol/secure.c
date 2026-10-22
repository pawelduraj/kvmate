#include "secure.h"

#include <string.h>

#include "codec.h"
#include "mbedtls/md.h"

#define LABEL(s) (const uint8_t *)(s), sizeof(s) - 1

static void hmac(const uint8_t *key, size_t key_len, const protocol_part_t *parts, size_t n, uint8_t out[32])
{
    mbedtls_md_context_t ctx;
    mbedtls_md_init(&ctx);
    mbedtls_md_setup(&ctx, mbedtls_md_info_from_type(MBEDTLS_MD_SHA256), 1);
    mbedtls_md_hmac_starts(&ctx, key, key_len);
    for (size_t i = 0; i < n; i++) {
        mbedtls_md_hmac_update(&ctx, parts[i].ptr, parts[i].len);
    }
    mbedtls_md_hmac_finish(&ctx, out);
    mbedtls_md_free(&ctx);
}

void protocol_secure_key_id(const uint8_t key[PROTOCOL_PAIRING_KEY_LEN], uint8_t out[8])
{
    uint8_t full[32];
    protocol_part_t parts[] = {{LABEL("kvmate/key-id")}};
    hmac(key, PROTOCOL_PAIRING_KEY_LEN, parts, 1, full);
    memcpy(out, full, 8);
}

void protocol_secure_reply_mac(const uint8_t key[PROTOCOL_PAIRING_KEY_LEN], const uint8_t hub_nonce[16],
                               const uint8_t board_nonce[16], uint8_t out[32])
{
    protocol_part_t parts[] = {{LABEL("kvmate/reply")}, {hub_nonce, 16}, {board_nonce, 16}};
    hmac(key, PROTOCOL_PAIRING_KEY_LEN, parts, 3, out);
}

static void session_keys(const uint8_t key[PROTOCOL_PAIRING_KEY_LEN], const uint8_t hub_nonce[16],
                         const uint8_t board_nonce[16], uint8_t hub_key[32], uint8_t board_key[32])
{
    uint8_t salt[32], prk[32];
    static const uint8_t one = 1;
    memcpy(salt, hub_nonce, 16);
    memcpy(salt + 16, board_nonce, 16);
    protocol_part_t ikm[] = {{key, PROTOCOL_PAIRING_KEY_LEN}};
    hmac(salt, sizeof(salt), ikm, 1, prk);
    protocol_part_t hub[] = {{LABEL("kvmate/hub->board")}, {&one, 1}};
    hmac(prk, sizeof(prk), hub, 2, hub_key);
    protocol_part_t board[] = {{LABEL("kvmate/board->hub")}, {&one, 1}};
    hmac(prk, sizeof(prk), board, 2, board_key);
}

void protocol_secure_session_end(protocol_secure_session_t *s)
{
    if (s->valid) {
        mbedtls_gcm_free(&s->tx);
        mbedtls_gcm_free(&s->rx);
    }
    memset(s, 0, sizeof(*s));
}

void protocol_secure_session_start(protocol_secure_session_t *s, const uint8_t key[PROTOCOL_PAIRING_KEY_LEN],
                                   const uint8_t hub_nonce[16], const uint8_t board_nonce[16])
{
    protocol_secure_session_end(s);
    uint8_t hub_key[32], board_key[32];
    session_keys(key, hub_nonce, board_nonce, hub_key, board_key);
    mbedtls_gcm_init(&s->tx);
    mbedtls_gcm_init(&s->rx);
    mbedtls_gcm_setkey(&s->tx, MBEDTLS_CIPHER_ID_AES, board_key, 256);
    mbedtls_gcm_setkey(&s->rx, MBEDTLS_CIPHER_ID_AES, hub_key, 256);
    memcpy(s->hub_nonce, hub_nonce, 16);
    memcpy(s->board_nonce, board_nonce, 16);
    memcpy(&s->id, board_nonce, sizeof(s->id));
    s->valid = true;
    memset(hub_key, 0, sizeof(hub_key));
    memset(board_key, 0, sizeof(board_key));
}

static void make_nonce(uint64_t counter, uint8_t nonce[12])
{
    memset(nonce, 0, 12);
    memcpy(nonce, &counter, 8);
}

size_t protocol_secure_seal(protocol_secure_session_t *s, const uint8_t *plain, size_t len, uint8_t *out,
                            size_t out_size)
{
    size_t total = sizeof(protocol_dgram_data_header_t) + len + PROTOCOL_DGRAM_TAG_LEN;
    if (!s->valid || len > PROTOCOL_DGRAM_MAX_PLAIN || out_size < total) {
        return 0;
    }
    protocol_dgram_data_header_t head = {.kind = PROTOCOL_DGRAM_DATA, .session_id = s->id, .counter = s->tx_counter++};
    memcpy(out, &head, sizeof(head));
    uint8_t nonce[12];
    make_nonce(head.counter, nonce);
    uint8_t *body = out + sizeof(head);
    if (mbedtls_gcm_crypt_and_tag(&s->tx, MBEDTLS_GCM_ENCRYPT, len, nonce, sizeof(nonce), out, sizeof(head), plain,
                                  body, PROTOCOL_DGRAM_TAG_LEN, body + len) != 0) {
        return 0;
    }
    return total;
}

uint32_t protocol_secure_data_session(const uint8_t *dgram, size_t len)
{
    if (len < sizeof(protocol_dgram_data_header_t) + PROTOCOL_DGRAM_TAG_LEN || dgram[0] != PROTOCOL_DGRAM_DATA) {
        return 0;
    }
    protocol_dgram_data_header_t head;
    memcpy(&head, dgram, sizeof(head));
    return head.session_id;
}

static bool window_fresh(const protocol_secure_session_t *s, uint64_t counter)
{
    if (counter >= s->rx_next) {
        return true;
    }
    uint64_t age = s->rx_next - 1 - counter;
    return age < 64 && (s->rx_seen & (1ULL << age)) == 0;
}

static void window_mark(protocol_secure_session_t *s, uint64_t counter)
{
    if (counter >= s->rx_next) {
        uint64_t shift = counter - s->rx_next + 1;
        s->rx_seen = shift >= 64 ? 0 : s->rx_seen << shift;
        s->rx_seen |= 1;
        s->rx_next = counter + 1;
    } else {
        s->rx_seen |= 1ULL << (s->rx_next - 1 - counter);
    }
}

int protocol_secure_open(protocol_secure_session_t *s, const uint8_t *dgram, size_t len, uint8_t *plain)
{
    if (!s->valid || protocol_secure_data_session(dgram, len) != s->id) {
        return -1;
    }
    protocol_dgram_data_header_t head;
    memcpy(&head, dgram, sizeof(head));
    if (!window_fresh(s, head.counter)) {
        return -1;
    }
    size_t body_len = len - sizeof(head) - PROTOCOL_DGRAM_TAG_LEN;
    if (body_len > PROTOCOL_DGRAM_MAX_PLAIN) {
        return -1;
    }
    uint8_t nonce[12];
    make_nonce(head.counter, nonce);
    const uint8_t *body = dgram + sizeof(head);
    if (mbedtls_gcm_auth_decrypt(&s->rx, body_len, nonce, sizeof(nonce), dgram, sizeof(head), body + body_len,
                                 PROTOCOL_DGRAM_TAG_LEN, body, plain) != 0) {
        return -1;
    }
    window_mark(s, head.counter);
    return (int)body_len;
}

bool protocol_secure_equal(const uint8_t *a, const uint8_t *b, size_t len)
{
    uint8_t diff = 0;
    for (size_t i = 0; i < len; i++) {
        diff |= a[i] ^ b[i];
    }
    return diff == 0;
}
