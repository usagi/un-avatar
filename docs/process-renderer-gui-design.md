# UN Avatar プロセス構成・レンダラー・GUI設計方針

> **v1 公開時点の扱い**: この文書は Supervisor / Renderer 分離の初期設計を追うための歴史的メモ。現在の公開概要は [`../README.md`](../README.md)、現行の v1 範囲は [`roadmap.md`](roadmap.md) と [`runtime-mvp.md`](runtime-mvp.md) を優先する。
>
> **本リポジトリでの保存**: `docs/process-renderer-gui-design.md`
>
> - **関連**: 製品全体の初期計画は [`development-plan.md`](development-plan.md)、クレート分割・IO の初期設計は [`crate-io-plugin-plan.md`](crate-io-plugin-plan.md)。
> - **整合メモ**: 本書の **`un-avatar-bridge-blender.exe` / `un-avatar-bridge-unity.exe`** 等は **配布用プロセス・バイナリ名**の例。Cargo workspace のクレート名は設計仕様書の **`un-avatar-io-blend` / `un-avatar-io-vrc`** 等を正とし、ビルド成果物のリネームまたは runner クレートで対応する。
> - **短期MVP**: 最初の安定対象は [`runtime-mvp.md`](runtime-mvp.md) の VRM / VMC / MToon / wgpu / Spout2 ランタイムとする。

## 1. 目的

本書は、UN Avatar における以下の設計方針を整理する。

- Tauri GUIプロセスとアバターレンダラー子プロセスの分離
- wgpu採用方針
- レンダラープロセス起動時のmanifest設計
- GUI技術スタック方針
- 複数アバターレンダラー子プロセスを前提としたGUIデザイン
- SupervisorとしてのTauri側の責務
- IPC、Motion routing、出力管理の基本方針

UN Avatar は単体アバター表示アプリではなく、複数のアバターレンダラーを制御できる **UN Avatar Supervisor Console** として設計する。

---

## 2. 基本方針

### 2.1 表示名と内部ID

```text
表示名: UN Avatar
内部ID: un-avatar
```

UNシリーズの命名規則に従い、ユーザー向け表示名は `UN Avatar`、リポジトリ・crate・IPC namespace・設定ディレクトリ等は `un-avatar` を用いる。

例。

```text
un-avatar
un-avatar-renderer
un-avatar-plugin-host
un-avatar-bridge-blender
un-avatar-bridge-unity
```

---

## 3. プロセス群の設計

### 3.1 標準構成

UN Avatar の標準構成は以下とする。

```text
un-avatar-supervisor.exe
  - Tauri Supervisor Console
  - tray
  - profile管理
  - renderer process supervisor
  - settings / IPC / diagnostics

un-avatar-renderer.exe
  - wgpu avatar renderer
  - avatar window
  - render loop
  - Spout2 output
  - screenshot/video capture
  - animation evaluation
  - physics update

un-avatar-plugin-host.exe
  - optional plugin host
  - importer/exporter plugin isolation
  - risky format isolation

un-avatar-bridge-blender.exe
  - optional Blender bridge supervisor

un-avatar-bridge-unity.exe
  - optional Unity/VRC bridge supervisor
```

ユーザーが直接起動するのは `un-avatar-supervisor.exe` のみとする。
レンダラーやbridge用の実行ファイルは、Supervisorが内部的に起動・停止・監視する。

### 3.2 配布ディレクトリ例

Windows配布時には、子プロセス用 `.exe` がユーザーの主操作対象に見えないように配置する。

```text
UN Avatar/
  un-avatar-supervisor.exe
  un-avatar-renderer.exe
  Spout.dll
  resources/
  plugins/
  assets/
  LICENSES/
    Spout2-BSD-2-Clause.txt
```

ショートカットやスタートメニューには `un-avatar-supervisor.exe` のみを登録する。

タスクマネージャー上では子プロセスが表示されるが、それはChromeやDiscordと同様の内部ランタイムとして扱う。

### 3.3 Tauri GUIプロセスの責務

Tauri側は、アプリ全体の管制塔である。

担当するもの。

