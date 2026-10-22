/* The board's BOOT button. A press under 1.5 s sends ButtonEvent SHORT on release: switch the
 * target. Holding it for 1.5 s sends LONG at once: go back to the hub's computer. Presses under
 * 30 ms are ignored. */
#pragma once

/* Starts the task that checks the button every 10 ms. */
void button_init(void);
