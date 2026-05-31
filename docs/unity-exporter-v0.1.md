# U.N. Avatar Unity Exporter v0.1 Plan

作成日: 2026-05-31

この文書は VRC / Unity avatar asset から `.unavatar` を生成する Unity Editor Exporter の設計メモである。

## 1. 境界

Unity Exporter は Unity / VRC / shader / Modular Avatar の解釈器であり、U.N. Avatar Runtime の依存先ではない。

```text
Unity Project / VRC SDK / lilToon / Modular Avatar
  -> U.N. Avatar Unity Exporter
  -> avatar.unavatar
  -> U.N. Avatar Runtime
```

Runtime は Rust / wgpu のまま自立させる。Unity DLL、VRC SDK、lilToon、Modular Avatar を Runtime crate や renderer process に入れない。

## 2. Repo 配置

Exporter は同一 repository に内包する。ただし Rust workspace からは疎結合にする。

```text
un-avatar/
  unity/
    un-avatar-unity-exporter/
      package.json
      Editor/
        UNAvatarExporter.cs
        ...
      Runtime/
      README.md
```

この形にする理由。

- `.unavatar` spec と Runtime importer の変更を同じ PR で追える。
- release package に exporter を同梱しやすい。
- ユーザーから見て変換ツールが本体と一緒に手に入る。
- Unity 依存を Rust workspace に混ぜずに済む。

## 3. CI / Build 方針

- 通常の `cargo xtask ci` は Unity Editor を要求しない。
- Unity Exporter は UPM package として独立させる。
- package 作成だけなら `.cs` / `.asmdef` / `package.json` / docs を配布レイアウトへコピーするだけでよい。
- Unity Editor を使う validation は任意の別 job / 手元確認に分ける。
- `cargo xtask unity-exporter-package` で Unity compile なしに package layout を作成する。

## 4. Release Packaging

U.N. Avatar 配布物には Exporter package を同梱する。

```text
release-packages/un-avatar-2.x.x.zip
  bin/
  assets/
  LICENSES/
  unity/
    un-avatar-unity-exporter/
      package.json
      Editor/
      Runtime/
      README.md
```

同梱はするが Runtime は依存しない。この区別を README と release notes に明記する。Unity package は基本的に source `.cs` と metadata の package であり、Unity が対象 project 内で compile して Editor assembly を生成する。事前ビルド済み MSIL `.dll` として配る必要はない。

## 5. Prototype / MVP 機能

v0.1 prototype は、実際の「瑞希」+ Modular Avatar 対応衣装を複数入れた Unity Project から `.unavatar` を出力し、過不足を検証できる状態を最優先にする。

- Export target の GameObject / prefab instance 指定
- SkinnedMeshRenderer 列挙
- Mesh / index / vertex / normal / uv / skin / blendshape 抽出
- Texture 収集
- glTF PBR fallback material 生成
- Humanoid bone mapping 抽出
- Modular Avatar 対応衣装を含む wardrobe set 候補の抽出
- `UN_avatar.manifest` 生成
- `UN_avatar.humanoid` 生成
- `UN_avatar.wardrobe` 生成
- `UN_avatar.provenance` 生成
- GLB 2.0 として `.unavatar` 出力

GLB writer は package 内蔵の最小 writer に固定する。UnityGLTF は v0.1 では使用しない。v0.1 prototype では GLB の JSON chunk を post-process して `UN_avatar` root extension を差し込む。外部 glTF exporter の採用は、built-in writer の品質や validator 互換で具体的な問題が出た場合に再評価する。

## 6. Material Extraction

最初に扱う shader。

1. Unity Standard / URP Lit fallback
2. MToon
3. lilToon major parameters
4. Poiyomi limited fallback

lilToon v0.1 優先項目。

- main texture / base color
- shade texture / shade color
- normal map
- matcap
- rim
- emission
- outline width / color
- alpha mode
- cull mode
- render queue hint