- GUI表示
- 設定画面
- タスクトレイ
- プロファイル管理
- 複数Renderer Instance管理
- 子プロセスの起動・停止・再起動
- 子プロセスの死活監視
- IPC endpoint管理
- Motion source管理
- Motion routing
- keybind / MIDI / OSC入力管理
- import/export job管理
- diagnostics収集
- crash recovery
- autosave
- update / package管理

Tauri側が直接担当しないもの。

- GPU描画ループ
- wgpu render surface管理
- Spout2 texture生成
- avatar physics simulationの毎フレーム実行
- 高頻度render frame処理

Spout2 DLL解決はTauri側の起動責務に含める。標準配布ではビルドプロセスでSpout2を取得・ビルドし、package root の `Spout.dll` と `LICENSES/Spout2-BSD-2-Clause.txt` を同梱する。package root をレンダラー子プロセス起動前の `PATH` に追加する。ライセンス表示は [`third-party-licenses.md`](third-party-licenses.md) と配布物 `LICENSES/` で行う。ユーザー向けのSpout2 DLLフォルダ設定は提供しない。`spout-rs` は `Spout.dll` を通常の動的リンクで読むため、DLL解決はレンダラー起動前に完了させる。

### 3.4 レンダラー子プロセスの責務

`un-avatar-renderer.exe` は、アバター描画に特化した子プロセスである。

担当するもの。

- wgpu初期化
- アバター描画ウィンドウ生成
- render loop
- avatar scene保持
- animation evaluation
- physics update
- material evaluation
- post process
- Spout2 output
- screenshot capture
- video capture hook
- renderer diagnostics
- crash report出力

レンダラー子プロセスは、Tauri GUIに依存しない。
起動manifestまたはIPC経由で初期化され、IPCによって制御される。

### 3.5 子プロセス方式の利点

#### 複数アバター同時制御

```text
UN Avatar Supervisor
  ├─ Renderer: Main Avatar
  ├─ Renderer: Guest Avatar
  ├─ Renderer: Debug View
  └─ Renderer: Recording View
```

想定用途。

- 自分用アバター
- コラボ相手用アバター
- Debug skeleton / motion confidence表示
- OBS用透明背景出力
- ローカル録画用高品質PBR出力
- 別構図の同時出力
- offscreen recording
- 表情・物理・材質の比較ビュー

#### クラッシュ隔離

wgpu、GPU driver、Spout2、動画エンコード、外部アセット読み込みなどは事故要因になりうる。

レンダラーを子プロセス化することで、レンダラーがクラッシュしてもGUIは生存し、以下を実行できる。

- crash検出
- renderer再起動
- profile復元
- diagnostics bundle作成
- ユーザーへの通知
- 他のrenderer instanceの継続

#### 実験性の確保

将来的に以下を切り替えられる。

```text
un-avatar-renderer-wgpu
un-avatar-renderer-bevy-experimental
un-avatar-renderer-headless
un-avatar-renderer-debug
```

標準は `wgpu` とし、Bevy等は実験backendとして扱う。

### 3.6 子プロセス方式のデメリット

- IPC設計が必要
- 状態同期が必要
- 子プロセスの起動・監視・終了管理が必要
- 配布物が増える
- デバッグ対象ログが増える
- GPUリソース共有がプロセス境界でやや面倒
- Motion streamとcontrol commandを分けて設計する必要がある

ただし、UN Avatar の規模と将来像を考えると、このコストは許容範囲である。

---

## 4. wgpu採用方針

### 4.1 結論

UN Avatar の本命レンダラーは **wgpu直実装** とする。

Bevyは研究・試作backendとして扱ってよいが、UN Avatarの長期目標では、レンダリングパイプライン制御性が重要であるため、最終的にはwgpu直実装を主軸とする。

### 4.2 wgpuを採用する理由

UN Avatar が必要とするもの。

```text
- MToon-like
- lilToon-like
- glTF metallic-roughness PBR
- UN-Extended-PBR
- skin/hair/eye/cloth専用shader
- HDR / IBL
- Spout2出力
- α付きrender target
- OBS向け色空間制御
- straight / premultiplied alpha制御
- deterministic replay
- offscreen render
- debug AOV
- motion confidence view
- custom post process
```

これらはゲームエンジン的な汎用機能より、レンダリングパイプラインの細かい制御を要求する。
そのため、wgpu直実装の方が長期的な制御性が高い。

