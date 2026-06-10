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

Alpha mode は glTF fallback material へも反映する。transparent queue / alpha mode は `BLEND`、cutout queue は `MASK` として出し、髪・レース・服飾の alpha 抜けを Runtime 側で扱えるようにする。lilToon は通常の Opaque shader でも `_Cutoff` property を持つため、`_Cutoff` が存在するだけでは `MASK` にしない。Runtime はまず raw `_SrcBlend` / `_DstBlend` / `_AlphaToMask` から source blend state を読む。`_DstBlend != 0` は transparent blend、`_AlphaToMask` は mask とし、render queue は source blend state が欠ける場合の補助 hint とする。raw alpha params の `Mask` / `Blend` は明示値として優先するが、`Opaque` 相当値は `Hidden/lilToonTransparent*` / `Refraction` / `Fur` などの shader variant hint を潰さない。古い export などで source params が不足していても、Runtime は `Hidden/lilToonOutline` / `Hidden/lilToon` のように opaque variant と判定できる shader を `Opaque` として扱う。通常の `lilToon` 名だけでは透明方式を断定しない。Cull mode は glTF fallback では Cull Off を `doubleSided=true`、Cull Back を `doubleSided=false` として出し、`.unavatar` Runtime では `UN_avatar_material.floatParams` の `_Cull` / `_CullMode` から Cull Off / Front / Back を正規化して扱う。

Exporter は shader property の完全再現を狙わず、U.N. Avatar Runtime で自然に見える `UNToon` parameter へ正規化する。v2 の `UNToon` は lilToon-compatible を基準にし、MToon はそこへ変換する入力 profile として扱う。実装上 `mtoon` という JSON key や Rust 型名が残る段階でも、それを MToon-like 設計正本とは見なさない。

lilToon 由来 material では、本家 shader の `_UseShadow`、`_UseMatCap`、`_UseRim`、`_UseEmission` を尊重する。OFF の機能は texture / color が残っていても UNToon 側で寄与させない。`_MatCapMainStrength` / `_MatCapBlend`、`_RimMainStrength`、`_EmissionMainStrength` は v0.1 では各 color factor へ乗算して近似する。Runtime importer も raw `floatParams` / `colorParams` から `_UseEmission`、`_EmissionColor`、`_EmissionMainStrength` を再解釈できるようにし、正規化値と source hint の差分検証に使う。

Exporter は glTF material の `extras.UN_avatar_material` に `sourceShader`、`family`、`renderQueue`、raw `floatParams` / `colorParams`、および初期 UNToon 正規化値を保持する。Runtime は正規化値で即時表示し、raw params は lilToon 互換性を段階的に上げるための診断・再解釈用 source hint とする。全 texture property を無差別に export すると未使用 texture を膨らませるため、v0.1 では main / shade / normal / matcap / rim / emission / outline mask / reflection など実際に使う slot だけを texture index または texture asset id として保持する。

Unity の Mesh UV はそのまま glTF / wgpu 側へ渡さない。Exporter は `TEXCOORD_0.y = 1 - unityUv.y` として glTF convention に変換し、main texture の Tiling / Offset も `offset_y = 1 - scale_y - unity_offset_y` に変換して `KHR_texture_transform` と `mtoon.uvOffsetScale` へ書く。`UN_avatar.textureCoordinateConvention = "gltf"` はこの変換済みを示す。preview 中の古い `.unavatar` は互換維持対象にせず、必要なら current exporter で再出力する。

## 7. Modular Avatar And Wardrobe Sets

VRC 向け衣装対応は v2 の重要機能として扱う。

Exporter は 3 つの export mode を持つ。

- `Current Only`: 現在 Unity 上で有効な状態だけを Modular Avatar bake して出す。fallback / debug export。
- `Wardrobe (Baked)`: 現行 preview の wardrobe mode。複数衣装・小物を 1 ファイルに同梱し、`UN_avatar.wardrobe.sets` で切り替える。現時点では baked model + authored wardrobe operations を同居させる。
- `Wardrobe (Split)`: v2 本命候補。ベイク前の素体 / 衣装 / Modular Avatar 由来 source graph を保持し、wardrobe set ごとに runtime resolve / cache できる形へ寄せる。大型衣装や多数衣装で `Wardrobe (Baked)` より有利かを検証する実験 mode。

