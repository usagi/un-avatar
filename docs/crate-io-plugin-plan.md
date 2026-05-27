# UN Avatar リポジトリ / crate分割 / IOプラグイン化 設計仕様

> **v1 公開時点の扱い**: この文書は初期の crate / IO / plugin host 設計を追うための歴史的メモ。現在の公開概要は [`../README.md`](../README.md)、現行の v1 範囲は [`roadmap.md`](roadmap.md) と [`runtime-mvp.md`](runtime-mvp.md) を優先する。
>
> **本リポジトリでの保存**: `docs/crate-io-plugin-plan.md`（製品 **UN Avatar** / ID **`un-avatar`**）
>
> - **関連**: プロダクト全体の目的・優先度（P0〜）・フェーズ毎の機能マイルストーン・レンダ/物理/GUI の詳細設計は [`development-plan.md`](development-plan.md) を優先する。
> - **運用**: [`development-guidelines.md`](development-guidelines.md)
> - **ランタイム**: Tauri Supervisor とレンダラー子プロセス分離・配布レイアウトは [`process-renderer-gui-design.md`](process-renderer-gui-design.md)
> - **MVPランタイム**: 現在のVRM / VMC / MToon / wgpu / Spout2垂直スライスは [`runtime-mvp.md`](runtime-mvp.md)

## 1. 目的

この文書は、UN Avatar の Rust workspace 構成、crate分割方針、責務境界、IOプラグイン化、実装フェイズ、コミット単位を定義する。

UN Avatar は、Unity / Unreal Engine に依存しない独立アバターレンダラーであり、同時に VRM / glTF / BVH / FBX / USD / VRC prefab / `.blend` / UNA などの形式を扱う IO ハブでもある。

この規模のアプリでは、最初に crate 境界を誤ると、以下が起きる。

- renderer と app GUI が密結合する
- IO 実装が core 型へ過剰依存する
- FBX / USD / VRC など重い依存が本体へ流入する
- feature flag が破綻する
- テスト不能な巨大crateになる
- 将来 plugin SDK を切り出せなくなる
- UNA 形式が内部実装と癒着する

したがって、UN Avatar は最初から **core schema / runtime / renderer / IO / app / plugin** を分離する。

---

## 2. 基本方針

## 2.1 crate分割の原則

原則は以下。

1. `core` は純粋データ構造と変換ロジックのみを持つ
2. `core` は renderer / GUI / network / file dialog / GPU に依存しない
3. IO crate は core 型へ変換する責務を持つ
4. renderer は core の scene / material / skeleton を読むが、IO形式を知らない
5. app は GUI / window / tray / profile / IPC を扱うが、重いIO実装を直接抱えない
6. bridge系IOは optional crate にする
7. plugin API は core 型と安定した import/export trait だけを公開する
8. 外部形式の未対応情報は捨てず、UNA extension blob に保持する
9. feature flag でビルド対象を制御する
10. `xtask` で CI / codegen / schema export / package / release を統制する

## 2.2 dependency direction

依存方向は原則として下記。

```text
un-avatar-types
  ↑
un-avatar-core
  ↑
un-avatar-motion / un-avatar-physics / un-avatar-scene / un-avatar-material
  ↑
un-avatar-render / un-avatar-io-* / un-avatar-output-*
  ↑
un-avatar-app / un-avatar-cli
```

避けるべき依存。

```text
un-avatar-core -> un-avatar-render        禁止
un-avatar-core -> un-avatar-app           禁止
un-avatar-core -> un-avatar-io-fbx        禁止
un-avatar-render -> un-avatar-app         原則禁止
un-avatar-io-* -> un-avatar-app           禁止
un-avatar-plugin-api -> concrete plugin  禁止
```

---

## 3. 推奨workspace構成

```text
un-avatar/
├─ Cargo.toml
├─ .cargo/
│  └─ config.toml
├─ crates/
│  ├─ un-avatar-types/
│  ├─ un-avatar-core/
│  ├─ un-avatar-scene/
│  ├─ un-avatar-skeleton/
│  ├─ un-avatar-motion/
│  ├─ un-avatar-animation/
│  ├─ un-avatar-expression/
│  ├─ un-avatar-material/
│  ├─ un-avatar-physics/
│  ├─ un-avatar-render/
│  ├─ un-avatar-render-wgpu/
│  ├─ un-avatar-render-bevy/
│  ├─ un-avatar-output/
│  ├─ un-avatar-output-spout2/
│  ├─ un-avatar-output-video/
│  ├─ un-avatar-io/
│  ├─ un-avatar-io-una/
│  ├─ un-avatar-io-gltf/
│  ├─ un-avatar-io-vrm/
│  ├─ un-avatar-io-bvh/
│  ├─ un-avatar-io-usd/
│  ├─ un-avatar-io-fbx/
│  ├─ un-avatar-io-blend/
│  ├─ un-avatar-io-vrc/
│  ├─ un-avatar-plugin-api/
│  ├─ un-avatar-plugin-host/
│  ├─ un-avatar-profile/
│  ├─ un-avatar-diagnostics/
│  ├─ un-avatar-app/
│  └─ un-avatar-cli/
├─ apps/
│  ├─ desktop-tauri/
│  └─ obs-plugin/
├─ plugins/
│  ├─ io-fbx-blender-bridge/
│  ├─ io-vrc-unity-bridge/
│  └─ sample-io-plugin/
├─ schemas/
│  ├─ una/
│  ├─ unmotionframe/
│  └─ profile/
├─ docs/
│  ├─ architecture/
│  ├─ formats/
│  ├─ plugins/
│  └─ dev/
├─ test-assets/
│  ├─ vrm/
│  ├─ gltf/
│  ├─ bvh/
│  ├─ usd/
│  └─ una/
└─ xtask/
```

---

## 4. crate責務定義

## 4.1 `un-avatar-types`

### 目的

全体で共有する低レベル型を置く。

### 責務

- ID型
- UUID wrapper
- numeric型
- coordinate system enum
- timestamp型
- error code共通型
- version型
- feature capability型

### 含める例

