# U.N. Avatar v2 Roadmap

作成日: 2026-05-31

この文書は v1.0.0 後の v2 開発計画の正本である。v1 の境界は [`../README.md`](../README.md)、[`roadmap.md`](roadmap.md)、[`runtime-mvp.md`](runtime-mvp.md) を優先する。

## 1. v2 の中核

v2 の中核は、U.N. Avatar を VRM/glTF レンダラーから、VRM・glTF・VRC/Unity アバター資産を軽量な非 Unity ランタイムで扱う実行基盤へ広げること。

ただし v2 でも Runtime は Unity / VRC SDK / lilToon / Modular Avatar に依存しない。Unity 固有解釈は Unity Editor Exporter 側へ閉じ込める。

## 2. 現行 repo から見た前提

v1.0.0 時点の実装で確認できる重要な前提。

- Runtime は Rust / wgpu ベースで、VRM / glTF を `UnaDocument` に読み込み、GPU skinning / GPU morph / MToon-like / SpringBone / VMC / UNMF/Z / Spout2 を扱う。
- `crates/un-avatar-core` は `UnaDocument`、scene snapshot、humanoid profile、expression catalog、MToon material、SpringBone settings を持つ。
- `crates/un-avatar-io-gltf` は glTF / GLB importer。mesh、skin、morph、PBR material を `UnaDocument` へ変換する。
- `crates/un-avatar-io-vrm` は VRM0 / VRM1 importer。VRM extension、humanoid、MToon、expression、SpringBone、VRM1 node constraint を取り込む。
- `crates/un-avatar-io-una` の `.una` / `.una.d` は現状 bootstrap 形式で、`.una` は UTF-8 TOML、`.una.d` は `manifest.toml` を読む空シーン相当。scene / VRM / humanoid / expression は v0 export で保持されない。実際の Renderer 入力としても使われていない。
- Renderer の `model_loader` は現状 VRM / GLB / glTF を直接判定して import しており、CLI の `IoRegistry` を全面利用していない。
- CLI / plugin host には `AvatarImporter` / `AvatarExporter` / `FormatDescriptor` / capability / stdio plugin 境界が既にある。
- glTF export は built-in では未実装。Unity Exporter から `.unavatar` GLB を直接生成するか、Rust 側 exporter を別途実装する必要がある。

## 3. 草案から修正する点

添付草案の方向性は妥当。ただし repo 現状に合わせて次を修正する。

- 既存 `.una` を `.unavatar` へ改名・転用しない。`.una` は v2 で obsolete とし、後方互換性維持も必須にしない。
- v2 の外部アバター資産形式は新規拡張子 `.unavatar` とし、中身は valid GLB 2.0 + U.N. Avatar glTF extensions とする。
- `.unavatar` は `un-avatar-format` に schema / parser / validator を置き、`un-avatar-io-unavatar` に `IoRegistry` integration を置く。
- 初期実装は分割 extension 群ではなく単一 `UN_avatar` extension を正本にする。安定後に `UN_avatar_manifest` などへ分割できる JSON 構造にしておく。
- `UNA Toon Material` は既存 `UnaMtoonMaterial` / `UnaMaterialPbr` の上に足す。MToon と別系統の material model を急に増やさない。
- PhysBone は新 physics engine として作り直すのではなく、まず既存 SpringBone runtime primitive へ近似変換する。
- Expressions は現状 morph 中心。material color、texture switch、node visibility、variant は v2 で `UnaDocument` と renderer control へ拡張する対象。
- Unity Exporter MVP は repo 内 Rust exporter の完成を待たず、Unity 側で GLB + `UN_avatar` extension を書ける構成を許容する。
- Unity Exporter は同一 repo に内包する。ただし UPM package として隔離し、Rust workspace / Runtime / 通常 CI は Unity に依存させない。

## 4. `.unavatar` v0.1 方針

`.unavatar` は Runtime-ready な標準アバター形式とする。

```text
avatar.unavatar
= valid GLB 2.0
+ extensions.UN_avatar
```

互換性方針。

- glTF 標準部には scene、nodes、meshes、skins、morph targets、textures、PBR fallback material を必ず入れる。
- `UN_avatar` は `extensionsUsed` に入れる。v0.1 では原則 `extensionsRequired` に入れない。
- U.N. Avatar 拡張を無視しても、他の glTF tool で最低限メッシュとテクスチャが見える状態を維持する。
- Runtime 側は `.unavatar` を GLB として読み、`UN_avatar` extension があれば `UnaDocument` の humanoid / material / expression / dynamics / provenance へ反映する。

v0.1 の最小 extension 構造。

```json
{
  "specVersion": "0.1.0",
  "manifest": {
    "generator": "UNAvatar Unity Exporter",
    "generatorVersion": "0.1.0",
    "sourceType": "vrc_unity_prefab"
  },
  "humanoid": {
    "humanBones": {
      "hips": 12,
      "head": 25,
      "leftHand": 33,
      "rightHand": 43
    }
  },
  "materials": [],
  "expressions": [],
  "dynamics": [],
  "provenance": {
    "redistributionAllowed": false
  }
}
```

