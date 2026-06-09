# SpringBone Physics v1 Design

UN Avatar v1 では、VRM SpringBone を単なる互換機能ではなく、モデルの見た目を自然に整えるためのアバター物理として扱う。

目的は次の 3 つ。

- 既存 VRM の SpringBone と大きく破綻しない互換モードを残す。
- UN Avatar 独自の安定した物理表現を選べるようにする。
- 将来の PhysBone / Unity model importer / glTF 汎用物理へ拡張できる設計にする。

## v2 Migration Note

v2 では、この文書の solver / collider 実装資産を利用してよいが、入力 schema の正本は VRM SpringBone 生データではなく UNPhysics umbrella 下の UNDynamics runtime model とする。

- VRM SpringBone と VRC PhysBone は source metadata を保持しつつ、UNDynamics group / chain / collider / parameter / limit / interaction view へ正規化する。
- `SpringBoneSimulator` は互換 shim として残してよいが、新規 behavior は UNDynamics runtime state を入力にし、format-specific な VRM / VRC / Unity component 判定を solver 内へ持ち込まない。
- PhysBone は v2 初期では完全再現ではなく、UNDynamics の SpringBone-like runtime primitive への近似 lower を最初の実装目標にする。
- Wardrobe / action / animation による dynamics enable state は、source scene を直接 mutate するのではなく resolved runtime state と pose buffer へ反映する。

## Solver Modes

v1 で扱う solver は 2 種に限定する。

| Solver | Purpose | Cost | Notes |
| --- | --- | --- | --- |
| `verlet` | VRM 互換・軽量 | low | UniVRM SpringBone に近い Verlet 系。既存モデルの見た目再現を優先する。 |
| `xpbd` | 高品質 | high | Time based fixed-step。`xpbd_compliance`、time-step 内 persistent lambda、collider constraint を反復解法で扱う。 |

既定 solver は `verlet` とする。UN Avatar の価値は「互換性だけ」ではなく、安定して美しく動くことにあるため。

互換性を優先したいモデルでは `verlet` を選択する。

## Time Model

`verlet` / `xpbd` はいずれも Verlet 系の tail 位置積分を基本とする。

```toml
[physics.spring_bone]
enabled = true
time_mode = "time_based" # frame_based | time_based
simulation_hz = 120
substeps = 1
```

制約:

- `simulation_hz` は 30..240 Hz。
- `substeps` は 1..8。
- `verlet / xpbd + frame_based` は指定されても `time_based` に正規化する。

## Physical Parameters

FPS 依存を避けるため、damping は half-life で持つ。

物理パラメータは SpringBone group を分類したカテゴリ単位で持つ。
未編集カテゴリは `Authored: Verlet/PBD` として、VRM の SpringBone 定義値をそのまま使う。
必要なカテゴリだけ `Override: Verlet/PBD` または `Override: XPBD` を選び、そのときだけプロファイルに部位別 override を保存する。

意味:

- `damping_half_life_ms`: 速度・揺れ残りが半分になる時間。
- `stiffness_hz`: Verlet 用の rest pose 復元 pull。XPBD では使わない。
- `xpbd_compliance`: XPBD 用の compliance。小さいほど硬く、0 はほぼ剛体制約。Verlet では使わない。
- `gravity_scale`: VRM 由来 gravityPower への倍率。
- `drag_scale`: legacy drag への倍率。互換モード用。
- `constraint_iterations`: XPBD など反復 solver の反復回数。

旧 VRM パラメータは import 時に保持する。ただし v1 UI では raw 値を直接いじるのではなく、上記物理寄りパラメータへ変換した操作系を正とする。

既存 VRM / UniVRM 互換の SpringBone パラメータは、60fps 更新を前提にした authored 値として扱う。`Override: Verlet/PBD` / `Override: XPBD` へ切り替えた場合も、ユーザーが一から物理値を調整しなくて済むよう、各 group の authored 値から初期物理値を自動変換する。

変換方針:

