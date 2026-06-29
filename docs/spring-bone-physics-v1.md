# SpringBone Physics v1 Design

UN Avatar v1 では、VRM SpringBone を単なる互換機能ではなく、モデルの見た目を自然に整えるためのアバター物理として扱う。

目的は次の 3 つ。

- 既存 VRM の SpringBone と大きく破綻しない互換モードを残す。
- UN Avatar 独自の安定した物理表現を選べるようにする。
- 将来の PhysBone / Unity model importer / glTF 汎用物理へ拡張できる設計にする。

## v2 Migration Note

v2 / v2.1 では、この文書の solver / collider 実装資産を利用してよいが、入力 schema と挙動設計の正本は VRM SpringBone 生データではなく UNPhysics umbrella 下の UNDynamics runtime model とする。

- VRM SpringBone と VRC PhysBone は source metadata を保持しつつ、UNDynamics group / chain / collider / parameter / limit / interaction view へ正規化する。
- `SpringBoneSimulator` は互換 shim として残してよいが、新規 behavior は UNDynamics runtime state を入力にし、format-specific な VRM / VRC / Unity component 判定を solver 内へ持ち込まない。
- PhysBone は内部 solver 再現を目標にしない。Pull / Spring / Gravity Falloff / Immobile / radius / limit などの authored term は UNDynamics normalized scalar / per-joint samples へ lower し、solver は SpringBone / PhysBone source semantics ではなく UNPhysics response model を読む。
- v2.1 以降の品質目標は SpringBone / PhysBone どちらかの数値互換ではなく、source authored intent から自然で安定し、UI 調整が効く UNPhysics response を作ることとする。
- Wardrobe / action / animation による dynamics enable state は、source scene を直接 mutate するのではなく resolved runtime state と pose buffer へ反映する。

## Solver Modes

v1 で扱う solver は 2 種に限定する。

| Solver | Purpose | Cost | Notes |
| --- | --- | --- | --- |
| `verlet` | 軽量 fixed-step | low | v1 SpringBone 実装資産を使う軽量 backend。source 互換ではなく UNPhysics response terms を解く。 |
| `xpbd` | 高品質 | high | Time based fixed-step。`xpbd_compliance`、time-step 内 persistent lambda、collider constraint を反復解法で扱う。 |

既定 solver は `verlet` とする。UN Avatar の価値は「互換性だけ」ではなく、安定して美しく動くことにあるため。

互換性を優先したいモデルでは `verlet` を選択する。

## Time Model

`verlet` / `xpbd` はいずれも Verlet 系の tail 位置積分を基本とする。