### 4.3 Bevyの位置づけ

Bevyには以下の利点がある。

- ECS
- asset management
- transform hierarchy
- animation基盤
- plugin構造
- rapid prototyping

一方で、UN Avatarでは以下の懸念がある。

- 独自shaderを深く扱うと内部実装へ潜る必要がある
- Tauri/Supervisorとの役割分担が曖昧になりやすい
- Spout2、透明背景、色空間、alpha処理は結局自前制御が必要
- アバター専用ランタイムとしては抽象が過剰になる可能性がある
- 子プロセス化する場合でも、Bevy App loopの主導権が設計上の制約になる

方針。

```text
標準: un-avatar-render-wgpu
実験: un-avatar-render-bevy
```

### 4.4 レンダラーcrate構成

```text
crates/
  un-avatar-render/
    renderer抽象
    RenderPreset
    MaterialPolicy
    LightingPolicy
    PostProcessPolicy
    RenderCommand/Event

  un-avatar-render-wgpu/
    wgpu実装
    native window
    render target
    Spout2向けtexture生成
    color/alpha management

  un-avatar-render-bevy/
    optional experimental backend
```

`un-avatar-render` は抽象層であり、wgpu固有の型を外へ漏らさない。

### 4.5 ネイティブウィンドウ・winit（プラットフォーム方針）

アバターレンダラープロセス（`un-avatar-renderer` / `un-avatar-render-wgpu`）がネイティブウィンドウと wgpu サーフェスを結ぶ層の方針。

- **Windows**: **winit** を採用する。サーフェス作成・イベントループ・リサイズ等は winit を前提に実装する。
- **GNU/Linux・macOS など**: **現時点では専用の設計要件を課さない。** クロスプラットフォーム対応の要求が**発生した時点**で、winit 相当のウィンドウ作成・イベント・生存期管理を **OS ごとに分岐**（または薄い抽象の背後で差し替え）する。先行して全 OS での共通化を追いかけない。

`winit` が将来のある OS で不足・不適合になった場合も、**該当プラットフォームのみ**別 backend に切り替えられるよう、wgpu の `Surface`/`Instance` 取得まわりを過度に winit 型で固定しないことを意識する。

---

## 5. レンダラープロセス起動manifest

### 5.1 目的

レンダラー子プロセス起動時には、Supervisorが子プロセスごとの起動スナップショットを生成する。

これはユーザーが通常編集するprofileではなく、以下のために使う。

- 子プロセス初期化
- デバッグ再現
- crash時の状態確認
- standalone renderer起動
- support/diagnostics

起動manifestはTOMLとする。

理由。

- 人間が読める
- Git diffが見やすい
- コメントを入れられる
- Rust側でserde + tomlとの相性がよい
- JSONより手編集しやすい
- YAMLより罠が少ない

### 5.2 manifestの位置づけ

```text
profile.toml:
  ユーザーが編集する永続設定

renderer-instance.toml:
  Supervisorが生成する子プロセス起動スナップショット

IPC:
  起動後の動的制御
```

重要な原則。

```text
TOMLは起動時の初期条件と再現性のために使う。
起動後の頻繁な更新や状態変更はIPCで行う。
```

### 5.3 ファイル配置例

```text
runtime/
  instances/
    main.renderer.toml
    guest-1.renderer.toml
    debug.renderer.toml
    recorder.renderer.toml
```

### 5.4 起動CLI

MVPでは以下を採用する。

```text
un-avatar-renderer.exe --manifest runtime/instances/main.renderer.toml
```

長期的には、IPC endpointだけを渡す方式も対応する。

```text
un-avatar-renderer.exe --instance main --control npipe://un-avatar/main/control
```

Phase別方針。

```text
Phase 1:
  --manifest <renderer-instance.toml>

Phase 2:
  --control <endpoint> で接続後に初期設定を送信
  ただし debug 用に manifest dump/load を残す

Phase 3:
  plugin-host / bridge / renderer をすべてSupervisor管理へ統合
```

### 5.5 renderer-instance.toml 例

