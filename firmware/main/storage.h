/* Settings kept in NVS, in the "kvmate" namespace. Each one is a fixed-size blob under its own key. */
#pragma once

#include <stdbool.h>
#include <stddef.h>

#include "esp_err.h"

/* Opens NVS, erasing it first if it is full or was written by a newer version. Call before anything
 * reads a setting. */
void storage_init(void);

/* Reads the setting `key` into `out`. Returns false if it isn't stored or has a different size (an
 * older layout). */
bool storage_load(const char *key, void *out, size_t len);

/* Stores `len` bytes of `data` under `key`. NULL `data` erases the setting. */
esp_err_t storage_save(const char *key, const void *data, size_t len);
