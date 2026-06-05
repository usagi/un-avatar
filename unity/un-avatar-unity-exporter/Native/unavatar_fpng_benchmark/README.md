# unavatar_fpng_benchmark

Optional native plugin used only by `Tools/U.N. Avatar/Benchmark PNG Encoders`.

The Unity exporter does not require this plugin for normal export. If the DLL is
missing, the benchmark records an `fpng(native)` error row and continues.

Build on Windows from a Visual Studio developer shell:

```powershell
cmake -S unity/un-avatar-unity-exporter/Native/unavatar_fpng_benchmark -B target/unity-fpng-benchmark -G "Visual Studio 17 2022" -A x64
cmake --build target/unity-fpng-benchmark --config Release
```

The build writes `unavatar_fpng_benchmark.dll` to
`unity/un-avatar-unity-exporter/Editor/Plugins/x86_64/`.

`third_party/fpng/fpng.cpp` and `third_party/fpng/fpng.h` are from
<https://github.com/richgel999/fpng>. fpng is released into the public domain
under the Unlicense text embedded at the end of `fpng.cpp`.