```toml
format = "un-avatar-renderer-instance"
format_version = "0.1.0"

instance_id = "main"
display_name = "Main Avatar"
role = "main_avatar"

working_dir = "C:/Users/usagi/AppData/Roaming/UN Avatar"

[profile]
path = "profiles/main.toml"

[ipc]
control_endpoint = "npipe://un-avatar/main/control"
event_endpoint = "npipe://un-avatar/main/events"

[motion]
source = "zenoh"
endpoint = "zenoh://un-avatar/motion/main"

[renderer]
backend = "wgpu"
gpu_preference = "high_performance"
render_preset = "avatar_toon"
quality = "high"

[window]
title = "UN Avatar Renderer: Main"
transparent = true
decorations = false
input_passthrough = false
always_on_top = false
width = 1280
height = 720
x = 100
y = 100

[output.spout2]
enabled = true
name = "UN Avatar Main"
width = 1280
height = 720
alpha_mode = "straight"

[color]
space = "srgb"
alpha_mode = "straight"
hdr = false

[diagnostics]
log_level = "info"
crash_report = true
```

SupervisorのProfile storage v0は、repo内 `profiles/` をdevelopment seed、OSユーザー設定ディレクトリの `profiles/` を実ユーザー保存先として扱う。Windowsでは `%APPDATA%/UN Avatar/profiles` を使う。読み込み時は両方を見るが、同じprofile idがある場合はuser側がseed側を上書きする。GUIのNew / Duplicateはuser側にだけ書き込む。

### 5.6 起動後のIPCで扱うもの

起動manifestではなく、IPCで動的に扱う操作。

```text
LoadAvatar
SetMotionSource
SetRenderPreset
SetWindowConfig
SetOutputConfig
SetSpoutEnabled
SetExpressionPreset
SetPhysicsConfig
CaptureStill
StartRecording
StopRecording
RestartRenderer
Shutdown
```

頻繁に変わるモーションデータもmanifestには含めない。
Motion streamはUNMotionFrame/Zenoh、またはSupervisor内Motion Routerから渡す。

---

## 6. IPC設計方針

### 6.1 IPCの種類

最低限、以下を分ける。

```text
Control channel:
  GUI/Supervisor -> Renderer
  命令送信用

Event channel:
  Renderer -> GUI/Supervisor
  状態・ログ・エラー送信用

Motion channel:
  Motion Router -> Renderer
  UNMotionFrame送信用
```

Control/Eventはnamed pipeまたはlocal TCPを想定する。
MotionはZenohまたは専用Motion Routerで扱う。

MVPではEvent channelの最初の実装として、Supervisorがrenderer起動時に `--runtime-status-address 127.0.0.1:port` を渡す。rendererはそのlocal TCP endpointでstatus snapshotを返し、Supervisorの `get_renderer_runtime_status(id)` がFPS/CPU ms/GPU ms/解像度/AA/texture policy・upload summary/Spout状態/protocol/control capabilitiesをGUIへ返す。互換用に無入力接続は1接続1JSONで閉じる。Supervisor本体は接続直後に `stream\n` を送ってnewline-delimited JSON streamを購読し、rendererごとのcacheからGUI/diagnosticsへ返す。Supervisorは同じ起動契約で `--close-hotkey` も渡す。このhotkeyはrenderer processローカルで、profile/manifestではなくApp Settingsの値を使う。

Control channelとして、Supervisorは `--runtime-control-address 127.0.0.1:port` も渡す。rendererはlocal TCPでnewline-delimited JSON commandを受け、winit user event経由でevent loop / camera stateへ反映する。Supervisorはrendererごとにcontrol sessionを再利用する。1接続1commandで閉じる旧clientも互換で動き、旧text `shutdown` も受ける。

```json
{"command":"shutdown"}
{"command":"reset_camera"}
{"command":"set_camera_orbit","longitude":0.0,"latitude":0.0,"radius":2.9}
{"command":"set_clear_color","r":0.0,"g":0.0,"b":0.0,"a":0.0}
{"command":"set_spout_output","enabled":true,"name":"UN Avatar Spout","width":1280,"height":720}
{"command":"set_window","transparent":true,"input_passthrough":true}
```

