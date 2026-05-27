# UN Avatar 開発計画書

> **v1 公開時点の扱い**: この文書は初期設計を追うための歴史的メモ。現在のユーザー向け概要は [`../README.md`](../README.md)、現行の v1 範囲は [`roadmap.md`](roadmap.md) と [`runtime-mvp.md`](runtime-mvp.md) を優先する。
>
> **本リポジトリでの保存**: `docs/development-plan.md`（**表示名**: **UN Avatar** / **ID**: **`un-avatar`**。Rust は Cargo パッケージ `un-avatar-*`、LIB 名 `un_avatar_*` 想定）。
>
> - **クレート / workspace / IO trait / Phase 0〜4 コミット単位の正本**: [`crate-io-plugin-plan.md`](crate-io-plugin-plan.md)
> - **運用**: [`development-guidelines.md`](development-guidelines.md)
> - **中期ロードマップ**: [`roadmap.md`](roadmap.md)
> - **プロセス・Supervisor・レンダラー子プロセス・GUI/IPC**: [`process-renderer-gui-design.md`](process-renderer-gui-design.md)
> - **現在のMVPランタイム正本**: [`runtime-mvp.md`](runtime-mvp.md)（VRM / VMC / MToon / wgpu / Spout2）

## 1. 目的

UN Avatar は、Unity / Unreal Engine に依存しない、独立型の高性能アバターレンダラーアプリである。

主目的は以下の通り。

- UNMotionFrame を中核としたアバターモーション再生・表示
- VMC/UDP、UNMotionFrame/Zenoh など複数モーション入力の受信
- VRM / glTF / BVH / FBX / USD / VRC / blend / UNA などのアバター・モーション形式の入出力ハブ
- MToon系互換表示から、glTF PBR、さらに **UN Avatar** 独自の高品質アバター特化PBRまで対応
- OBS等への低遅延映像出力
- 配信、検証、制作補助、モーション再生、アバター確認のための統合ランタイム

UN Avatar は、単なる「アバター表示アプリ」ではなく、**アバター・モーション・材質・物理・レンダリングの独立ランタイム兼IOハブ** として設計する。

---

## 2. 責務分界

### 2.1 UN Avatar の責務

UN Avatar が担当するもの。

- アバターファイルの読み込み
- メッシュ、ボーン、材質、表情、物理ボーンの保持
- モーション入力の受信
- UNMotionFrame の適用
- VMC/UDP → UNMotionFrame 変換
- リターゲット
- 表情・ポーズ・アニメーションの合成
- 物理ボーンシミュレーション
- アバターレンダリング
- Spout2等への映像出力
- 静止画・動画出力
- 設定GUI
- タスクトレイ管理
- 各種アバター・モーション形式の import/export
- UNA形式による完全内部表現保存

### 2.2 UNMotion 側の責務

以下は UN Avatar の主責務にはしない。

- 音声リップシンク推定
- 視線推定
- 顔・頭部・身体・手指の姿勢推定
- カメラ入力処理
- トラッキングフィルタ
- 複数入力のトラッキング融合
- 視線制御ロジック
- ARKit / MediaPipe / RTMPose 等の推定処理

ただし、UN Avatar はこれらの結果を **UNMotionFrame または関連イベントとして受信し、表示に反映する**。

### 2.3 境界設計

UNMotion 側が出すもの。

- head pose
- body pose
- hand pose
- finger pose
- gaze direction
- eye look target
- blink / expression weight
- viseme / lip sync weight
- confidence
- timestamp

UN Avatar 側が行うもの。

- アバター固有のボーンへリターゲット
- 表情プリセットへのマッピング
- BlendShape / Expression の合成
- アニメーションレイヤーとの合成
- 物理ボーン更新
- レンダリング

---

## 3. 基本アーキテクチャ

### 3.0 現在のMVPランタイム

現時点の実装は、VRM0/VRM1読み込み、VMC Marionette入力、rest-pose-aware Humanoid retarget、MToon専用描画、wgpu renderer、Spout2出力の垂直スライスを先に安定化する。

この短期正本は [`runtime-mvp.md`](runtime-mvp.md) とする。FBX / USD / `.blend` / VRC bridge、録画、複数renderer GUI、GPU morphなどは、設計境界を保ちながらMVP安定後に進める。

### 3.1 全体構成（論理ブロック）

クレート名・分割粒度の**正本**は [`crate-io-plugin-plan.md`](crate-io-plugin-plan.md) 第3〜4章。ここでは能力の束ね方のみ示す。

- **`un-avatar-types` / `un-avatar-core`** — 共有低レベル型、UNA 内部表現の中核、`ImportReport` / `ExportReport`、拡張 blob 方針
- **`un-avatar-scene`** — レンダリング対象のシーン・カメラ・ライト・インスタンス
- **`un-avatar-skeleton`** — SkeletonProfile、RetargetMap、座標・ポーズ補正
- **`un-avatar-motion`** — UNMotionFrame 受信、VMC/UDP、Zenoh、`MotionBuffer`、補間・時刻（推定その物は UNMotion 側）
- **`un-avatar-animation` / `un-avatar-expression` / `un-avatar-material` / `un-avatar-physics`**
- **`un-avatar-render` / `un-avatar-render-wgpu`**（`un-avatar-render-bevy` は実験用オプション想定）
- **`un-avatar-io` + `un-avatar-io-*`** — 形式別 IO
- **`un-avatar-output` / `un-avatar-output-spout2` / `un-avatar-output-video`** — 映像・録画抽象
- **`un-avatar-profile` / `un-avatar-diagnostics`**
- **`un-avatar-app` / `un-avatar-cli`**
- **`un-avatar-plugin-api` / `un-avatar-plugin-host`**

---

## 4. crate分割方針

### 4.1 方針（開発計画書側の要約）

UN Avatar は機能範囲が広いため、最初から crate 境界を明確にする。