```rust
pub struct AssetId(pub uuid::Uuid);
pub struct JointId(pub String);
pub struct MaterialId(pub String);
pub struct MotionTimestampNs(pub i64);

pub enum Handedness
{
 Left,
 Right,
}

pub enum Axis
{
 PosX,
 NegX,
 PosY,
 NegY,
 PosZ,
 NegZ,
}
```

### 禁止

- renderer依存
- file IO
- network
- GUI
- format parser

---

## 4.2 `un-avatar-core`

### 目的

UN Avatar の中心データモデルを提供する。

### 責務

- UNA内部表現の中核
- scene graphの抽象
- extension blob
- import/export report共通型
- asset dependency表現
- profileへの参照可能な安定ID

### 含めるもの

- `AvatarDocument`
- `UnaDocument`
- `SceneDocument`
- `AvatarAsset`
- `ExtensionBlob`
- `ImportReport`
- `ExportReport`
- `AssetDependency`

### 禁止

- wgpu型
- Bevy型
- Tauri型
- format-specific parser
- OBS / Spout2

---

## 4.3 `un-avatar-scene`

### 目的

レンダリング対象としての scene / avatar instance / camera / light を定義する。

### 責務

- scene graph
- avatar instance
- camera
- light rig
- render layer
- background
- composition setting

### 主要型

```rust
pub struct Scene
{
 pub avatars: Vec<AvatarInstance>,
 pub cameras: Vec<Camera>,
 pub lights: Vec<Light>,
 pub background: Background,
}
```

---

## 4.4 `un-avatar-skeleton`

### 目的

SkeletonProfile / RetargetMap / coordinate conversion を担当する。

### 責務

- humanoid joint定義
- arbitrary skeleton
- VRM humanoid mapping
- rest pose
- bind pose
- T-pose / A-pose補正
- twist bone分配
- missing bone fallback
- scale correction
- axis correction

### 重要性

このcrateは最重要基盤の1つ。BVH、FBX、VRM、glTF、USD、VRC 全ての入出力がここに依存する。

---

## 4.5 `un-avatar-motion`

### 目的

UNMotionFrame とモーション入力処理を扱う。

### 責務

- UNMotionFrame型
- MotionBuffer
- jitter buffer
- interpolation
- extrapolation
- VMC rawからの中間表現
- Zenoh受信抽象
- JSONL motion stream

### 非責務

音声リップシンク推定、視線推定、姿勢推定そのものは UNMotion 側。UN Avatar では受信と適用まで。

---

## 4.6 `un-avatar-animation`

### 目的

アニメーション再生、レイヤー合成、シーク、JSONL pose/animation IO を担当する。

### 責務

- animation clip
- timeline
- sampler
- layer
- blend mode
- masked blend
- additive blend
- pose override
- frame accurate seek

---

## 4.7 `un-avatar-expression`

### 目的

表情・BlendShape・Expression presetを扱う。

### 責務

- VRM0 BlendShapeClip相当
- VRM1 Expression相当
- ARKit系BlendShape名の受け口
- VMC blendshape名の受け口
- expression preset
- priority
- additive / override / multiply
- key / MIDI / OSC mappingの対象定義

### 注意

リップシンク推定は行わない。viseme weightを受けてアバターに適用する。

---

## 4.8 `un-avatar-material`

### 目的

材質モデルと材質変換を定義する。

### 責務

- Unlit
- Simple Lit
- MToon-like
- lilToon-like abstraction
- glTF metallic-roughness PBR
- UN-Extended-PBR
- Skin / Hair / Eye / Cloth / Teeth / Nails material
- MaterialPolicy
- conversion report
- unsupported parameter preservation

### 重要型

```rust
pub enum AvatarMaterialKind
{
 Unlit,
 SimpleLit,
 MToonLike,
 LilToonLike,
 GltfPbr,
 UnExtendedPbr,
 SkinPbr,
 HairPbr,
 EyePbr,
 ClothPbr,
 TeethPbr,
 NailsPbr,
 Debug,
}
```

---

## 4.9 `un-avatar-physics`

### 目的

SpringBone / PhysBone相当のアバター物理を提供する。

### 責務

- PhysicsBoneGroup
- PhysicsBone
- Collider
- ColliderGroup
- fixed timestep simulation
- deterministic replay向け設定
- debug geometry

### 初期Collider

- sphere
- capsule

### 将来Collider

- plane
- box
- SDF approximation
- mesh approximation

---

## 4.10 `un-avatar-render`

### 目的

renderer backend非依存の描画抽象を定義する。

### 責務

- Renderer trait
- RenderPreset
- MaterialPolicy
- LightingPolicy
- PostProcessPolicy
- RenderTarget abstraction
- alpha / color space setting
- debug view enum

### 禁止

具体的な wgpu / Bevy 実装は置かない。

---

## 4.11 `un-avatar-render-wgpu`

### 目的

wgpuベースの実レンダラー。

### 責務

- native window render target
- GPU resource管理
- mesh upload
- texture upload
- material pipeline
- skinning
- post process
- transparent background
- render-to-texture

### 優先度

MVPの第一候補。

### ネイティブウィンドウ（winit）

- **Windows**: アバター描画ウィンドウは **winit** で扱う。詳細は [`process-renderer-gui-design.md`](process-renderer-gui-design.md) §4.5。
- **GNU/Linux・macOS**: 現時点では要件を置かず、必要になった段階で winit 相当部分を **プラットフォーム別に分岐**する。

---

## 4.12 `un-avatar-render-bevy`

### 目的

Bevy backend 実験・代替実装。

### 責務

- Bevy ECS利用
- Bevy renderer統合
- prototype

### 注意

MVPでは `un-avatar-render-wgpu` を優先し、Bevy backendは実験扱いにする。Bevyの更新速度と抽象化は便利だが、UN Avatar の細かい材質・Spout2・OBS連携には自前wgpuの方が制御しやすい可能性が高い。

---

## 4.13 `un-avatar-output`

### 目的

映像・画像・動画出力の共通抽象。

### 責務