- `dragForce`: 明示 `damping_half_life_ms` が無い場合はモデル定義値を直接使う。これにより solver 導入前の VRM SpringBone 挙動を保つ。
- `stiffness`: 既存 Verlet 式の `stiffness * dt` と同じ出発点になるよう `stiffness_hz` の初期値へ写す。
- `stiffness`: XPBD では `stiffness * 10Hz` 相当の硬さから `xpbd_compliance = 1 / (tau * effective_hz)^2` へ変換する。
- `gravityPower` / `gravityDir`: authored 値を保持し、必要な場合だけ `gravity_scale` で倍率をかける。
- `verlet`: authored 値から変換した `stiffness_hz` を使う VRM 互換・軽量モード。古い `compat_univrm` / `compat_euler` 文字列は読み込み互換 alias として `verlet` に読み替える。
- `xpbd`: `xpbd_compliance` を使い、`stiffness_hz` は読まない。明示 override がある場合だけ override を優先する。

`stiffness_hz` と `xpbd_compliance` は別パラメーターとして保存する。solver を切り替えても片方の値で片方を上書きしない。

Supervisor Console の mode は次の 3 種。

- `Authored: Verlet/PBD`: profile override を保存しない。モデルに記録された SpringBone 定義をそのまま使う。
- `Override: Verlet/PBD`: 対象カテゴリの authored 値と等価な初期値で Verlet override を作る。
- `Override: XPBD`: 対象カテゴリの authored Verlet 挙動へ近づく初期値で XPBD override を作る。

カテゴリ reset は現在の override mode を維持したまま、対象カテゴリの authored 値から再計算した初期値へ戻す。`Authored: Verlet/PBD` を選んだ場合は、そのカテゴリの `[[physics.spring_bone.overrides]]` を削除する。

## Recommended XPBD Presets

おすすめ設定は `Override: XPBD` を選んだカテゴリだけに表示する。これは solver mode を切り替える操作ではなく、XPBD のパラメータ調整に迷ったユーザーへ初期候補を出すための補助操作である。

v1 では次のカテゴリに短いプリセットを置く。

| Category | Presets | Intent |
| --- | --- | --- |
| `hair` | Soft / Natural / Snappy | 長髪の柔らかい揺れ、標準、短髪や前髪向けの硬め反応 |
| `ears` | Soft / Natural / Snappy | 柔らかい耳、標準、反応の速い耳 |
| `tail` | Light / Natural / Heavy | 軽く大きい揺れ、標準、質量感のある尾 |
| `cloth` | Light / Natural / Firm | 薄手の布、標準、制服や袖向けの硬め |

プリセット適用は対象カテゴリの `[[physics.spring_bone.overrides]]` に `solver = "xpbd"`、`damping_half_life_ms`、`xpbd_compliance`、`constraint_iterations` を書く。`stiffness_hz` は XPBD では使わないため書かない。

Backend でも、対象カテゴリが現在 `Override: XPBD` でない場合は `preset` 更新を拒否する。UI 表示条件だけに依存すると、古い UI state や外部呼び出しで意図せず mode を変える経路ができるため。

## Group Classification

SpringBone group にはカテゴリ ID を持たせる。カテゴリ ID は文字列を正本とし、固定 enum を永続 schema にしない。

v1 の既定カテゴリは次の 6 種。

| Category | Typical chains |
| --- | --- |
| `hair` | hair, bangs, side hair, back hair |
| `ears` | animal ears, long ears |
| `tail` | tail, ponytail-like tail chain |
| `cloth` | skirt, sleeve, ribbon cloth, cape |
| `accessory` | ornaments, cords, chains, medals |
| `other` | unclassified |

ただし、この 6 種は「組み込み既定カテゴリ」であって、ユーザー定義カテゴリを排除しない。将来 UI でカテゴリの追加、削除、名称変更、group の再分類を可能にしても、既存 TOML schema を破壊しないこと。

内部実装で `BuiltinSpringBoneCategory` のような enum を使ってもよいが、それは分類推定や preset 表示の helper に限定する。`UnaSpringBoneGroup` や profile schema では `category: String` 相当を正とする。

カテゴリ定義は list 方式で持つ。`id` は永続参照用の安定キー、`name` は UI 表示名、`matches` は自動分類用の literal alias 群とする。