## 5. Runtime 設計変更

v2 の Runtime 側は次を進める。

1. `model_loader` を拡張し、`.unavatar` を GLB として読み込む。
2. `GltfImporter` から glTF root JSON extension を参照できる入口を作る。
3. `UN_avatar` parser を追加し、`UnaDocument` の既存型へ可能な範囲で正規化する。
4. `UnaDocument` に不足する表現を小さく追加する。
5. Runtime status / diagnostics に `.unavatar` spec version、source type、loss / approximation を出す。

最初に足す `UnaDocument` 側の候補。

- `manifest`: spec version、generator、source type
- `provenance`: source asset / license / redistribution metadata
- `wardrobe`: outfit set / node visibility / blendshape override / lazy asset group
- `expression bindings`: morph 以外の material / visibility binding
- `dynamics source`: VRM SpringBone / VRC PhysBone 由来を区別する metadata

## 6. Unity Exporter 計画

VRC / Unity 資産対応は Unity Editor Exporter を使う。

Exporter の仮使用 MVP 責務。

- Avatar Prefab / Scene object の指定
- Prefab 展開後の SkinnedMeshRenderer 列挙
- Mesh / skin / blendshape / texture 抽出
- Humanoid bone mapping 抽出
- Modular Avatar 対応衣装を含む variant 候補の抽出
- Standard / MToon / lilToon 主要 material parameter 抽出
- PBR fallback material 生成
- `UN_avatar` extension 生成
- `.unavatar` GLB 出力

Unity 依存に閉じるもの。

- Unity serialized object / GUID / prefab 参照
- VRC Avatar Descriptor
- VRC PhysBone / Collider
- VRC Expression Parameters / Expression Menu
- lilToon / Poiyomi shader property 解釈
- Modular Avatar bake 後状態の取得

Exporter は repo 内 `unity/un-avatar-unity-exporter/` に UPM package として置く方針とする。U.N. Avatar 配布パッケージには展開済み package として同梱してよい。ただし Runtime は Unity に依存しない。通常の `cargo xtask ci` も Unity Editor を要求しない。

詳細は [`unity-exporter-v0.1.md`](unity-exporter-v0.1.md) を正とする。GLB writer は package 内蔵の最小 writer に固定し、v0.1 では UnityGLTF を使わない。

## 7. Wardrobe / Outfit System

複数衣装、アクセサリ、小物切替は `.unavatar` 内の `wardrobe.sets` として扱う。基本方針は 1 avatar = 1 `.unavatar`、中に複数 wardrobe set と必要資産を同梱すること。

初期 operation は `subtreeVisibility` / `nodeVisibility` / `blendShapeWeight` を最優先にする。衣装 mesh / accessory mesh の ON/OFF と素体 shrink / sock / skin 表示用 blendshape 差分が切替機能の最小価値になる。その後、material override、expression weight、dynamics enable を足す。

`.unavatar` は全衣装資産を保持できるが、Runtime は最初から outfit group 単位の lazy GPU upload / unload を前提にする。数百着規模の衣装を 1 `.unavatar` に含める利用者を想定し、全衣装 mesh / texture の GPU 常駐を前提にしない。

Unity Exporter の出力モード。

- `All Wardrobe Sets In One .unavatar`: 複数衣装・小物を 1 ファイルに同梱し、Runtime で切り替える。最初からこの方式を MVP とする。
- `Current State Only`: Unity 上で現在有効な見た目だけを bake して出す。fallback / debug export として残してよい。
- `Split Wardrobe Sets Into Separate .unavatar Files`: 大型衣装や配布都合向け。

Modular Avatar 対応衣装は、Unity 側で MA / VRC menu / active state を解釈し、Runtime 側では visibility / blendshape / material / dynamics 差分として軽く切り替える。

Unity Exporter は手入力を主導線にしない。`Capture Base` と `Capture Wardrobe Set` で Unity 上の現在状態を比較し、active state / renderer enabled / blendshape weight の差分を wardrobe operations として記録する。Color 1 / Color 13 のような色違いは set 複製後、対象 outfit group の ON/OFF 差分だけを変更できるようにする。

## 8. 品質改善テーマ

v2 では機能追加だけでなく、v1 の実用面を強くする。

- Import diagnostics: unsupported / approximate / lost feature を機械可読に残す。
- Renderer smoke: `.unavatar` sample を headless または screenshot で検証する。
- Material regression: MToon / lilToon-like の shade、matcap、rim、emission、outline の崩れを fixture 化する。
- Physics regression: SpringBone と PhysBone 近似の揺れすぎ、めり込み、発散を検出する。
- Startup UX: import failure の原因を Supervisor に短く出す。
- License UX: VRM / VRC 由来 metadata と再配布注意を profile 選択時に確認できる。
- Performance: texture cache / compression は v1 の資産を継続し、Unity Exporter 側でも texture resize / embedding policy を明示する。

