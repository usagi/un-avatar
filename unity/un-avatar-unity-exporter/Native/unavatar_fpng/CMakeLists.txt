cmake_minimum_required(VERSION 3.20)
project(unavatar_fpng LANGUAGES CXX)

add_library(unavatar_fpng SHARED
    unavatar_fpng.cpp
    third_party/fpng/fpng.cpp)

target_compile_features(unavatar_fpng PRIVATE cxx_std_17)
target_include_directories(unavatar_fpng PRIVATE third_party/fpng)
target_compile_definitions(unavatar_fpng PRIVATE FPNG_NO_STDIO=1)

if(MSVC)
    target_compile_options(unavatar_fpng PRIVATE /O2)
else()
    target_compile_options(unavatar_fpng PRIVATE -O3 -fno-strict-aliasing)
endif()

set(UNAVATAR_FPNG_OUTPUT_DIR
    "${CMAKE_CURRENT_SOURCE_DIR}/../../Editor/Plugins/x86_64"
    CACHE PATH "Output directory for the Unity fpng native plugin")
set(unavatar_fpng_output_dir "${UNAVATAR_FPNG_OUTPUT_DIR}")
set_target_properties(unavatar_fpng PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${unavatar_fpng_output_dir}"
    LIBRARY_OUTPUT_DIRECTORY "${unavatar_fpng_output_dir}")

foreach(config DEBUG RELEASE RELWITHDEBINFO MINSIZEREL)
    set_target_properties(unavatar_fpng PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY_${config} "${unavatar_fpng_output_dir}"
        LIBRARY_OUTPUT_DIRECTORY_${config} "${unavatar_fpng_output_dir}")
endforeach()