SupervisorのStop/Stop Allはまず `shutdown` によるgraceful shutdownを試し、短時間で終了しない場合だけprocess killへfallbackする。App Settingsの `stop_all_on_console_exit` がonの場合は、Supervisor Consoleがtrayへ隠れず実際に閉じる時にも同じStop Allを実行する。Renderers tabのReset Viewは `reset_camera`、clear color presetは `set_clear_color`、Spout output toggle / 解像度preset（720p / 1080p / Match Window）は `set_spout_output` を送るため、renderer restartなしで反映する。
Renderers tabのBorderless / Transparent / Topmost / Click-throughは `set_window` を送るため、同じくrenderer restartなしで反映する。renderer側は枠なしwindowの四隅と上下左右をresize領域にし、cursorも対応するresize cursorへ変える。Transparent時はclear alpha 0と明示的なwindow透過alpha modeを使う。Click-throughはTransparent時だけmouse hit-testを無効化し、透明背景の背面ウィンドウ操作を優先する。ピクセル単位ではなくwindow全体がmouse操作を受けないruntime modeなので、解除はSupervisorから行う。

### 6.2 RendererCommand

例。

```text
RendererCommand
  LoadAvatar(path)
  ReloadAvatar
  SetRenderPreset(preset)
  SetMaterialPolicy(policy)
  SetLightingPolicy(policy)
  SetPostProcessPolicy(policy)
  SetWindowConfig(config)
  SetOutputConfig(config)
  SetSpoutEnabled(enabled)
  CaptureStill(settings)
  StartRecording(settings)
  StopRecording
  SetExpressionPreset(preset)
  PlayAnimation(clip)
  PauseAnimation
  SeekAnimation(time)
  Shutdown
```

### 6.3 RendererEvent

例。

```text
RendererEvent
  Ready
  StatusChanged(status)
  AvatarLoaded(report)
  ImportReport(report)
  FrameStats(stats)
  OutputStatus(status)
  RecordingStarted
  RecordingStopped
  ScreenshotSaved(path)
  Warning(message)
  Error(error)
  Crash(report)
```

### 6.4 Motion routing

複数Rendererが同じMotion sourceを使う場合がある。

```text
VMC UDP :39539
  ├─ Main Avatar
  ├─ Debug View
  └─ Recorder
```

各rendererが個別にVMCを受信すると、ポート競合や同期差が出る。
そのため、Tauri Supervisor側、または専用のMotion Routerが一度受けてUNMotionFrameへ変換し、複数rendererへ配る構成を推奨する。

```text
Motion Receiver / Router
  - VMC/UDP受信
  - UNMotionFrame化
  - MotionBuffer
  - 複数Rendererへ配信
```

GUI上ではMotion Sourceとして管理する。

```text
Motion Sources
  ● Main VMC Input :39539
      used by: Main Avatar, Debug View, Recorder
```

---

## 7. GUI技術スタック方針

### 7.1 採用技術

Tauri GUI側は以下を採用する。

```text
- Tauri
- Svelte 5 runes
- Vite
- TypeScript
```

TauriはアプリのSupervisorとして扱い、GUI・tray・設定・IPC・プロセス管理を担当する。

Svelte 5 runes + Vite + TypeScript は、設定画面、複数Renderer管理、Flowgraph UI、プロファイル管理に適している。

### 7.2 GUIの役割

GUIは単なる設定画面ではなく、複数Renderer Instanceを管理するSupervisor Consoleである。

担当する画面。

- Dashboard
- Renderer Instance一覧
- Renderer Detail
- Motion Sources
- Profiles
- Avatar Library
- Import/Export Jobs
- Output settings
- Controls / keybind / MIDI / OSC
- Logs
- Diagnostics
- Advanced
- 将来: Flowgraph editor

### 7.3 Flowgraphの位置づけ

VAC-v2で開発中のFlowgraph engineをUN Avatarへ入れる場合、初期MVPの生命維持系にはしない。

Flowgraphは以下の層に置く。

```text
UN Avatar Core Runtime:
  必須機能。Rust/Tauri commandで明示実装。

UN Flowgraph Layer:
  Automation / Control Graph。
  起動後の柔軟な制御。
```

Flowgraphが担当するとよいもの。

- 表情制御
- MIDI / keybind / OSCルーティング
- Renderer preset切替
- Spout2 ON/OFF
- 録画開始/停止
- Twitch EventSub連携
- motion confidenceに応じたfallback
- renderer crash時の自動復旧動作
- 時間制御
- 演出制御