- FrameOutput trait
- ImageOutput trait
- VideoOutput trait
- alpha mode
- color metadata
- frame timestamp

---

## 4.14 `un-avatar-output-spout2`

### 目的

Spout2送信。

### 責務

- shared texture送信
- sender name
- resolution
- alpha mode
- Direct3D interop

### 注意

Windows専用。feature flagで分離する。Spout2本体はBSD-2-Clauseのため、標準配布ではビルドプロセスにSpout2取得・ビルドを組み込み、package root の `Spout.dll` と `LICENSES/Spout2-BSD-2-Clause.txt` を再配布パッケージに含める。Supervisorは package root をレンダラー子プロセス起動前に `PATH` へ追加する。ユーザー向けのSpout2 DLLフォルダ設定は提供しない。`spout-rs` は `Spout` を動的リンクするため、DLL解決は起動前に完了させる。

---

## 4.15 `un-avatar-output-video`

### 目的

動画保存。

### 責務

- mp4 + h.264
- mp4 + av1
- mkv + av1
- mkv + hevc
- 将来 FFV1 / ProRes 4444
- frame pacing
- encoder abstraction

### 実装方針

初期は ffmpeg bridge でよい。将来、利用可能ならネイティブencoder abstractionを追加する。

---

## 4.16 `un-avatar-io`

### 目的

IOプラグインの共通インターフェースとregistryを提供する。

### 責務

- Importer trait
- Exporter trait
- FormatDescriptor
- Capability
- ImportContext
- ExportContext
- ImportOptions
- ExportOptions
- registry

### 重要性

全IO形式はこのcrateのtraitを実装する。

---

## 4.17 `un-avatar-io-una`

### 目的

UNA形式の読み書き。

### 責務

- `.una`
- `.una.d/`
- manifest
- version migration
- extension blob
- original blob
- asset packing/unpacking

### 優先度

Phase 0〜1で必須。

---

## 4.18 `un-avatar-io-gltf`

### 目的

glTF 2.0 import/export。

### 責務

- mesh
- node
- skin
- animation
- material
- texture
- extension preservation

### 優先度

Phase 1でimport、Phase 3でanimation export。

---

## 4.19 `un-avatar-io-vrm`

### 目的

VRM0 / VRM1 import/export。

### 責務

- VRM metadata
- humanoid
- expression
- MToon
- SpringBone
- lookAt受け口
- VRM extensions

### 注意

glTFベースなので `un-avatar-io-gltf` を内部利用する。

---

## 4.20 `un-avatar-io-bvh`

### 目的

BVH import/export。

### 責務

- hierarchy
- channels
- frame sampling
- coordinate conversion
- retarget source skeleton生成

---

## 4.21 `un-avatar-io-usd`

### 目的

USD / UsdSkel import/export。

### 責務

- skeleton
- skinning
- animation
- material fallback
- stage metadata

### 注意

USD依存は重い可能性があるため optional feature とする。完全対応ではなく UsdSkel 中心から始める。

---

## 4.22 `un-avatar-io-fbx`

### 目的

FBX import/export。

### 実装方針

初期は bridge 前提。

- Blender bridge
- 将来 optional Autodesk FBX SDK bridge
- さらに将来 limited pure Rust reader/writer

### 注意

FBXをcoreに直接混ぜない。必ずbridge/optional扱い。

---

## 4.23 `un-avatar-io-blend`

### 目的

`.blend` import/export。

### 実装方針

自前完全解析はしない。Blender headless bridgeを使う。

### 責務

- Blender検出
- Python script生成
- temp workspace
- glTF/USD/UNA中間変換
- import/export report生成

---

## 4.24 `un-avatar-io-vrc`

### 目的

VRC prefab import/export。

### 実装方針

Unity Editor bridgeを使う。

### 責務

- Unity package bridge
- VRC Avatar Descriptor抽出
- PhysBone抽出
- Expression Menu抽出
- Animator Controller抽出
- Material/Shader情報抽出
- UNA extension blob保持

### 注意

UN Avatar ランタイムはUnity非依存。VRC IOだけUnity bridgeを使う。

---

## 4.25 `un-avatar-plugin-api`

### 目的

外部プラグインに公開する安定API。

### 責務

- plugin manifest
- ABI安定化方針
- importer/exporter traitまたはcommand protocol
- capability negotiation
- diagnostics

### 注意

Rust traitをそのまま動的プラグインABIにするのは危険。初期は out-of-process plugin protocol を推奨する。

---

## 4.26 `un-avatar-plugin-host`

### 目的

プラグイン実行・管理。

### 責務

- plugin discovery
- plugin process起動
- RPC
- sandbox設定
- timeout
- crash isolation
- version check

### bootstrap（実装済みの範囲）

