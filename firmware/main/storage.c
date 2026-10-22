#include "storage.h"

#include "esp_log.h"
#include "nvs.h"
#include "nvs_flash.h"

#define NAMESPACE "kvmate"

static const char *TAG = "storage";

void storage_init(void)
{
    esp_err_t err = nvs_flash_init();
    if (err == ESP_ERR_NVS_NO_FREE_PAGES || err == ESP_ERR_NVS_NEW_VERSION_FOUND) {
        nvs_flash_erase();
        err = nvs_flash_init();
    }
    if (err != ESP_OK) {
        ESP_LOGE(TAG, "nvs: %s", esp_err_to_name(err));
    }
}

bool storage_load(const char *key, void *out, size_t len)
{
    nvs_handle_t h;
    if (nvs_open(NAMESPACE, NVS_READONLY, &h) != ESP_OK) {
        return false;
    }
    size_t stored = len;
    bool ok = nvs_get_blob(h, key, out, &stored) == ESP_OK && stored == len;
    nvs_close(h);
    return ok;
}

esp_err_t storage_save(const char *key, const void *data, size_t len)
{
    nvs_handle_t h;
    esp_err_t err = nvs_open(NAMESPACE, NVS_READWRITE, &h);
    if (err != ESP_OK) {
        return err;
    }
    if (data) {
        err = nvs_set_blob(h, key, data, len);
    } else {
        err = nvs_erase_key(h, key);
        if (err == ESP_ERR_NVS_NOT_FOUND) {
            err = ESP_OK;
        }
    }
    if (err == ESP_OK) {
        err = nvs_commit(h);
    }
    nvs_close(h);
    return err;
}
