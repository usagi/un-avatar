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

Alpha mode は glTF fallback material へも反映する。transparent queue / alpha mode は `BLEND`、cutout queue / `_Cutoff` は `MASK` として出し、髪・レース・服飾の alpha 抜けを Runtime 側で扱えるようにする。Cull mode は glTF が表現できる範囲で扱い、Cull Off は `doubleSided=true`、Cull Back は `doubleSided=false` にする。Cull Front は現段階では専用表現を持たない。

Exporter は shader property の完全再現を狙わず、U.N. Avatar Runtime で自然に見える `UNToon` parameter へ正規化する。v2 の `UNToon` は lilToon-compatible を基準にし、MToon はそこへ変換する入力 profile として扱う。実装上 `mtoon` という JSON key や Rust 型名が残る段階でも、それを MToon-like 設計正本とは見なさない。

lilToon 由来 material では、本家 shader の `_UseShadow`、`_UseMatCap`、`_UseRim`、`_UseEmission` を尊重する。OFF の機能は texture / color が残っていても UNToon 側で寄与させない。`_MatCapMainStrength` / `_MatCapBlend`、`_RimMainStrength`、`_EmissionMainStrength` は v0.1 では各 color factor へ乗算して近似する。

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

preview exporter では per-set bake を行わない。Export 対象 clone は Base を適用して Modular Avatar bake するが、`wardrobe.base` と各 `wardrobe.sets` の operations は bake 後 snapshot ではなく、ユーザーが Unity 上で capture した authored snapshot / diff を正本にする。Modular Avatar bake が active state を再構成する場合でも、Renderer 側の Base / set 切替は capture した状態を復元できなければならない。

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
- texture source / fallback summary
- approximations
- unsupported features
- lost features
- license / redistribution note

Prototype の report は、人間が調整相談しやすいことを優先する。JSON report に加え、Unity Editor window 上にも exported / approximated / unsupported / lost を短く表示する。

Texture report は次を記録する。

- exported texture count
- source extension / MIME / source byte length
- output MIME / output byte length
- source bytes をそのまま使ったか、PNG fallback したか
- PNG fallback の理由

これにより `.exr` や runtime generated texture が silent に PNG 化された場合も、`.unavatar.report.json` から追跡できるようにする。

## 10. User Flow

想定 UI。

1. Unity project に U.N. Avatar Unity Exporter package を入れる。
2. VRC avatar prefab / scene object を選ぶ。
3. Export mode を選ぶ。
4. Material policy / texture embedding policy を選ぶ。
5. Validate を実行する。
6. `.unavatar` を export する。
7. U.N. Avatar Runtime / Supervisor で読み込む。

## 11. Texture Embedding Policy

Exporter は原則として Unity の texture asset 元ファイルをそのまま `.unavatar` に埋め込む。

- source bytes と MIME を保持する。PNG / JPEG に限定せず、`.unavatar` spec 側は任意 binary + MIME + metadata を受けられる前提にする。
- 元ファイルを取得できない texture、または v0.1 writer がそのまま扱えない形式だけ fallback encode で埋め込む。
- Exporter では重い再圧縮、WebP/KTX2/BCn 変換、resize を行わない。品質劣化、世代劣化、Unity 側 encoder 依存、export 時間増加を避ける。
- Exporter が `.unavatar` 内部の texture を最適化目的で置換する機能は持たない。

PNG / JPEG 非対応の pixel format は、PNG fallback だけで済ませない。

- Asset-backed EXR / HDR / KTX2 / DDS: 元ファイル bytes を `UN_avatar` texture asset として保持し、glTF core image は必要な場合だけ fallback として別に出す。
- Runtime-generated / unreadable texture: GPU readback で用途に合う形式へ取り出す。HDR / half float は `RGBAHalf` readback を優先し、KTX2 raw `RGBA16F` として格納する。
- Normal / mask / data texture: sRGB 変換を避け、linear/data として metadata に記録する。
- KTX2 encoder: v0.1 では最小 raw KTX2 writer を exporter 内蔵候補にする。BasisU / UASTC / BCn などの重い圧縮は optimizer 側の責務にする。
- glTF compatibility: `KHR_texture_basisu` は BasisU/KTX2 圧縮互換の経路として使い、非圧縮 `RGBA16F` KTX2 は `UN_avatar` extension asset として扱う。

v0.1 実装では asset-backed EXR を `UN_avatar.textureAssets` に保持する。EXR は glTF core `images` には入れず、LDR PNG fallback も自動生成しない。Exporter は EXR header の `channels` / `dataWindow` を読み、`sourcePixelFormat`、`channels`、`width`、`height` を metadata として記録する。material property は `matcapTextureIndexAsset` のように asset id を参照し、Runtime importer が decode 後に通常の texture index へ解決する。

`.unavatar` の後段最適化は別途 `un-avatar-optimizer` のような専用 CLI で扱う。optimizer は WebP / KTX2 / BCn / texture resize / dedup / wardrobe asset group 単位の再配置を担当し、Supervisor からは Optimize ボタンで呼び出せる形にする。optimizer は既定で入力 `.unavatar` を上書きせず、別名の optimized package を出力する。

## 12. 非目標

- Unity Runtime player を作ること
- U.N. Avatar Runtime を Unity に依存させること
- VRC SDK runtime 互換
- Animator Controller / FX Layer 完全再生
- Poiyomi 完全再現
- `.unitypackage` を Runtime が直接読む構成