- **データ定義とランタイム処理を分離する**
- **IO 実装をコアから切り離す**
- **GUI はコアに依存してよいが、コアは GUI に依存しない**
- **レンダラー実装は内部表現に依存するが、内部表現はレンダラーに依存しない**
- **重い外部依存を feature または plugin crate へ隔離する**
- **bridge 系、FFmpeg 系、Blender / Unity 連携系は本体必須依存にしない**

### 4.2 ワークスペース・クレート名・責務・依存方向の正本

**リポジトリのディレクトリ構成、`un-avatar-types` / `un-avatar-scene` / `un-avatar-skeleton` / `un-avatar-io-blend` / `un-avatar-io-vrc` などの**物理クレート一覧**、各 crate の禁止事項、依存グラフ、feature フラグ、Import/Export パイプライン、**Phase 0〜4 の Commit 単位**は、設計仕様書 [`crate-io-plugin-plan.md`](crate-io-plugin-plan.md)（第2〜3章、第4章以降、第16章）を優先して従う。

本書（開発計画書）との表記差は、原則として **crate-io-plugin-plan を実装・ドキュメント更新の基準**とし、本書に古い名前（例: `un-avatar-io-bridge-blender`）が残る場合は同仕様書に合わせて読み替える。

---

## 5. IOプラグイン化仕様

### 5.1 目的

IOプラグイン化の目的は以下。

- FBX / USD / blend / VRC など重い依存を本体から切り離す
- 対応形式を追加しやすくする
- formatごとのcapabilityを明示する
- import/export時の損失を報告する
- 外部ツール依存のbridgeを安全に隔離する
- 将来的にサードパーティ拡張を可能にする

### 5.2 Trait・コンテキスト・3層 IO の仕様正本

`AvatarImporter` / `AvatarExporter` の具体的シグネチャ、`FormatDescriptor`、`ImportContext` / `ExportContext`、Built-in / Optional Bridge / External plugin の分け方、stdio JSON-RPC、manifest、Import/Export パイプライン図は [`crate-io-plugin-plan.md`](crate-io-plugin-plan.md) 第6〜10章を正とする（開発計画書の旧 `PluginInfo` / `UnaScene` 案は参照しない）。

### 5.3 IO プラグイン種別（製品目線）

Importer / Exporter / Converter / Validator / Asset Resolver / Bridge Adapter。

### 5.4 Capability・レポートの観点（要件チェックリスト）

形式ごとに「何を正確に扱えるか」を明示する。Capability は「読める / 書ける」だけでなく近似・拡張 blob 保持も含めて整理する。

```text
  - mesh / skeleton / skinning / morph target
  - material: basic, pbr, toon, custom
  - texture: embedded / external
  - animation: bone / morph / material
  - physics: springbone / physbone
  - expression / humanoid mapping / scene hierarchy / camera / light / metadata
  - extension preservation
```

import/export の結果には **必ず report** を付ける。重大度・種別の体系の詳細は crate-io 設計書 §7〜8 に合わせる。

**例**（ユーザー向けメッセージのイメージ）:

```text
VRM1 import:
  Info: MToon material preserved
VRC prefab import:
  Warning: lilToon material approximated; Animator preserved as extension blob
glTF export:
  Warning: UNPhysicalSkin lowered to metallic-roughness approximation
```

### 5.5 ロード方式（段階）

1. **Phase A**: in-tree の静的リンク（`un-avatar-io-una` / `gltf` / `vrm` / `bvh`）
2. **Phase B**: 必要に応じ DLL 等の動的ロード（重い SDK）
3. **Phase C**: **別プロセス**（`.blend` / FBX / VRC bridge、未信頼ファイル）— crate-io が推奨する方針に従う
4. **Phase D**: WASM 等（将来候補）

### 5.10 Bridge plugin

Bridge pluginは、UN Avatar 内部で直接ファイルを読むのではなく、外部ツールに変換を委譲する。

#### Blender bridge

対象。

- `.blend`
- FBX
- 一部USD
- DCC向け変換

構成。

```text
UN Avatar
  -> un-avatar-io-blend（Blender headless bridge）
  -> temporary workspace
  -> blender --background --python un_avatar_blender_bridge.py
  -> intermediate UNA/glTF/USD
  -> import report
```

#### Unity/VRC bridge

対象。

- VRC prefab
- VRC Avatar Descriptor
- PhysBone
- Expression Menu
- Animator Controller
- Unity material
- lilToon

構成。

```text
Unity Editor plugin
  -> export UNA package
  -> preserve VRC/Unity-specific metadata as extension

UN Avatar
  -> import UNA
  -> render/convert
```

UN Avatar ランタイムはUnityに依存しない。
VRC prefab IOだけがUnity Editor bridgeを使う。

### 5.11 Plugin security

必須方針。

- import対象パスはcanonicalizeする
- archive展開時のpath traversalを禁止
- 外部script実行は明示許可制
- network accessは原則禁止
- bridge pluginはtemporary directoryを使う
- plugin crashを本体crashにしない
- diagnostics logを残す

---

## 6. 優先度分類

### 6.1 P0: 絶対に最初から設計に入れる基盤

実装は段階的でよいが、後から足すと破綻するため、最初に仕様として確定する。

- UNA schema
- UNMotionFrame schema
- 座標系定義
- 時刻系定義
- SkeletonProfile
- RetargetMap
- Material abstraction
- Animation layer model
- Expression model
- Physics bone model
- Import/export report
- Versioning
- Extension blob
- Profile / project設定形式
- crate分割方針
- IO plugin API

### 6.2 P1: MVPに必要な実用機能

最初に「アプリとして成立する」ための機能。

- VRM0 / VRM1 import
- glTF 2.0 import
- VMC/UDP受信
- UNMotionFrame内部適用
- 基本Humanoidリターゲット
- MToon-like表示
- glTF metallic-roughness PBR表示
- SpringBone相当
- Spout2送信
- 透明背景ON/OFF
- タイトルバー・枠ON/OFF
- Tauri設定GUI
- タスクトレイ
- PNG静止画保存
- 基本プロファイル保存