```toml
[[physics.spring_bone.categories]]
id = "hair"
name = "Hair"
matches = ["hair", "bangs", "side_hair", "back_hair", "髪", "前髪", "横髪", "後ろ髪"]

[[physics.spring_bone.categories]]
id = "ears"
name = "Ears"
matches = ["ears", "ear", "animal_ear", "long_ear", "耳", "ミミ", "けもみみ"]

[[physics.spring_bone.categories]]
id = "tail"
name = "Tail"
matches = ["tail", "尻尾", "しっぽ"]

[[physics.spring_bone.categories]]
id = "cloth"
name = "Cloth"
matches = ["cloth", "skirt", "sleeve", "cape", "布", "スカート", "袖", "ケープ"]

[[physics.spring_bone.categories]]
id = "accessory"
name = "Accessory"
matches = ["accessory", "ornament", "chain", "cord", "ribbon", "装飾", "アクセサリ", "飾り", "リボン"]

[[physics.spring_bone.categories]]
id = "other"
name = "Other"
matches = []
```

ユーザー定義カテゴリも同じ形で追加する。

```toml
[[physics.spring_bone.categories]]
id = "long_ribbon"
name = "Long ribbon"
matches = ["long_ribbon", "long ribbon", "長いリボン"]
```

`name` は GUI 表示専用であり、分類や override 参照には使わない。表示名変更で既存設定が壊れることを避けるため。表示名そのものでも分類したい場合は、同じ文字列を `matches` に明示的に入れる。

カテゴリ ID の正規化:

- ASCII lower snake case を推奨する。
- `id` は空白を含めない。
- 未知カテゴリはエラーにせず、そのまま保持する。
- 空または不正なカテゴリ ID だけ `other` へ正規化する。

`matches` の正規化:

- v1 は正規表現ではなく literal match とする。
- ASCII は lower case 化し、space / hyphen / underscore の差を吸収する。
- 日本語など非 ASCII 文字列はそのまま扱う。
- group comment/name、root bone 名、chain node 名、必要なら mesh/material 名の正規化文字列に `matches` のいずれかが含まれたら一致とする。
- 複数カテゴリへ一致した場合は、category list で先に出たものを優先する。

分類は importer または runtime preprocessing で行う。

優先順位:

1. group exact override の `category`
2. `physics.spring_bone.categories[].matches`
3. 組み込み classifier
4. 未分類 `other`

`tail` は `hair` と分ける。長い単一チェーンで大きく揺れることが多く、damping と stiffness の期待値が異なるため。

## Overrides

カテゴリ単位の override を正とする。TOML は list 方式を正本にし、将来カテゴリをユーザーが追加・削除できる UI になっても schema を変えずに扱えるようにする。

```toml
[[physics.spring_bone.overrides]]
category = "hair"
solver = "xpbd"
damping_half_life_ms = 180
stiffness_hz = 2.8
xpbd_compliance = 0.025
constraint_iterations = 6

[[physics.spring_bone.overrides]]
category = "ears"
solver = "verlet"
damping_half_life_ms = 90
stiffness_hz = 5.0
xpbd_compliance = 0.015

[[physics.spring_bone.overrides]]
category = "tail"
solver = "xpbd"
damping_half_life_ms = 220
stiffness_hz = 2.2
xpbd_compliance = 0.035
constraint_iterations = 8

[[physics.spring_bone.overrides]]
category = "long_ribbon"
solver = "verlet"
damping_half_life_ms = 160
```

`category` は `physics.spring_bone.categories[].id` を参照する。表示名は category definition の `name` を使う。未定義カテゴリを override で参照していた場合もエラーにせず、`id` から暫定表示名を生成して保持する。

v1 の category override では `simulation_hz` と `substeps` を上書きしない。これらは renderer 単位の物理 scheduler 設定とする。部位ごとに更新周期を変えると、pose snapshot 合成、worker wakeup、constraint 同期のコストが増え、見た目の差より実装複雑性が大きくなるため。

解決順:

1. group exact override（将来）
2. category override
3. imported VRM SpringBone parameter

将来の group exact override も list 方式のまま追加する。

```toml
[[physics.spring_bone.group_overrides]]
group_id = "model1:vrm0:secondaryAnimation:3:root:417"
category = "tail"
solver = "xpbd"
```

`group_id` は importer が安定生成する ID とする。カテゴリ編集 UI は、既定カテゴリ・ユーザー定義カテゴリ・group override を同じ文字列カテゴリ空間で扱う。

## Runtime Architecture

段階実装にする。

### Phase 1: Solver Abstraction

- `SpringBoneSimulator` 内部を solver trait または enum dispatch へ分離する。
- 現在の Verlet 実装を `verlet` solver として整理する。
- `verlet` を UniVRM 互換経路として扱う。
- `xpbd` は compliance 付き rest constraint + time-step 内 persistent lambda + collider constraint から始める。
- render thread 内 fixed-step accumulator で動かす。

