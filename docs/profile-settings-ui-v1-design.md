# Supervisor Console Profiles UI v1 Design

UN Avatar v1 の Supervisor Console `Profiles` 画面を、製品向けの profile editor として作り直すための設計文書。

この文書の目的は、実装中に「どの項目をどこへ置くか」「Simple と Advanced をどう扱うか」「将来レンダリングモードを増やしたとき既存 profile の見た目を変えないか」で迷わないようにすること。

## 目標

- VStreamer が初見でも「なんとなく良い感じ」に設定できる。
- 上級ユーザーは個別パラメーターを直接調整できる。
- 開発・診断用の項目は必要なときだけ表示し、通常の profile 編集を邪魔しない。
- レンダラー splash / VRM metadata 画面と同じく、USAGI.NETWORK らしい cool / cute / technical な印象を持たせる。
- v1.1 以後に PBR / Realistic / Offline RT の rendering style を足しても UI の大改修を避ける。
- 新バージョンで既存 profile のレンダリング結果を勝手に変えない。

## 非目標

- v1 で PBR / Realistic / Offline RT を実装すること。
- Simple / Advanced / Developer を完全に別ページ化すること。
- profile の永続状態を preset 名だけに依存させること。
- すべての内部 debug flag を一般ユーザーに見せること。

## 基本思想

Profile editor は `Simple / Advanced / Developer` の別ページではなく、同じ画面の中で段階的に読める UI にする。

- **Simple**: 意味ベースの quick set。複数の Advanced パラメーターをまとめて「おすすめ値」にするボタン。
- **Advanced**: 実際に保存される個別パラメーター。Simple を押すとここが変わる。
- **Developer**: デバッグ、診断、開発データ取得。Settings の `Show developer controls` で表示を切り替える。

Simple は隠しモードではなく、Advanced の上に置く「操作補助」。ユーザーが専門用語を理解していなくても使えるが、何が変わったかは下の Advanced controls で見える。

## 永続化ルール

既存 profile の見た目を新バージョンで勝手に変えないことを最優先する。

1. Simple preset は「適用操作」であり、永続状態の唯一の真実ではない。
2. Simple preset を押したら、最終的な個別パラメーターを書き込む。
3. Advanced を手で変更したら、その section の preset 表示は `Custom` になる。
4. 将来 preset 定義が変わっても、過去に保存済みの profile は変わらない。
5. 将来 rendering style が増えても、既存 profile は `MToon` 互換として読み続ける。
6. profile editor は未知の section / key を保持し、理解できない値を保存時に破壊しない。

将来の schema 例:

```toml
[rendering]
style = "mtoon"
style_version = 1
compatibility = "unavatar-v1-mtoon"
```

v1 時点でこの section を必須実装する必要はない。ただし UI は「Rendering style」という上位概念を置ける構造にしておく。

## 画面構造

Profiles 画面は現行の左 profile list / 右 editor を維持する。ただし右側の editor は以下の構造に変える。

```text
Profile Header
  name / group / avatar summary / running status / profile storage

Profile Body
  1. Avatar
  2. Rendering & Presentation
  3. Motion
  4. Output & Window
  5. Render Quality
  6. Camera
  7. Developer Diagnostics (Settings で表示時のみ)
```

セクションはカードを入れ子にせず、横幅いっぱいの panel / band として扱う。各 section の中だけで必要な control group を分ける。

## Section Pattern

各 section は原則として同じ構造にする。

```text
Section Header
  Title / short status / Live or Restart badges

Quick Set
  2-4 個の意味ベースボタン

Advanced Controls
  実際に保存される field controls
```

Quick Set のボタンは「値」ではなく「意図」を表す。クリックすると複数 field をまとめて更新する。

例:

```text
Rendering & Presentation
  Quick Set: Natural / Clear / Stream Pop / Soft Studio
  Advanced: rendering style, look, outline, rim, shadow, bloom, color grading
```

## Field Badges

各 control group には反映タイミングを明示する。

| Badge | 意味 |
| --- | --- |
| `Live` | 起動中 renderer に即時反映される。 |
| `Restart` | profile には保存されるが renderer 再起動が必要。 |
| `Profile` | launch-time でも runtime でもなく、profile metadata / 管理情報。 |
| `Debug` | Developer controls 表示時のみ使う。 |

既存の「起動時」表現は、Avatar Effects のような runtime 反映項目には使わない。

## Profile Header

右 editor の最上部に常に表示する。

表示内容:

- profile icon
- display name
- group
- avatar file name / VRM metadata status
- storage: User / Seed copy
- running count
- short render summary: `MToon · SMAA · Stream Pop` のような表示
- primary actions: duplicate, delete, reveal file, launch

Header は設定画面の anchor。スクロールしても Profile の文脈が失われないように、必要なら sticky 化する。