### 6.3 P2: 配信・実用運用に必要

- UNMotionFrame/Zenoh受信
- MotionBuffer
- jitter buffer
- 補間・外挿
- 表情プリセット
- キーバインド
- MIDI入力
- OSC入力
- アニメーション再生・一時停止・シーク
- ポージング編集
- AVIF / WebP保存
- MP4 / MKV録画
- レンダリングプリセット
- Debug表示
- 物理ボーンCollider
- Profile複製・自動保存

### 6.4 P3: IOハブとしての価値を高める機能

- BVH import/export
- glTF bone animation import/export
- USD / UsdSkel import/export
- FBX bridge
- blend bridge
- VRC bridge
- UNA package format
- import/export loss report
- external asset dependency管理
- material conversion
- animation conversion
- physics conversion

### 6.5 P4: UN Avatar 独自の高品質性

- UN-Extended-PBR
- skin shader
- hair shader
- eye shader
- cloth shader
- teeth / nails shader
- HDR / OpenEXR
- PNG sequence
- EXR sequence
- FFV1
- ProRes 4444
- advanced post process
- TAA
- advanced color management

### 6.6 P5: 制作ツール化・拡張性

- animation editor
- material editor
- physics editor
- node / graph based expression editor
- plugin SDK
- OBS native source plugin
- Stream Deck連携
- HTTP / WebSocket local API
- deterministic replay
- diagnostics bundle

---

## 7. フェイズ計画

> **bootstrap / crate 命名の正本**: 本章は製品マイルストーンと機能単位の説明に使う。**Phase 0〜4 の具体的 Commit 粒度・順序**は [`crate-io-plugin-plan.md`](crate-io-plugin-plan.md) 第16章を優先する。クレート名・ディレクトリは同文書第3〜4章（例: `un-avatar-io-blend` / `un-avatar-io-vrc`）に合わせて読み替える。

## Phase 0: 仕様基盤の確定

### 目的

実装を始める前に、後戻りが高コストになる基盤仕様を固定する。

### 成果物

- UNA schema v0
- UNMotionFrame schema v0
- SkeletonProfile v0
- RetargetMap v0
- 座標系仕様
- 時刻同期仕様
- Material abstraction v0
- Animation layer仕様
- Expression仕様
- PhysicsBone仕様
- Profile設定仕様
- crate分割仕様
- IO plugin API v0

### Commit単位

#### Commit 0.1: Repository bootstrap

内容。

- Rust workspace作成
- `crates/` 構成作成
- `xtask` 追加
- CI雛形
- lint / fmt / testタスク定義

完了条件。

- `cargo xtask check`
- `cargo xtask test`
- `cargo xtask fmt`
- CIで通る

#### Commit 0.2: Crate architecture skeleton

内容。

- 主要crate作成
- dependency direction定義
- feature flag方針
- optional dependency方針

完了条件。

- crate間依存方向が整理されている
- 循環依存がない
- 重い依存がcoreへ入っていない

#### Commit 0.3: IO plugin API skeleton

内容。

- `un-avatar-io`
- `un-avatar-plugin-api`
- Importer / Exporter trait（**具体的シグネチャは [`crate-io-plugin-plan.md`](crate-io-plugin-plan.md) 第6章に一致させる**）
- `FormatDescriptor` / Capability / `ImportReport` / `ExportReport`

完了条件。

- 静的plugin登録ができる
- report付きimport/export APIが定義されている

#### Commit 0.4: Core crate skeleton

内容。

- `un-avatar-types` / `un-avatar-core`
- `un-avatar-scene` / `un-avatar-skeleton`
- `un-avatar-motion`
- `un-avatar-render`
- `un-avatar-io`
- `un-avatar-app`
- `un-avatar-physics`
- 各 crate の公開 API 雛形（省略する crate は crate-io Phase 0 と同じ方針で後追いしてよい）

完了条件。

- crate間依存方向が整理されている
- 循環依存がない

#### Commit 0.5: Coordinate system specification

内容。

- 座標系型定義
- handedness
- up axis
- forward axis
- unit
- quaternion convention
- matrix layout
- asset/render/motion/world/tracking spaceの区別

完了条件。

- glTF / VRM / BVH / Unity / Blender変換方針が書かれている
- 単体テストで基本変換が確認できる

#### Commit 0.6: Timebase specification

内容。

- monotonic timestamp
- source timestamp
- frame index
- clock domain
- latency estimate
- jitter estimate
- frame duration

完了条件。

- MotionFrameに時刻情報を付与できる
- 不連続・遅延・欠落を表現できる

#### Commit 0.7: UNMotionFrame v0

内容。

- joint transform
- local/world transform
- confidence
- tracking state
- velocity
- angular velocity
- expression/event拡張枠
- serialization

完了条件。

- JSON / MessagePack相当でserialize/deserialize可能
- 後方互換用version fieldあり

#### Commit 0.8: UNA schema v0

内容。

- scene
- avatar
- skeleton
- mesh
- material
- texture
- animation
- expression
- physics
- metadata
- extensions
- original blobs

完了条件。

- UNAを内部完全表現として使う方針が明文化されている
- 未対応情報をextension blobとして保持できる

#### Commit 0.9: SkeletonProfile / RetargetMap v0

内容。

- Humanoid joint定義
- VRM Humanoid mapping
- arbitrary skeleton profile
- rest pose
- bind pose
- axis correction
- scale correction
- missing joint fallback

完了条件。

- ソースSkeletonからターゲットSkeletonへの基本対応が表現できる

---

## Phase 1: 最小アバターレンダラー

### 目的

VRMまたはglTFアバターを読み込み、VMC入力で動かし、Spout2へ出せる最小実用アプリを作る。

### 開発優先度（骨組み〜 Spout2）