Flowgraphに背負わせないもの。

- renderer子プロセス起動の根幹
- IPC protocolの根幹
- profile load/saveの基本
- VMC/Zenoh受信の基本
- render loop
- crash recoveryの最低限

---

## 8. 複数レンダラー前提のGUIデザイン

### 8.1 基本概念

UN Avatar GUIは、複数のRenderer Instanceを管理する。

```text
UN Avatar GUI / Supervisor
  ├─ Renderer Instance: main
  ├─ Renderer Instance: guest-1
  ├─ Renderer Instance: debug
  └─ Renderer Instance: recorder
```

ユーザーに見せる名前。

```text
Main Avatar
Guest Avatar
Debug View
Recording View
```

内部ID。

```text
main
guest-1
debug
recorder
```

### 8.2 最上位レイアウト

基本は、左にRenderer一覧、右に選択中Rendererの詳細を置く。

```text
┌──────────────────────────────────────────────┐
│ UN Avatar                                    │
├───────────────┬──────────────────────────────┤
│ Renderers     │ Main Avatar                  │
│               │                              │
│ ● Main        │ [Status] Running             │
│ ○ Guest       │ [Avatar] dr-usagi.una.d      │
│ ○ Debug       │ [Input] VMC UDP :39539       │
│ ○ Recorder    │ [Output] Spout2 ON           │
│               │                              │
│ [+ Add]       │ Tabs:                        │
│ [Start All]   │  Overview | Avatar | Motion  │
│ [Stop All]    │  Render | Output | Controls  │
│               │  Debug | Logs | Advanced     │
└───────────────┴──────────────────────────────┘
```

### 8.3 RendererStatus

GUI側では各rendererを状態機械として扱う。

```text
RendererStatus
  NotStarted
  Starting
  Running
  Stopping
  Stopped
  Crashed
  Restarting
  Unresponsive
```

UI表示例。

```text
● Running
◐ Starting
○ Stopped
▲ Unresponsive
✖ Crashed
```

### 8.4 RendererInstance model

TypeScript側の概念例。

```ts
type RendererInstance = {
  id: string;
  name: string;
  role: RendererRole;
  status: RendererStatus;
  pid?: number;
  avatar?: AvatarRef;
  motion: MotionConfig;
  render: RenderConfig;
  window: WindowConfig;
  output: OutputConfig;
  stats: RendererStats;
  lastError?: string;
};
```

Supervisor全体。

```ts
type SupervisorState = {
  renderers: RendererInstance[];
  selectedRendererId?: string;
  motionSources: MotionSource[];
  globalProfiles: ProfileRef[];
  notifications: AppNotification[];
};
```

GUIは `selectedRendererId` に対するdetail editorとして設計する。

### 8.5 RendererRole

roleを持たせることで、テンプレートや既定値を変えられる。

```text
RendererRole
  MainAvatar
  GuestAvatar
  DebugView
  Recorder
  Preview
  Offscreen
```

roleごとの用途。

| Role | 既定用途 |
| --- | --- |
| MainAvatar | 自分のメイン表示 |
| GuestAvatar | コラボ相手 |
| DebugView | 骨・信頼度・wireframe確認 |
| Recorder | 高品質録画・offscreen |
| Preview | 設定確認用 |
| Offscreen | OBS/動画出力専用 |

### 8.6 Renderer一覧の操作

最初から入れたい操作。

```text
Start Renderer
Stop Renderer
Restart Renderer
Duplicate Renderer
Rename Renderer
Open Renderer Window
Hide Renderer Window
Focus Renderer Window
Reset Renderer
Export Diagnostics
```

複数管理で欲しい操作。

```text
Start All
Stop All
Restart Crashed
Apply Preset to Selected
Apply Motion Source to Selected
Sync Render Settings
Sync Camera Settings
Duplicate as Debug View
Duplicate as Recorder
```

### 8.7 Renderer Detail Tabs

選択中Rendererの詳細画面はタブ構成とする。

```text
Overview
Avatar
Motion
Render
Window
Output
Controls
Debug
Logs
Advanced
```

#### Overview

- status
- avatar thumbnail
- current FPS
- motion latency
- output status
- process status
- last error