この段階では worker 化しない。

### Phase 2: Runtime Reconfiguration

- Profiles の `physics.spring_bone` を実行中 Renderer へ反映する。
- SpringBone node reset と solver rebuild を明示的に行う。
- category override の UI を追加する。

### Phase 3: Physics Worker

- 物理更新を renderer draw loop から分離する。
- `simulation_hz` で固定周期更新し、render 側は最新 pose snapshot を読む。
- scene 共有は直接 `UnaDocument.scene` を書くのではなく、physics pose buffer を介する。
- 目標更新周波数は 30..240 Hz。

worker 化は Phase 1 と同時に行わない。solver の正しさと thread synchronization の問題を分離するため。

## Performance And Memory Constraints

実行時効率とメモリー効率のため、v1 では次の制約を置く。

- `simulation_hz` / `substeps` は renderer 単位で 1 つだけ持つ。
- category override は solver と物理パラメータだけを変える。更新周期は変えない。
- solver が混在する場合は、group を `solver + constraint_iterations` 単位で batch 化して更新する。
- category `id` / `matches` の文字列処理は import、profile load、runtime reconfiguration 時だけ行う。frame loop では文字列比較しない。
- UI 表示用の authored SpringBone category 集計は、avatar path、file size、mtime をキーにキャッシュする。プロファイル値を 1 つ変えるたびに VRM / GLB を再 import しない。
- Supervisor Console の profile value 更新では、変更された profile だけを差し替える。全 profile の `list_avatar_settings()` と runtime status 全更新は、明示更新や作成・削除など必要な操作に限定する。
- profile load 後、category `id` は内部の small integer category index へ解決してよい。ただし保存 schema は文字列のまま維持する。
- `matches` は regex ではなく literal match とする。regex や glob は v1 では入れない。
- SpringBone node state は group ごとの小さな object graph ではなく、可能な範囲で flat arrays に寄せる。
- physics worker と render thread の受け渡しは、最終 bone transform の double buffer または triple buffer とする。frame ごとの allocation は行わない。
- collider は runtime 計算済みの compact representation にする。bone-based collider の生成や mesh/weight 解析は reconfiguration 時に行い、frame loop では半径・中心・対象 bone transform だけを見る。
- UI 表示用の category name や group label は runtime state から切り離し、debug overlay 表示時だけ参照する。
- SpringBone OFF、または対象 group が 0 件の場合は worker / accumulator を停止し、rest pose reset だけ行う。

この制約により、将来ユーザー定義カテゴリを増やしても、frame loop の計算量はカテゴリ数ではなく SpringBone node 数と collider 数に比例する。

## UI

Profiles > Motion に `SpringBone Physics` パネルを置く。

Basic:

- Enabled
- Simulation Hz

Advanced:

- Category overrides

カテゴリ UI:

- Hair
- Ears
- Tail
- Cloth
- Accessory
- Other

各カテゴリは、VRM 由来の Verlet 基準値を初期表示し、編集されたカテゴリだけ `[[physics.spring_bone.overrides]]` として保存する。

`Override: XPBD` のカテゴリでは Reset の横におすすめプリセットボタンを表示する。`Authored: Verlet/PBD` と `Override: Verlet/PBD` では表示しない。

カテゴリ編集 UI は、将来次の項目を編集できる前提で内部 schema を固定する。

- ID
- Display name
- Match aliases
- Sort order

このため UI 表示の `name` を永続参照キーにしない。

## Acceptance

- 既存 model1 / model2 で SpringBone が初期状態から爆発しない。
- SpringBone OFF で揺れもの node が rest pose に戻る。
- `verlet` / `xpbd` の切替が実行中 Renderer へ反映される。
- `simulation_hz` を 30 / 60 / 120 / 240 に変えても、time based solver の見た目が大きく変わらない。
- Hair / Ears / Tail / Cloth / Accessory / Other の分類が UI で確認できる。
- `matches = ["ears", "耳", "ミミ"]` のような alias で分類ルールを拡張できる。
- category の表示名変更で override 参照が壊れない。
- Tail は Hair とは別カテゴリとして override できる。
- `Override: XPBD` の Hair / Ears / Tail / Cloth でおすすめプリセットを適用できる。
- 通常の profile 値変更で Supervisor Console が全 profile 再読込によって数秒止まらない。
