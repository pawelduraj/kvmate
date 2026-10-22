#include "codec.h"

#include <string.h>

static uint16_t s_crc_table[256];
static bool s_crc_ready;

static void crc_init(void)
{
    for (int i = 0; i < 256; i++) {
        uint16_t crc = (uint16_t)(i << 8);
        for (int bit = 0; bit < 8; bit++) {
            crc = (crc & 0x8000) ? (uint16_t)((crc << 1) ^ 0x1021) : (uint16_t)(crc << 1);
        }
        s_crc_table[i] = crc;
    }
    s_crc_ready = true;
}

uint16_t protocol_crc16(const uint8_t *data, size_t len)
{
    if (!s_crc_ready) {
        crc_init();
    }
    uint16_t crc = 0xFFFF;
    for (size_t i = 0; i < len; i++) {
        crc = (uint16_t)((crc << 8) ^ s_crc_table[((crc >> 8) ^ data[i]) & 0xFF]);
    }
    return crc;
}

size_t protocol_cobs_encode(const uint8_t *src, size_t len, uint8_t *dst, size_t dst_cap)
{
    if (dst_cap == 0) {
        return 0;
    }
    size_t code_idx = 0;
    size_t out = 1;
    uint8_t code = 1;
    for (size_t i = 0; i < len; i++) {
        if (src[i] == 0) {
            dst[code_idx] = code;
            code_idx = out++;
            code = 1;
        } else {
            if (out >= dst_cap) {
                return 0;
            }
            dst[out++] = src[i];
            if (++code == 0xFF) {
                dst[code_idx] = code;
                code_idx = out++;
                code = 1;
            }
        }
        if (code_idx >= dst_cap) {
            return 0;
        }
    }
    dst[code_idx] = code;
    return out;
}

bool protocol_cobs_decode(const uint8_t *src, size_t len, uint8_t *dst, size_t dst_cap, size_t *out_len)
{
    size_t i = 0;
    size_t out = 0;
    while (i < len) {
        uint8_t code = src[i++];
        if (code == 0) {
            return false;
        }
        for (uint8_t k = 1; k < code; k++) {
            if (i >= len || out >= dst_cap) {
                return false;
            }
            dst[out++] = src[i++];
        }
        if (code < 0xFF && i < len) {
            if (out >= dst_cap) {
                return false;
            }
            dst[out++] = 0;
        }
    }
    *out_len = out;
    return true;
}

size_t protocol_encode_frame(uint8_t msg_type, uint16_t seq, const protocol_part_t *parts, size_t n_parts,
                             uint8_t *out, size_t out_cap)
{
    size_t payload_len = 0;
    for (size_t i = 0; i < n_parts; i++) {
        payload_len += parts[i].len;
    }
    if (payload_len > PROTOCOL_MAX_PAYLOAD || out_cap < 2) {
        return 0;
    }

    uint8_t raw[PROTOCOL_MAX_FRAME_LEN];
    protocol_frame_header_t header = {
        .protocol_version = PROTOCOL_VERSION,
        .msg_type = msg_type,
        .sequence = seq,
        .payload_len = (uint16_t)payload_len,
    };
    memcpy(raw, &header, PROTOCOL_HEADER_LEN);
    size_t pos = PROTOCOL_HEADER_LEN;
    for (size_t i = 0; i < n_parts; i++) {
        if (parts[i].len) {
            memcpy(raw + pos, parts[i].ptr, parts[i].len);
            pos += parts[i].len;
        }
    }
    uint16_t crc = protocol_crc16(raw, pos);
    raw[pos++] = (uint8_t)(crc & 0xFF);
    raw[pos++] = (uint8_t)(crc >> 8);

    out[0] = 0;
    size_t n = protocol_cobs_encode(raw, pos, out + 1, out_cap - 2);
    if (n == 0) {
        return 0;
    }
    out[1 + n] = 0;
    return n + 2;
}

protocol_err_t protocol_parse_frame(const uint8_t *raw, size_t len, protocol_frame_t *out)
{
    if (len < PROTOCOL_HEADER_LEN + PROTOCOL_CRC_LEN) {
        return PROTOCOL_ERR_TOO_SHORT;
    }
    size_t body_len = len - PROTOCOL_CRC_LEN;
    uint16_t expected = (uint16_t)(raw[body_len] | (raw[body_len + 1] << 8));
    if (protocol_crc16(raw, body_len) != expected) {
        return PROTOCOL_ERR_CRC;
    }
    memcpy(&out->header, raw, PROTOCOL_HEADER_LEN);
    if (out->header.protocol_version != PROTOCOL_VERSION) {
        return PROTOCOL_ERR_VERSION;
    }
    if (out->header.payload_len != body_len - PROTOCOL_HEADER_LEN) {
        return PROTOCOL_ERR_LENGTH;
    }
    out->payload = raw + PROTOCOL_HEADER_LEN;
    return PROTOCOL_OK;
}

void protocol_decoder_init(protocol_decoder_t *d)
{
    memset(d, 0, sizeof(*d));
}

void protocol_decoder_push(protocol_decoder_t *d, const uint8_t *data, size_t len, protocol_frame_cb cb, void *ctx)
{
    for (size_t i = 0; i < len; i++) {
        uint8_t byte = data[i];
        if (byte != 0) {
            if (d->len < sizeof(d->buf)) {
                d->buf[d->len++] = byte;
            } else {
                d->overflow = true;
            }
            continue;
        }
        size_t collected = d->len;
        bool overflow = d->overflow;
        d->len = 0;
        d->overflow = false;
        size_t n;
        protocol_frame_t frame;
        if (!overflow && collected > 0 && protocol_cobs_decode(d->buf, collected, d->raw, sizeof(d->raw), &n) &&
            protocol_parse_frame(d->raw, n, &frame) == PROTOCOL_OK) {
            cb(&frame, ctx);
        }
    }
}
