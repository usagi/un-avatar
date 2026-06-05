#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <vector>

#include "fpng.h"

#if defined(_WIN32)
#define UNAVATAR_FPNG_EXPORT extern "C" __declspec(dllexport)
#else
#define UNAVATAR_FPNG_EXPORT extern "C" __attribute__((visibility("default")))
#endif

namespace
{
std::once_flag g_fpngInitOnce;
}

UNAVATAR_FPNG_EXPORT int unavatar_fpng_encode_rgba32(
    const void* rgba,
    int width,
    int height,
    uint8_t** png,
    int* png_size)
{
    if (!rgba || !png || !png_size || width <= 0 || height <= 0)
    {
        return 1;
    }

    *png = nullptr;
    *png_size = 0;
    std::call_once(g_fpngInitOnce, []() { fpng::fpng_init(); });

    std::vector<uint8_t> encoded;
    if (!fpng::fpng_encode_image_to_memory(
            rgba,
            static_cast<uint32_t>(width),
            static_cast<uint32_t>(height),
            4,
            encoded,
            0))
    {
        return 2;
    }
    if (encoded.empty() || encoded.size() > static_cast<size_t>(INT32_MAX))
    {
        return 3;
    }

    auto* out = static_cast<uint8_t*>(std::malloc(encoded.size()));
    if (!out)
    {
        return 4;
    }
    std::memcpy(out, encoded.data(), encoded.size());
    *png = out;
    *png_size = static_cast<int>(encoded.size());
    return 0;
}

UNAVATAR_FPNG_EXPORT void unavatar_fpng_free(void* png)
{
    std::free(png);
}