#### Avatar

- avatar file
- reload
- import report
- skeleton profile
- expression list
- physics info

#### Motion

- input type
- VMC/UDP
- UNMotionFrame/Zenoh
- JSONL replay
- source status
- retarget map
- motion confidence

#### Render

- preset
- quality
- material policy
- lighting policy
- post process
- background
- color management

#### Window

- transparent
- decorations
- always on top
- position
- size
- display
- capture-safe mode

#### Output

- Spout2 enabled
- Spout name
- output resolution
- alpha mode
- screenshot
- recording

#### Controls

- keybinds
- MIDI
- OSC
- expression preset
- animation trigger

#### Debug

- debug render mode
- skeleton overlay
- collider overlay
- motion confidence overlay
- GPU timings

#### Logs

- renderer log
- supervisor log
- crash log
- diagnostics export

MVP diagnostics export writes `target/tmp/diagnostics/un-avatar-supervisor-<unix-secs>.json` from the Logs tab. The bundle includes Supervisor version, repo root, frontend/package version, git HEAD, current executable, renderer executable path, app settings, native notification status, profile storage directories, Avatar Settings, tray Launch candidates, renderer process snapshots, runtime endpoint/status snapshots including protocol/control capabilities, stderr tails, and in-app notifications. The Logs tab lists existing bundles newest-first, can filter history, can preview JSON in place with a structured summary, automatic findings, renderer drilldown, and search match counts, can compare two bundles by time/size plus key bundle fields and renderer deltas, can reveal them in the OS file manager, and can create same-basename `.zip` archives for sharing. Automatic findings include Spout runtime notes and texture compression fallback notes derived from runtime status.

#### Advanced

- PID
- IPC endpoint
- manifest path
- renderer backend
- launch arguments
- environment variables

### 8.8 ユーザーに「プロセス」を見せない

通常UIでは「子プロセス」「PID」「exe」を前面に出さない。

通常ユーザー向け表示。

```text
Main Avatar is running
Guest Avatar is stopped
Debug View crashed
```

Advanced/Debug表示。

```text
Process ID: 12345
IPC endpoint: npipe://un-avatar/main/control
Manifest: runtime/instances/main.renderer.toml
Executable: resources/runtimes/un-avatar-renderer.exe
```

### 8.9 profile.tomlにおける複数Renderer定義

例。

```toml
profile_version = "0.1.0"
name = "Default Streaming Setup"

[[renderers]]
id = "main"
name = "Main Avatar"
role = "main_avatar"
enabled = true
auto_start = true

[renderers.avatar]
path = "avatars/dr-usagi.una.d"

[renderers.motion]
source = "main-vmc"

[renderers.window]
transparent = true
decorations = false
input_passthrough = false
always_on_top = false
width = 1280
height = 720

[renderers.output.spout2]
enabled = true
name = "UN Avatar Main"
width = 1280
height = 720
alpha_mode = "straight"

[renderers.render]
preset = "avatar_toon"
quality = "high"

[[renderers]]
id = "debug"
name = "Debug View"
role = "debug_view"
enabled = false
auto_start = false

[renderers.motion]
source = "main-vmc"

[renderers.render]
preset = "debug"
quality = "medium"

[[motion_sources]]
id = "main-vmc"
name = "Main VMC Input"
type = "vmc_udp"
host = "127.0.0.1"
port = 39539
```

---

## 9. Supervisor設計

### 9.1 Renderer Instance Manager

Tauri/Supervisor側に `RendererInstanceManager` を置く。

責務。

- RendererInstance一覧保持
- 子プロセス起動
- 子プロセス停止
- crash検出
- restart制御
- IPC endpoint発行
- renderer-instance.toml生成
- RendererEvent集約
- GUI state更新

### 9.2 Motion Source Manager

`MotionSourceManager` を置く。

責務。

- VMC/UDP受信
- UNMotionFrame/Zenoh受信
- OSC等の入力受付
- MotionBuffer
- MotionSource状態監視
- 複数Rendererへの配信
- source使用状況の管理

### 9.3 Output Name Manager

Spout2等の出力名衝突を管理する。

例。

```text
UN Avatar Main
UN Avatar Guest 1
UN Avatar Debug
UN Avatar Recorder
```