- コード: `crates/un-avatar-plugin-host`
- §9.4 相当の manifest（`formats[]` を含む）をファイルから read（**`.toml` / `.json`**。拡張子なしは TOML 優先の二段試行）、**直下では `manifest.toml` を優先、無ければ `manifest.json`** を返す `discover_manifests_in_dir`（非再帰）
- 子プロセス起動、**stdio 改行区切り JSON-RPC 2.0**、`initialize` 握手（プロトコル版 `0.1`）、`import` RPC 結果を `un-avatar-io` の `ImportResult` としてデシリアライズする `PluginChild::rpc_import_path`。**`export` RPC** は `path` と `document`（`UnaDocument`）を送り、戻りを `ExportResult` として `PluginChild::rpc_export_path` がパースする。**`export` 応答 1 行**の待ちは **`UN_AVATAR_PLUGIN_RPC_EXPORT_TIMEOUT_SECS`**（有効時）、未設定・0・無効は **その子の import 応答と同じ上限**。**stdout 1 行の待ち**は **`UN_AVATAR_PLUGIN_RPC_HANDSHAKE_TIMEOUT_SECS`** / **`UN_AVATAR_PLUGIN_RPC_IMPORT_TIMEOUT_SECS`** で個別指定可能。どちらも未設定なら **`UN_AVATAR_PLUGIN_RPC_TIMEOUT_SECS`**（1〜86400、無効・0・未設定は既定 120 秒）。タイムアウト時は子を kill し **`HandshakeError::ReadTimeout`** を返す。**`UN_AVATAR_PLUGIN_RPC_SESSION_WALL_SECS`**（1〜86400、未設定・0・無効は無制限）で **子起動からの壁時計**をかけ、各応答行の待ちを **残り時間**までに切り詰める。壁を使い切ったあとの RPC は **`HandshakeError::SessionWallTimeout`**
- **`StdioJsonRpcImporter`**: manifest の **`can_import`** 形式（`primary_import_format`）から `FormatDescriptor` を組み立て `AvatarImporter` を実装。実行ファイルは manifest 同階層を優先し、なければ祖先ディレクトリの `target/{debug|release}/` を探索（workspace 開発用）。子プロセスの **カレントディレクトリ**は既定で manifest 所在ディレクトリ（bundle 根）。**`UN_AVATAR_PLUGIN_CHILD_CWD=host`**（大小無視）のときだけ `current_dir` を付けずホスト cwd に合わせる
- **`StdioJsonRpcExporter`**: manifest の **`can_export`** 形式（`primary_export_format`）から `FormatDescriptor` を組み立て `AvatarExporter` を実装（子 cwd・実行ファイル解決は importer と同じ）
- **`register_stdio_importers_from_manifest_dir`**: ディレクトリ直下の manifest から importer を生成し登録（件数を返す。個別失敗はスキップ）。**同一 `FormatId` が既にレジストリにある**状態でさらに登録するとき **stderr に警告**する（`importer_by_id` / 同点時の先勝ちは**先に登録された importer**）。
- **`register_stdio_exporters_from_manifest_dir`**: exporter 用。同一 `FormatId` の重複登録時も **stderr に警告**（`exporter_by_id` は先勝ち）。
- **`register_stdio_importers_from_plugin_root`** / **`register_stdio_exporters_from_plugin_root`**: 直下に manifest が無いとき **`UN_AVATAR_PLUGIN_DISCOVERY_MAX_DEPTH`**（既定 8、最大 64）まで幅優先で子ディレクトリを列挙し bundle を探す。`target` / `node_modules` 等はスキップ。**manifest から登録に成功したディレクトリは下に降りない**。CLI は **`--plugin-dir`**（複数可）と **`UN_AVATAR_PLUGIN_PATH`** をマージして **importer / exporter の両方**を登録する
- 結合テスト: **`plugins/sample-io-plugin`** が dev-dependency でホストを引き、`tests/rpc_stdio.rs` で握手＋ dummy import を検証。ホスト側はユニットテスト＋ `tests/manifest_discovery.rs`（循環依存回避）

未着手に近い: **より深い無制限**な discovery（いまは最大深さ＋bundle 葉で打ち切り）、**協調キャンセル**など壁時計を超えるホスト制御、**サンドボックス**（OS 別・未実装）。stdio RPC の **セッション壁時計**は **`UN_AVATAR_PLUGIN_RPC_SESSION_WALL_SECS`** で足せる。CLI の **`--plugin-dir`**（グローバル、複数可）は `register_stdio_importers_from_plugin_root` **と `register_stdio_exporters_from_plugin_root`**／ **`UN_AVATAR_PLUGIN_PATH`** に接続済み。

---

## 4.27 `un-avatar-profile`

### 目的

ユーザー設定・プロファイル管理。

### 責務

- app profile
- render profile
- input profile
- output profile
- avatar profile
- keybind profile
- MIDI mapping
- OSC mapping
- recent files

---

## 4.28 `un-avatar-diagnostics`

### 目的

ログ・診断・不具合報告支援。

### 責務

- structured log
- GPU info
- profile dump
- import/export report archive
- motion statistics
- latency statistics
- crash report
- diagnostics bundle

---

## 4.29 `un-avatar-app`

### 目的

アプリ統合層。

### 責務

- Tauri IPC endpoint
- renderer process control
- tray action
- profile loading
- plugin loading
- window management

### 禁止

- format parser本体
- renderer shader本体
- heavy bridge本体

---

## 4.30 `un-avatar-cli`

### 目的

変換・検証・自動処理用CLI。

### 用途

- `un-avatar convert input.vrm output.una`
- `un-avatar inspect avatar.una`
- `un-avatar validate avatar.una`
- `un-avatar render avatar.una --frame 100 --out out.png`
- `un-avatar record --input vmc --out motion.jsonl`

CLIはテストとCIでも重要。GUIなしで処理できる経路を確保する。

---

## 5. feature flag方針

## 5.1 workspace feature例

```toml
[features]
default = ["vrm", "gltf", "bvh", "spout2", "wgpu"]

wgpu = ["un-avatar-render-wgpu"]
bevy = ["un-avatar-render-bevy"]

una = ["un-avatar-io-una"]
gltf = ["un-avatar-io-gltf"]
vrm = ["un-avatar-io-vrm", "gltf"]
bvh = ["un-avatar-io-bvh"]
usd = ["un-avatar-io-usd"]
fbx = ["un-avatar-io-fbx"]
blend = ["un-avatar-io-blend"]
vrc = ["un-avatar-io-vrc"]

spout2 = ["un-avatar-output-spout2"]
video = ["un-avatar-output-video"]
plugin-host = ["un-avatar-plugin-host"]
```

## 5.2 platform feature

```text
Windows:
  spout2
  d3d11 interop
  tray
  webview2

macOS:
  no spout2
  Metal/wgpu
  future Syphon candidate

Linux:
  no spout2
  Vulkan/wgpu
  PipeWire / DMABUF candidate
```

Spout2はWindows専用として扱う。macOS/Linuxでは別出力方式を将来追加する。

---

## 6. IOプラグイン設計

## 6.1 目的

IO形式は増え続ける。すべてを本体へ直結すると、依存・ビルド時間・バグ・ライセンス・セキュリティリスクが増える。

したがって、IOは以下の3層に分ける。

```text
Built-in IO:
  UNA / glTF / VRM / BVH

Optional Bridge IO:
  FBX / blend / USD / VRC

External Plugin IO:
  サードパーティ形式 / 実験形式 / proprietary bridge
```