preview 実装では `Wardrobe (Split)` は Modular Avatar bake を実行せず、clone に Base operations も焼かない。inactive child も強制 ON にしない。GLB には Unity hierarchy の source graph をできるだけそのまま出し、`UN_avatar.wardrobe` に captured Base / set operations を保持する。Runtime 側の source graph resolver は未完成のため、この mode はまず `.unavatar` 出力と診断用である。

短期 preview では `Wardrobe (Baked)` を残すが、`Wardrobe (Split)` のデータ量・編集性・runtime resolve/cache の利点が大きく、致命的な欠点がなければ v2 の唯一の wardrobe mode へ昇格する。その場合 `Wardrobe (Baked)` は開発停止し、UI から外す。

2026-06-01 の実験サンプルでは、`Wardrobe (Split)` export は数秒で完了し、`Wardrobe (Baked)` の支配的コストは Modular Avatar bake だった。出力サイズも baked とほぼ同等で、texture payload が支配的なサンプルでは mode 差は 1MB 未満だった。Runtime 診断では debug build の `un-avatar-cli diagnose --wardrobe-probe-all` で import 約 25 秒、全 wardrobe probe 約 1.3 秒、各 set probe 約 300ms だった。これは診断用の document clone / visible mesh scan を含む値であり、wardrobe 合成そのものは load-time import より十分小さい。今後の本命は、Split graph から選択 set の render data を resolve/cache し、asset group 単位の lazy upload / unload へつなげること。

注意点として、Split は bake 後結果ではなく captured operations を正本にする。set が衣装 root を ON にしたとき露出する子孫を OFF にしたい場合、その子孫の `nodeEnabled=false` / `subtreeEnabled=false` が set operation に含まれていなければ Runtime は推測しない。これは意図しない破壊を避けるためで、Baked 版との差分だけを正誤判定には使わない。

Wardrobe Set は「衣装 root を 1 つ選ぶ」だけの機能ではない。Modular Avatar 対応衣装は、配布時点で色違い、パンツ / スカート、帽子、小物、演出用オブジェクトなどの細かな ON/OFF バリエーションを内包していることが多い。UNAvatar の `wardrobe.sets` は、素体側の貫通防止 blendshape、衣装 root、色、スタイル、小物 ON/OFF を合成した見た目プリセットとして扱う。`assetGroups` は lazy upload / unload のための資産単位であり、ユーザーが選ぶ wardrobe set とは 1:1 とは限らない。

`Base` は「裸の素体」ではなく、安全な初期表示状態である。配信や録画中に wardrobe reset / fallback / 操作ミスが起きても露出事故にならないよう、ユーザーはデフォルト衣装込みの状態を `Capture Base` してよい。素体 mesh や body shrink は内部 source asset / operation として扱い、ユーザー向け baseline state に素体だけの姿を強制しない。

### Capture Diff Workflow

Exporter はユーザーに object path や blendshape 名を手入力させることを主導線にしない。Unity 上で見た目を作り、その状態差分を capture する。

1. 配信中に表示されても安全な初期状態を整えて `Capture Base`。
2. Color 1 などの衣装状態を Unity 上で整える。
3. `Capture Wardrobe Set` で base との差分を記録する。
4. Color 13 などは既存 set を複製し、対象 outfit subtree / asset group の差分だけ変更する。

capture 対象。

- GameObject active state
- Renderer enabled
- SkinnedMeshRenderer blendshape weight
- 将来: material property, dynamics enable

GameObject active state は `activeSelf` を記録する。`activeInHierarchy` は親 OFF の影響を受ける実効状態であり、wardrobe の local state 正本には使わない。`subtreeEnabled` は対象 node の local enabled state を切り替える操作として扱い、子孫の実効可視は親から継承して Runtime が計算する。特定の子だけを落としたい場合は同じ set の後続 operation で `nodeEnabled=false` または `subtreeEnabled=false` を出す。Exporter は親を ON にする set の配下にある inactive child を明示的な `nodeEnabled=false` として出力し、`Color 1=true` と `Color 1/Noble Trace_Pants=false` のような組み合わせを保持する。

captured snapshot は wardrobe 状態の許可リストとして扱う。Capture / Update 後に追加された GameObject は、その snapshot に存在しないため Apply 時に OFF になり、export 時にも reference hierarchy に存在する未記録 node として `subtreeEnabled=false` が出力される。これにより、後から追加した MA 衣装が古い Base / set で勝手に有効になることを避ける。新しい衣装を特定 set に含めたい場合は、その set を Apply して衣装を ON にしてから Update する。