## Avatar

目的: ユーザーが最初に探す「どのモデルを使うか」を最上位に置く。

Quick Set:

| Button | 効果 |
| --- | --- |
| `Standard VRM` | spring bones ON、metadata 確認導線を表示、通常の avatar file 運用。 |
| `Lightweight` | spring bones は維持しつつ重い runtime effect は触らない。avatar file 中心。 |

Advanced Controls:

- Avatar file
- VRM metadata review
- Spring bones
- Motion primary source
- VMC address / port
- UNMotion / Zenoh
- Apply VMC root translation

Notes:

- `Browse` の横に `Metadata` を置く。
- VRM が設定されている場合、metadata の再確認導線は常に見える。
- Avatar file は Render Quality より上に維持する。

## Rendering & Presentation

目的: 見た目作りの中心。v1 では MToon を扱うが、v1.1 以後の rendering style 追加に耐える構造にする。

Top Controls:

- Rendering style: v1 は `MToon` 固定表示、将来 `PBR / Realistic / Offline RT` を追加。
- Compatibility: v1 は `UNAvatar v1 MToon` 相当を表示できる余地を持つ。
- Look preset: style-specific ではなく style-agnostic な名前にする。

Quick Set:

| Button | 意図 | v1 MToon での代表的な適用 |
| --- | --- | --- |
| `Natural` | authored 表現を尊重 | outline authored、bloom off、SSAO off、color neutral |
| `Clear` | 配信で輪郭を見やすく | outline override light、contact shadow subtle、contrast slight |
| `Stream Pop` | 画面映え優先 | outline override、bloom compact、color look pop/warm、shadow on |
| `Soft Studio` | 柔らかい studio 表現 | color look soft、contact shadow subtle、bloom low、SSAO low |

Advanced Controls:

- Rendering style (v1: disabled MToon)
- Color look / exposure / contrast / saturation / temperature / tint
- Outline policy / width / color / lighting / roundness
- Rim policy / intensity / power / color
- Matcap accent
- Specular accent
- Authored ambient occlusion
- Contact shadow
- SSAO
- Bloom quality / strength / threshold / radius

Notes:

- `MToon controls` という subsection は Advanced 側に置く。Simple preset 名は MToon 固有語にしない。
- 将来 PBR では同じ Quick Set を PBR 側パラメーターへ map する。
- `Offline RT` は runtime renderer とは性質が違うため、将来は style option ではなく export/render job として扱う可能性もある。UI 上は同じ `Rendering style` の文脈に置けるよう余地を残す。

## Motion

目的: アバターの動きの入力と補正を扱う。v1 では Avatar section に混ぜてもよいが、Profile editor が大きくなるため分離する。

Quick Set:

| Button | 効果 |
| --- | --- |
| `VMC Standard` | VMC を primary、root translation off。 |
| `UNMotion` | UNMotion/Zenoh を primary。 |
| `Full Body Ready` | root translation と torso/legs 系の将来項目を見つけやすくする。 |

Advanced Controls:

- Primary motion source
- VMC address / port
- UNMotion / Zenoh key
- Root translation
- Spring bones は Avatar 側に置くか Motion 側に置くかを実装時に決める。一般ユーザー視点では Avatar 側の方が見つけやすい。

## Output & Window

目的: 配信ソフトやデスクトップ上での出力形態を扱う。

Quick Set:

| Button | 効果 |
| --- | --- |
| `Desktop Window` | decorations ON、transparent OFF、Spout OFF。 |
| `Transparent Overlay` | transparent ON、input passthrough はユーザー確認後。 |
| `Spout Output` | Spout ON、サイズ設定を目立たせる。 |

Advanced Controls:

- transparent
- input passthrough
- decorations
- always on top
- minimized
- window size / position
- Spout enabled / name / width / height

Notes:

- input passthrough は透明時だけ有効。
- Window position / size は renderer から profile へ保存できる導線を維持する。

## Render Quality

目的: 見た目ではなく技術品質と負荷を扱う。

Quick Set:

| Button | 意図 | 代表値 |
| --- | --- | --- |
| `Light` | 低負荷 | FXAA、texture limit 2K/auto、compression auto |
| `Balanced` | v1 既定 | SMAA、texture limit off or auto、compression auto/source policy |
| `Quality` | 高品質 | SMAA/MSAA、texture limit off、Mitchell/Lanczos、cache on |

Advanced Controls:

- AA
- texture resolution limit
- texture compression
- advanced compression roles
- mipmap filter
- render backend
- BCn encoder / CPU threads
- processed texture cache

Notes:

- Render Quality は `Rendering & Presentation` と混ぜない。
- AA は post pass 実装でも effects.post ではなく品質設定。
- launch-time 項目が多いため `Restart` badge を明確にする。

## Camera

