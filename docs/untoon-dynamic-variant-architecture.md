# UNToon Dynamic Variant Architecture

状態: v2 設計方針。lilToon 互換実装の完成後に、MToon / lilToon / 将来の toon 入力 profile を UNToon 体系へ整理するための正本。

## Purpose

UNToon は U.N. Avatar runtime の toon rendering family である。

v2 の正本は、MToon 専用 renderer と lilToon-like renderer を並べて維持することではない。入力 material profile を UNToon semantic material へ正規化し、モデル単位で必要な feature / resource を集計して、必要十分な shader / bind layout / resource set を構成する。

目標。

- MToon は UNToon への入力 profile として扱う。
- lilToon は UNToon semantic の最大互換 target として扱う。
- `Full` / `Portable` の固定 shader tier は正式概念にしない。
- 使わない機能の texture slot、sampler、uniform、branch、compute pass を背負わない。
- GPU skinning / morph / future PhysBone など、material 以外の GPU resource も同じ planning に含める。
- 環境制約を超えた場合だけ、最低保証構成へ落とす。

## Terminology

### Source Profile

Authoring shader / material family 由来の入力。

- `mtoon`: VRM0 / VRM1 MToon。
- `liltoon`: lilToon / lilToon variant。
- `untoon`: 将来の U.N. Avatar native material。

Source profile は保存時の provenance と import policy を表す。renderer shader family 名ではない。

### UNToon Semantic Material

Runtime が扱う正規化 material 表現。

MToon importer は MToon parameter を UNToon semantic へ変換する。lilToon importer も lilToon parameter を同じ semantic へ変換する。MToon のための別 shader family は新設しない。

UNToon semantic は lilToon 互換表現を上限として設計する。ただし MToon source material が使わない feature は required feature に含めない。

### Feature Set

モデルに必要な描画・計算機能の集合。

例。

- main color
- shade
- normal map / 2nd normal
- matcap / 2nd matcap
- emission / emission 2nd
- rim / backlight
- outline
- alpha mask / transparent / transparent zwrite
- glitter
- refraction / gem
- dissolve
- parallax
- fur
- AudioLink
- GPU morph
- GPU skinning

Feature set は material 単体ではなく、モデル単位で集計する。Wardrobe hot switch を考慮し、全 wardrobe set で必要になりうる feature を同じ model variant に含める。

### Dynamic Variant

Feature set、resource budget、GPU capability から作る shader / bind layout / pipeline の実体。

`UNToonFull` や `Portable16` のような固定 tier を正式 API にしない。Full 相当のモデルは結果として Full 相当 variant になる。MToon-only モデルは自然に小さい variant になる。

## Import Policy

### MToon

MToon は UNToon semantic material へ変換する。

MToon 由来で主に必要な feature。

- main texture / color
- shade texture / shade color
- normal map
- matcap
- rim
- emission
- outline
- alpha mode
- cull / render queue
- UV animation

MToon input は lilToon superset へ写像されるが、glitter、fur、refraction、AudioLink、multi-layer lilToon expression などは required feature にしない。これにより VRoid / VRM 系モデルは軽い dynamic variant を選ぶ。

### lilToon

lilToon は UNToon semantic の最大互換 target として扱う。

Renderer は本家 lilToon の material semantics を根拠に required feature を抽出する。使われていない feature は shader resource / branch から外してよい。ただし material / wardrobe / animation で後から有効になる可能性がある feature は model analysis で検出し、variant に含める。

### Native UNToon

将来の native UNToon material は、source profile 変換を経ずに semantic material を直接保持できる。ただし dynamic variant planning のルールは同じ。

## Variant Planning

Variant planner は model load 時に実行する。

入力。

- `.unavatar` source package
- material semantic values
- wardrobe sets
- source animation / expression / action hints
- mesh attributes
- skin / morph presence
- GPU adapter capability
- runtime quality policy

出力。

- `UntoonVariantKey`
- required shader modules
- material texture slots
- non-material resource slots
- sampler requirements
- bind group layout
- pipeline key
- fallback decision

モデル単位 planning を基本にする。wardrobe set 切替ごとに shader variant を変えると hot switch の障害になるためである。

### Current Implementation Notes

現 renderer はまだ固定 shader source を使うが、`SceneMeshRuntimeRequirements` と runtime status で最初の feature bits を収集・公開している。

現在収集済み。

- `runtime_requires_audio_link_texture`
- `runtime_requires_screen_refraction`
- `runtime_requires_fur`

