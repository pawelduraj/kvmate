cmake_minimum_required(VERSION 3.16)

include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(kvmate-firmware)

idf_component_get_property(tinyusb_lib espressif__tinyusb COMPONENT_LIB)
target_compile_definitions(${tinyusb_lib} PUBLIC
    CFG_TUD_AUDIO=1
    CFG_TUD_AUDIO_ENABLE_EP_OUT=1
    CFG_TUD_AUDIO_ENABLE_EP_IN=1
    CFG_TUD_AUDIO_FUNC_1_EP_OUT_SZ_MAX=196
    CFG_TUD_AUDIO_FUNC_1_EP_IN_SZ_MAX=98
    CFG_TUD_AUDIO_FUNC_1_EP_OUT_SW_BUF_SZ=3920
    CFG_TUD_AUDIO_FUNC_1_EP_IN_SW_BUF_SZ=6144
)
