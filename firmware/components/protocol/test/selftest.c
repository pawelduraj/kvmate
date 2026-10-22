#include <string.h>

#include "codec.h"
#include "secure.h"
#include "test_vectors.h"

typedef struct {
    const test_vector_t *tv;
    bool ok;
    int seen;
} selftest_ctx_t;

static void on_frame(const protocol_frame_t *frame, void *arg)
{
    selftest_ctx_t *ctx = arg;
    const test_vector_t *tv = ctx->tv;
    ctx->seen++;
    ctx->ok = frame->header.msg_type == tv->msg_type && frame->header.sequence == tv->seq &&
              frame->header.payload_len == tv->payload_len &&
              memcmp(frame->payload, tv->payload, tv->payload_len) == 0;
}

bool protocol_selftest(void)
{
    static uint8_t wire[PROTOCOL_MAX_ENCODED_LEN];
    static protocol_decoder_t decoder;

    for (size_t i = 0; i < TEST_VECTOR_COUNT; i++) {
        const test_vector_t *tv = &TEST_VECTORS[i];

        protocol_part_t part = {tv->payload, tv->payload_len};
        size_t n = protocol_encode_frame(tv->msg_type, tv->seq, &part, 1, wire, sizeof(wire));
        if (n != tv->wire_len || memcmp(wire, tv->wire, n) != 0) {
            return false;
        }

        selftest_ctx_t ctx = {.tv = tv};
        protocol_decoder_init(&decoder);
        protocol_decoder_push(&decoder, tv->wire, tv->wire_len, on_frame, &ctx);
        if (ctx.seen != 1 || !ctx.ok) {
            return false;
        }
    }
    return true;
}

static bool oversized_refused(protocol_secure_session_t *s, uint8_t *plain)
{
    static uint8_t dgram[PROTOCOL_DGRAM_MAX_LEN + 1];
    protocol_dgram_data_header_t head = {.kind = PROTOCOL_DGRAM_DATA, .session_id = s->id};
    memcpy(dgram, &head, sizeof(head));
    memset(plain, 0xA5, PROTOCOL_DGRAM_MAX_PLAIN);
    return protocol_secure_open(s, dgram, sizeof(dgram), plain) < 0 && plain[0] == 0xA5;
}

bool protocol_secure_selftest(void)
{
    uint8_t buf[32];
    protocol_secure_key_id(tv_secure_key, buf);
    bool ok = memcmp(buf, tv_secure_key_id, 8) == 0;
    protocol_secure_reply_mac(tv_secure_key, tv_secure_hub_nonce, tv_secure_board_nonce, buf);
    ok = ok && memcmp(buf, tv_secure_reply_mac, 32) == 0;

    static protocol_secure_session_t s;
    static uint8_t out[PROTOCOL_DGRAM_MAX_LEN];
    protocol_secure_session_start(&s, tv_secure_key, tv_secure_hub_nonce, tv_secure_board_nonce);
    size_t n = protocol_secure_seal(&s, tv_secure_plain, sizeof(tv_secure_plain), out, sizeof(out));
    ok = ok && n == sizeof(tv_secure_to_hub) && memcmp(out, tv_secure_to_hub, n) == 0;
    int m = protocol_secure_open(&s, tv_secure_to_board, sizeof(tv_secure_to_board), out);
    ok = ok && m == (int)sizeof(tv_secure_plain) && memcmp(out, tv_secure_plain, m) == 0;
    ok = ok && protocol_secure_open(&s, tv_secure_to_board, sizeof(tv_secure_to_board), out) < 0;

    protocol_secure_session_start(&s, tv_secure_key, tv_secure_hub_nonce, tv_secure_board_nonce);
    static uint8_t bad[sizeof(tv_secure_to_board)];
    memcpy(bad, tv_secure_to_board, sizeof(bad));
    bad[sizeof(bad) - 1] ^= 0x01;
    ok = ok && protocol_secure_open(&s, bad, sizeof(bad), out) < 0;
    ok = ok && oversized_refused(&s, out);
    protocol_secure_session_end(&s);
    return ok;
}
