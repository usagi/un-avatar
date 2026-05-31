# U.N. Avatar v2 Implementation Decisions

作成日: 2026-05-31

この文書は v2 実装前に相談した項目と、現時点の採用方針を短く保持する。実装中に問題が出た場合は、その時点で再相談して更新する。

## 1. Unity Exporter GLB Writer

### Option A: C# で最小 GLB writer を自前実装

利点。

- 依存が少ない。
- `.unavatar` に必要な subset だけを制御できる。
- ライセンス確認と同梱が単純。
- exporter prototype を小さく始められる。

欠点。

- bufferView / accessor / alignment / image embedding / skin / morph の実装ミスを自分で潰す必要がある。
- glTF validator との往復調整が必要。
- 機能が増えると writer 保守が重くなる。

### Option B: 既存 Unity glTF exporter library を使う

利点。

- glTF / GLB の基本構造、buffer、image、material 出力を流用できる。
- skin / morph / texture まわりの初期実装が速い可能性がある。
- glTF validator に通る出力へ近づけやすい。

欠点。

- ライセンス、依存 package、Unity version 対応を確認する必要がある。
- VRC / Modular Avatar / lilToon 由来の情報を `UN_avatar` extension として足す接続点が必要。
- 依存先の設計に引っ張られる。

### Option C: Unity から中間 JSON / binary を出し、Rust CLI で GLB 化

利点。

- GLB writer / schema validation を Rust 側に集約できる。
- Runtime importer と formatter / validator の型を共有しやすい。

欠点。

- Unity Project 上の UX が重くなる。
- exporter prototype の往復が増える。
- ユーザーに Rust CLI / exe 連携を意識させやすい。

### 採用方針

ユーザーが別途 UnityGLTF を install しなくてよいよう、Option A を標準経路に変更する。v0.1 では UnityGLTF bridge も採用しない。調査結果は [`unity-exporter-dependency-research.md`](unity-exporter-dependency-research.md) に固定した。

- built-in minimal GLB writer: exporter package 単体で動く。初期は mesh / skin / basic material / main texture / humanoid / variants を優先する。
- UnityGLTF: MIT、Unity 2021.3、GLB export、`GLTFRoot` callback / plugin があり `UN_avatar` extension を注入しやすいが、v0.1 では不採用。
- glTFast: Apache-2.0、GLB export は強いが、調査時点の package は Unity 6000.0 かつ custom root extension 注入が内部実装寄り。fallback / 比較対象。
- UniVRM / UniGLTF: MIT、VRM / MToon 参照実装として使うが、VRC / MA / lilToon exporter の主軸にはしない。

built-in writer の品質や validator 互換で問題が出た場合は、UnityGLTF や glTFast fallback を再評価する。

Option C は初期 prototype では採用しない。Unity 上の検証ループを鈍らせないことを優先する。

## 2. Rust Crate Boundary

### Option A: `un-avatar-format`

`.unavatar` schema、parser helper、validator、将来 exporter を置く。

利点。

- Runtime / CLI / validator / exporter で型を共有できる。
- `.unavatar` が単なる IO plugin ではなく製品 format だと明確になる。
- 将来、Rust 側で `.unavatar` を生成する時に自然。

欠点。

- 初期に crate が増える。
- `un-avatar-core` との責務境界を決める必要がある。

### Option B: `un-avatar-io-unavatar`

既存 IO crate 群に合わせて importer / exporter を置く。

利点。

- 現行 `un-avatar-io-gltf` / `un-avatar-io-vrm` と並ぶ。
- `IoRegistry` への登録が分かりやすい。
- 初期実装が単純。

欠点。

- schema / validator / format docs の責務が IO crate に寄りすぎる。
- Unity Exporter や将来 Rust exporter と型共有しにくい。

### Option C: 両方

`un-avatar-format` に schema / validator、`un-avatar-io-unavatar` に IO integration を置く。

利点。

- 長期的に責務がきれい。
- Runtime loader、CLI validator、将来 exporter が同じ schema 型を使える。

欠点。

- MVP には crate が多い。
- 初期は boilerplate が増える。

### 採用方針

Option C を採用する。

- `un-avatar-format`: `.unavatar` schema、parser helper、validator、将来 exporter
- `un-avatar-io-unavatar`: `IoRegistry` integration、importer / exporter wrapper

`.unavatar` は v2 の中核形式なので、schema / validator を IO crate に閉じ込めない。

## 3. `.una` Removal

`.una` / `.una.d` は廃止対象。

選択肢。

- すぐ削除: `un-avatar-io-una`、CLI tests、xtask smoke から外す。
- 段階削除: v2 branch で deprecated warning を出し、次の cleanup commit で削除。

採用方針は「すぐ削除」。実運用の後方互換性がなく、維持するほど誤解が増えるため。

## 4. Wardrobe Set Editing

VRC / Modular Avatar 対応衣装の切替は `variants` ではなく `wardrobe.sets` を正本にする。

### 採用方針

- `.unavatar` は全衣装資産を保持できる。
- Runtime は最初から outfit group 単位の lazy GPU upload / unload を前提にする。
- wardrobe set は base state からの差分 patch として持つ。
- operation は `subtreeVisibility` / `nodeVisibility` / `rendererVisibility` / `blendShapeWeight` を初期対象にする。
- target は `nodeId` を保存上の正本、Unity hierarchy path を表示 / fallback とする。
- 具体性の高い operation を優先し、同じ具体性なら後勝ちにする。
- Supervisor profile のユーザー override は `.unavatar` 内蔵 set より後に適用する。

Unity Exporter は手入力ではなく capture diff workflow を主導線にする。

1. `Capture Base` で素体状態を記録する。
2. Unity 上で衣装状態を作る。
3. `Capture Wardrobe Set` で base との差分だけを記録する。
4. 色違い衣装は set 複製後、対象 outfit group / subtree の ON/OFF 差分を変更する。

この方式なら、瑞希 + Noble Trace Color 1 / Color 13 のように、素体パーツ非表示、帽子非表示、Skirt / Pants 切替、Body_b のニーソ用 blendshape 変更を、最小の差分 operation として保持できる。