**プロジェクト合意**: 最初に整備するのは **Commit 1.1 から 1.12（Spout2 送出を含む）までの一連**とする。`process-renderer-gui-design.md` のプロセス分離・Supervisor は設計正本として併読しつつ、**実装の直列はウィンドウ／レンダラ／IO／VMC／リターゲット／Spout2 が優先**。**Commit 1.13（Tauri）以降**は骨組みが通ったあとの層とする。

- **1.10（表情）** … 体のリターゲットと同列で**強く推奨**（固定顔のままでは実用が厳しい）。
- **1.11（SpringBone）** … **骨組み完了の必須条件にはしない**。後追い・並行でよい。

**Phase 2 以降**（常用配信機能の本格化）、**Phase 3 Bridge**、**プラグインサンドボックス**、stdio／CLI の**骨組みに無関係な拡張**は、**上記が揃うまで保留・オプション**（止めどころの参照は [`roadmap.md`](roadmap.md)「優先ルート」）。

### 成果物

- ネイティブ描画ウィンドウ
- VRM/glTF import
- MToon-like / Simple PBR表示
- VMC/UDP受信
- 基本リターゲット
- Spout2送信
- Tauri設定GUI
- タスクトレイ

### Commit単位

#### Commit 1.1: Native avatar window

内容。

- **Windows**: **winit** でウィンドウ・イベントループを組み、**wgpu** で描画（本命）。Bevy 経路は実験扱い
- 他 OS: クロスプラットフォーム要件が出た時点で winit 相当層を分岐（`process-renderer-gui-design.md` §4.5）
- 背景色設定
- 透明背景ON/OFF
- タイトルバー・枠ON/OFF
- window activation API

完了条件。

- 独立ウィンドウでレンダリングできる
- GUIとは別プロセスまたは別ウィンドウとして扱える

#### Commit 1.2: Basic render loop

内容。

- camera
- light
- render target
- resize handling
- frame timing
- FPS計測

完了条件。

- 空sceneを安定描画できる
- GPU/CPU frame timeが取得できる

#### Commit 1.3: glTF 2.0 static import plugin

内容。

- `un-avatar-io-gltf`
- glTF mesh読み込み
- texture読み込み
- material読み込み
- node hierarchy読み込み
- skinなし静的表示
- ImportReport対応

完了条件。

- glTFモデルを表示できる
- import reportが出る

#### Commit 1.4: glTF skinning import

内容。

- skin joints
- inverse bind matrices
- skeletal animation用データ構造
- GPU skinningまたはCPU skinning初期実装

完了条件。

- skinned meshをbind poseで表示できる

#### Commit 1.5: VRM0/VRM1 import basic plugin

内容。

- `un-avatar-io-vrm`
- VRMファイル読み込み
- humanoid mapping抽出
- VRM metadata抽出
- MToon系材質情報抽出
- ImportReport対応

完了条件。

- VRMアバターを表示できる
- humanoid skeleton profileを作成できる

#### Commit 1.6: MaterialPolicy v0

内容。

- Unlit
- Simple Lit
- MToon-like basic
- glTF metallic-roughness PBR basic
- fallback material

完了条件。

- VRM/glTF材質を最低限破綻なく表示できる

#### Commit 1.7: VMC/UDP receiver

内容。

- VMC/UDP受信
- bone transform packet解析
- blendshape packet解析
- root transform解析
- VMC raw packet logging

完了条件。

- VMC入力を内部イベントへ変換できる

#### Commit 1.8: VMC to UNMotionFrame

内容。

- VMC bone data → UNMotionFrame
- VMC blendshape data → expression event
- timestamp付与
- confidence default値

完了条件。

- VMC入力がUNMotionFrameとして処理される

#### Commit 1.9: Basic retargeting

内容。

- VRM Humanoidへの基本リターゲット
- local transform適用
- root motion処理
- scale補正

完了条件。

- VMC入力でVRMアバターが動く

#### Commit 1.10: Basic expression apply

内容。

- VRM0 BlendShapeClip
- VRM1 Expression
- VMC blendshape名との対応
- weight合成

完了条件。

- VMCの表情入力がVRM表情へ反映される

#### Commit 1.11: SpringBone basic

内容。

- VRM SpringBone読み込み
- basic verletまたはsemi-implicit simulation
- gravity
- stiffness
- drag
- endpoint

完了条件。

- 髪・服飾の基本揺れが動作する

#### Commit 1.12: Spout2 output basic

内容。

- D3D11 shared textureまたはSpout2送信
- RGBA出力
- alpha mode指定
- resolution指定
- Spout2 SDK連携は `spout-rs` を基本にする。開発ビルドでは `SPOUT2_SDK_DIR`（`SpoutSender.h`）と `SPOUT2_LIB_DIR`（`Spout.lib`）を使い、実行時はSupervisorが package root の `Spout.dll` を解決できるよう、レンダラー起動前に package root を `PATH` に追加する。
- Spout2本体はBSD-2-Clauseなので、標準配布ではビルドプロセスにSpout2取得・ビルドを組み込み、`Spout.dll` と `LICENSES/Spout2-BSD-2-Clause.txt` を再配布パッケージに含める。ユーザー向けの外部DLLフォルダ指定は提供しない。

完了条件。

- OBS側で受信できる
- 透明背景が正しく合成できる

#### Commit 1.13: Tauri app shell

内容。

- Tauri起動
- Svelte5 + Vite + TypeScript構成
- 設定GUI雛形
- renderer processとのIPC

完了条件。

- GUIからレンダラー設定を変更できる

#### Commit 1.14: Tray integration

内容。

- タスクトレイ常駐
- メインアバターウィンドウ表示
- 設定GUI表示
- 終了
- 再起動

完了条件。

- trayから基本操作可能

#### Commit 1.15: Profile save/load v0

内容。

- avatar path
- input source
- render preset
- window settings
- Spout2 settings（sender名、解像度、alpha、DLL解決状態。標準は bundled、user-selected DLL directory は開発・非常用）
- TOML保存

完了条件。

- アプリ再起動後に設定復元できる

---

## Phase 2: 配信・日常運用向け実用化

### 目的