## 6.2 Importer trait

```rust
pub trait AvatarImporter
{
 fn descriptor(&self) -> FormatDescriptor;
 fn probe(&self, input: &ImportProbe) -> ImportProbeResult;
 fn import(&self, ctx: &mut ImportContext, input: ImportInput, options: ImportOptions) -> Result<ImportResult, ImportError>;
}
```

## 6.3 Exporter trait

```rust
pub trait AvatarExporter
{
 fn descriptor(&self) -> FormatDescriptor;
 fn can_export(&self, document: &UnaDocument, options: &ExportOptions) -> ExportCapability;
 fn export(&self, ctx: &mut ExportContext, document: &UnaDocument, output: ExportOutput, options: ExportOptions) -> Result<ExportResult, ExportError>;
}
```

## 6.4 FormatDescriptor

```rust
pub struct FormatDescriptor
{
 pub id: FormatId,
 pub display_name: String,
 pub extensions: Vec<String>,
 pub media_types: Vec<String>,
 pub direction: FormatDirection,
 pub capabilities: FormatCapabilities,
 pub stability: PluginStability,
 /// 任意: 外部プラグイン manifest のトップレベル `id`（stdio 由来の形式向け）。組み込みは省略。
 pub provider_plugin_id: Option<String>,
}
```

JSON 列挙（CLI `formats list --json`）では `provider_plugin_id` は **値があるときだけ**出力する。

`formats probe --json` では **入力パス**に対する importer 行に加え、同一路径を **出力パス**とみなした exporter 候補（空の `UnaDocument` で `can_export` と拡張子一致の confidence 目安）および CLI 集約フィールド **`best_exporter`** / **`best_exporter_provider_plugin_id`**（レジストリの `best_exporter_for` と一致）を出す。

```text
id: io.vrm1
display_name: VRM 1.0
extensions: ["vrm"]
capabilities:
  mesh: import/export
  skeleton: import/export
  humanoid: import/export
  material_mtoon: import/export
  spring_bone: import/export
  expression: import/export
```

## 6.5 Capability model

IOごとに、何を正確に扱えるか明示する。

```rust
pub struct FormatCapabilities
{
 pub mesh: Capability,
 pub skeleton: Capability,
 pub skinning: Capability,
 pub animation: Capability,
 pub expression: Capability,
 pub material: Capability,
 pub physics: Capability,
 pub cameras: Capability,
 pub lights: Capability,
 pub custom_extensions: Capability,
}

pub enum Capability
{
 Unsupported,
 ImportOnly,
 ExportOnly,
 ImportExport,
 Approximate,
 PreserveOnly,
}
```

`PreserveOnly` は重要。内容を理解して編集できなくても、original blobとして保持し再エクスポート時に戻せる可能性を示す。

---

## 7. Import pipeline

## 7.1 基本流れ

```text
input file
  ↓
format probe
  ↓
importer selection
  ↓
raw parse
  ↓
source document
  ↓
coordinate normalization
  ↓
skeleton profile generation
  ↓
material conversion / preservation
  ↓
physics conversion / preservation
  ↓
expression conversion / preservation
  ↓
UNA document
  ↓
import report
```

## 7.2 ImportContext

```rust
pub struct ImportContext
{
 pub asset_root: PathBuf,
 pub temp_dir: PathBuf,
 pub coordinate_policy: CoordinatePolicy,
 pub material_policy: ImportMaterialPolicy,
 pub physics_policy: ImportPhysicsPolicy,
 pub extension_policy: ExtensionPolicy,
 pub report: ImportReportBuilder,
}
```

## 7.3 ImportResult

```rust
pub struct ImportResult
{
 pub document: UnaDocument,
 pub report: ImportReport,
 pub dependencies: Vec<AssetDependency>,
 pub thumbnails: Vec<Thumbnail>,
}
```

## 7.4 ImportReport

**実装の正本（現状）**: `crates/un-avatar-core/src/lib.rs`。以下は構造の対応表（詳細フィールド名・`Serialize` はコード参照）。

```rust
pub struct ImportReport {
    /// 人間向けフラット行（ログ互換）。`push_info` 等で diagnostics と二重に積む。
    pub messages: Vec<String>,
    /// 重大度・任意 code 付きの 1 行（JSON レポート・ツール連携向け）。
    pub diagnostics: Vec<ReportMessage>,
    pub source_format: Option<FormatId>,
    pub status: ReportStatus,
    pub preserved_extensions: Vec<PreservedExtension>,
    pub approximations: Vec<Approximation>,
    pub lost_features: Vec<LostFeature>,
}
```

将来、フラット `messages` を廃止して `diagnostics` のみに寄せる場合は、`xtask` / CLI の JSON を含めて移行する。

### ExportReport（bootstrap）

export 側は import と対称に、現状は次の形（同じく `un-avatar-core`）。

```rust
pub struct ExportReport {
    pub messages: Vec<String>,
    pub diagnostics: Vec<ReportMessage>,
    pub target_format: Option<FormatId>,
    pub status: ReportStatus,
}
```

Report message分類。

```text
Info:
  問題なしの通知

Warning:
  近似変換された

Error:
  一部読み込み不能

Fatal:
  import失敗
```

---

## 8. Export pipeline

## 8.1 基本流れ

```text
UNA document
  ↓
exporter selection
  ↓
capability check
  ↓
material lowering
  ↓
physics lowering
  ↓
expression lowering
  ↓
coordinate conversion
  ↓
format write
  ↓
export report
```

## 8.2 ExportContext

```rust
pub struct ExportContext
{
 pub output_root: PathBuf,
 pub temp_dir: PathBuf,
 pub coordinate_policy: CoordinatePolicy,
 pub material_policy: ExportMaterialPolicy,
 pub physics_policy: ExportPhysicsPolicy,
 pub extension_policy: ExtensionPolicy,
 pub report: ExportReportBuilder,
}
```

## 8.3 Export policies

