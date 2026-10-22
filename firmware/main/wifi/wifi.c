#include "wifi.h"

#include <string.h>

#include "esp_event.h"
#include "esp_log.h"
#include "esp_netif.h"
#include "esp_timer.h"
#include "esp_wifi.h"
#include "freertos/FreeRTOS.h"
#include "freertos/queue.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "link.h"
#include "storage.h"
#include "wifi_udp.h"

static const char *TAG = "wifi";

#define NVS_KEY "wifi"
#define DEFAULT_HOSTNAME "kvmate"
#define FAILED_AFTER 3
#define RETRY_MAX_MS 30000

typedef enum { CMD_APPLY, CMD_SCAN, CMD_RETRY } cmd_t;

static QueueHandle_t s_cmds;

static SemaphoreHandle_t s_lock;
static protocol_wifi_config_t s_cfg;
static protocol_wifi_config_t s_new_cfg;
static uint8_t s_state = PROTOCOL_WIFI_STATE_OFF;
static uint8_t s_fail = PROTOCOL_WIFI_FAIL_NONE;
static uint8_t s_ip[4];
static uint8_t s_channel;
static int s_clients;
static int s_attempts;
static bool s_running;
static protocol_wifi_status_t s_last_reported;

static esp_netif_t *s_sta_netif;
static esp_netif_t *s_ap_netif;
static esp_timer_handle_t s_retry_timer;

static void lock(void)
{
    xSemaphoreTake(s_lock, portMAX_DELAY);
}

static void unlock(void)
{
    xSemaphoreGive(s_lock);
}

static void post(cmd_t cmd)
{
    xQueueSend(s_cmds, &cmd, 0);
}

static void load_config(void)
{
    if (!storage_load(NVS_KEY, &s_cfg, sizeof(s_cfg)) || s_cfg.mode > PROTOCOL_WIFI_MODE_ACCESS_POINT ||
        s_cfg.ssid_len > PROTOCOL_WIFI_SSID_MAX || s_cfg.password_len > PROTOCOL_WIFI_PASSWORD_MAX - 1 ||
        s_cfg.hostname_len > PROTOCOL_WIFI_HOSTNAME_MAX - 1) {
        memset(&s_cfg, 0, sizeof(s_cfg));
    }
}

static const uint8_t *pairing_key(const protocol_wifi_config_t *cfg)
{
    return cfg->mode != PROTOCOL_WIFI_MODE_OFF ? cfg->pairing_key : NULL;
}

static uint8_t fail_reason(uint8_t reason)
{
    switch (reason) {
    case WIFI_REASON_NO_AP_FOUND:
    case WIFI_REASON_NO_AP_FOUND_W_COMPATIBLE_SECURITY:
    case WIFI_REASON_NO_AP_FOUND_IN_AUTHMODE_THRESHOLD:
    case WIFI_REASON_NO_AP_FOUND_IN_RSSI_THRESHOLD:
        return PROTOCOL_WIFI_FAIL_NOT_FOUND;
    case WIFI_REASON_AUTH_FAIL:
    case WIFI_REASON_4WAY_HANDSHAKE_TIMEOUT:
    case WIFI_REASON_HANDSHAKE_TIMEOUT:
    case WIFI_REASON_MIC_FAILURE:
        return PROTOCOL_WIFI_FAIL_AUTH;
    default:
        return PROTOCOL_WIFI_FAIL_OTHER;
    }
}

