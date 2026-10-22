/* Frames, byte for byte as crates/protocol makes them (src/wire): the header, payload and CRC,
 * COBS-encoded, between two 0x00 bytes. protocol_selftest() checks this code against frames made by
 * Rust. */
#pragma once

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#include "protocol.h"

/* The CRC-16/CCITT-FALSE of `len` bytes, the same CRC the Rust side uses. */
uint16_t protocol_crc16(const uint8_t *data, size_t len);

/* COBS-encodes `len` bytes of `src` into `dst`, without the 0x00 bytes around it. Returns the
 * encoded length, or 0 if it doesn't fit in `dst_cap` bytes. */
size_t protocol_cobs_encode(const uint8_t *src, size_t len, uint8_t *dst, size_t dst_cap);

/* Decodes one COBS block (without its 0x00 bytes) into `dst` and sets `out_len`. Returns false if
 * the block is malformed or decodes to more than `dst_cap` bytes. */
bool protocol_cobs_decode(const uint8_t *src, size_t len, uint8_t *dst, size_t dst_cap, size_t *out_len);

/* One piece of a payload. It lets a struct and the data after it go out without being copied
 * together first. */
typedef struct {
    const void *ptr;
    size_t len;
} protocol_part_t;

/* Writes a whole frame to `out`. The payload is the `n_parts` parts joined, at most
 * PROTOCOL_MAX_PAYLOAD bytes. Returns the frame's length, or 0 if the payload is too long or the
 * frame doesn't fit in `out_cap` bytes. PROTOCOL_MAX_ENCODED_LEN bytes are always enough. */
size_t protocol_encode_frame(uint8_t msg_type, uint16_t seq, const protocol_part_t *parts, size_t n_parts,
                             uint8_t *out, size_t out_cap);

/* The result of protocol_parse_frame(). */
typedef enum {
    PROTOCOL_OK = 0,
    /* Shorter than a header and a CRC. */
    PROTOCOL_ERR_TOO_SHORT,
    PROTOCOL_ERR_CRC,
    /* A different PROTOCOL_VERSION. */
    PROTOCOL_ERR_VERSION,
    /* The header's length doesn't match the payload. */
    PROTOCOL_ERR_LENGTH,
} protocol_err_t;

/* A received frame; `payload` (header.len bytes) points into the buffer it was parsed from. */
typedef struct {
    protocol_frame_header_t header;
    const uint8_t *payload;
} protocol_frame_t;

/* Checks one COBS-decoded frame (header, payload, CRC) of `len` bytes and fills `out`. */
protocol_err_t protocol_parse_frame(const uint8_t *raw, size_t len, protocol_frame_t *out);

/* Called by protocol_decoder_push() for every valid frame. `frame` is valid only during the call. */
typedef void (*protocol_frame_cb)(const protocol_frame_t *frame, void *ctx);

/* Cuts a byte stream into frames at its 0x00 bytes. */
typedef struct {
    /* The COBS block so far. */
    uint8_t buf[PROTOCOL_MAX_ENCODED_LEN];
    size_t len;
    /* The block is too long for any frame; it is dropped at the next 0x00. */
    bool overflow;
    /* The decoded frame. */
    uint8_t raw[PROTOCOL_MAX_FRAME_LEN];
} protocol_decoder_t;

/* Clears the decoder; call it once before the first protocol_decoder_push(). */
void protocol_decoder_init(protocol_decoder_t *d);

/* Feeds `len` received bytes and calls `cb(frame, ctx)` for every valid frame they complete. Damaged
 * frames are dropped. One decoder per stream; it must not be fed from two tasks at once. */
void protocol_decoder_push(protocol_decoder_t *d, const uint8_t *data, size_t len, protocol_frame_cb cb, void *ctx);

/* Encodes and decodes the frames in test_vectors.h, made by the Rust code. Returns true if every
 * byte matches. */
bool protocol_selftest(void);