```rust
pub enum ExportMaterialPolicy
{
 PreserveOriginalIfPossible,
 ConvertToTargetNative,
 BakeToTextures,
 FallbackToPbr,
 FallbackToUnlit,
}

pub enum ExportPhysicsPolicy
{
 PreserveOriginalIfPossible,
 ConvertToSpringBone,
 ConvertToPhysBone,
 BakeToAnimation,
 DropWithWarning,
}
```

---

## 9. Plugin実装方式

## 9.1 初期方針

初期は **in-tree plugin crate** として始める。

理由。

- Rust traitで素直に実装できる
- 型安全
- デバッグしやすい
- CIしやすい
- MVPが速い

対象。

- UNA
- glTF
- VRM
- BVH

## 9.2 Bridge plugin

FBX / blend / VRC は bridge plugin とする。

```text
UN Avatar ホスト
  ↓
bridge controller
  ↓
external tool
  - Blender headless
  - Unity Editor
  - optional Autodesk FBX SDK helper
  ↓
temporary UNA / glTF / JSON report
  ↓
UN Avatar へ取り込み
```

## 9.3 Out-of-process plugin

将来的に外部pluginを許可する場合、Rust dynamic library ABIではなく、プロセス分離RPCを推奨する。

候補。

- stdio JSON-RPC
- MessagePack-RPC
- Cap'n Proto
- gRPC
- local socket

推奨初期。

```text
stdio JSON-RPC
```

理由。

- 実装が簡単
- 言語非依存
- sandboxしやすい
- crash isolationしやすい
- CLI pluginとしてテストしやすい

## 9.4 Plugin manifest

**オンディスク manifest**（ユーザーが編集しうる）は [`development-guidelines.md`](development-guidelines.md) の方針どおり **TOML を第一選択**とする想定。実装・互換の过渡として JSON（`manifest.json`）を読む場合もよい。下記の JSON 例は **-RPC やツール連携と同じ形の論理モデル**を示すための参考（フィールド名・意味は TOML 版と同一にする）。

```json
{
  "schema_version": "0.1",
  "id": "network.usagi.un_avatar.plugin.io.example",
  "name": "Example IO Plugin",
  "version": "0.1.0",
  "vendor": "USAGI.NETWORK",
  "entry": "un-avatar-plugin-example.exe",
  "protocol": "stdio-json-rpc",
  "capabilities": [
    "import.avatar",
    "export.avatar"
  ],
  "formats": [
    {
      "id": "example.avatar",
      "extensions": ["exampleavatar"],
      "can_import": true,
      "can_export": true
    }
  ]
}
```

## 9.5 Plugin security

外部ファイルを扱うため、安全対策を標準化する。

必須。

- timeout
- max memory目安
- temp dir隔離
- path traversal対策
- external script execution off by default
- network access禁止を推奨
- crash isolation
- import report必須

Bridge pluginでは特に注意。

- Blender Python script実行
- Unity Editor script実行
- FBX SDK helper

これらは安全な入力とは限らない。

---

## 10. Built-in IO優先順位

## 10.1 MVP built-in

```text
P1:
  UNA import/export
  glTF import
  VRM0 import
  VRM1 import
  BVH import minimum
```

## 10.2 Phase 2

```text
P2:
  glTF animation import
  BVH export
  VRM expression/export improvement
  UNA package stabilization
```

## 10.3 Phase 3

```text
P3:
  glTF export
  glTF animation export
  USD/UsdSkel basic
  Blender bridge
  FBX bridge
  VRC Unity bridge
```

## 10.4 Phase 4以降

```text
P4:
  USD material improvement
  VRC export
  blend export
  advanced material baking
  physics baking/export
```

---

## 11. app / renderer / GUI分離

## 11.1 プロセス構成候補

### 案A: 単一プロセス複数ウィンドウ

```text
Tauri app process
  ├─ settings GUI window
  └─ native avatar render window
```

長所。

- 実装が簡単
- IPCが軽い
- 配布が簡単

短所。

- GUIとrendererのクラッシュ分離が弱い
- wgpuとWebViewの相性問題が出る可能性

### 案B: app host + renderer process

```text
Tauri app process
  ├─ settings GUI
  ├─ tray
  └─ renderer child process
       └─ native avatar window
```

長所。

- rendererクラッシュを隔離できる
- renderer再起動が可能
- 将来CLI/headless rendererにも流用しやすい

短所。

- IPC設計が必要

### 推奨

MVPは案Aでもよいが、設計上は案Bへ移行可能にする。

`un-avatar-render` と `un-avatar-app` を明確に分離しておけば、後からrenderer process化できる。

---

## 12. IPC設計

GUIとrenderer間の設定更新はコマンド化する。

## 12.1 Command例

```text
LoadAvatar(path)
SetRenderPreset(preset)
SetBackgroundTransparency(enabled)
SetWindowChrome(enabled)
SetSpoutOutput(enabled)
SetSpoutSenderName(name)
SetInputSource(source)
SetProfile(profile_id)
StartRecording(options)
StopRecording()
CaptureStill(options)
ApplyExpressionPreset(id, state)
```

## 12.2 Event例

```text
AvatarLoaded
AvatarLoadFailed
RenderStatsUpdated
MotionInputConnected
MotionInputDisconnected
SpoutStarted
SpoutStopped
RecordingStarted
RecordingStopped
ImportReportGenerated
ErrorRaised
```

## 12.3 Transport

単一プロセスなら Tauri command/event。
別プロセスなら local socket / NNG / Zenoh / JSON-RPC を検討。

UN系全体との親和性を考えるなら、将来的には Zenoh / NNG 系の内部pub-subに接続可能にするのがよい。

---

## 13. `xtask` 方針

## 13.1 目的

開発支援処理を Rust で統制する。

## 13.2 コマンド案

```text
cargo xtask check
cargo xtask test
cargo xtask fmt
cargo xtask ci
cargo xtask schema
cargo xtask gen-bindings
cargo xtask build-app
cargo xtask build-renderer
cargo xtask build-plugins
cargo xtask package
cargo xtask validate-assets
cargo xtask golden-tests
cargo xtask release
```

