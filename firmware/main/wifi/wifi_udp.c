#include "wifi_udp.h"

#include <string.h>

#include "esp_log.h"
#include "esp_random.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "lwip/sockets.h"
#include "secure.h"

static const char *TAG = "wifi";

static SemaphoreHandle_t s_lock;
static int s_sock = -1;
static wifi_rx_fn s_on_rx;
static uint8_t s_key[PROTOCOL_PAIRING_KEY_LEN];
static uint8_t s_key_id[8];
static bool s_has_key;
static protocol_secure_session_t s_sessions[2];
static int s_active = -1;
static int s_pending = -1;
static struct sockaddr_in s_peer;
static volatile int64_t s_last_rx_us;
static uint8_t s_tx_dgram[PROTOCOL_DGRAM_MAX_LEN];

static void handle_hello(const uint8_t *dgram, const struct sockaddr_in *from)
{
    protocol_dgram_hello_t hello;
    memcpy(&hello, dgram, sizeof(hello));
    if (hello.kind != PROTOCOL_DGRAM_HELLO || hello.version != PROTOCOL_DGRAM_VERSION) {
        return;
    }
    protocol_dgram_reply_t reply = {.magic = PROTOCOL_DGRAM_MAGIC, .kind = PROTOCOL_DGRAM_REPLY};
    xSemaphoreTake(s_lock, portMAX_DELAY);
    if (!s_has_key || !protocol_secure_equal(hello.key_id, s_key_id, sizeof(s_key_id))) {
        xSemaphoreGive(s_lock);
        return;
    }
    int slot = -1;
    for (int i = 0; i < 2; i++) {
        if (s_sessions[i].valid && (i == s_active || i == s_pending) &&
            memcmp(s_sessions[i].hub_nonce, hello.hub_nonce, PROTOCOL_DGRAM_NONCE_LEN) == 0) {
            slot = i;
        }
    }
    if (slot < 0) {
        slot = s_active == 0 ? 1 : 0;
        uint8_t board_nonce[PROTOCOL_DGRAM_NONCE_LEN];
        esp_fill_random(board_nonce, sizeof(board_nonce));
        protocol_secure_session_start(&s_sessions[slot], s_key, hello.hub_nonce, board_nonce);
        s_pending = slot;
    }
    memcpy(reply.hub_nonce, hello.hub_nonce, PROTOCOL_DGRAM_NONCE_LEN);
    memcpy(reply.board_nonce, s_sessions[slot].board_nonce, PROTOCOL_DGRAM_NONCE_LEN);
    protocol_secure_reply_mac(s_key, reply.hub_nonce, reply.board_nonce, reply.reply_mac);
    xSemaphoreGive(s_lock);
    sendto(s_sock, &reply, sizeof(reply), 0, (const struct sockaddr *)from, sizeof(*from));
}

static void udp_task(void *arg)
{
    (void)arg;
    static uint8_t dgram[PROTOCOL_DGRAM_MAX_LEN + 64];
    static uint8_t plain[PROTOCOL_DGRAM_MAX_PLAIN];
    for (;;) {
        struct sockaddr_in from;
        socklen_t from_len = sizeof(from);
        int n = recvfrom(s_sock, dgram, sizeof(dgram), 0, (struct sockaddr *)&from, &from_len);
        if (n <= 0) {
            vTaskDelay(pdMS_TO_TICKS(10));
            continue;
        }
        if (dgram[0] != PROTOCOL_DGRAM_DATA) {
            uint32_t magic;
            memcpy(&magic, dgram, sizeof(magic));
            if (n >= (int)sizeof(protocol_dgram_hello_t) && magic == PROTOCOL_DGRAM_MAGIC) {
                handle_hello(dgram, &from);
            }
            continue;
        }
        uint32_t session = protocol_secure_data_session(dgram, n);
        bool switched = false;
        int len = -1;
        xSemaphoreTake(s_lock, portMAX_DELAY);
        int slot = -1;
        if (s_active >= 0 && s_sessions[s_active].id == session) {
            slot = s_active;
        } else if (s_pending >= 0 && s_sessions[s_pending].id == session) {
            slot = s_pending;
        }
        if (slot >= 0) {
            len = protocol_secure_open(&s_sessions[slot], dgram, n, plain);
        }
        if (len >= 0) {
            if (slot == s_pending) {
                if (s_active >= 0) {
                    protocol_secure_session_end(&s_sessions[s_active]);
                }
                s_active = slot;
                s_pending = -1;
                switched = true;
            }
            s_peer = from;
            s_last_rx_us = esp_timer_get_time();
        }
        xSemaphoreGive(s_lock);
        if (switched) {
            char ip[16];
            inet_ntoa_r(from.sin_addr, ip, sizeof(ip));
            ESP_LOGI(TAG, "encrypted session with the hub at %s", ip);
        }
        if (len > 0) {
            s_on_rx(plain, (size_t)len);
        }
    }
}

bool wifi_hub_alive(void)
{
    return s_active >= 0 && esp_timer_get_time() - s_last_rx_us < (int64_t)PROTOCOL_LINK_TIMEOUT_MS * 1000;
}

bool wifi_send(const uint8_t *data, size_t len)
{
    if (s_sock < 0) {
        return false;
    }
    xSemaphoreTake(s_lock, portMAX_DELAY);
    bool sent = false;
    if (s_active >= 0) {
        size_t n = protocol_secure_seal(&s_sessions[s_active], data, len, s_tx_dgram, sizeof(s_tx_dgram));
        if (n > 0) {
            sent = sendto(s_sock, s_tx_dgram, n, 0, (const struct sockaddr *)&s_peer, sizeof(s_peer)) == (int)n;
        }
    }
    xSemaphoreGive(s_lock);
    return sent;
}

void wifi_udp_set_key(const uint8_t *key)
{
    xSemaphoreTake(s_lock, portMAX_DELAY);
    protocol_secure_session_end(&s_sessions[0]);
    protocol_secure_session_end(&s_sessions[1]);
    s_active = -1;
    s_pending = -1;
    s_has_key = key != NULL;
    if (key) {
        memcpy(s_key, key, sizeof(s_key));
    } else {
        memset(s_key, 0, sizeof(s_key));
    }
    protocol_secure_key_id(s_key, s_key_id);
    xSemaphoreGive(s_lock);
}

void wifi_udp_init(wifi_rx_fn on_rx, const uint8_t *key)
{
    s_on_rx = on_rx;
    s_lock = xSemaphoreCreateMutex();
    wifi_udp_set_key(key);

    s_sock = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    struct sockaddr_in addr = {
        .sin_family = AF_INET,
        .sin_port = htons(PROTOCOL_WIFI_UDP_PORT),
        .sin_addr.s_addr = htonl(INADDR_ANY),
    };
    int yes = 1;
    setsockopt(s_sock, SOL_SOCKET, SO_BROADCAST, &yes, sizeof(yes));
    if (s_sock < 0 || bind(s_sock, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
        ESP_LOGE(TAG, "UDP port %d unavailable", PROTOCOL_WIFI_UDP_PORT);
        if (s_sock >= 0) {
            close(s_sock);
        }
        s_sock = -1;
    } else {
        xTaskCreate(udp_task, "wifi_udp", 4096, NULL, 11, NULL);
    }
}
