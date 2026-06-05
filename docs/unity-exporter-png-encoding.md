# Unity Exporter PNG Encoding Policy

作成日: 2026-06-05

この文書は Unity Editor Exporter が RAW RGBA を PNG 化せざるを得ない場合の encoder 方針を固定する。既存 asset の PNG / JPEG source bytes を再エンコードする方針ではない。

## Decision

Unity Exporter は、source bytes を取得できる texture asset では元 binary を優先する。PNG / JPEG asset を PNG として再エンコードしない。これは品質劣化、容量増加、処理時間増加、検証範囲拡大を避けるためである。

RAW RGBA が exporter 内で生成された場合だけ、PNG encoder policy を適用する。v0.1 では高速 encoder 候補として native `fpng` shim を使えるようにする。Unity 内蔵 PNG encoder は compact output が必要な場合と native plugin が使えない場合の fallback とする。

## Why Native FFI

Unity の `Texture2D.EncodeToPNG()` / `ImageConversion.EncodeArrayToPNG()` は使いやすいが、large preview や generated texture fallback では export time の支配的コストになりやすい。特に wardrobe preview、cubemap strip、latlong / generated texture などは GPU readback 後に RAW RGBA を PNG 化するため、source preservation で回避できない。

`fpng` は C++ single-library encoder で、Unity C# からは native plugin FFI が必要になる。FFI を使う理由は次の通り。

- RAW RGBA -> PNG の encode time が Unity 内蔵 encoder より大幅に短い。
- Exporter の処理高速化が目的であり、PNG/JPEG source asset を再処理する用途ではない。
- Native boundary は `Raw RGBA input -> PNG bytes output` に限定でき、Unity / Runtime の texture policy を汚さない。
- Benchmark menu で decode verification を行い、encoder output が元 RGBA と一致することを採用条件にできる。

## Scope

fpng の適用候補は、Exporter が新しく生成した RAW RGBA だけである。

| Scope | Current path | fpng policy |
| --- | --- | --- |
| Wardrobe preview/sample images | `RenderTexture` -> `ReadPixels` -> `EncodeToPNG()` | Use fast RAW RGBA PNG encoder |
| Texture fallback image | `Graphics.Blit` -> `ReadPixels` -> `EncodeToPNG()` | Use fast RAW RGBA PNG encoder when output is PNG |
| Cubemap horizontal strip fallback | 6 faces -> RGBA strip -> `EncodeToPNG()` | Use fast RAW RGBA PNG encoder when not EXR |
| Generated latlong / cube derived PNG | generated RAW RGBA -> PNG | Use fast RAW RGBA PNG encoder |
| Diagnostics / temporary preview output | generated RAW RGBA -> PNG | Use fast RAW RGBA PNG encoder |

Non-scope:

- Asset-backed `.png`, `.jpg`, `.jpeg`: preserve source bytes.
- Asset-backed `.exr`, `.hdr`, `.ktx2`, `.dds`: preserve source bytes as `UN_avatar.textureAssets` when possible.
- Optimizer compression, texture resize, WebP / KTX2 / BCn conversion: external optimizer responsibility.
- Final package texture recompression for size reduction: not Unity Exporter v0.1 responsibility.

## Benchmark Reference

Environment: Unity Editor on Windows, benchmark menu `Tools > U.N. Avatar > Benchmark PNG Encoders`, 8 timed iterations after 2 warmups. Values are p50 milliseconds from `un-avatar-png-encoder-benchmark.csv`. Each output row was decoded and compared against the original RGBA input; all rows below had `decode_matches=true`.

| Case | Texture2D.EncodeToPNG | ImageConversion main | ImageConversion worker | fpng native | fpng size note |
| --- | ---: | ---: | ---: | ---: | --- |
| 512 gradient | 5.176 ms | 5.090 ms | 5.126 ms | 1.326 ms | 15,068 B -> 238,392 B |
| 512 noise | 28.922 ms | 28.378 ms | 29.679 ms | 1.547 ms | 923,176 B -> 1,013,540 B |
| 1024 gradient | 19.999 ms | 18.590 ms | 19.312 ms | 3.657 ms | 61,133 B -> 525,239 B |
| 1024 noise | 114.806 ms | 113.518 ms | 112.844 ms | 5.768 ms | 3,668,246 B -> 4,061,173 B |
| 2048 gradient | 90.927 ms | 76.261 ms | 74.468 ms | 12.667 ms | 232,622 B -> 1,067,214 B |
| 2048 noise | 467.686 ms | 456.730 ms | 457.978 ms | 24.516 ms | 14,460,439 B -> 16,209,808 B |

Interpretation:

- fpng is substantially faster for generated RAW RGBA.
- fpng output is often larger, especially simple gradient-like images where Unity's encoder compresses much more tightly.
- Therefore fpng is the default candidate for preview/intermediate/generated fallback speed, not a general-purpose package size optimizer.
- Compact PNG output remains useful when final package size matters more than export time.

## Correctness Constraint

The native fpng shim must treat Unity `Texture2D` raw memory order explicitly. Unity `ReadPixels` / `GetRawTextureData` style RGBA rows are bottom-to-top relative to PNG scanlines, while PNG encoders consume top-to-bottom rows. The shim flips rows before calling fpng.

Benchmark adoption requires:

- `decode_matches=true` for all benchmark cases.
- `decode_error` empty.
- Native plugin missing or unsupported must fall back to Unity encoder instead of failing export.

## Implementation Boundary

Introduce a small RAW RGBA PNG encoder boundary rather than replacing arbitrary PNG calls ad hoc.

Expected shape:

```text
RawRgbaPngEncoder
  input: byte[] / NativeArray<byte>, width, height, colorSpace hint
  output: PNG bytes
  policy: FastFpng if available, UnityCompact fallback
```

Only call this boundary after the exporter has already decided it has generated RAW RGBA that must become PNG. Do not call it from source byte preservation paths.