目的: 起動時の見え方と保存済み camera state を扱う。

Quick Set:

| Button | 効果 |
| --- | --- |
| `Bust Shot` | 上半身配信用。 |
| `Full Body` | 全身表示。 |
| `Face Focus` | 顔寄り。 |

Advanced Controls:

- target x/y/z
- longitude / latitude
- radius
- diagonal FOV
- camera lock
- Save from running renderer
- Restore to running renderer

## Developer Diagnostics

Settings の `Show developer controls` が ON のときだけ表示する。

内容:

- debug toggles
- show axes
- material diagnostics
- texture/cache summary
- runtime protocol / control capabilities
- raw manifest path
- reveal profile file
- diagnostics export
- screenshot / debug capture hooks

Developer section は通常の見た目作りから分離する。ユーザーが `Stream Pop` を選びたいだけのときに debug flag が視界へ入らないようにする。

## Simple Preset State

各 section は、現在値からどの quick set に近いかを推定できるとよい。ただし厳密一致にこだわらない。

推奨表示:

- 完全一致: selected
- 近いが一部変更: `Custom from Clear`
- 不明: `Custom`

実装初期は `Custom` 判定だけでもよい。重要なのは、Simple preset を押したあとに Advanced controls が実際の保存値として変わること。

## i18n

表示文字列は Rust 側 locale TOML を原本とする既存パターンに従う。

追加 namespace 案:

```toml
[profiles.editor.sections]
avatar = "Avatar"
presentation = "Rendering & Presentation"
motion = "Motion"
output_window = "Output & Window"
render_quality = "Render Quality"
camera = "Camera"
developer = "Developer Diagnostics"

[profiles.editor.quick_sets]
natural = "Natural"
clear = "Clear"
stream_pop = "Stream Pop"
soft_studio = "Soft Studio"
light = "Light"
balanced = "Balanced"
quality = "Quality"
custom = "Custom"
```

Simple preset 名は英語のままでも product tone として成立するが、日本語 locale では説明文を自然にする。

## Visual Design

方向性:

- 暗めの panel を基調に、細い cyan / pink / amber accent。
- card を入れ子にしない。
- preset button は小さな icon + title + 1行説明の segmented card。
- Advanced controls は密度高め。ただし label / slider / number input の整列を崩さない。
- Live / Restart badge は控えめだが必ず見える。
- 変更中 / running renderer 適用中 / restart required は色と文言で明確にする。

避けるもの:

- landing page 的な hero。
- 大きすぎる装飾カード。
- 紫一色の単調な palette。
- 専門語だけの button label。
- text が button / card 内で折り返せず崩れる状態。

## 実装順

### Phase 1: Design Scaffold

- Profile Header を作る。
- Section Pattern の共通 CSS を作る。
- `Rendering & Presentation` を新パターンへ移す。
- `Render Quality` を新パターンへ移す。
- Developer controls 表示 flag の置き場を決める。

### Phase 2: Quick Set

- `Rendering & Presentation` quick set を実装。
- `Render Quality` quick set を実装。
- quick set は複数 field update を順に呼ぶ。
- 起動中 renderer へ Live field が反映されることを確認する。

### Phase 3: Section Reorganization

- Avatar / Motion / Output & Window / Camera を新構造へ移す。
- Debug toggles を Developer section へ隔離する。
- Live / Restart / Profile / Debug badge を整理する。

### Phase 4: i18n / Polish

- locale TOML に文言を追加。
- 日本語 / 英語で text overflow を確認。
- mobile-ish narrow width と desktop wide width を Playwright screenshot で確認。
- 既存 profile の読み書き互換をテストする。

## Acceptance Criteria

- Avatar file 設定が Render Quality より上にある。
- Simple preset と Advanced controls が同一 section 内にあり、別ページ切替ではない。
- Simple preset を押すと Advanced controls の値が変わる。
- Advanced controls を手で変えても profile は壊れない。
- Developer controls は通常非表示にできる。
- Runtime 反映項目と Restart 必要項目が区別されている。
- v1 既存 profile の見た目は UI 改修だけでは変わらない。
- 将来 rendering style を増やしても `Rendering & Presentation` section に自然に追加できる。
- `cargo test -p un-avatar-supervisor`、`npm run check`、`npm run build` が通る。
- Playwright screenshot で Profiles editor に text overlap がない。

## Open Decisions

1. v1 で `[rendering] style = "mtoon"` を実際に書き始めるか。
   - 推奨: UI では `MToon` 固定表示を置くが、profile schema 追加は別 commit で慎重に行う。
2. Developer controls の表示設定を app settings に保存するか。
   - 推奨: app settings に保存。profile には入れない。
3. Quick Set の正確な preset 値。
   - 推奨: 実装時にまず conservative な値で始め、動作確認で調整する。
