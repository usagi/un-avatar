# lilToon Fur 技術目標

状態: UNAvatar Fur 実装のための調査メモ。2026-06-05 時点で、実装方針は Compute Fur Cards として確定済み。

実装設計: [`compute-fur-cards-design.md`](compute-fur-cards-design.md)。

## 参照元

- upstream local repo は `C:\Users\the\tmp\lilToon`。tag `2.3.2`、commit `56d5095`。
- 公式 docs では、Fur は normal / vector、length mask、gravity、randomize、noise、mask、AO、mesh type、layer count、root width、contact controls を持つ専用の高負荷 rendering mode として扱われている。
- 公式 shader structure docs では、`lts_fur.shader` と関連 Fur variants は通常の lilToon material variant ではなく、独自 pass を持つ例外的 shader として扱われている。

## lilToon Fur の実体

lilToon Fur は古典的な uniform shell-only technique ではない。

中核は geometry shader による fur generator。

- `lil_common_vert_fur.hlsl` は per-vertex fur vector を object / tangent space で計算する。`_FurVector`、任意 vertex color、任意 `_FurVectorTex`、`_FurVectorScale`、`_FurVector.w`、pre-pass 用 `_FurCutoutLength`、world transform、gravity、任意 contact deformation、randomization を合成する。
- `geom()` は triangle を受け取り、`AppendFur()` を通して生成された line / card-like pair を出力する。
- `AppendFur()` は `furLayer = 0` の inner vertex と、補間 fur vector で offset した `furLayer = 1` の outer vertex を出力する。
- `_FurLayerNum` は shell instance count ではない。triangle 内の固定 barycentric sample positions を選ぶ。
  - 1: triangle vertices。
  - 2: vertices + edge midpoints。
  - 3: 上記に加えて interior / biased barycentric samples。
  - `RestartStrip()` 前に最後の vertex sample がもう一度 append される。
- Unity Scene view で low-poly mesh から細かい毛束が出る理由はこれ。shader が source mesh の膨張コピーではなく、triangle ごとに追加 fur segment / card を生成している。

`lil_common_vert_fur_thirdparty.hlsl` には UnlitWF / UnToon 由来の FakeFur path もある。これは `_FurLayerNum` loop で triangle center / interpolated fur strips を生成する。ただし current Fur shaders が使う default lilToon path は `AppendFur()` geometry path。

## Fragment 挙動

`lil_pass_forward_fur.hlsl` は Fur 専用 fragment path を持つ。

- 通常の lilToon main color と lighting setup を実行する。
- `lil_common_frag.hlsl` の `OVERRIDE_FUR` を適用する。
- 次を計算する。
  - `furLayerShift = furLayer - furLayer * _FurRootOffset + _FurRootOffset`
  - `_FurNoiseMask` から noise
  - noise と `furLayerShift` から alpha
  - `_FurMask` による mask multiplication
  - `fd.col.a` への alpha multiplication
  - `fd.col.rgb` への Fur AO
- cutout / pre path は `fd.col.a = saturate(fd.col.a * 5.0 - 2.0)` を使い、alpha 0 を discard する。
- transparent path は `_Cutoff` で clip する。
- `input.furLayer`、`_FurRimColor`、`_FurRimFresnelPower`、`_FurRimAntiLight`、light color、view angle を使って Fur rim contribution を足す。

重要な違い:

- `LIL_RENDER == 1` または `LIL_FUR_PRE` では、alpha は cubic の `furLayerShift * abs^3 + 0.25` formula と、`fwidth(input.furLayer)` に基づく shell-style AO を使う。
- transparent Fur では、alpha は square formula と別の noise-driven AO expression を使う。

## Pass 構成

lilToon には複数の Fur rendering mode がある。