配信用・常用アプリとして成立する品質にする。

### 成果物

- UNMotionFrame/Zenoh受信
- MotionBuffer
- 表情プリセット
- キーバインド/MIDI/OSC
- アニメーション再生
- ポーズ編集
- 静止画・動画保存
- Debug表示
- レンダリングプリセット

### Commit単位

#### Commit 2.1: UNMotionFrame receiver

内容。

- UNMotionFrame stream受信
- JSONL / MessagePack / CBOR等の入力抽象
- source識別
- reconnect

完了条件。

- UNMotion由来のデータを受信できる

#### Commit 2.2: Zenoh transport

内容。

- Zenoh subscriber
- topic設定
- QoS相当設定
- reconnect
- diagnostics

完了条件。

- Zenoh経由でUNMotionFrameを受信できる

#### Commit 2.3: MotionBuffer

内容。

- jitter buffer
- interpolation
- extrapolation
- late frame handling
- dropped frame handling
- resampling

完了条件。

- 不安定な入力でも滑らかに描画できる

#### Commit 2.4: Animation playback core

内容。

- clip
- timeline
- play/pause
- seek
- speed
- loop
- frame accurate sampling

完了条件。

- アニメーションをバーで再生・一時停止・シークできる

#### Commit 2.5: Animation layer v0

内容。

- base layer
- motion input layer
- expression layer
- pose override layer
- physics layer
- override/additive/masked blend

完了条件。

- 入力モーションとポーズ・表情を合成できる

#### Commit 2.6: Pose editor basic

内容。

- skeleton tree
- joint selection
- rotate/translate gizmo
- pose save/load
- reset pose
- mirror pose

完了条件。

- 手動ポージングができる

#### Commit 2.7: Pose/animation JSONL import/export

内容。

- frameごとのjoint transform
- expression weight
- timestamp
- metadata
- import/export

完了条件。

- ポーズまたはアニメーションをJSONLで保存・再読込できる

#### Commit 2.8: Expression preset system

内容。

- expression preset定義
- ON/OFF
- toggle/hold
- priority
- blend time
- additive/override

完了条件。

- 表情プリセットを定義・切替できる

#### Commit 2.9: Keyboard binding

内容。

- keybind設定
- expression toggle
- render preset切替
- camera preset切替
- recording start/stop

完了条件。

- キーボードで主要操作ができる

#### Commit 2.10: MIDI input

内容。

- MIDI device選択
- note/control mapping
- expression toggle
- continuous weight mapping

完了条件。

- MIDI信号で表情プリセットを操作できる

#### Commit 2.11: OSC input

内容。

- OSC server
- expression
- pose event
- preset switch
- camera switch

完了条件。

- OSCで外部制御できる

#### Commit 2.12: Render presets v0

内容。

- Compatibility VRM
- Performance
- Avatar Toon
- Natural PBR
- UN-PBR placeholder
- Debug

完了条件。

- UIからレンダリングモードを切り替えられる

#### Commit 2.13: Debug render modes

内容。

- normal
- roughness
- metallic
- bone weight
- joint
- UV
- motion confidence
- wireframe

完了条件。

- 問題調査用表示ができる

#### Commit 2.14: Post process v0

内容。

- tonemap
- exposure
- bloom
- outline
- color grading
- AA mode: OFF / FXAA / SMAA / MSAA（TAA系は当面対象外）
- mipmap / anisotropic filtering
- transparent sort / hair material order
- sRGB / linear consistency

詳細方針は [`render-quality-plan.md`](render-quality-plan.md) を参照。

完了条件。

- Avatar Toon / Natural PBRの見た目が成立する

#### Commit 2.15: Image export

内容。

- PNG
- WebP
- AVIF
- alpha support
- resolution override

完了条件。

- 静止画保存ができる

#### Commit 2.16: Video export basic

内容。

- MP4 + H.264
- MP4 + AV1
- MKV + AV1
- MKV + HEVC
- audioなし/あり設定
- fixed framerate export

完了条件。

- 動画保存ができる

#### Commit 2.17: Physics collider v0

内容。

- sphere collider
- capsule collider
- collider groups
- debug display

完了条件。

- 髪や服が身体を貫通しにくくなる

#### Commit 2.18: Profile management

内容。

- profile list
- duplicate
- rename
- autosave
- recent avatars
- portable config

完了条件。

- 日常運用に耐える設定管理ができる

---

## Phase 3: IOハブ化

### 目的

UN Avatar をアバター・モーション形式変換の中心にする。

### 成果物

- UNA package
- BVH import/export
- glTF animation import/export
- USD / UsdSkel
- FBX bridge
- blend bridge
- VRC bridge
- Import/export report
- Asset dependency管理

### Commit単位

#### Commit 3.1: UNA package format plugin

内容。

- `un-avatar-io-una`
- `.una`
- `.una.d/`
- manifest
- assets
- meshes
- textures
- materials
- motions
- physics
- expressions
- original blobs

完了条件。

- UNA単一ファイルとディレクトリ形式を扱える

#### Commit 3.2: Import/export report UI

内容。

- exact
- approximated
- unsupported
- lost
- preserved as extension
- warning/error分類
- GUI表示

完了条件。

- 変換時の損失がユーザーに提示される

#### Commit 3.3: Asset dependency database

内容。

- texture path
- external file
- embedded asset
- license metadata
- thumbnail
- cache

完了条件。

- 外部依存を持つアバターを管理できる

#### Commit 3.4: BVH import plugin

内容。

- `un-avatar-io-bvh`
- hierarchy
- channels
- frame time
- motion data
- axis conversion
- scale conversion

完了条件。

- BVHモーションを読み込み、アバターへ適用できる

#### Commit 3.5: BVH export plugin

内容。

- target skeleton selection
- channel output
- frame sampling
- coordinate conversion

完了条件。

- UNMotionFrame/animationをBVHへ出力できる

#### Commit 3.6: glTF animation import

内容。

- animation sampler
- channels
- node animation
- bone animation
- interpolation

