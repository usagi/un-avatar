cmake_minimum_required(VERSION 3.20)
project(unavatar_fpng_benchmark LANGUAGES CXX)

add_library(unavatar_fpng_benchmark SHARED
    unavatar_fpng_benchmark.cpp
    third_party/fpng/fpng.cpp)

target_compile_features(unavatar_fpng_benchmark PRIVATE cxx_std_17)
target_include_directories(unavatar_fpng_benchmark PRIVATE third_party/fpng)
target_compile_definitions(unavatar_fpng_benchmark PRIVATE FPNG_NO_STDIO=1)

if(MSVC)
    target_compile_options(unavatar_fpng_benchmark PRIVATE /O2)
else()
    target_compile_options(unavatar_fpng_benchmark PRIVATE -O3 -fno-strict-aliasing)
endif()

set(unavatar_fpng_output_dir "${CMAKE_CURRENT_SOURCE_DIR}/../../Editor/Plugins/x86_64")
set_target_properties(unavatar_fpng_benchmark PROPERTIES
    RUNTIME_OUTPUT_DIRECTORY "${unavatar_fpng_output_dir}"
    LIBRARY_OUTPUT_DIRECTORY "${unavatar_fpng_output_dir}")

foreach(config DEBUG RELEASE RELWITHDEBINFO MINSIZEREL)
    set_target_properties(unavatar_fpng_benchmark PROPERTIES
        RUNTIME_OUTPUT_DIRECTORY_${config} "${unavatar_fpng_output_dir}"
        LIBRARY_OUTPUT_DIRECTORY_${config} "${unavatar_fpng_output_dir}")
endforeach()