static void on_event(void *arg, esp_event_base_t base, int32_t id, void *data)
{
    (void)arg;
    lock();
    bool station = s_running && s_cfg.mode == PROTOCOL_WIFI_MODE_STATION;
    if (base == WIFI_EVENT) {
        switch (id) {
        case WIFI_EVENT_STA_START:
            if (station) {
                esp_wifi_connect();
            }
            break;
        case WIFI_EVENT_STA_CONNECTED:
            if (station) {
                s_channel = ((wifi_event_sta_connected_t *)data)->channel;
            }
            break;
        case WIFI_EVENT_STA_DISCONNECTED:
            if (station) {
                s_fail = fail_reason(((wifi_event_sta_disconnected_t *)data)->reason);
                s_attempts++;
                s_state = s_attempts >= FAILED_AFTER ? PROTOCOL_WIFI_STATE_FAILED : PROTOCOL_WIFI_STATE_CONNECTING;
                memset(s_ip, 0, sizeof(s_ip));
                int shift = s_attempts < 5 ? s_attempts : 5;
                uint64_t delay_ms = 1000u << shift;
                if (delay_ms > RETRY_MAX_MS) {
                    delay_ms = RETRY_MAX_MS;
                }
                esp_timer_stop(s_retry_timer);
                esp_timer_start_once(s_retry_timer, delay_ms * 1000);
            }
            break;
        case WIFI_EVENT_AP_START:
            if (s_running && s_cfg.mode == PROTOCOL_WIFI_MODE_ACCESS_POINT) {
                esp_netif_ip_info_t ip;
                if (esp_netif_get_ip_info(s_ap_netif, &ip) == ESP_OK) {
                    memcpy(s_ip, &ip.ip.addr, sizeof(s_ip));
                }
                s_state = PROTOCOL_WIFI_STATE_READY;
            }
            break;
        case WIFI_EVENT_AP_STACONNECTED:
            s_clients++;
            break;
        case WIFI_EVENT_AP_STADISCONNECTED:
            if (s_clients > 0) {
                s_clients--;
            }
            break;
        default:
            break;
        }
    } else if (base == IP_EVENT && id == IP_EVENT_STA_GOT_IP && station) {
        memcpy(s_ip, &((ip_event_got_ip_t *)data)->ip_info.ip.addr, sizeof(s_ip));
        s_attempts = 0;
        s_fail = PROTOCOL_WIFI_FAIL_NONE;
        s_state = PROTOCOL_WIFI_STATE_READY;
    } else if (base == IP_EVENT && id == IP_EVENT_STA_LOST_IP && station) {
        memset(s_ip, 0, sizeof(s_ip));
        s_state = PROTOCOL_WIFI_STATE_CONNECTING;
    }
    unlock();
}

static void on_retry(void *arg)
{
    (void)arg;
    post(CMD_RETRY);
}

static uint16_t scan_now(wifi_ap_record_t *records, uint16_t max)
{
    wifi_scan_config_t cfg = {.show_hidden = false};
    if (esp_wifi_scan_start(&cfg, true) != ESP_OK) {
        esp_wifi_clear_ap_list();
        return 0;
    }
    uint16_t n = max;
    if (esp_wifi_scan_get_ap_records(&n, records) != ESP_OK) {
        return 0;
    }
    return n;
}

static wifi_ap_record_t s_records[32];

static uint8_t pick_channel(void)
{
    esp_wifi_set_mode(WIFI_MODE_STA);
    if (esp_wifi_start() != ESP_OK) {
        return 6;
    }
    uint16_t n = scan_now(s_records, sizeof(s_records) / sizeof(s_records[0]));
    esp_wifi_stop();
    static const uint8_t candidates[] = {1, 6, 11};
    uint8_t best = 6;
    int best_score = -1;
    for (size_t c = 0; c < sizeof(candidates); c++) {
        int score = 0;
        for (uint16_t i = 0; i < n; i++) {
            int distance = s_records[i].primary - candidates[c];
            if (distance > -5 && distance < 5) {
                score += 100 + s_records[i].rssi;
            }
        }
        if (best_score < 0 || score < best_score) {
            best_score = score;
            best = candidates[c];
        }
    }
    return best;
}

static void stop_wifi(void)
{
    lock();
    bool running = s_running;
    s_running = false;
    s_state = PROTOCOL_WIFI_STATE_OFF;
    s_clients = 0;
    s_attempts = 0;
    memset(s_ip, 0, sizeof(s_ip));
    unlock();
    esp_timer_stop(s_retry_timer);
    if (running) {
        esp_wifi_stop();
    }
}