```toml
[physics.dynamics.solver]
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
必要なカテゴリだけ `Override: Verlet/PBD` または `Override: XPBD` を選び、そのときだけプロファイルにカテゴリ override を保存する。より狭い対象には `match_overrides` または `group_overrides` を使う。

意味:

- `damping_half_life_ms`: 速度・揺れ残りが半分になる時間。
- `rest_response`: rest pose 復元応答。v2.1 UNDynamics では source 互換値ではなく UNPhysics の normalized response として読む。部位別の差分は template / preset / match override で明示的に保存する。
- `stiffness_hz`: 旧 profile 互換キー。v2.1 では `rest_response` の読み込み alias とし、新規保存では使わない。
- `bounce_scale`: source-authored Spring / Momentum 由来の揺れ返しを倍率で調整する。`0` は余韻を抑え、`1` は source intent をそのまま使う。
- `motion_coupling`: parent / center motion を tail state へどれだけ追従させるか。低いほど揺れ、1 に近いほど硬く追従する。
- `xpbd_compliance`: XPBD 用の compliance。小さいほど硬く、0 はほぼ剛体制約。Verlet では使わない。
- `gravity_scale`: VRM 由来 gravityPower への倍率。
- `drag_scale`: source-authored drag への倍率。UNPhysics の damping 調整が未指定の場合の補助値。
- `constraint_iterations`: XPBD など反復 solver の反復回数。

旧 VRM パラメータは import 時に保持する。ただし v1 UI では raw 値を直接いじるのではなく、上記物理寄りパラメータへ変換した操作系を正とする。

既存 VRM / UniVRM 由来の SpringBone パラメータは、60fps 更新を前提にした authored intent として扱う。`Override: Verlet/PBD` / `Override: XPBD` へ切り替えた場合も、ユーザーが一から物理値を調整しなくて済むよう、各 group の authored 値から初期物理値を自動変換する。

変換方針:

- `dragForce`: 明示 `damping_half_life_ms` が無い場合はモデル定義値を source-authored damping intent として使う。
- VRM SpringBone `stiffness`: rest-pull intent として `pull` / `rest_response` へ lower し、`shape_preservation` へは使わない。
- VRC PhysBone `stiffness`: local shape-preservation intent として `shape_preservation` へ lower し、Pull 由来の `rest_response` と混ぜない。
- `gravityPower` / `gravityDir`: authored 値を保持し、必要な場合だけ `gravity_scale` で倍率をかける。
- `verlet`: authored 値から lower した UNPhysics response terms を使う軽量 backend。古い `compat_univrm` / `compat_euler` 文字列は読み込み互換 alias として `verlet` に読み替える。
- `xpbd`: `xpbd_compliance` を constraint backend 設定として使い、`stiffness_hz` は UNDynamics の `rest_response` override、`bounce_scale` は `bounce` override として読む。`rest_response` override がある場合は、その値から暗黙 compliance を導出し、明示 `xpbd_compliance` がそれより硬い場合でも softness floor として優先する。明示 override がある場合だけ override を優先する。

`stiffness_hz` は旧 profile 読み込み alias として残すが、新規保存では `rest_response` を使う。`rest_response` と `xpbd_compliance` は別パラメーターとして保存する。solver を切り替えても片方の値で片方を上書きしない。
v2.1 UNDynamics では source-authored 値を final response として直接使わず、source-neutral response term へ lower する。カテゴリは UI grouping、diagnostics、preset seed のためのヒントであり、solver core はカテゴリ名だけで Pull / Stiffness / Spring / Motion coupling を暗黙補正しない。明示 profile override は final response として扱い、source / chain shaping の再スケールを受けない。

Source limit は authored intent として保存するが、布カテゴリでは cone / polar / hinge 角度 limit を硬い runtime 拘束として適用しない。cloth panel / sleeve / skirt などは bind pose の角度範囲が狭いだけで布の落下方向を固定する意図とは限らず、硬拘束にすると #8 のように一部の布だけ持ち上がって固着する。布では stretch / squish / stretchMotion など長さ方向の制約だけを runtime に反映し、角度情報は diagnostics と将来の soft guide 用 metadata として扱う。hair / ears / accessory など布以外では現 solver backend の angle / polar / hinge 近似を維持する。

Supervisor Console の mode は次の 3 種。

- `Authored: Verlet/PBD`: profile override を保存しない。モデルに記録された SpringBone 定義をそのまま使う。
- `Override: Verlet/PBD`: 対象カテゴリの authored 値と等価な初期値で Verlet override を作る。
- `Override: XPBD`: 対象カテゴリの authored Verlet 挙動へ近づく初期値で XPBD override を作る。

カテゴリ reset は現在の override mode を維持したまま、対象カテゴリの authored 値から再計算した初期値へ戻す。`Authored: Verlet/PBD` を選んだ場合は、そのカテゴリの `[[physics.dynamics.solver.overrides]]` を削除する。旧 `[[physics.spring_bone.overrides]]` は読み込み互換だけに使い、新規保存では使わない。

## Recommended UNPhysics Presets

おすすめ設定は `Override: Verlet/PBD` と `Override: XPBD` のどちらでも表示する。これは solver mode を切り替える操作ではなく、UNPhysics response terms の調整に迷ったユーザーへ初期候補を出すための補助操作である。

Preset は `rest_response`、`shape_preservation`、`motion_coupling`、`damping_half_life_ms`、`bounce_scale` をまとめて書き込む。現在の solver が XPBD の場合だけ、追加で `xpbd_compliance` と `constraint_iterations` も書き込む。Verlet/PBD で preset を押しても XPBD へ自動切り替えしない。

v1 では次のカテゴリに短いプリセットを置く。

| Category | Presets | Intent |
| --- | --- | --- |
| `hair` | Soft / Natural / Snappy | 長髪の柔らかい揺れ、標準、短髪や前髪向けの硬め反応 |
| `ears` | Soft / Natural / Snappy | 柔らかい耳、標準、反応の速い耳 |
| `tail` | Light / Natural / Heavy | 軽く大きい揺れ、標準、質量感のある尾 |
| `cloth` | Light / Natural / Firm | 薄手の布、標準、制服や袖向けの硬め |

プリセット適用は対象カテゴリの `[[physics.dynamics.solver.overrides]]` または診断から作った `[[physics.dynamics.solver.match_overrides]]` に `solver`、`rest_response`、`shape_preservation`、`damping_half_life_ms`、`bounce_scale`、必要に応じて `xpbd_compliance`、`constraint_iterations` を書く。v2.1 では compliance だけを柔らかさとして扱わず、`rest_response`、`shape_preservation`、`bounce`、damping、`motion_coupling` も同時に動かす。mode を `Override` へ切り替えるだけの初期 seed も source-authored Pull と Stiffness を分け、Pull は `rest_response`、Stiffness は `shape_preservation` の材料として扱い、両者を `max()` などで混ぜない。

Backend でも、対象カテゴリの現在 solver を読み取り、その solver を保ったまま preset を適用する。UI 表示条件だけに依存すると、古い UI state や外部呼び出しで意図せず mode を変える経路ができるため、preset 適用は保存時にも solver-preserving として扱う。

## Group Classification

SpringBone group にはカテゴリ ID を持たせる。カテゴリ ID は文字列を正本とし、固定 enum を永続 schema にしない。

v1 / v2.1 の既定カテゴリは次の 7 種。

| Category | Typical chains |
| --- | --- |
| `hair` | hair, bangs, side hair, back hair |
| `ears` | animal ears, long ears |
| `tail` | tail, ponytail-like tail chain |
| `cloth` | skirt, sleeve, ribbon cloth, cape |
| `accessory` | ornaments, cords, chains, medals |
| `soft_body` | breast / butt body jiggle |
| `other` | unclassified |

ただし、この 7 種は「組み込み既定カテゴリ」であって、ユーザー定義カテゴリを排除しない。将来 UI でカテゴリの追加、削除、名称変更、group の再分類を可能にしても、既存 TOML schema を破壊しないこと。

内部実装で `BuiltinSpringBoneCategory` のような enum を使ってもよいが、それは分類推定や preset 表示の helper に限定する。`UnaSpringBoneGroup` や profile schema では `category: String` 相当を正とする。

カテゴリ定義は list 方式で持つ。`id` は永続参照用の安定キー、`name` は UI 表示名、`matches` は自動分類用の literal alias 群とする。

```toml
[[physics.dynamics.solver.categories]]
id = "hair"
name = "Hair"
matches = ["hair", "bangs", "side_hair", "back_hair", "髪", "前髪", "横髪", "後ろ髪"]

