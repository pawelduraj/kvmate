/* The board's USB headphones and microphone, 48 kHz, 16 bit.
 *
 * Headphones: what the controlled computer plays goes to the hub in 4 ms AudioPlayback messages,
 * and the hub resamples it. The volume (-60..0 dB in 1 dB steps) and mute set on that computer go
 * to the hub in AudioStatus; the hub applies them.
 *
 * Microphone: AudioCapture sound from the hub waits in a USB buffer until the computer reads it.
 * The board reports how full it is in AudioStatus every 20 ms, and the hub uses that to match the
 * computer's clock. The stream starts with 10 ms of silence and gets 2 ms more whenever less than
 * 2 ms is left, so the computer always reads full packets. */
#pragma once

#include <stddef.h>
#include <stdint.h>

/* Starts the audio task. */
void usb_audio_init(void);

/* Queues 16-bit mono PCM for the controlled computer. It is dropped while the computer isn't
 * recording, and so is whatever doesn't fit in the buffer (the hub keeps it far from full). */
void usb_audio_mic_write(const uint8_t *pcm, size_t len);