static void start_wifi(void)
{
    lock();
    protocol_wifi_config_t cfg = s_cfg;
    unlock();
    if (cfg.mode == PROTOCOL_WIFI_MODE_OFF) {
        return;
    }
    wifi_config_t wc;
    memset(&wc, 0, sizeof(wc));
    uint8_t channel = 0;
    if (cfg.mode == PROTOCOL_WIFI_MODE_STATION) {
        memcpy(wc.sta.ssid, cfg.ssid, cfg.ssid_len);
        memcpy(wc.sta.password, cfg.password, cfg.password_len);
        wc.sta.threshold.authmode = cfg.password_len > 0 ? WIFI_AUTH_WPA2_PSK : WIFI_AUTH_OPEN;
        wc.sta.sae_pwe_h2e = WPA3_SAE_PWE_BOTH;
        wc.sta.scan_method = WIFI_ALL_CHANNEL_SCAN;
        wc.sta.sort_method = WIFI_CONNECT_AP_BY_SIGNAL;
        esp_wifi_set_mode(WIFI_MODE_STA);
        esp_wifi_set_config(WIFI_IF_STA, &wc);
        char hostname[PROTOCOL_WIFI_HOSTNAME_MAX];
        if (cfg.hostname_len > 0) {
            memcpy(hostname, cfg.hostname, cfg.hostname_len);
            hostname[cfg.hostname_len] = 0;
        } else {
            strcpy(hostname, DEFAULT_HOSTNAME);
        }
        esp_netif_set_hostname(s_sta_netif, hostname);
    } else {
        channel = cfg.ap_channel >= 1 && cfg.ap_channel <= 13 ? cfg.ap_channel : pick_channel();
        memcpy(wc.ap.ssid, cfg.ssid, cfg.ssid_len);
        wc.ap.ssid_len = cfg.ssid_len;
        memcpy(wc.ap.password, cfg.password, cfg.password_len);
        wc.ap.channel = channel;
        wc.ap.authmode = WIFI_AUTH_WPA2_PSK;
        wc.ap.max_connection = 1;
        esp_wifi_set_mode(WIFI_MODE_AP);
        esp_wifi_set_config(WIFI_IF_AP, &wc);
        esp_netif_dhcps_stop(s_ap_netif);
        uint8_t no_router = 0;
        esp_netif_dhcps_option(s_ap_netif, ESP_NETIF_OP_SET, ESP_NETIF_ROUTER_SOLICITATION_ADDRESS, &no_router,
                               sizeof(no_router));
        esp_netif_dhcps_start(s_ap_netif);
    }
    lock();
    s_running = true;
    s_state = PROTOCOL_WIFI_STATE_CONNECTING;
    s_fail = PROTOCOL_WIFI_FAIL_NONE;
    s_channel = channel;
    unlock();
    esp_err_t err = esp_wifi_start();
    esp_wifi_set_ps(WIFI_PS_NONE);
    if (err != ESP_OK) {
        lock();
        s_state = PROTOCOL_WIFI_STATE_FAILED;
        s_fail = PROTOCOL_WIFI_FAIL_OTHER;
        unlock();
        ESP_LOGE(TAG, "Wi-Fi start failed: %s", esp_err_to_name(err));
        return;
    }
    if (cfg.mode == PROTOCOL_WIFI_MODE_STATION) {
        ESP_LOGI(TAG, "joining network \"%.*s\"", cfg.ssid_len, (const char *)cfg.ssid);
    } else {
        ESP_LOGI(TAG, "own network \"%.*s\" on channel %d", cfg.ssid_len, (const char *)cfg.ssid, channel);
    }
}

static void apply_new(void)
{
    lock();
    protocol_wifi_config_t cfg = s_new_cfg;
    unlock();

    esp_err_t err = storage_save(NVS_KEY, cfg.mode == PROTOCOL_WIFI_MODE_OFF ? NULL : &cfg, sizeof(cfg));
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "cannot store Wi-Fi settings: %s", esp_err_to_name(err));
        return;
    }
    stop_wifi();
    lock();
    s_cfg = cfg;
    unlock();
    wifi_udp_set_key(pairing_key(&cfg));
    if (cfg.mode == PROTOCOL_WIFI_MODE_OFF) {
        ESP_LOGI(TAG, "Wi-Fi settings erased");
    }
    start_wifi();
}

static uint8_t auth_kind(wifi_auth_mode_t mode)
{
    switch (mode) {
    case WIFI_AUTH_OPEN:
    case WIFI_AUTH_OWE:
        return PROTOCOL_WIFI_AUTH_OPEN;
    case WIFI_AUTH_ENTERPRISE:
    case WIFI_AUTH_WPA3_ENT_192:
    case WIFI_AUTH_WPA3_ENTERPRISE:
    case WIFI_AUTH_WPA2_WPA3_ENTERPRISE:
    case WIFI_AUTH_WPA_ENTERPRISE:
        return PROTOCOL_WIFI_AUTH_ENTERPRISE;
    default:
        return PROTOCOL_WIFI_AUTH_PERSONAL;
    }
}