[[physics.dynamics.solver.categories]]
id = "ears"
name = "Ears"
matches = ["ears", "ear", "animal_ear", "long_ear", "耳", "ミミ", "けもみみ"]

[[physics.dynamics.solver.categories]]
id = "tail"
name = "Tail"
matches = ["tail", "尻尾", "しっぽ"]

[[physics.dynamics.solver.categories]]
id = "cloth"
name = "Cloth"
matches = ["cloth", "skirt", "sleeve", "cape", "shirt", "sweater", "blouse", "dress", "coat", "frill", "frills", "stocking", "stockings", "布", "スカート", "袖", "ケープ", "シャツ", "セーター", "ブラウス", "ドレス", "コート", "フリル", "靴下", "ストッキング"]

[[physics.dynamics.solver.categories]]
id = "accessory"
name = "Accessory"
matches = ["accessory", "ornament", "chain", "cord", "ribbon", "accessories", "bag", "bookbag", "earring", "earrings", "earringroot", "shoe", "shoes", "maryjane", "mary_jane", "footwear", "boot", "boots", "watch", "pocket_watch", "brooch", "broach", "hat", "hatroot", "tie", "tieroot", "bowroot", "bow_tie", "bowties", "necklace", "potion", "bottle", "cable", "nervecable", "strings", "装飾", "アクセサリ", "飾り", "リボン", "鞄", "バッグ", "時計", "ブローチ", "靴", "ブーツ", "帽子", "ネクタイ", "蝶ネクタイ", "首飾り", "ネックレス", "瓶", "ボトル", "ケーブル", "紐"]

