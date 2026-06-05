# unavatar_fpng

Optional native plugin used by RAW RGBA PNG encoding and
`Tools/U.N. Avatar/Benchmark PNG Encoders`.

The Unity exporter does not require this plugin for correctness. If the DLL is
missing, export falls back to Unity's built-in PNG encoder. The benchmark records
an `fpng(native)` error row and continues.

Build on Windows from a Visual Studio developer shell:

```powershell
cmake -S unity/un-avatar-unity-exporter/Native/unavatar_fpng -B target/unity-fpng -G "Visual Studio 17 2022" -A x64
cmake --build target/unity-fpng --config Release
```

The build writes `unavatar_fpng.dll` to
`unity/un-avatar-unity-exporter/Editor/Plugins/x86_64/`.

`third_party/fpng/fpng.cpp` and `third_party/fpng/fpng.h` are from
<https://github.com/richgel999/fpng>. fpng is released into the public domain
under the Unlicense text embedded at the end of `fpng.cpp`.