static void do_scan(void)
{
    lock();
    uint8_t mode = s_running ? s_cfg.mode : PROTOCOL_WIFI_MODE_OFF;
    unlock();

    if (mode == PROTOCOL_WIFI_MODE_OFF) {
        esp_wifi_set_mode(WIFI_MODE_STA);
        esp_wifi_start();
    } else if (mode == PROTOCOL_WIFI_MODE_ACCESS_POINT) {
        esp_wifi_set_mode(WIFI_MODE_APSTA);
    }
    uint16_t n = scan_now(s_records, sizeof(s_records) / sizeof(s_records[0]));
    if (n == 0 && mode == PROTOCOL_WIFI_MODE_STATION) {
        vTaskDelay(pdMS_TO_TICKS(1500));
        n = scan_now(s_records, sizeof(s_records) / sizeof(s_records[0]));
    }
    if (mode == PROTOCOL_WIFI_MODE_OFF) {
        esp_wifi_stop();
    } else if (mode == PROTOCOL_WIFI_MODE_ACCESS_POINT) {
        esp_wifi_set_mode(WIFI_MODE_AP);
    }

    static protocol_wifi_network_t nets[PROTOCOL_WIFI_SCAN_MAX];
    size_t count = 0;
    for (;;) {
        int best = -1;
        for (uint16_t i = 0; i < n; i++) {
            if (s_records[i].ssid[0] != 0 && (best < 0 || s_records[i].rssi > s_records[best].rssi)) {
                best = i;
            }
        }
        if (best < 0 || count >= PROTOCOL_WIFI_SCAN_MAX) {
            break;
        }
        wifi_ap_record_t *r = &s_records[best];
        size_t len = strnlen((const char *)r->ssid, sizeof(r->ssid));
        bool seen = false;
        for (size_t j = 0; j < count && !seen; j++) {
            seen = nets[j].ssid_len == len && memcmp(nets[j].ssid, r->ssid, len) == 0;
        }
        if (!seen && len <= PROTOCOL_WIFI_SSID_MAX) {
            protocol_wifi_network_t *net = &nets[count++];
            memset(net, 0, sizeof(*net));
            net->rssi_dbm = r->rssi;
            net->channel = r->primary;
            net->auth = auth_kind(r->authmode);
            net->ssid_len = (uint8_t)len;
            memcpy(net->ssid, r->ssid, len);
        }
        r->ssid[0] = 0;
    }
    protocol_wifi_scan_header_t head = {.network_count = (uint8_t)count};
    protocol_part_t parts[] = {{&head, sizeof(head)}, {nets, count * sizeof(protocol_wifi_network_t)}};
    link_send(PROTOCOL_MSG_TYPE_WIFI_SCAN_RESULT, parts, 2);
}

static void ctl_task(void *arg)
{
    (void)arg;
    start_wifi();
    for (;;) {
        cmd_t cmd;
        if (xQueueReceive(s_cmds, &cmd, portMAX_DELAY) != pdTRUE) {
            continue;
        }
        switch (cmd) {
        case CMD_APPLY:
            apply_new();
            break;
        case CMD_SCAN:
            do_scan();
            break;
        case CMD_RETRY: {
            lock();
            bool station = s_running && s_cfg.mode == PROTOCOL_WIFI_MODE_STATION;
            unlock();
            if (station) {
                esp_wifi_connect();
            }
            break;
        }
        }
    }
}

void wifi_apply(const protocol_wifi_config_t *cfg)
{
    lock();
    s_new_cfg = *cfg;
    if (s_new_cfg.ssid_len > PROTOCOL_WIFI_SSID_MAX) {
        s_new_cfg.ssid_len = PROTOCOL_WIFI_SSID_MAX;
    }
    if (s_new_cfg.password_len > PROTOCOL_WIFI_PASSWORD_MAX - 1) {
        s_new_cfg.password_len = PROTOCOL_WIFI_PASSWORD_MAX - 1;
    }
    if (s_new_cfg.hostname_len > PROTOCOL_WIFI_HOSTNAME_MAX - 1) {
        s_new_cfg.hostname_len = PROTOCOL_WIFI_HOSTNAME_MAX - 1;
    }
    if (s_new_cfg.mode > PROTOCOL_WIFI_MODE_ACCESS_POINT) {
        s_new_cfg.mode = PROTOCOL_WIFI_MODE_OFF;
    }
    if (s_new_cfg.mode == PROTOCOL_WIFI_MODE_OFF) {
        memset(&s_new_cfg, 0, sizeof(s_new_cfg));
    }
    unlock();
    post(CMD_APPLY);
}