[[physics.dynamics.solver.categories]]
id = "soft_body"
name = "Soft Body"
matches = ["breast", "bust", "butt", "cheek", "胸", "尻", "お尻", "頬"]

[[physics.dynamics.solver.categories]]
id = "other"
name = "Other"
matches = []
```

ユーザー定義カテゴリも同じ形で追加する。

```toml
[[physics.dynamics.solver.categories]]
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
- group comment/name、source id の leaf component 名、root bone 名、chain node 名、必要なら mesh/material 名の正規化文字列に `matches` のいずれかが含まれたら一致とする。
- full source path は leaf component / node names で決まらない場合の fallback とする。親階層の衣装名や部位名が、実際の component/root 自身の意味名を上書きしてはならない。
- 複数カテゴリへ一致した場合は、category list で先に出たものを優先する。

分類は importer または runtime preprocessing で行う。

優先順位:

1. group exact override の `category`
2. `physics.dynamics.solver.categories[].matches`
3. 組み込み classifier
4. 未分類 `other`

`tail` は UI / template category として `hair` と分ける。長い単一チェーンで大きく揺れることが多く、ユーザーが選ぶ preset の初期候補が異なるため。ただし solver core はカテゴリ名だけで damping や response を変えない。必要な差は category override、match override、group override として明示的に保存する。

## Overrides

カテゴリ単位の override は最初に触る標準調整面とする。TOML は list 方式を正本にし、将来カテゴリをユーザーが追加・削除できる UI になっても schema を変えずに扱えるようにする。これはユーザー補助の調整面であり、solver core の部位別特殊処理ではない。

ただし v2.1 の設計根はカテゴリ固定ではなく一般マッチ規則である。カテゴリやおすすめプリセットは便利な初期補助であり、ユーザーが任意の source id、名前、正規表現で揺れもの群を指定できることを最終的な調整面にする。特殊ケースは固定実装ではなく、match rule のテンプレートや診断からの seed として扱う。

```toml
[[physics.dynamics.solver.overrides]]
category = "hair"
solver = "xpbd"
damping_half_life_ms = 180
rest_response = 0.12
shape_preservation = 0.08
bounce_scale = 0.85
motion_coupling = 0.50
xpbd_compliance = 0.025
constraint_iterations = 6

[[physics.dynamics.solver.overrides]]
category = "ears"
solver = "verlet"
damping_half_life_ms = 90
rest_response = 0.18
shape_preservation = 0.15
bounce_scale = 0.70
motion_coupling = 0.55
xpbd_compliance = 0.015

[[physics.dynamics.solver.overrides]]
category = "tail"
solver = "xpbd"
damping_half_life_ms = 220
rest_response = 0.08
shape_preservation = 0.06
bounce_scale = 0.95
motion_coupling = 0.40
xpbd_compliance = 0.035
constraint_iterations = 8

[[physics.dynamics.solver.overrides]]
category = "long_ribbon"
solver = "verlet"
damping_half_life_ms = 160
```