## 9. Milestones

### Milestone 0: v2 plan freeze

- この文書を v2 計画の正本として追加する。
- 既存 `.una` と新 `.unavatar` の違いを明文化する。
- `docs/README.md` と `docs/roadmap.md` から参照する。

### Milestone 1: `.unavatar` preview spec

- `docs/unavatar-format-v0.1.md` を追加する。
- `UN_avatar` JSON schema の最小形を固定する。
- sample `.unavatar` の検証方針を決める。
- ライセンス注意文を README / docs へ追加する。

### Milestone 2: Unity Exporter prototype

- repo 内 `unity/un-avatar-unity-exporter/` に UPM package として隔離する。
- 「瑞希」+ Modular Avatar 対応衣装を複数入れた Unity Project から `.unavatar` を出力できる仮実装を作る。
- 最初から `All Wardrobe Sets In One .unavatar` で export する。
- export report で不足、近似、未対応を見えるようにする。
- U.N. Avatar 配布パッケージへ展開済み package として同梱する方針を維持する。

### Milestone 3: Runtime loader MVP

- `un-avatar-format` と `un-avatar-io-unavatar` を追加する。
- `.unavatar` extension probe を追加する。
- `.unavatar` を GLB として読み、既存 `GltfImporter` の scene snapshot を再利用する。
- `UN_avatar.humanoid` / `manifest` / `provenance` を読む。
- Renderer の `model_loader` から `.unavatar` を読み込めるようにする。

### Milestone 4: Expressions / wardrobe foundation

- `UnaDocument` に morph 以外の expression binding を追加する。
- node visibility binding を renderer に反映する。
- material color / emission binding を最小実装する。
- Supervisor から expression / toggle を操作できる runtime control を広げる。

### Milestone 5: lilToon major parameters

- lilToon 検出。
- main texture、shade texture、normal、matcap、rim、emission、outline、alpha、cull / render queue 相当を抽出する。
- Runtime の既存 MToon-like shader / material path に寄せて描画する。

### Milestone 6: PhysBone to U.N. dynamics

- VRC PhysBone root / collider / radius / pull / spring / stiffness / gravity の近似抽出。
- 既存 SpringBone runtime primitive へ変換する。
- debug view と tuning UI を追加する。

### Milestone 7: VRC expressions / outfit toggles

- VRC Expression Menu のうち配信で使う toggle / expression を抽出する。
- node visibility / material binding / accessory toggle を `.unavatar` に保持する。
- Hotkey / UI / VMC / UNMF/Z との binding 方針を決める。

### Milestone 8: Modular Avatar bake support

- Modular Avatar 適用後状態を export 対象にする。
- 複数衣装・小物の bake と variant metadata を保持する。
- Runtime 側で outfit / accessory 切替を扱う。

### Milestone 9: Validator and compatibility report

- CLI validator を追加する。
- `.unavatar` spec version / required extension / missing fallback / unsupported feature を検査する。
- Unity Exporter と Runtime の compatibility report を揃える。

## 10. 非目標

v2 初期では次をやらない。

- VRChat client の完全再現
- VRC SDK runtime 互換実装
- FX Layer / Animator Controller 完全再生
- Poiyomi 完全再現
- `.unitypackage` の Runtime 直接読み込み
- 第三者 BOOTH 資産の再配布を前提にした package workflow
- Unity を Runtime に組み込む構成
- `.una` の後方互換維持

## 11. ライセンス注意文

`.unavatar` には元アバター、衣装、テクスチャ等のデータが含まれる場合がある。生成された `.unavatar` の第三者配布、販売、共有は必ず元アセットの利用規約に従うこと。U.N. Avatar は、ユーザー自身が正規に保有するアセットをローカル環境で配信等に利用することを主目的とする。

## 12. 関連文書

- [`unavatar-format-v0.1.md`](unavatar-format-v0.1.md): `.unavatar` GLB extension preview spec
- [`unity-exporter-v0.1.md`](unity-exporter-v0.1.md): Unity Editor Exporter の境界、配置、MVP
- [`v2-open-decisions.md`](v2-open-decisions.md): GLB writer、crate 境界、`.una` 廃止の採用方針

## 13. 直近の実装順

1. `unity/un-avatar-unity-exporter/` の UPM package skeleton と `cargo xtask unity-exporter-package` を作る。
2. Unity Project から `All Wardrobe Sets In One .unavatar` を出す exporter prototype を作る。
3. export report で過不足を検証できるようにする。
4. `un-avatar-format` / `un-avatar-io-unavatar` の crate 境界を作る。
5. `.unavatar` probe と GLB import path を追加する。
6. `UN_avatar` manifest / humanoid / provenance / variants parser を入れる。
7. Renderer `model_loader` を `.unavatar` 対応にする。
8. Wardrobe `subtreeVisibility` / `nodeVisibility` / `blendShapeWeight` の runtime representation を実装する。