## 13.3 schema export

`UNA`、`UNMotionFrame`、`Profile` は schema を出力する。

候補。

- JSON Schema
- TypeScript type
- Rust docs
- Markdown spec

`xtask schema` で生成する。

---

## 14. テスト方針

## 14.1 crate別テスト

```text
un-avatar-skeleton:
  coordinate conversion
  retarget mapping
  rest pose correction

un-avatar-motion:
  interpolation
  jitter buffer
  dropped frame handling

un-avatar-material:
  material conversion
  unsupported parameter preservation

un-avatar-io-*:
  import/export roundtrip
  report generation

un-avatar-render-wgpu:
  render smoke test
  golden image test
```

## 14.2 Golden test

アバター・モーション形式変換では golden test が重要。

```text
test-assets/vrm/avatar_a.vrm
  -> import
  -> UNA
  -> export VRM
  -> report check

BVH motion
  -> import
  -> retarget
  -> export BVH
  -> key joint comparison
```

## 14.3 Render golden test

```text
input:
  avatar.una
  pose.json
  render_profile.json

output:
  image.png
  stats.json
```

許容差を設けて比較する。

---

## 15. CI方針

## 15.1 基本CI

- fmt
- clippy
- test
- feature matrix
- schema generation check
- docs build

## 15.2 feature matrix例

```text
minimal:
  no default features

mvp:
  una + gltf + vrm + bvh + wgpu

windows-full:
  mvp + spout2 + video

io-heavy:
  usd + fbx + blend + vrc
```

重いbridge系は毎回CIしない。nightly / manual workflow に回す。

---

## 16. フェイズ別実装計画: crate / plugin追加版

## Phase 0: workspaceと基盤crate

### Commit 0.1: workspace bootstrap

内容。

- workspace作成
- `.cargo/config.toml`
- `xtask`
- fmt/check/test/ciコマンド

完了条件。

- `cargo xtask ci` が空実装で通る

### Commit 0.2: foundational crates

内容。

- `un-avatar-types`
- `un-avatar-core`
- `un-avatar-skeleton`
- `un-avatar-motion`
- `un-avatar-material`
- `un-avatar-io`

完了条件。

- crate依存方向が確定
- 循環依存なし

### Commit 0.3: IO trait v0

内容。

- `AvatarImporter`
- `AvatarExporter`
- `FormatDescriptor`
- `FormatCapabilities`
- `ImportContext`
- `ExportContext`
- `ImportReport`
- `ExportReport`

完了条件。

- dummy importer/exporter が実装できる

### Commit 0.4: UNA schema crate

内容。

- `un-avatar-io-una`
- UNA document serialize/deserialize
- `.una.d/` minimum
- version field

完了条件。

- 空sceneをUNAとして保存・読込できる

### Commit 0.5: Plugin API draft

内容。

- `un-avatar-plugin-api`
- plugin manifest schema
- protocol design document
- stdio JSON-RPC案

完了条件。

- built-in IOとexternal pluginの境界が文書化される

---

## Phase 1: MVP IOとレンダラー

### Commit 1.1: glTF importer crate

内容。

- `un-avatar-io-gltf`
- static mesh import
- material import
- texture dependency

完了条件。

- glTFをUNA documentへ変換できる

### Commit 1.2: VRM importer crate

内容。

- `un-avatar-io-vrm`
- VRM0/VRM1 probe
- humanoid抽出
- MToon-like抽出
- SpringBone抽出

完了条件。

- VRMをUNA documentへ変換できる

### Commit 1.3: BVH importer minimum

内容。

- `un-avatar-io-bvh`
- hierarchy
- frame data
- skeleton profile生成

完了条件。

- BVHをmotion clipへ変換できる

### Commit 1.4: renderer abstraction

内容。

- `un-avatar-render`
- Renderer trait
- render preset
- material policy
- lighting policy
- post policy

完了条件。

- backend差し替え可能な境界ができる

### Commit 1.5: wgpu renderer crate

内容。

- `un-avatar-render-wgpu`
- window render
- mesh upload
- simple material

完了条件。

- glTF/VRM由来meshを描画できる

### Commit 1.6: output abstraction

内容。

- `un-avatar-output`
- frame output trait
- image output trait
- alpha/color設定

完了条件。

- renderer frameをoutputへ渡せる

### Commit 1.7: Spout2 output crate

内容。

- `un-avatar-output-spout2`
- Windows feature
- sender name
- texture handoff

完了条件。

- OBSでSpout2受信できる

### Commit 1.8: app integration shell

内容。

- `un-avatar-app`
- Tauri IPC境界
- renderer制御
- profile読込

完了条件。

- GUIからアバター読込・Spout2 ON/OFFできる

---

## Phase 2: 実用IO / Plugin registry

### Commit 2.1: IO registry

内容。

- built-in importer/exporter registry
- format probe
- extension matching
- capability listing

完了条件。

- GUI/CLIから利用可能形式一覧を取得できる

### Commit 2.2: CLI convert command

内容。

- `un-avatar-cli convert`
- importer/exporter selection
- report output

完了条件。

- GUIなしで変換可能

### Commit 2.3: JSON report output

内容。

- ImportReport JSON
- ExportReport JSON
- diagnostics連携

完了条件。

- 変換損失が機械可読に出る

### Commit 2.4: External plugin host prototype

内容。

- `un-avatar-plugin-host`
- plugin discovery
- manifest load
- process起動
- stdio JSON-RPC handshake

完了条件。

- sample pluginとhandshakeできる（**`crates/un-avatar-plugin-host`** で `initialize` まで達成。広義の discovery は Commit 2.5 以降）。

### Commit 2.5: Sample external importer

内容。

- `plugins/sample-io-plugin`
- manifest
- dummy format import

完了条件。

- 外部プロセスが JSON-RPC `import` で **`un-avatar-io` の `ImportResult` と同一 JSON 形**を返し、ホストが型としてパースできる（**`plugins/sample-io-plugin`**／バイナリ名 `sample-io-plugin`、統合テスト `tests/rpc_stdio.rs`）。

---