`category` は `physics.dynamics.solver.categories[].id` を参照する。表示名は category definition の `name` を使う。未定義カテゴリを override で参照していた場合もエラーにせず、`id` から暫定表示名を生成して保持する。旧 `physics.spring_bone.overrides` は読み込み互換として扱い、新規保存では使わない。

複数の group をカテゴリより細かく、source id 完全一致より一般的に調整したい場合は `match_overrides` を使う。`source_id` は完全一致、`source_id_contains` は source id / comment / chain node names を separator / camel-case 正規化したテキストへの token-aware contains 配列であり、既存 profile 互換のため separator を落とした compact contains も許容する。`source_id_regex` は同じ対象へ照合する正規表現配列である。複数の rule が一致した場合は list 順に merge する。

```toml
[[physics.dynamics.solver.match_overrides]]
name = "soft cloth panels"
source_id_contains = ["cloth", "skirt"]
solver = "verlet"
damping_half_life_ms = 180
rest_response = 0.05
shape_preservation = 0.025
bounce_scale = 0.65
motion_coupling = 0.30

[[physics.dynamics.solver.match_overrides]]
name = "ribbon regex"
source_id_regex = ["(?i)ribbon|bow"]
rest_response = 0.04
motion_coupling = 0.25
```

Supervisor UI から保存する場合、`source_id_regex` は保存時に検証し、壊れた正規表現は拒否する。手編集された既存 profile / manifest は読み込みを壊さず、runtime diagnostics の `dynamics_warnings` と response group の `invalid_match_regexes` に表示する。診断画面は意味のある group 名から `match_overrides` を seed できるが、`bone` / `root` / `head` のような汎用名は広すぎる誤爆を避けるため seed しない。

特定 group だけカテゴリ設定と異なる response にしたい場合は source id 完全一致の group override を手動の最終 pin として使う。通常は `match_overrides` で source/comment/chain の意味名へ一般化し、カテゴリ推論が安全に決めきれない資産や、同じ部位カテゴリ内でも重さ・長さ・役割が異なる単一 chain だけを group override へ残す。category override の後に適用されるため、同じ key がある場合は group override が優先される。

```toml
[[physics.dynamics.solver.group_overrides]]
source_id = "physbone:Armature/Hips/Spine/Chest/Neck/Head/J_Bip_C_Head/J_Bip_C_Head_2/Bone"
solver = "verlet"
rest_response = 0.04
shape_preservation = 0.03
bounce_scale = 0.80
motion_coupling = 0.25
```

v1 の category override では `simulation_hz` と `substeps` を上書きしない。これらは renderer 単位の物理 scheduler 設定とする。部位ごとに更新周期を変えると、pose snapshot 合成、worker wakeup、constraint 同期のコストが増え、見た目の差より実装複雑性が大きくなるため。

解決順:

1. imported VRM SpringBone / VRC PhysBone authored intent を UNPhysics response terms へ lower する。
2. category override (`physics.dynamics.solver.overrides`)
3. ordered match override (`physics.dynamics.solver.match_overrides`)
4. group exact override (`physics.dynamics.solver.group_overrides`)

## Runtime Architecture

段階実装にする。

v2.1 の renderer runtime では、solver core は source path / outfit name / bone leaf name を判定条件にしない。近接する分割布地を補助する mesh cloth assist は solver constraint ではなく authored skin weight の補正層として扱い、mesh topology、隣接 vertex、runtime dynamics membership、static cloth bridge evidence から reconfiguration 時に導出し、特定衣装名への分岐として扱わない。既存 dynamic lane への伝播や missing dynamic lane の seed は、隣接頂点の最も強い dynamic joint weight を上限にし、複数 dynamic joint の合計を単一 seed 先へ畳み込まない。mesh cloth assist の mesh 対象判定は明示 `mesh_path_contains` が無い場合、現在の profile category 定義の `cloth.matches` を使い、固定の衣装名リストへ戻さない。mesh cloth assist の joint role 分類は runtime dynamics membership を優先し、membership が無い診断 fallback だけ一般 cloth alias を使う共通 helper に集約する。diagnostics も個別衣装名の whitelist ではなく bounded sample / count として出す。