既存の captured set を再設定しなくても新しい diff 正規化が効くように、export 時には captured snapshot が残っている set を base snapshot から再 diff して出力する。snapshot が残っていない古い imported set は、保存済み operations をそのまま出力する。

Exporter UI の `Apply Base` / `Apply` は、captured snapshot が残っている場合は operations ではなく snapshot を直接復元する。これは UI 操作で Unity シーンを壊さないための規則であり、operations は `.unavatar` 出力と Runtime 適用用の表現として扱う。

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

Exporter report は renderer ごとの node id / path / glTF mesh / primitive / material / image index を `wardrobeAssetOwnershipDiagnostics` に出す。Package metadata 本体には、renderer path と PhysBone source path の top-level name から既存 capture と同じ `outfit:<top>` group を推定し、wardrobe set が宣言している group だけ `wardrobe.assetGroupOwnership` として自動追加する。推定できない renderer / dynamics や非 outfit group は未所有のまま残す。

## 8. PhysBone Extraction

PhysBone は完全互換ではなく、Runtime の軽量 dynamics へ近似する。

Preview exporter は VRC SDK への asmdef 直接依存を避け、`VRCPhysBone` を反射で検出する。現在有効な PhysBone component だけを `UN_avatar.dynamics[]` に出力し、Runtime importer が SpringBone-like runtime group へ lower する。

初期抽出。

- root transform
- child chain
- ignored transforms
- multi child mode
- radius
- stiffness / pull / spring
- gravity
- source collider metadata
- endpoint position
- collision / grabbing / posing / limit source hints

現段階では `drag` は runtime default 相当、limits / grabbing / posing は source metadata として保存し、Runtime importer が runtime dynamics group の `limit` / `interaction` metadata へ正規化する。`ignoreTransforms` は chain traversal の除外に使い、`multiChildType=Ignore` は最初の有効 child chain だけへ近似する。`endpointPosition` は leaf root に synthetic endpoint child を作って通常 chain へ正規化する。PhysBone source id は原則 `physbone:<transform path>` とし、同一 Transform 上に複数 component がある場合だけ `:2` 以降の ordinal を付ける。Contact source id は `contact:<transform path>:<sender|receiver>` を基本にし、同一種別の重複だけ ordinal を付ける。PhysBone collider は `sourceParams.colliders` に保存し、Sphere / Capsule は runtime solver collider と debug draw へ接続する。`insideBounds` collider は tail を collider 内側へ留める制約として近似する。limits の solver 挙動、grabbing、posing の挙動再現はまだ非対応。`allowCollision=false` は source collider を solver へ渡さない。

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
- wardrobe asset ownership diagnostics with renderer mesh/material/image indices
- dynamics source samples including PhysBone collider, limit, and interaction metadata
- texture source / fallback summary
- approximations
- unsupported features
- lost features
- license / redistribution note

Prototype の report は、人間が調整相談しやすいことを優先する。JSON report に加え、Unity Editor window 上にも exported / approximated / unsupported / lost を短く表示する。

Runtime / CLI 側の `diagnose` は、再エクスポート要否を判断できる warning を出す。特に lilToon material で `renderQueue`、`floatParams`、`colorParams` が欠けている場合は旧 exporter 生成物と見なし、UNToon 互換調整前に再エクスポートを促す。`MASK` かつ通常 cutoff の lilToon material、完全透明 helper material も warning として出し、alpha / draw skip / authoring helper の切り分けに使う。

Texture report は次を記録する。

- exported texture count
- source extension / MIME / source byte length
- output MIME / output byte length
- source bytes をそのまま使ったか、PNG fallback したか
- PNG fallback の理由

これにより `.exr` や runtime generated texture が silent に PNG 化された場合も、`.unavatar.report.json` から追跡できるようにする。

## 10. Morph Target Export

SkinnedMeshRenderer の BlendShape は、wardrobe operation で参照された名前だけに絞らず、`sharedMesh` に存在する全 BlendShape を glTF morph target として保存する。Perfect Sync / ARKit / VRC expression 入力は wardrobe 差分に現れないことが多く、ここを pruning すると `.unavatar` runtime では復元不能になるためである。

初期 weight は SkinnedMeshRenderer の現在 weight を glTF `mesh.weights` に保存する。Wardrobe の `blendShapeWeight` operation は import 時・runtime wardrobe 切替時にこの default morph state を上書きする。

## 11. User Flow

想定 UI。