## Phase 3: Bridge IO

### Commit 3.1: Blender bridge foundation

内容。

- `un-avatar-io-blend`
- Blender検出
- temp workspace
- python script runner
- security options

完了条件。

- Blender headlessを安全に起動できる

### Commit 3.2: blend import bridge

内容。

- `.blend` -> glTF/UNA intermediate
- asset collection
- report

完了条件。

- `.blend` をUN Avatar へ取り込める

### Commit 3.3: FBX import bridge

内容。

- `un-avatar-io-fbx`
- Blender経由FBX import
- report

完了条件。

- FBXをUN Avatar へ取り込める

### Commit 3.4: USD importer basic

内容。

- `un-avatar-io-usd`
- UsdSkel skeleton
- mesh binding
- animation

完了条件。

- UsdSkel basicを取り込める

### Commit 3.5: Unity/VRC bridge foundation

内容。

- `un-avatar-io-vrc`
- Unity bridge protocol
- Unity package side design
- UNA export/import bridge

完了条件。

- VRC prefab対応のbridge仕様が固まる

---

## Phase 4: plugin SDK化

### Commit 4.1: plugin API stabilization

内容。

- API version
- capability negotiation
- error model
- report model
- compatibility policy

完了条件。

- 外部plugin開発者向け仕様が書ける

### Commit 4.2: plugin SDK docs

内容。

- example importer
- example exporter
- manifest guide
- security guide
- test guide

完了条件。

- サンプルを見てpluginを書ける

### Commit 4.3: plugin distribution format

内容。

- plugin package
- signature optional
- install location
- user plugin dir
- system plugin dir

完了条件。

- pluginを配布・インストールできる

---

## 17. 最終的なcrate依存イメージ

```text
un-avatar-app
  ├─ un-avatar-profile
  ├─ un-avatar-diagnostics
  ├─ un-avatar-render
  ├─ un-avatar-render-wgpu
  ├─ un-avatar-output
  ├─ un-avatar-output-spout2
  ├─ un-avatar-io
  ├─ un-avatar-plugin-host
  └─ un-avatar-core

un-avatar-render-wgpu
  ├─ un-avatar-render
  ├─ un-avatar-scene
  ├─ un-avatar-material
  ├─ un-avatar-skeleton
  └─ un-avatar-core

un-avatar-io-vrm
  ├─ un-avatar-io
  ├─ un-avatar-io-gltf
  ├─ un-avatar-core
  ├─ un-avatar-skeleton
  ├─ un-avatar-material
  ├─ un-avatar-expression
  └─ un-avatar-physics

un-avatar-io-bvh
  ├─ un-avatar-io
  ├─ un-avatar-motion
  ├─ un-avatar-skeleton
  └─ un-avatar-core
```

---

## 18. 判断基準

## 18.1 crateを分けるべき場合

以下なら分ける。

- 依存が重い
- platform依存がある
- optional featureにしたい
- 責務が明確に異なる
- 単体テストしたい
- 将来plugin化したい
- ライセンスや外部SDKが特殊

例。

```text
Spout2:
  Windows専用なので分ける

FBX:
  SDK/bridgeが重いので分ける

VRC:
  Unity依存bridgeなので分ける

Material:
  rendererとIOの両方が使うため分ける
```

## 18.2 crateを分けない方がよい場合

以下なら急いで分けない。

- 型だけが少量
- 依存が増えない
- 責務境界が未確定
- まだ実験段階
- APIが激しく変わる

初期は `un-avatar-core` 内で実験し、安定したら分離してよい。

---

## 19. リスクと対策

## 19.1 crate過分割リスク

リスク。

- importが増える
- 開発速度が落ちる
- API調整が面倒
- feature graphが複雑化

対策。

- Phase 0では最小crateだけ実体化
- 空crateを作りすぎない
- 実装が一定量になってから分離
- `un-avatar-types` に何でも入れない

## 19.2 plugin ABIリスク

リスク。

- Rust trait object ABIは安定しない
- dynamic library pluginは壊れやすい
- OS別ロード問題

対策。

- 初期はin-tree plugin
- 外部pluginはout-of-process JSON-RPC
- manifest + capability negotiation

## 19.3 Bridge securityリスク

リスク。

- Blender Python実行
- Unity Editor script実行
- 悪意あるパス展開
- 外部プロセスcrash

対策。

- temp dir隔離
- path traversal対策
- timeout
- explicit user consent for scripts
- diagnostics report

---

## 20. 結論

UN Avatar のリポジトリ設計は、最初から以下を守る。

1. `core` は純粋データモデルとして保つ
2. renderer / GUI / IO / output を分離する
3. IOは built-in / bridge / external plugin の3層にする
4. FBX / blend / VRC はbridge扱いにする
5. 外部pluginは out-of-process protocol を基本にする
6. UNAは完全内部表現かつIOハブ形式にする
7. ImportReport / ExportReport を全IOに義務化する
8. feature flagで重い依存を切り離す
9. CLI経由でも変換・検証できるようにする
10. `xtask` でschema/codegen/CI/packageを統制する

最初に実装するべき最小セットは以下。

```text
un-avatar-types
un-avatar-core
un-avatar-skeleton
un-avatar-motion
un-avatar-material
un-avatar-io
un-avatar-io-una
un-avatar-io-gltf
un-avatar-io-vrm
un-avatar-io-bvh
un-avatar-render
un-avatar-render-wgpu
un-avatar-output
un-avatar-output-spout2
un-avatar-profile
un-avatar-app
un-avatar-cli
xtask
```

これで、MVPに必要な **VRM/glTF/BVH/UNA + VMC/UNMotionFrame + wgpu描画 + Spout2 + Tauri GUI** の骨格が成立する。

その後、USD / FBX / blend / VRC / external plugin を追加しても、core と renderer が腐りにくい。

UN Avatar は、最初から巨大アプリとして作るのではなく、**安定したcore schemaとplugin可能なIO境界を持つ小さなランタイム**として始めるべきだ。そこにレンダリング品質、形式変換、制作ツール機能を段階的に積む。これが最も壊れにくい。