CLI の `dynamics-import-audit` / `dynamics-vertex-probe` も同じ境界に揃える。監査用 node / skin / vertex sample は特定モデル名や衣装名の whitelist ではなく、runtime dynamics node、skinned mesh と dynamic joint の接続、mesh bounds からの汎用 spatial region、settle 後 displacement で bounded に選ぶ。

### Phase 1: Solver Abstraction

- `SpringBoneSimulator` 内部を solver trait または enum dispatch へ分離する。
- 現在の Verlet 実装を `verlet` solver として整理する。
- `verlet` を UNPhysics response terms を解く軽量経路として扱う。
- `xpbd` は compliance 付き rest constraint + time-step 内 persistent lambda + collider constraint から始める。
- render thread 内 fixed-step accumulator で動かす。

この段階では worker 化しない。

### Phase 2: Runtime Reconfiguration

- Profiles の `[physics.dynamics]` / `[physics.dynamics.solver]` を実行中 Renderer へ反映する。
- dynamics node reset と solver rebuild を明示的に行う。
- category / match / group override の UI を追加する。

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
- dynamics node state は group ごとの小さな object graph ではなく、可能な範囲で flat arrays に寄せる。
- physics worker と render thread の受け渡しは、最終 bone transform の double buffer または triple buffer とする。frame ごとの allocation は行わない。
- collider は runtime 計算済みの compact representation にする。bone-based collider の生成や mesh/weight 解析は reconfiguration 時に行い、frame loop では半径・中心・対象 bone transform だけを見る。
- UI 表示用の category name や group label は runtime state から切り離し、debug overlay 表示時だけ参照する。
- UNDynamics OFF、または対象 group が 0 件の場合は worker / accumulator を停止し、rest pose reset だけ行う。

この制約により、将来ユーザー定義カテゴリを増やしても、frame loop の計算量はカテゴリ数ではなく dynamics node 数と collider 数に比例する。

## UI

Profiles > Motion に `UNPhysics` / `UNDynamics` パネルを置く。

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

各カテゴリは、source-authored intent から lower した UNPhysics response を初期表示し、編集されたカテゴリだけ `[[physics.dynamics.solver.overrides]]` として保存する。

`Override: Verlet/PBD` と `Override: XPBD` のカテゴリでは Reset の横におすすめプリセットボタンを表示する。`Authored` では source authored values を尊重するため表示しない。

カテゴリ編集 UI は、将来次の項目を編集できる前提で内部 schema を固定する。

- ID
- Display name
- Match aliases
- Sort order

このため UI 表示の `name` を永続参照キーにしない。

## Acceptance

- 既存 model1 / model2 で UNDynamics が初期状態から爆発しない。
- UNDynamics OFF で揺れもの node が rest pose に戻る。
- `verlet` / `xpbd` の切替が実行中 Renderer へ反映される。
- `simulation_hz` を 30 / 60 / 120 / 240 に変えても、time based solver の見た目が大きく変わらない。
- Hair / Ears / Tail / Cloth / Accessory / Other の分類が UI で確認できる。
- `matches = ["ears", "耳", "ミミ"]` のような alias で分類ルールを拡張できる。
- category の表示名変更で override 参照が壊れない。
- Tail は Hair とは別カテゴリとして override できる。
- `Override: Verlet/PBD` と `Override: XPBD` の Hair / Ears / Tail / Cloth でおすすめプリセットを適用でき、preset は現在の solver を勝手に変更しない。
- 通常の profile 値変更で Supervisor Console が全 profile 再読込によって数秒止まらない。