完了条件。

- glTF内アニメーションを再生できる

#### Commit 3.7: glTF animation export

内容。

- skeleton
- skin
- animation clip
- material basics
- texture basics

完了条件。

- UNA/内部アニメーションをglTFへ出力できる

#### Commit 3.8: USD / UsdSkel import basic plugin

内容。

- `un-avatar-io-usd`
- UsdSkel skeleton
- joints
- animation
- mesh binding
- material fallback

完了条件。

- UsdSkelの基本アバターを読み込める

#### Commit 3.9: USD / UsdSkel export basic

内容。

- skeleton
- skinning
- animation
- basic material

完了条件。

- 内部アバターをUsdSkelへ出力できる

#### Commit 3.10: Blender bridge design

内容。

- `un-avatar-io-blend`
- headless Blender検出
- Python bridge
- temporary workspace
- conversion command
- security setting

完了条件。

- Blenderを外部変換エンジンとして呼び出す基盤がある

#### Commit 3.11: .blend import via Blender bridge

内容。

- `.blend` → glTF/USD/UNA中間変換
- texture収集
- animation収集
- report生成

完了条件。

- `.blend` をUN Avatar へ取り込める

#### Commit 3.12: .blend export via Blender bridge

内容。

- UNA → Blender scene生成
- mesh/material/skeleton/animation出力
- report生成

完了条件。

- 内部アバターを`.blend`へ出力できる

#### Commit 3.13: FBX import via bridge

内容。

- BlenderまたはFBX SDK bridge
- mesh
- skeleton
- animation
- material fallback

完了条件。

- FBXを取り込める

#### Commit 3.14: FBX export via bridge

内容。

- skeleton
- mesh
- animation
- material fallback
- report

完了条件。

- FBXを書き出せる

#### Commit 3.15: Unity/VRC bridge design

内容。

- Unity Editor側 exporter/importer案
- VRC Avatar Descriptor相当抽出
- PhysBone抽出
- Expression Menu抽出
- Animator抽出
- Material抽出

完了条件。

- VRC prefab対応の現実的な橋渡し仕様が決まる

#### Commit 3.16: VRC prefab import via Unity bridge

内容。

- Unity project内でUNA export
- UN Avatar 側でUNA import
- VRC固有情報をextensionsへ保持

完了条件。

- VRC prefab由来アバターをUN Avatar で表示できる

#### Commit 3.17: VRC prefab export via Unity bridge

内容。

- UNA → Unity package/prefab
- PhysBone近似復元
- material fallback
- report

完了条件。

- UN Avatar からVRC向け出力ができる

---

## Phase 4: 高品質レンダリング

### 目的

UN Avatar 独自の価値である、アバター特化PBRと高品質出力を実装する。

### 成果物

- UN-Extended-PBR
- Eye shader
- Hair shader
- Skin shader
- Cloth shader
- HDR pipeline
- EXR / PNG sequence
- advanced post process

### Commit単位

#### Commit 4.1: Color management

内容。

- sRGB
- linear
- HDR
- exposure
- tonemap
- alpha mode
- premultiplied / straight alpha

完了条件。

- OBS合成や画像保存で色とαが破綻しない

#### Commit 4.2: IBL / HDR environment

内容。

- HDR environment map
- diffuse irradiance
- specular prefilter
- BRDF LUT

完了条件。

- Natural PBRが成立する

#### Commit 4.3: UN-Extended-PBR base

内容。

- clearcoat
- sheen
- transmission
- volume
- iridescence
- anisotropy
- specular color
- thin-film
- multi-scattering GGX

完了条件。

- 拡張PBR材質を表現できる

#### Commit 4.4: Eye shader

内容。

- sclera
- iris
- pupil
- cornea
- cornea IOR
- wetness
- iris parallax
- highlight

完了条件。

- アバターの目の質感が大きく向上する

#### Commit 4.5: Hair shader

内容。

- anisotropy
- tangent
- primary specular
- secondary specular
- shift
- roughnessAlong
- roughnessAcross

完了条件。

- 髪が標準PBRより自然に見える

#### Commit 4.6: Skin shader

内容。

- subsurfaceColor
- subsurfaceStrength
- curvature/thickness
- microNormal
- specularTint
- oiliness
- wrap diffuse
- screen-space subsurface blur
- curvature-based red shift
- dual-lobe specular

完了条件。

- 肌が単なるPBR材質より自然に見える

#### Commit 4.7: Cloth shader

内容。

- sheenColor
- sheenRoughness
- fuzz
- cloth normal
- roughness

完了条件。

- 布系衣装の質感が向上する

#### Commit 4.8: Teeth / nails shader

内容。

- enamel-like specular
- subtle translucency
- wetness
- nail layered specular

完了条件。

- 顔・手元のアップに耐える

#### Commit 4.9: Advanced post process

内容。

- SMAA
- MSAA
- Alpha-to-Coverage
- better bloom
- color grading LUT
- depth of field
- optional vignette

TAA系は、motion vector・履歴buffer・透明・Spout出力の整合が重いため当面保留する。

完了条件。

- 配信・撮影向け見た目を作れる

#### Commit 4.10: OpenEXR export

内容。

- HDR beauty
- optional AOV
- depth
- normal
- roughness
- metallic
- motion vector

完了条件。

- PBR検証用画像を保存できる

#### Commit 4.11: Image sequence export

内容。

- PNG sequence
- EXR sequence
- frame numbering
- metadata sidecar

完了条件。

- フレーム単位の検証・合成素材出力ができる

#### Commit 4.12: Lossless / alpha video export

内容。

- FFV1 in MKV
- ProRes 4444
- alpha handling
- color metadata

完了条件。

- 透明背景・可逆動画を保存できる

---

## Phase 5: 制作ツール化・拡張

### 目的

UN Avatar を単なるランタイムではなく、制作補助ツールとして発展させる。

### 成果物

- Material editor
- Physics editor
- Animation editor
- Plugin SDK
- OBS native source plugin
- Replay system
- Diagnostics

