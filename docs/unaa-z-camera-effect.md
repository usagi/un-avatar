# Renderer単位のUNAA/Zカメラ演出

## 目的

UNAvatar Rendererは、UN App API over Zenoh（UNAA/Z）でRendererごとに独立したidentityを公開し、`camera.effect`を一つの原子的な演出として実行する。主用途は、配信中のチャネルポイント交換などを起点に、指定した一体だけへカメラを寄せ、保持後に元の状態へ戻すことだ。

複数Rendererへbroadcastしない。UNVACなどの呼び出し側は、対象AvatarLinkに設定された`app_id = "un-avatar"`と`instance_id`を完全一致で解決する。

## Identityとdiscovery

- `instance_id`: 安定した論理個体ID。通常はmanifestの`[profile].id`を使う。Supervisor起動では`--unaa-z-instance-id <profile.id>`を明示する。
- `session_id`: Renderer起動ごとに変わる実行個体ID。再起動前に遅延した命令を拒否するために使う。
- snapshot: `un/app/un-avatar/{instance_id}/snapshot`
- announce: `un/app/un-avatar/{instance_id}/announce`
- command: `un/app/un-avatar/{instance_id}/command`
- response: `un/app/un-avatar/{instance_id}/response/{request_id}`

snapshotは1秒ごとにannounceされ、能動queryにも応答する。`ready`はGPU sceneの起動完了後だけ`true`になる。能力は`renderer`と`camera_control`、命令は`camera.effect`を広告する。

## `camera.effect`

命令の`args`は次の形を取る。

```json
{
  "target_session_id": "renderer-1234-...",
  "pose": {
    "kind": "orbit",
    "target_xyz": [0.0, 1.35, 0.0],
    "longitude_deg": 180.0,
    "latitude_deg": 3.0,
    "radius": 0.85
  },
  "diagonal_fov_deg": 28.0,
  "enter": { "duration_ms": 450, "easing": "ease_in_out_cubic" },
  "hold_ms": 5000,
  "leave": { "duration_ms": 650, "easing": "ease_in_out_cubic" },
  "conflict_policy": "replace",
  "return_to": "captured_at_start"
}
```

`target_session_id`だけは安全な宛先固定のため必須。カメラ値と演出制御値は部分更新として省略できる。

- `pose`自体を省略できる。`orbit`の`target_xyz` / `longitude_deg` / `latitude_deg` / `radius`、`cartesian`の`position_xyz` / `rotation_euler_deg`も個別に省略でき、未指定値は演出開始時のカメラ状態を継承する。
- `rotation_euler_deg`内の`yaw` / `pitch` / `roll`も個別に省略できる。`rotation_order`の省略値は`yxz`、rollの省略値は現在のorbit cameraが表現できる`0`。
- `diagonal_fov_deg`を省略すると開始時FOVを維持する。
- `enter` / `leave`を省略すると、それぞれ`450 ms` / `650 ms`の`ease_in_out_cubic`。object内では`duration_ms`と`easing`も個別に省略でき、`easing = "none"`だけを指定した場合は`duration_ms = 0`になる。
- `hold_ms`は`5000`、`conflict_policy`は`replace`、`return_to`は`captured_at_start`が省略値。
- poseまたはFOVを一つも変更しないno-op命令は拒否する。

例えば、現在の構図とFOVを維持して距離だけ寄せる最小命令は次の形になる。

```json
{
  "target_session_id": "renderer-1234-...",
  "pose": { "kind": "orbit", "radius": 1.05 }
}
```

Rendererは実際に演出を開始する時点のカメラ状態を捕捉する。進入、保持、復帰はRenderer内の一つの状態機械が所有するため、受理後にUNVACとの通信が切れても復帰処理は継続する。

- `replace`: 実行中・待機中のカメラ演出を破棄し、新しい演出の開始時状態を復帰先とする。
- `queue`: 受付順に実行し、各演出が実際に始まる時点で復帰先を捕捉する。
- 通常の手動カメラ操作や明示的なcamera state変更は演出を取り消し、ユーザー操作を優先する。

easingは`none`、`linear`、sine / quad / cubic / quart / quintの`ease_in_*`、`ease_out_*`、`ease_in_out_*`を扱う。`none`は`duration_ms = 0`と必ず組み合わせる。

## 座標方式

`orbit`はUNAvatarの内部カメラ状態を直接表す。`cartesian`は次の形も受理する。

```json
{
  "kind": "cartesian",
  "position_xyz": [0.0, 1.4, 1.0],
  "rotation_euler_deg": { "yaw": 0.0, "pitch": 0.0, "roll": 0.0 },
  "rotation_order": "yxz"
}
```

UNAvatarの現行カメラはorbit表現でrollを持たない。そのため`roll`は0のみ受理し、焦点距離は演出開始時の`radius`を維持してCartesian位置・向きからtargetを導出する。この制約を黙って近似せず、非ゼロrollはエラーにする。

## 安全境界

- envelopeのschema、kind、`app_id`、`instance_id`、commandを検査する。
- `target_session_id`が現在のRenderer sessionと完全一致しない命令は拒否する。
- FOV、半径、緯度、時間、非有限値を検証する。
- identity不一致や未対応命令には成功応答を返さない。
- 画面内文字や外部観測内容を命令として解釈しない。
