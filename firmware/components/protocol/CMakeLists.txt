idf_component_register(
    SRCS "codec.c" "secure.c" "test/selftest.c" "test/layout_check.c"
    INCLUDE_DIRS "include"
    REQUIRES mbedtls
)