Renderer追加時に自動採番し、同名出力を防ぐ。

### 9.4 Diagnostics Manager

複数プロセスのログを集約する。

集約対象。

- Supervisor log
- Renderer log
- plugin host log
- bridge log
- crash report
- import/export report
- GPU info
- profile snapshot
- renderer-instance manifest

---

## 10. 実装フェイズ

### Phase 1: 単一rendererだが複数前提GUI

目的。

- GUI構造を複数Renderer前提にする
- 実際に起動するRendererは1つだけ

実装。

- RendererInstance型
- Renderer一覧UI
- selectedRendererId
- Renderer Detail Tabs
- renderer-instance.toml生成
- `un-avatar-renderer.exe --manifest ...`
- basic IPC
- Start/Stop/Restart
- status表示

完了条件。

- GUI上はRenderer Instanceとして管理される
- 1 rendererを起動・停止できる
- 子プロセスmanifestで初期化できる
- Spout2 outputまで到達できる

### Phase 2: 複数renderer起動

目的。

- 複数Renderer Instanceを同時起動できるようにする

実装。

- Add Renderer
- Duplicate Renderer
- Remove Renderer
- Start All / Stop All
- instanceごとのIPC endpoint
- instanceごとのmanifest
- Spout2名衝突管理
- MotionSource共有

完了条件。

- Main Avatar + Debug Viewを同時起動できる
- 同一Motion Sourceを複数Rendererが使える
- 出力名が衝突しない

### Phase 3: Role templates

目的。

- Main/Guest/Debug/Recorder等の用途別テンプレートを用意する

実装。

- RendererRole
- Add as Main Avatar
- Add as Guest Avatar
- Add as Debug View
- Add as Recorder
- Duplicate as Debug
- Duplicate as Recorder
- role別既定render preset

完了条件。

- ユーザーが用途からRendererを追加できる

### Phase 4: Supervisor強化

目的。

- 配信中の事故に耐える

実装。

- crash recovery
- restart crashed
- unresponsive検出
- diagnostics bundle
- renderer health monitor
- GPU/CPU stats
- process tree表示
- advanced logging

完了条件。

- rendererが落ちてもGUIが生存し、再起動・診断できる

### Phase 5: Flowgraph automation

目的。

- 複数Renderer Instanceの自動制御

実装。

- RendererEvent node
- RendererCommand node
- MotionConfidence node
- Key/MIDI/OSC node
- Timer node
- Gate/Latch/Toggle node
- flowgraph per profile
- flowgraph per renderer

完了条件。

- FlowgraphでRenderer制御・表情制御・出力制御ができる

---

## 11. 開発用モード

### 11.1 renderer standalone

開発・検証用にRenderer単体起動を許可する。

```text
un-avatar-renderer.exe --manifest debug.renderer.toml
```

用途。

- renderer単体デバッグ
- CIでのheadless/render test
- crash再現
- GPU backend検証

### 11.2 single-process mode

必要なら開発用に単一プロセスモードを残す。

```text
un-avatar-supervisor.exe --single-process
```

ただし標準構成にはしない。
標準はTauri GUI/Supervisor + renderer child processである。

---

## 12. 最終方針

UN Avatar のアプリ構成は以下とする。

```text
標準:
  Tauri GUI/Supervisor process
  + one or more renderer child processes

Renderer:
  wgpu direct implementation

GUI:
  Svelte 5 runes + Vite + TypeScript

Renderer起動:
  Supervisor generated TOML manifest
  + IPC control/event channels

Motion:
  SupervisorまたはMotion Routerがsource管理
  複数Rendererへ配信

GUI design:
  複数Renderer Instance前提
  単一アバター設定画面ではなくSupervisor Console

Future:
  plugin-host / bridge process / OBS native source plugin / Flowgraph automation
```

最初のMVPでは実際のRendererは1個でもよい。
ただし、GUI state、profile schema、Supervisor設計、IPC endpoint設計は、最初から複数Renderer Instanceを前提にする。

これにより、UN Avatarは単なるアバター表示アプリではなく、複数アバター・複数出力・複数構図・デバッグビュー・録画ビューを統合管理するアバターランタイムSupervisorとして成長できる。
