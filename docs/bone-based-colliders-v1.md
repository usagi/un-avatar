# Bone-based Colliders v1

この文書は UN Avatar v1 で追加する **ボーンベースコライダー** の短期仕様を固定する。

UN Avatar は VRM 専用アプリではなく、glTF 汎用ランタイムに将来展望を持ちつつ、v1 では glTF 拡張としての VRM に注力する。したがって、この機能は `SpringBone collider` ではなく、skeleton と skin weight から生成される汎用的な `Bone-based Colliders` として設計する。

## 目的

- 一般的な VRM に collider が入っていない現状を前提に、UN Avatar 側で簡易 collider を生成する。
- v1 では生成した collider を SpringBone solver のめり込み抑制に使う。
- 将来は cloth、accessory、plugin physics、UNA physics などにも流用できる設計名と schema にする。
- 既存 VRM collider がある場合は将来的に尊重できる余地を残す。ただし v1 の主役は自動生成 collider とする。

## 非目的

- 正確な人体衝突判定や mesh collider。
- 手指、衣装、スカート、髪束単位の細かい collider editor。
- VRChat PhysBone 互換の完全再現。
- renderer の可視 mesh に影響する collision。

v1 の目的は、髪、耳、衣装、アクセサリなどの SpringBone が Head / Torso / Arm / Hand に明らかにめり込む状態を軽減すること。

## Profile schema

TOML の正本は `[physics.bone_colliders]` とする。

```toml
[physics.bone_colliders]
enabled = true

[physics.bone_colliders.radius_mm]
head = 120
neck_chest = 80
torso = 140
upper_arms = 55
lower_arms = 45
hands = 50

[debug]
show_bone_colliders = false
```

部位 radius は mm 単位の `f32`。`value < 0.001` は OFF として扱う。

UI 表示でも `0` ではなく `OFF` と表示する。数値入力と slider は同じ値を編集し、slider は `0..300mm`、step は `1mm` を基本にする。テキスト入力は `0..1000mm` まで受け付ける。

旧 `parts.*` 倍率設定は v1 初期化中の設計ミスとして読み込まない。既存プロファイルは `radius_mm.*` の既定値へフォールバックする。

## v1 部位

v1 で自動生成する部位。

| Part | Primitive | Notes |
| --- | --- | --- |
| `head` | sphere | Head bone 周辺。髪、前髪、耳の基礎 collider。 |
| `neck_chest` | capsule chain or spheres | Neck / Chest / UpperChest の連続領域。 |
| `torso` | capsule | Spine / Chest / Hips 間の体幹。 |
| `upper_arms` | capsules | LeftUpperArm / RightUpperArm。 |
| `lower_arms` | capsules | LeftLowerArm / RightLowerArm。 |
| `hands` | spheres or short capsules | LeftHand / RightHand。 |

Legs / feet / skirt は v1 では扱わない。スカートは必要 collider の形状と期待挙動が異なり、v1 で一緒に扱うと調整 UI と solver complexity が増える。

## 推定方針

入力は読み込み済み scene の rest pose、humanoid bone mapping、skin joint / weight。

1. Humanoid bone の rest world transform と親子関係を取得する。
2. mesh primitive の vertex について、該当 bone に十分 weight されている vertex を収集する。
3. capsule axis は対象 bone segment から決める。
4. radius は axis から vertex までの距離分布から percentile で推定する。
   - 最大値は使わない。袖、髪、アクセサリ、外れ頂点に引っ張られやすいため。
   - まず 80〜90 percentile を候補にし、実モデルで調整する。
5. vertex が不足する場合は humanoid 身長比 fallback を使う。
6. part scale を radius に掛ける。
7. scale が OFF の part は collider を生成しない。

生成 collider は rest pose 基準の local definition として保持し、毎フレーム現在 pose の bone transform から world collider に展開する。

## Solver integration

v1 では SpringBone solver の tail 更新後に collider constraint を適用する。

- sphere / capsule のみ。
- collider は SpringBone tail point を外へ押し戻す。
- 長さ拘束は維持する。押し戻し後も joint からの距離を chain length に戻す。
- collider 適用は固定 substep 内で行う。
- collider が原因で NaN や過大 displacement が出た場合は、その collider constraint を無視して frame を継続する。

将来、VRM collider を読む場合は、VRM collider と bone-based collider を同じ runtime primitive 表現へ正規化する。

## Renderer / UI

Profiles 画面。

- Motion / Physics 付近に `ボーンベースコライダー` を置く。
- `enabled` checkbox。
- 部位別 scale slider + number input。
- `OFF` 表示規則を使う。

Window / Display 付近。

- `XYZ 軸を表示` の近くに `簡易コライダーを表示` checkbox を置く。
- v1 では暫定配置でよい。リリース前 UI 整理時に Display グループ全体を再設計する。

Renderers タブ右側 Display グループ。

- `XYZ Axes`
- `Bone Colliders`

ここは runtime toggle とする。Profile へ保存する値は `[debug] show_bone_colliders`。

## Runtime control / telemetry

Control command。

```text
SetShowBoneColliders { enabled: bool }
```

Telemetry。

```text
show_bone_colliders: bool
bone_collider_count: u32
bone_collider_source: "off" | "auto" | "auto+vrm" | "vrm"
dynamics_group_count: u32
dynamics_vrm_spring_bone_group_count: u32
dynamics_vrc_physbone_group_count: u32
dynamics_unknown_group_count: u32
dynamics_collider_count: u32
dynamics_vrm_spring_bone_collider_count: u32
dynamics_vrc_physbone_collider_count: u32
dynamics_unknown_collider_count: u32
```

`bone_collider_source` は v1 では `"off"` / `"auto"` のみでよい。将来 VRM collider 読み込みを足す時に値を拡張する。
`dynamics_*` は renderer が正規化後に保持している runtime dynamics group / collider の件数であり、raw `.unavatar` `dynamics` entry 数ではない。raw entry と runtime group の食い違いは `un-avatar-cli diagnose` の warning で検出する。

Diagnostics には profile 値、生成 collider 数、part ごとの有効/無効と scale を出す。

## Debug draw

`debug.show_bone_colliders` または runtime `SetShowBoneColliders` が ON のとき、renderer が半透明 wire primitive として collider を描画する。

- sphere: wire sphere
- capsule: line segment + end sphere でもよい
- 描画色は SpringBone / motion と混同しない色にする
- 深度テストは ON を基本にするが、見にくい場合は薄い overlay 表示を検討する

## 受け入れ条件

- collider なしの model1 / model2 で、Hair / ear / clothing SpringBone の体幹・頭部めり込みが目視で軽減する。
- scale を OFF にした part は collider 生成されない。
- `show_bone_colliders` ON/OFF が Profiles 初期値と Renderers runtime toggle の両方で動く。
- 既存 profile では、未指定でも破壊的に見た目が変わりすぎない。
- `cargo test -p un-avatar-skeleton` と `cargo test -p un-avatar-render-wgpu` が通る。

## v1 後

- VRM0 / VRM1 collider group の読み込みと統合。
- skirt / legs 向け collider preset。
- model-specific collider editor。
- plugin physics / UNA physics との共有。
- collider debug draw の見た目と操作 UI の整理。
