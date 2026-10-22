#include "log_forward.h"

#include <stdarg.h>
#include <stdio.h>
#include <string.h>

#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include "link.h"

typedef struct {
    uint8_t level;
    uint8_t len;
    char text[198];
} log_line_t;

static QueueHandle_t s_log_queue;

static void log_task(void *arg)
{
    (void)arg;
    log_line_t line;
    for (;;) {
        if (xQueueReceive(s_log_queue, &line, portMAX_DELAY) == pdTRUE) {
            protocol_log_header_t head = {.level = line.level};
            protocol_part_t parts[] = {{&head, sizeof(head)}, {line.text, line.len}};
            link_send(PROTOCOL_MSG_TYPE_LOG, parts, 2);
        }
    }
}

static int log_vprintf(const char *fmt, va_list args)
{
    static __thread bool in_log;
    if (in_log || xPortInIsrContext() || s_log_queue == NULL) {
        return 0;
    }
    in_log = true;

    char text[200];
    int len = vsnprintf(text, sizeof(text), fmt, args);
    if (len > (int)sizeof(text) - 1) {
        len = sizeof(text) - 1;
    }
    char clean[200];
    int out = 0;
    for (int i = 0; i < len; i++) {
        if (text[i] == '\033') {
            while (i < len && text[i] != 'm') {
                i++;
            }
            continue;
        }
        if (text[i] != '\n' && text[i] != '\r') {
            clean[out++] = text[i];
        }
    }

    log_line_t line = {.level = PROTOCOL_LOG_INFO};
    switch (out > 0 ? clean[0] : 'I') {
    case 'E': line.level = PROTOCOL_LOG_ERROR; break;
    case 'W': line.level = PROTOCOL_LOG_WARN; break;
    case 'D':
    case 'V': line.level = PROTOCOL_LOG_DEBUG; break;
    default: break;
    }
    if (out > (int)sizeof(line.text)) {
        out = sizeof(line.text);
    }
    if (out > 0) {
        line.len = (uint8_t)out;
        memcpy(line.text, clean, out);
        xQueueSend(s_log_queue, &line, 0);
    }
    in_log = false;
    return len;
}

void log_forward_init(void)
{
    s_log_queue = xQueueCreate(16, sizeof(log_line_t));
    xTaskCreate(log_task, "link_log", 3072, NULL, 4, NULL);
    esp_log_set_vprintf(log_vprintf);
}