### Commit単位

#### Commit 5.1: Inspector

内容。

- skeleton tree
- material inspector
- texture viewer
- animation clip viewer
- expression monitor
- physics monitor
- motion frame monitor

完了条件。

- 内部状態をGUIで確認できる

#### Commit 5.2: Physics editor

内容。

- bone group編集
- collider編集
- parameter編集
- debug preview
- preset保存

完了条件。

- SpringBone/PhysBone相当をGUIで調整できる

#### Commit 5.3: Material editor

内容。

- material parameter編集
- texture差し替え
- shader選択
- render preset preview

完了条件。

- 材質をUN Avatar 上で調整できる

#### Commit 5.4: Animation editor basic

内容。

- timeline
- keyframe
- curve
- layer
- pose insertion
- clip export

完了条件。

- 簡単なアニメーション編集ができる

#### Commit 5.5: Deterministic replay

内容。

- motion stream recording
- input event recording
- fixed timestep physics
- frame exact replay

完了条件。

- 同じ入力から同じ映像を再現できる

#### Commit 5.6: Diagnostics bundle

内容。

- app log
- GPU info
- profile
- import report
- motion stats
- crash report
- anonymization option

完了条件。

- 不具合報告に必要な情報をまとめて出せる

#### Commit 5.7: Plugin API design

内容。

- importer plugin
- exporter plugin
- motion receiver plugin
- material converter plugin
- post process plugin
- input plugin

完了条件。

- 拡張ポイントが安定APIとして定義される

#### Commit 5.8: OBS native source plugin prototype

内容。

- OBS source plugin
- shared state
- texture handoffまたはOBS内描画
- alpha handling

完了条件。

- Spout2を使わずOBSに表示できる

---

## 8. レンダリング設計

### 8.1 RenderPreset

ユーザー向けプリセット。

| Preset | 目的 |
|---|---|
| Compatibility VRM | VRM/MToon-like互換重視 |
| Performance | Unlit/Simple Lit中心の軽量表示 |
| Avatar Toon | MToon-like + outline + bloom + color grading |
| Natural PBR | glTF PBR + HDRI + tonemap |
| UN-PBR | skin/hair/eye/cloth専用シェーダー有効 |
| Debug | normal, roughness, metallic, bone weight, joint, UV, motion confidence, wireframe |

### 8.2 内部ポリシー

RenderPresetは内部的に以下へ分解する。

- MaterialPolicy
- LightingPolicy
- PostProcessPolicy
- QualityPolicy
- DebugPolicy

#### MaterialPolicy

- PreserveSource
- ForceUnlit
- ForceMToonLike
- ConvertToPbr
- HybridAvatar
- UNPhysicalAvatar

#### LightingPolicy

- Flat
- SimpleStudio
- AvatarToon
- HDRI
- PhysicalUnits

#### PostProcessPolicy

- Off
- Minimal
- Toon
- Cinematic
- Debug

---

## 9. モーション設計

### 9.1 入力

- VMC/UDP
- UNMotionFrame direct
- UNMotionFrame/Zenoh
- JSONL replay
- BVH
- glTF animation
- UNA animation

### 9.2 内部処理

- MotionBuffer
- retarget
- animation layer blend
- pose override
- expression apply
- physics update
- render pose generation

### 9.3 出力

- rendered frame
- Spout2
- image/video export
- BVH export
- glTF animation export
- UNA animation export
- 将来: VMC/UDP送信
- 将来: UNMotionFrame/Zenoh送信

---

## 10. UNA形式設計方針

UNAは、UN Avatar の完全内部表現形式であり、同時に各形式間のIOハブ形式とする。

### 10.1 目的

- 変換時の情報損失を最小化する
- 未対応情報を保持する
- 再エクスポート時に可能な限り元情報を復元する
- UN独自のPBR、物理、Expression、Animation Layerを保存する
- 将来の拡張に耐える

### 10.2 パッケージ形式

- `.una`
  - 単一ファイル
  - 配布向け

- `.una.d/`
  - ディレクトリ形式
  - 開発・編集向け

### 10.3 推奨構造

- manifest
- scene
- avatars
- skeletons
- meshes
- materials
- textures
- animations
- expressions
- physics
- motions
- profiles
- metadata
- extensions
- original

### 10.4 versioning

必須項目。

- format version
- producer
- creation time
- asset UUID
- schema version
- extension version

### 10.5 extension blob

対応不能な情報は捨てずに保持する。

例。

- original VRM extensions
- original VRC PhysBone settings
- original lilToon parameters
- original Unity prefab metadata
- original USD material network
- original Blender custom properties

---

## 11. Import / Export方針

### 11.1 直接対応

優先して直接対応する。

- VRM0
- VRM1
- glTF 2.0
- BVH
- UNA

### 11.2 Bridge対応

外部ツールを使う。

- FBX
- `.blend`
- VRC prefab
- USDの高度なmaterial network

### 11.3 Bridge方針

#### Blender bridge

用途。

- `.blend` import/export
- FBX import/export
- 一部USD変換
- mesh/material/animation変換補助

#### Unity/VRC bridge

用途。

- VRC prefab import/export
- VRC Avatar Descriptor
- PhysBone
- Expression Menu
- Animator Controller
- Unity固有material情報

ランタイムはUnityに依存しない。
ただし、VRC prefab IOのための補助ブリッジとしてUnity Editorを利用する。

---

## 12. 物理ボーン設計

### 12.1 目標

- VRM SpringBone相当
- VRC PhysBone相当
- UN独自拡張
- 固定タイムステップ
- 再現性
- collider対応
- debug visualization

### 12.2 主要要素

- PhysicsBoneGroup
- PhysicsBone
- Collider
- ColliderGroup
- ParameterSet
- SimulationSpace
- UpdateMode

### 12.3 Collider

初期対応。

- sphere
- capsule

将来対応。

- plane
- box
- signed distance field
- mesh collider approximation

### 12.4 UpdateMode