`audio_link_texture_needed` は AudioLink 入力 source が有効な場合だけ実際の worker / texture upload を起動する実行時判定である。`runtime_requires_audio_link_texture` は material set が AudioLink texture を要求しているかを表す variant planning 用の事実であり、入力 source の ON/OFF とは分けて扱う。

## Resource Budget

Texture / sampler budget は material だけで使い切ってはいけない。

Variant planner は先に runtime-reserved resources を確保し、残りを material feature に配分する。

予約対象。

- fallback textures
- environment / lighting LUT
- AudioLink texture if required
- GPU skinning resources
- GPU morph resources
- future dynamics / physics resources

GPU skinning が storage buffer path を使える環境では texture budget を圧迫しない。texture-backed fallback が必要な環境では、skinning palette / index / weight 用 slot を最低保証構成へ含める。

## Baseline Fallback

`Portable` という恒久 profile は持たない。

環境制約で dynamic variant が成立しない場合、renderer は `UNToon Baseline` へ落とす。Baseline は品質名ではなく最低保証構成である。

Baseline の原則。

- 16 sampled textures 以下で成立する。
- GPU skinning / GPU morph の最低限の resource を予約する。
- MToon source material は原則として Baseline 内に収まる。
- lilToon high-cost feature は必要に応じて無効化、近似、または multi-pass fallback とする。
- fallback は runtime status / diagnostics に出す。

Baseline で落としてよい候補。

- 2nd / 3rd layer texture sampling
- 2nd normal
- 2nd matcap
- high-capability fur masks
- dissolve noise texture
- parallax / POM
- glitter shape / color texture
- AudioLink-driven optional modulation

Baseline でも守るべき候補。

- main color
- shade
- alpha mode
- cull / render queue
- outline basic
- MToon-compatible rim / emission
- GPU skinning / morph basic

## Shader Source Composition

任意の文字列置換で shader を作らない。

採用する形。

- feature module 単位で WGSL / shader fragment を管理する。
- resource declaration は module metadata から生成する。
- module dependency を明示する。
- variant key から deterministic な shader source を生成する。
- generated shader は cache できる。

避ける形。

- full shader 文字列から regex で機能を削る。
- 16 texture 版を ad hoc な文字列操作で作る。
- material ごとに runtime frame 中に shader を組み直す。

## Suggested Data Shape

概念上の型。

```text
UntoonSourceProfile =
  MToon | LilToon | Native

UntoonFeatureSet {
  material_features
  mesh_features
  animation_features
  runtime_features
}

UntoonResourcePlan {
  sampled_textures
  samplers
  uniform_buffers
  storage_buffers
  fallback_textures
  skinning_resources
  morph_resources
}

UntoonVariantKey {
  feature_bits
  texture_budget_class
  skinning_mode
  morph_mode
  alpha_modes
  pass_set
  gpu_capability_class
}
```

## Implementation Order

短期の順序。

1. UNToon semantic / source profile / variant planning の用語を docs に固定する。
2. VRC import base の `.unavatar` skinning を既存 GPU skinning pipeline に接続・検証する。
3. 既存 GPU skinning resource を variant planning の予約対象として扱えるようにする。
4. 既存 lilToon-like renderer の `FullOnePass` / `Portable16` 実装名を内部歴史名として閉じ込める。
5. MToon input を UNToon semantic へ変換し、MToon-only モデルが小さい dynamic variant を選ぶようにする。
6. shader module metadata と deterministic source generation を導入する。
7. fixed full / fixed portable の外部概念を廃止する。

## Non-goals

- VRChat client shader variant system の完全 clone。
- Poiyomi 互換。
- MToon 専用 renderer の新設。
- Full shader と Portable shader の二重保守。
- wardrobe set 切替ごとの shader rebuild。
- 互換性検証が済んだ lilToon behavior を命名整理だけで書き換えること。

## Open Questions

- Baseline の正確な sampled texture 上限を 16 固定にするか、adapter limit から自動で下げるか。
- 既存 GPU skinning の storage buffer path を Baseline の最低保証としてよいか、texture-backed fallback が必要か。
- shader module generator を Rust build-time / runtime cache / WGSL include preprocessor のどこへ置くか。
- `.unavatar` schema に feature summary を保存するか、runtime analysis のみにするか。
- animation / expression / wardrobe が material feature を後から有効化する場合の static analysis 範囲。
