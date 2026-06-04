# v2 lilToon-like Compute Fur Cards

状態: 実装済み方針。UNAvatar の v2 lilToon-like Fur は、Unity/lilToon の Geometry Shader Fur を Compute 生成バッファ + 通常 render pipeline で再現する。

## 方針

UNAvatar は wgpu を前提にするため Geometry Shader / Tessellation Shader を実装 primitive として使えない。したがって lilToon Fur の互換実装は `compute_fur_cards.wgsl` で Fur card 頂点・index を生成し、`mesh.wgsl` の `vs_compute_fur_cards` / `vs_compute_fur_cards_pre` と Fur fragment path で描く。

この実装を Compute Fur Cards と呼ぶ。旧称の CSFC / CBF は歴史的な試行名であり、現行コード・文書では使わない。

第一目標は高度な独自 Fur ではなく、lilToon の `AppendFur()` geometry path を wgpu で透過的に再現すること。Area/UV-density based sampling や Strand/Groom は、lilToon 互換の上位 extension として別扱い。

## 現行実装

- source triangle の3頂点を Compute 入力にする。
- `_FurLayerNum` は本家の barycentric sample sequence として扱う。
  - 1: 4 points / 3 segments
  - 2: 7 points / 6 segments
  - 3: 13 points / 12 segments
- 各 sample point で root (`furLayer = 0`) と tip (`furLayer = 1`) を生成する。
- 隣接 sample point の root/tip を、本家 TriangleStream と同じ意味の Fur card topology として接続する。
- per-vertex fur vector を作り、sample point では barycentric 補間する。
- skin palette 更新がある場合、source vertices を更新してから Compute Fur Cards を dispatch する。
- Compute 生成できない場合に旧 shell path へ fallback しない。

## 入力

Compute Fur Cards は lilToon authoring input の範囲で動く。独自 Fur 専用入力は要求しない。

- `_FurVector`
- `_VertexColor2FurVector`
- `_FurVectorTex`
- `_FurVectorScale`
- `_FurVector.w`
- `_FurCutoutLength`
- `_FurGravity`
- `_FurRandomize`
- `_FurLengthMask`
- `_FurNoiseMask`
- `_FurMask`
- `_FurRootOffset`
- `_FurAO`
- `_FurRimColor`
- `_FurRimFresnelPower`
- `_FurRimAntiLight`

## Fragment 互換

Fur fragment は通常 toon fragment の流用だけでは不十分。`mesh.wgsl` は Fur 専用の alpha / AO / rim を持つ。

- `furLayerShift = furLayer - furLayer * _FurRootOffset + _FurRootOffset`
- `_FurNoiseMask` と `_FurMask` による alpha breakup
- `_FurAO` による Fur AO
- Fur / FurCutout / FurTwoPass 相当の cutout / transparent alpha handling
- `_FurRimColor` / `_FurRimFresnelPower` / `_FurRimAntiLight` による Fur rim

Portable16 tier では high-tier Fur textures を落とし、texture budget を維持する。

## 非目標

- Compute Fur Cards を旧 instanced shell path へ黙って fallback すること。
- 見た目改善のために、本家と異なるノイズ、alpha、接続 topology を互換モードへ混ぜること。
- Fur 専用の独自 AO map など、lilToon authoring input に無い必須入力を増やすこと。
- Strand/Groom や persistent strand simulation を lilToon 互換 Fur と同一機能として扱うこと。

## 受け入れ条件

- `_FurLayerNum` ごとの generated topology が本家 GS と一致する。
- `_FurVector` / vector texture / length mask / gravity / randomize の効き方が本家と一致する。
- `_FurNoiseMask` / `_FurMask` / `_FurRootOffset` による alpha breakup が本家と一致する。
- Fur 発生源の base mesh が不要に見えず、Fur で形状が構成されて見える。
- 毛が透明なペイント面ではなく、1本の毛/束として認識できる。
- `mizuki-split` の目視試験で、Fur の破綻が確認されない。

## 現状

2026-06-05 時点で、旧 instanced shell fallback は削除済み。Compute Fur Cards 実装は `mizuki-split` の動作試験で美しく表現され、不具合は目視確認されていない。