- render timestep
- fixed timestep
- motion timestep

動画出力・replayでは fixed timestep を推奨する。

---

## 13. GUI設計

### 13.1 技術

- Tauri
- Svelte 5 runes
- Vite
- TypeScript

### 13.2 GUIの役割

- アバター選択
- 入力設定
- 出力設定
- レンダリング設定
- プロファイル管理
- 表情プリセット
- キーバインド
- MIDI設定
- OSC設定
- 物理設定
- Debug表示
- import/export操作

### 13.3 メインアバターウィンドウ

- ネイティブ軽量描画
- wgpuまたはBevy
- 透明背景ON/OFF
- タイトルバーON/OFF
- 枠ON/OFF
- 最前面表示ON/OFF
- 解像度設定
- Spout2出力解像度設定

### 13.4 タスクトレイ

- メインアバターウィンドウをアクティブ
- 設定GUIを開く
- プロファイル切替
- Spout2 ON/OFF
- 録画開始/停止
- 終了

---

## 14. 出力設計

### 14.1 映像出力

初期。

- Spout2

将来。

- OBS native source plugin
- NDI
- WebRTC
- SRT

### 14.2 静止画

初期。

- PNG
- WebP
- AVIF

将来。

- JPEG XL
- OpenEXR

### 14.3 動画

初期。

- MP4 + H.264
- MP4 + AV1
- MKV + AV1
- MKV + HEVC

将来。

- MKV + FFV1
- ProRes 4444
- PNG sequence
- EXR sequence

### 14.4 αと色空間

必須設定。

- straight alpha
- premultiplied alpha
- sRGB
- linear
- HDR

OBS合成では α mode と color space を明示する。

---

## 15. Debug / Diagnostics

### 15.1 Debug表示

- normal
- roughness
- metallic
- bone weight
- joint
- UV
- motion confidence
- wireframe
- collider
- physics bone
- retarget mapping
- expression weight
- frame latency

### 15.2 Diagnostics

- log
- GPU情報
- CPU情報
- profile
- renderer settings
- input settings
- import/export report
- crash report
- motion statistics
- dropped frame statistics
- latency statistics

---

## 16. MVP範囲

最初に完成と呼べる範囲は以下。

### 16.1 MVP必須

- VRM0/VRM1 import
- glTF import
- VMC/UDP受信
- VMC → UNMotionFrame
- Humanoid retarget
- MToon-like表示
- glTF PBR表示
- SpringBone basic
- Spout2送信
- 透明背景
- タスクトレイ
- Tauri設定GUI
- Profile保存
- PNG保存

### 16.2 MVPでは後回し

- FBX
- USD
- VRC prefab
- `.blend`
- UN-Extended-PBR
- Skin/Hair/Eye専用シェーダ
- 動画編集向け可逆形式
- OBS native source plugin
- plugin SDK
- full animation editor

---

## 17. 技術的リスク

### 17.1 最大リスク

- 対応形式を広げすぎること
- FBX / VRC / blend / USD の完全対応を早期に狙うこと
- 物理ボーン互換を甘く見積もること
- MToon / lilToon / PBR変換を完全変換と誤認すること
- 座標系・rest pose・retargetを後回しにすること
- GUIとレンダラーの責務が混ざること
- IO plugin APIを後から足すこと
- 重い依存がcore crateへ侵入すること

### 17.2 対策

- 直接対応形式とbridge対応形式を分ける
- UNAにoriginal blobを保持する
- import/export reportを必ず出す
- RetargetMapをPhase 0で作る
- MotionBufferをPhase 2で入れる
- レンダリングはMaterialPolicyで抽象化する
- IOは最初からplugin API越しにする
- MVPではVRM/glTF/VMC/Spout2に集中する

---

## 18. 推奨実装順まとめ

最短で実用へ持っていく順序。

1. Repository / xtask / workspace
2. crate分割
3. IO plugin API
4. Coordinate system
5. Timebase
6. UNMotionFrame
7. UNA schema
8. SkeletonProfile / RetargetMap
9. wgpu/Bevy window
10. glTF import plugin
11. VRM import plugin
12. basic material rendering
13. VMC receiver
14. VMC → UNMotionFrame
15. retarget
16. expression apply
17. SpringBone basic
18. Spout2 output
19. Tauri GUI
20. tray
21. profile save/load
22. MotionBuffer
23. UNMotionFrame/Zenoh
24. animation playback
25. pose editor
26. expression preset / keybind / MIDI / OSC
27. image/video export
28. BVH / glTF animation IO
29. UNA package
30. Blender bridge
31. FBX / blend / VRC / USD
32. UN-PBR

---

## 19. 最終到達像

UN Avatar の最終到達像は以下。

- Unity / Unrealに依存しない独立アバターレンダラー
- UNMotionFrameを中核とした高精度モーション表示
- VMC互換とUN独自Zenoh通信の両対応
- Spout2 / OBS向け低遅延出力
- VRM / glTF / BVH / FBX / USD / VRC / blend / UNA のIOハブ
- MToon互換からUN独自PBRまでの広いレンダリング表現
- SpringBone / PhysBone相当の物理ボーン
- 表情・ポーズ・アニメーションの編集・再生
- 配信・検証・制作補助を兼ねる統合ランタイム
- plugin化されたIO/出力/bridgeにより、将来の形式追加に耐える構造

UN Avatar の中核価値は、単に「アバターを表示する」ことではない。
**UNMotionFrameを受け、あらゆるアバター形式を内部表現UNAへ収束させ、互換表示と高品質表示の両方を成立させること**だ。

そのため、最初に固めるべきはレンダリングの派手な部分ではなく、次の8つだ。

- crate分割
- IO plugin API
- UNA schema
- UNMotionFrame schema
- Coordinate system
- Timebase
- SkeletonProfile
- RetargetMap

ここを間違えなければ、UN Avatar は後からいくらでも強くできる。
逆にここを軽視すると、対応形式が増えるたびに内部が腐る。