- Fur: transparent-ish Fur pass。Fur ZWrite なし、AlphaToMask なし。
- FurCutout: cutout Fur。Fur ZWrite on、AlphaToMask on。
- FurTwoPass:
  - `FORWARD_FUR_PRE`: `LIL_FUR_PRE`、ZWrite On、`Blend One Zero`、AlphaToMask On、cutout-like pre coverage。
  - `FORWARD_FUR`: transparent Fur、configurable Fur blending、通常 ZWrite Off。
  - ForwardAdd Fur pre / Fur passes も持つ。

これは単なる style toggle ではなく、quality / stability design。pre pass が stable coverage / depth を作り、transparent pass が結果を柔らかくする。

## UNAvatar の目標

旧 UNAvatar instanced-shell path は正しい quality target ではない。現行実装では shell fallback を削除し、Compute Fur Cards を唯一の実用 Fur path とする。

## Compute 方針

UNAvatar は wgpu を対象にするため、geometry shader と tessellation shader を実装 primitive として使えない。この制約で設計目標が変わる。

実装するのは Compute Fur Cards。これは lilToon geometry shader の `AppendFur()` を Compute で透過的に実行する互換実装であり、v2-lilToon-like Fur の完了条件とする。旧称の CBF / CSFC は歴史的な呼称であり、現行コード名ではない。

Compute Fur Cards で合わせるもの:

- triangle-local な固定 barycentric sample sequence。
- `_FurLayerNum` 1 / 2 / 3 に対応する 4 / 7 / 13 points。
- 各 sample の root (`furLayer = 0`) と tip (`furLayer = 1`)。
- 頂点単位で計算した fur vector を sample point へ barycentric 補間する構造。
- `_FurVectorTex`, `_FurLengthMask`, `_FurGravity`, `_FurRandomize`, `_FurCutoutLength`。
- Fur 専用 fragment alpha、AO、rim、cutout / transparent state。

Area/UV-density based sampling、Area-Weighted Blue-Noise Fur、Strand/Groom は lilToon 互換が成立した後の上位機能。これらは理論上 Compute Fur Cards を内包できるべきだが、互換実装を崩して先へ進まない。

## 実装状態

- Compute Fur Cards:
  - primary Fur path。
  - Unity/lilToon の `AppendFur()` topology、fur vector、fragment alpha、AO、rim、render state を一致させる。
  - bones / morphs / material animation が fur source に影響する場合は per-frame で source vertices を更新する。
  - Fur-specific toon fragment path で generated buffers を描画する。
  - FurTwoPass-equivalent rendering として pre cutout / AlphaToMask-style coverage + transparent Fur pass を持つ。
  - `_FurVectorTex`, `_FurLengthMask`, `_FurNoiseMask`, `_FurMask`, `_FurRootOffset`, `_FurAO`, `_FurCutoutLength`, `_FurRimColor`, `_FurRimFresnelPower`, `_FurRimAntiLight`, blend / ZWrite / ZTest / cull controls を一致対象にする。
  - 旧 instanced shell fallback は削除済み。

- Future:
  - lilToon 互換を保った上で Area/UV-density based sampling や Area-Weighted Blue-Noise Fur を追加する。
  - 必要に応じて Fur generation 前に compute tessellation / subdivision を行う。
  - lilToon compatibility を超える UNAvatar extension として compute strand / groom mode を実装する。

## 受け入れ基準

UNAvatar Fur は、Compute Fur Cards が `mizuki-split` など既知の lilToon FurTwoPass asset で Unity Editor Scene view quality 以上に到達することを complete 条件とする。

最初の visual milestone:

- Fur silhouette が smooth inflated shell ではなく、fine generated fur segments で構成される。
- sparse source mesh でも dense fur が出る。
- Fur length / mask / noise が Unity に近い strand breakup を出す。
- Two-pass Fur が hard cutout-only aliasing と transparent-shell blobbing の両方を避ける。
- comparison target で lilToon 2.3.2 Scene view と同等以上。

2026-06-05 時点の動作試験では、`mizuki-split` の Fur は美しく表現され、不具合は目視確認されていない。
