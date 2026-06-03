# v2 lilToon-like Fur 設計

状態: 正式方針。UNAvatar の v2 lilToon-like Fur は、まず Unity/lilToon の Geometry Shader Fur を Compute で透過的に再現する。

## 方針

UNAvatar は wgpu を前提にするため Geometry Shader / Tessellation Shader を使えない。したがって lilToon Fur の互換実装は Compute で生成バッファを作り、通常の render pipeline で描く。

第一目標は、より高度な独自 Fur ではない。第一目標は lilToon の Fur / FurCutout / FurTwoPass のパラメータと表現結果を、アニメーションなしの静的比較で可能な限り一致させること。

この互換実装を CBF: Compute Barycentric Fur と呼ぶ。CBF は lilToon の `lil_common_vert_fur.hlsl` / `lil_pass_forward_fur.hlsl` の Compute 移植であり、v2-lilToon-like Fur の基準線とする。

## 非目標

- 互換CBFが完成する前に、Area/UV-density based Compute Fur Cards を本命化すること。
- 互換CBFが完成する前に、Strand/Groom、persistent strand buffer、compute simulation を実装すること。
- Fur専用の独自AO mapなど、lilToon authoring inputに無い必須入力を増やすこと。
- 見た目改善のために、本家と異なるノイズ、alpha、接続トポロジを互換モードへ混ぜること。

## 互換CBFの要件

CBF は Geometry Shader の `AppendFur()` を Compute でエミュレートする。

- source triangle の3頂点を入力にする。
- 各頂点で lilToon と同じ fur vector を作る。
  - `_FurVector`
  - `_VertexColor2FurVector`
  - `_FurVectorTex`
  - `_FurVectorScale`
  - `_FurVector.w`
  - `_FurCutoutLength`
  - `_FurGravity`
  - `_FurRandomize`
  - `_FurLengthMask`
- `_FurLayerNum` は本家の barycentric sample sequence として扱う。
  - 1: 4 points / 3 segments
  - 2: 7 points / 6 segments
  - 3: 13 points / 12 segments
- 各 sample point で root (`furLayer = 0`) と tip (`furLayer = 1`) を生成する。
- 隣接 sample point の root/tip を、本家 TriangleStream と同じ順序で接続する。
- random な blue-noise sample 同士をつながない。

## Fragment互換

Fur fragment は通常 toon fragment の流用だけでは不十分。lilToon Fur 専用の alpha / AO / rim を再現する。

- `OVERRIDE_FUR`
  - `furLayerShift = furLayer - furLayer * _FurRootOffset + _FurRootOffset`
  - `_FurNoiseMask`
  - `furAlpha = saturate(noise - shiftedLayerTerm + 0.25)` 系
  - `_FurMask`
  - `_FurAO`
- Fur alpha
  - Fur / FurCutout / FurTwoPass の render mode に応じて cutout / transparent を切り替える。
  - Cutout系では `fd.col.a = saturate(fd.col.a * 5.0 - 2.0)` と discard を再現する。
  - Transparent系でも `_Cutoff` による clip を尊重する。
- Fur rim
  - `input.furLayer`
  - `_FurRimColor`
  - `_FurRimFresnelPower`
  - `_FurRimAntiLight`
  - light color / view angle

## 実装段階

1. CBF互換 topology
   - 本家の barycentric sequence を固定で生成する。
   - 三角形内を横断する独自ランダム接続を禁止する。

2. CBF互換 vector
   - fur vector を頂点単位で計算し、sample point では barycentric 補間する。
   - sample point で texture を再サンプルして独自方向を作らない。

3. CBF互換 fragment
   - Fur専用 alpha、AO、rim、cutout/transparent state を分離実装する。
   - 通常 toon material の透明ブレンドだけで Fur を描かない。

4. Fur render states
   - Fur
   - FurCutout
   - FurTwoPass
   - pre pass / transparent pass / ZWrite / AlphaToMask / cull / renderQueue を本家に合わせる。

5. 画像比較
   - `mizuki-split` を固定比較ターゲットにする。
   - Unity Editor Scene view の lilToon 2.3.2 と、UNAvatar の同一マテリアル・同一姿勢を比較する。
   - 静止状態で一致しない差分は、独自改善ではなく互換バグとして扱う。

## 高度Furの位置付け

Area/UV-density based Compute Fur Cards、Area-Weighted Blue-Noise Fur、Strand/Groom は破棄しない。ただし、それらは CBF 互換が成立した後の上位機能とする。

上位機能は CBF を内包できるべきだが、CBF を実装せずに上位機能へ進むことはしない。互換モードは常に本家 lilToon の `AppendFur()` 表現へ戻せる必要がある。

## 受け入れ条件

- `_FurLayerNum` ごとの generated topology が本家 GS と一致する。
- `_FurVector` / vector texture / length mask / gravity / randomize の効き方が本家と一致する。
- `_FurNoiseMask` / `_FurMask` / `_FurRootOffset` による alpha breakup が本家と一致する。
- Fur発生源のbase meshが不要に見えず、Furで形状が構成されて見える。
- 毛が透明なペイント面ではなく、1本の毛/束として認識できる。
- `mizuki-split` の比較で Unity/lilToon Scene view に近い結果になる。
