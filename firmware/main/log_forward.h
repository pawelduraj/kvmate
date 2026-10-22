/* Sends the board's log to the hub as Log messages: one line each, without colours or line ends,
 * at most 198 bytes. A separate task sends them, so logging from inside the Wi-Fi code never sends
 * over Wi-Fi itself. A line is dropped if it comes from an interrupt, arrives while another line is
 * being formatted, or finds 16 lines already waiting. */
#pragma once

/* Takes over the board's log. Call once link_send() works. */
void log_forward_init(void);