Exporter は shader property の完全再現を狙わず、U.N. Avatar Runtime で自然に見える `una_toon` / MToon-like parameter へ正規化する。

## 7. Modular Avatar And Wardrobe Sets

VRC 向け衣装対応は v2 の重要機能として扱う。

Exporter は 3 つの export mode を持つ。

- `All Wardrobe Sets In One .unavatar`: 複数衣装・小物を 1 ファイルに同梱し、`UN_avatar.wardrobe.sets` で切り替える。最初から MVP として実装する。
- `Current State Only`: 現在 Unity 上で有効な状態だけを bake して出す。fallback / debug export。
- `Split Wardrobe Sets Into Separate .unavatar Files`: wardrobe set ごとに別 `.unavatar` を生成する。

MVP は `All Wardrobe Sets In One .unavatar`。Current State Only だけでは Modular Avatar 対応衣装の切替検証に不足するため、最初から wardrobe set を出す。

### Capture Diff Workflow

Exporter はユーザーに object path や blendshape 名を手入力させることを主導線にしない。Unity 上で見た目を作り、その状態差分を capture する。

1. 素体状態を整えて `Capture Base`。
2. Color 1 などの衣装状態を Unity 上で整える。
3. `Capture Wardrobe Set` で base との差分を記録する。
4. Color 13 などは既存 set を複製し、対象 outfit subtree / asset group の差分だけ変更する。

capture 対象。

- GameObject active state
- Renderer enabled
- SkinnedMeshRenderer blendshape weight
- 将来: material property, dynamics enable

### Variant Extraction Sources

候補。

- VRC Expression Menu toggle
- VRC Expression Parameters
- Modular Avatar Menu Item
- GameObject active state
- Animator / FX Layer のうち単純な object toggle と material change

v0.1 では Animator Controller の完全評価はしない。配信で使う衣装・小物切替に必要な visibility / blendshape operation を優先する。

v0.1 の Modular Avatar 方針は、MA / NDMF を再実装せず、MA が提供する bake entrypoint で複製 avatar を bake してから export する。wardrobe 候補は bake 前の Modular Avatar MenuItem / ObjectToggle / active state / VRC Expression Menu から抽出し、最終的な set は capture diff で人間が確認できる形にする。

### Runtime Asset Group Assumption

`.unavatar` は全衣装を保持できるが、Runtime は最初から outfit group 単位の lazy upload / unload を前提にする。Exporter は wardrobe set が参照する outfit group を metadata として出す。

## 8. PhysBone Extraction

PhysBone は完全互換ではなく、Runtime の軽量 dynamics へ近似する。

抽出候補。

- root transform
- endpoint / child chain
- radius
- stiffness / pull / spring / drag 相当
- gravity
- colliders
- exclusions の一部

Contacts / interactions / limits の完全再現は非目標。

## 9. Export Report

Exporter は `.unavatar` と同時に validation report を生成できるようにする。

Report に含めるもの。

- source Unity version
- VRC SDK version
- lilToon / Modular Avatar version
- exported renderer count / mesh count / material count / texture count
- generated wardrobe sets
- wardrobe source mapping
- approximations
- unsupported features
- lost features
- license / redistribution note

Prototype の report は、人間が調整相談しやすいことを優先する。JSON report に加え、Unity Editor window 上にも exported / approximated / unsupported / lost を短く表示する。

## 10. User Flow

想定 UI。

1. Unity project に U.N. Avatar Unity Exporter package を入れる。
2. VRC avatar prefab / scene object を選ぶ。
3. Export mode を選ぶ。
4. Material policy / texture embedding policy を選ぶ。
5. Validate を実行する。
6. `.unavatar` を export する。
7. U.N. Avatar Runtime / Supervisor で読み込む。

## 11. 非目標

- Unity Runtime player を作ること
- U.N. Avatar Runtime を Unity に依存させること
- VRC SDK runtime 互換
- Animator Controller / FX Layer 完全再生
- Poiyomi 完全再現
- `.unitypackage` を Runtime が直接読む構成
