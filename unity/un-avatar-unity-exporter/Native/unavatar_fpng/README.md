# unavatar_fpng

Optional native plugin used by RAW RGBA PNG encoding and
`Tools/U.N. Avatar/Benchmark PNG Encoders`.

The Unity exporter does not require this plugin for correctness. If the DLL is
missing, export falls back to Unity's built-in PNG encoder. The benchmark records
an `fpng(native)` error row and continues.

The release path is:

```powershell
cargo xtask unity-exporter-package
```

`xtask` builds this native plugin into `target/unity-fpng/plugin/`, then copies
the platform native library (`unavatar_fpng.dll` on Windows) and its Unity plugin
`.meta` into both:

- the development local package at
  `unity/un-avatar-unity-exporter/Editor/Plugins/x86_64/`
- the staged package under
  `target/unity/un-avatar-unity-exporter/Editor/Plugins/x86_64/`

The development copy is ignored by git.

Windows Unity Editor / VCC usage is the validated path. The build helper keeps
library-name handling for macOS/Linux, but those Editor package importer settings
are not treated as release-ready until tested on those platforms.

For only refreshing the development native plugin:

```powershell
cargo xtask unity-fpng
```

If Unity Editor is running and has loaded the existing DLL, Windows may lock the
development copy. In that case, close Unity and run `cargo xtask unity-fpng`
again. `cargo xtask unity-exporter-package` still stages the release package even
if the development copy cannot be overwritten.

Manual build on Windows from a Visual Studio developer shell:

```powershell
cmake -S unity/un-avatar-unity-exporter/Native/unavatar_fpng -B target/unity-fpng/build -G "Visual Studio 17 2022" -A x64 -DUNAVATAR_FPNG_OUTPUT_DIR=target/unity-fpng/plugin
cmake --build target/unity-fpng/build --config Release
```

`xtask` defaults to the Visual Studio 2022 x64 CMake generator on Windows.
Override with `UN_AVATAR_CMAKE_GENERATOR` and `UN_AVATAR_CMAKE_ARCH` when needed.

`third_party/fpng/fpng.cpp` and `third_party/fpng/fpng.h` are from
<https://github.com/richgel999/fpng>. fpng is released into the public domain
under the Unlicense text embedded at the end of `fpng.cpp`.