void wifi_scan(void)
{
    post(CMD_SCAN);
}

bool wifi_status(protocol_wifi_status_t *out)
{
    memset(out, 0, sizeof(*out));
    lock();
    out->mode = s_cfg.mode;
    out->state = s_state;
    out->fail_reason = s_fail;
    out->channel = s_channel;
    memcpy(out->ip_address, s_ip, sizeof(out->ip_address));
    out->ssid_len = s_cfg.ssid_len;
    memcpy(out->ssid, s_cfg.ssid, s_cfg.ssid_len);
    if (s_clients > 0) {
        out->flags |= PROTOCOL_WIFI_FLAG_CLIENT;
    }
    bool station_ready = s_running && s_cfg.mode == PROTOCOL_WIFI_MODE_STATION && s_state == PROTOCOL_WIFI_STATE_READY;
    bool ap_client = s_running && s_cfg.mode == PROTOCOL_WIFI_MODE_ACCESS_POINT && s_clients > 0;
    unlock();
    if (wifi_hub_alive()) {
        out->flags |= PROTOCOL_WIFI_FLAG_HUB_SESSION;
    }
    if (station_ready) {
        wifi_ap_record_t ap;
        if (esp_wifi_sta_get_ap_info(&ap) == ESP_OK) {
            out->rssi_dbm = ap.rssi;
        }
    } else if (ap_client) {
        wifi_sta_list_t list;
        if (esp_wifi_ap_get_sta_list(&list) == ESP_OK && list.num > 0) {
            out->rssi_dbm = list.sta[0].rssi;
        }
    }
    protocol_wifi_status_t compare = *out;
    compare.rssi_dbm = s_last_reported.rssi_dbm;
    bool changed = memcmp(&compare, &s_last_reported, sizeof(compare)) != 0;
    s_last_reported = *out;
    return changed;
}

bool wifi_failed(void)
{
    return s_cfg.mode != PROTOCOL_WIFI_MODE_OFF && s_state == PROTOCOL_WIFI_STATE_FAILED;
}

void wifi_init(wifi_rx_fn on_rx)
{
    s_lock = xSemaphoreCreateMutex();
    s_cmds = xQueueCreate(8, sizeof(cmd_t));
    load_config();

    esp_log_level_set("wifi", ESP_LOG_WARN);
    esp_log_level_set("wifi_init", ESP_LOG_WARN);
    esp_log_level_set("phy_init", ESP_LOG_WARN);
    esp_log_level_set("esp_netif_handlers", ESP_LOG_WARN);
    esp_log_level_set("net80211", ESP_LOG_WARN);
    esp_log_level_set("pp", ESP_LOG_WARN);

    ESP_ERROR_CHECK(esp_netif_init());
    ESP_ERROR_CHECK(esp_event_loop_create_default());
    s_sta_netif = esp_netif_create_default_wifi_sta();
    s_ap_netif = esp_netif_create_default_wifi_ap();
    wifi_init_config_t init = WIFI_INIT_CONFIG_DEFAULT();
    ESP_ERROR_CHECK(esp_wifi_init(&init));
    esp_wifi_set_storage(WIFI_STORAGE_RAM);
    esp_event_handler_instance_register(WIFI_EVENT, ESP_EVENT_ANY_ID, on_event, NULL, NULL);
    esp_event_handler_instance_register(IP_EVENT, IP_EVENT_STA_GOT_IP, on_event, NULL, NULL);
    esp_event_handler_instance_register(IP_EVENT, IP_EVENT_STA_LOST_IP, on_event, NULL, NULL);
    const esp_timer_create_args_t retry = {.callback = on_retry, .name = "wifi_retry"};
    esp_timer_create(&retry, &s_retry_timer);

    wifi_udp_init(on_rx, pairing_key(&s_cfg));
    xTaskCreate(ctl_task, "wifi_ctl", 4096, NULL, 6, NULL);
}
