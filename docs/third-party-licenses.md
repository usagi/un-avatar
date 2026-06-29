# Third-Party Licenses

U.N. Avatar の配布物に同梱する第三者コンポーネント、および互換性検証のために参照する第三者プロジェクトのライセンス表示をまとめる。リリースパッケージでは、この文書または同等内容を `LICENSES/` に含める。

## Reference Implementations

U.N. Avatar の toon rendering / UNToon / Wardrobe 実装は Rust / wgpu / WGSL / Unity Exporter で独立実装する。ただし、互換性検証、shader behavior、avatar assembly behavior の理解、必要な範囲での移植にあたり、次の MIT licensed project を参考実装として扱う。

### lilToon

- Project: lilToon
- Repository: <https://github.com/lilxyzw/lilToon>
- Use in UN Avatar: UNToon v2 / lilToon-compatible material and shader behavior reference
- License: MIT License
- License URL: <https://github.com/lilxyzw/lilToon/blob/master/LICENSE>

### MToon

- Project: MToon
- Repository: <https://github.com/Santarh/MToon>
- Use in UN Avatar: VRM / MToon material compatibility reference
- License: MIT License
- License URL: <https://github.com/Santarh/MToon/blob/master/LICENSE>

### Modular Avatar

- Project: Modular Avatar
- Repository: <https://github.com/bdunderscore/modular-avatar>
- Use in UN Avatar: `.unavatar` Wardrobe / MergeArmature / BoneProxy / ObjectToggle / expression-menu metadata assembly reference
- License: MIT License for all files except official `Editor/images` assets.
- Note: Upstream `COPYING.md` restricts redistribution of `Editor/images` to official Modular Avatar packages only; modified redistributions should replace or remove those assets.
- Copyright: Copyright (c) 2022 bd_
- License URL: <https://github.com/bdunderscore/modular-avatar/blob/main/COPYING.md>

現時点では、これらの project 名は互換性の説明と謝辞を目的とする。将来、実質的な source code の移植または substantial portions の取り込みを行う場合は、該当 copyright notice と MIT License text を配布物の `LICENSES/` に保持する。

## Unity Exporter Native PNG Encoding

### fpng

- Project: fpng
- Repository: <https://github.com/richgel999/fpng>
- Use in UN Avatar: Unity Editor Exporter RAW RGBA -> PNG fast encoder (`unavatar_fpng.dll`)
- License: Public domain / Unlicense

```text
This is free and unencumbered software released into the public domain.

Anyone is free to copy, modify, publish, use, compile, sell, or
distribute this software, either in source code form or as a compiled
binary, for any purpose, commercial or non-commercial, and by any
means.

In jurisdictions that recognize copyright laws, the author or authors
of this software dedicate any and all copyright interest in the
software to the public domain. We make this dedication for the benefit
of the public at large and to the detriment of our heirs and
successors. We intend this dedication to be an overt act of
relinquishment in perpetuity of all present and future rights to this
software under copyright law.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY CLAIM, DAMAGES OR
OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
OTHER DEALINGS IN THE SOFTWARE.

For more information, please refer to <http://unlicense.org/>

Richard Geldreich, Jr.
12/30/2021
```

## Spout2

- Project: Spout2
- Repository: <https://github.com/leadedge/Spout2>
- Use in UN Avatar: Windows の Spout2 出力用 `Spout.dll` / SDK
- License: BSD 2-Clause License

```text
BSD 2-Clause License

Copyright (c) 2020-2024, Lynn Jarvis
All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```