1. Unity project に U.N. Avatar Unity Exporter package を入れる。
2. VRC avatar prefab / scene object を選ぶ。
3. Export mode を選ぶ。
4. Material policy / texture embedding policy を選ぶ。
5. Validate を実行する。
6. `.unavatar` を export する。
7. U.N. Avatar Runtime / Supervisor で読み込む。

## 12. Texture Embedding Policy

Exporter は原則として Unity の texture asset 元ファイルをそのまま `.unavatar` に埋め込む。

- source bytes と MIME を保持する。PNG / JPEG に限定せず、`.unavatar` spec 側は任意 binary + MIME + metadata を受けられる前提にする。
- 元ファイルを取得できない texture、または v0.1 writer がそのまま扱えない形式だけ fallback encode で埋め込む。
- Exporter では重い再圧縮、WebP/KTX2/BCn 変換、resize を行わない。品質劣化、世代劣化、Unity 側 encoder 依存、export 時間増加を避ける。
- Exporter が `.unavatar` 内部の texture を最適化目的で置換する機能は持たない。

RAW RGBA を exporter 内で生成し、PNG 化が避けられない場合の encoder 方針は [`unity-exporter-png-encoding.md`](unity-exporter-png-encoding.md) を正とする。この方針は source-backed PNG / JPEG の再エンコードではなく、wardrobe preview、generated fallback、cubemap strip などの生成画像だけに適用する。

PNG / JPEG 非対応の pixel format は、PNG fallback だけで済ませない。

- Asset-backed EXR / HDR / KTX2 / DDS: 元ファイル bytes を `UN_avatar` texture asset として保持し、glTF core image は必要な場合だけ fallback として別に出す。Radiance HDR は `image/vnd.radiance` / `RGBE8` / `rgb` / `linear` として記録する。
- Asset-backed PNG / JPEG: 元ファイル bytes を glTF core image として保持する。TextureImporter が `TextureCube` として扱う場合も、source PNG / JPEG を勝手に EXR / float / equirectangular image へ変換しない。
- Runtime-generated / unreadable texture: GPU readback で用途に合う形式へ取り出す。HDR / half float は `RGBAHalf` readback を優先し、KTX2 raw `RGBA16F` として格納する。
- Normal / mask / data texture: sRGB 変換を避け、`colorSpace` / `sRGB` / `textureType` / `textureShape` metadata に記録する。PNG fallback を作る場合も `sRGB=false` の texture は Linear readback を使う。
- KTX2 encoder: v0.1 では最小 raw KTX2 writer を exporter 内蔵候補にする。BasisU / UASTC / BCn などの重い圧縮は optimizer 側の責務にする。
- glTF compatibility: `KHR_texture_basisu` は BasisU/KTX2 圧縮互換の経路として使い、非圧縮 `RGBA16F` KTX2 は `UN_avatar` extension asset として扱う。

v0.1 実装では asset-backed EXR / HDR を `UN_avatar.textureAssets` に保持する。EXR / HDR は glTF core `images` には入れず、LDR PNG fallback も自動生成しない。Exporter は source header から `sourcePixelFormat`、`channels`、`width`、`height` を読み、Unity の filter / wrap sampler、TextureImporter の color / type / shape 情報を metadata として記録する。EXR は `channels` / `dataWindow`、Radiance HDR は resolution line から metadata を読む。TextureCube では `sourceLayout` と `unityGenerateCubemap` も記録し、Unity importer がどの cubemap 生成方式で source を解釈していたかを runtime へ渡す。material property は `matcapTextureIndexAsset` のように asset id を参照し、Runtime importer が decode 後に通常の texture index へ解決する。`_ReflectionCubeTex` の source が PNG/JPEG の場合は glTF core image、EXR/HDR 等の場合は `UN_avatar.textureAssets` とし、どちらも `textureShape=TextureCube` を保持する。Exporter は cubemap source を品質劣化する形式へ再エンコードしない。

`.unavatar` の後段最適化は別途 `un-avatar-optimizer` のような専用 CLI で扱う。optimizer は WebP / KTX2 / BCn / texture resize / dedup / wardrobe asset group 単位の再配置を担当し、Supervisor からは Optimize ボタンで呼び出せる形にする。optimizer は既定で入力 `.unavatar` を上書きせず、別名の optimized package を出力する。

## 12. 非目標

- Unity Runtime player を作ること
- U.N. Avatar Runtime を Unity に依存させること
- VRC SDK runtime 互換
- Animator Controller / FX Layer 完全再生
- Poiyomi 完全再現
- `.unitypackage` を Runtime が直接読む構成
