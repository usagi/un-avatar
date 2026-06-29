# Development Guidelines

U.N. Avatar の開発とリリース前確認で使う最小限の運用メモ。

## プロジェクト識別

- 表示名: **U.N. Avatar**
- Repository / package base: **`un-avatar`**
- Cargo package prefix: **`un-avatar-*`**
- Rust library crate prefix: **`un_avatar_*`**
- Author: **usagi / USAGI.NETWORK**
- License: **MIT**

配布物に含める第三者コンポーネントは [`third-party-licenses.md`](third-party-licenses.md) に表示し、リリースパッケージでは同等内容を `LICENSES/` に含める。

## ローカル検証

push 前または大きな変更後は、リポジトリルートで次を実行する。

```sh
cargo xtask ci
```

`ci` は `apps/un-avatar-supervisor` の frontend check、`fmt --check`、`check --workspace`、`test --workspace`、CLI smoke、renderer smoke を順に実行する。ローカルでは `svelte-check` があれば `npm run check` を使い、未準備なら `npm ci`、半端な `node_modules` が残っている場合は `npm install --package-lock=false` で補修してから check する。GitHub Actions は先に `npm ci` で依存を準備し、`cargo xtask ci` 内の `npm run check` で Svelte / TypeScript の型崩れを release 前に落とす。

個別には次を使う。

```sh
cargo xtask fmt
cargo xtask check
cargo xtask test
cargo xtask render-smoke
```

v2 release-prep 中の小さな修正では、まず次の guard で直近の壊れやすい経路だけを確認する。

```sh
cargo xtask release-guard
```

`release-guard` は Renderer tray / startup splash / wardrobe transition / runtime status / standalone handoff / Supervisor static source checks などの unit/static regression guard をまとめて実行する。GUI、package rebuild、Spout2 実機確認、manual release evidence は含めないため、candidate 確定前は通常の `ci`、`release-package`、`release-audit`、`package-render-smoke`、手動 checklist を別途通す。

## Supervisor / Renderer の起動確認

開発中に Supervisor と Renderer の両方を更新して起動する場合は、`cargo run` / `cargo build` ではなく xtask を使う。

```sh
cargo xtask build
cargo xtask run
cargo xtask run --release
```

`cargo xtask build` / `run` は Supervisor frontend を build し、Supervisor と Renderer を同じ cargo profile で build してから起動する。workspace の default member だけを build すると Renderer の変更が反映されないことがある。

Renderer だけを起動前検証する場合は:

```sh
cargo xtask render-smoke
```

実ウィンドウ付きで profile manifest を開く場合は:

```sh
cargo xtask run-renderer --profile model1
cargo xtask run-renderer --profile model2 -- --debug-material-dump
```

## リリースパッケージ

標準の Windows リリースパッケージは Spout2 runtime を含める。

```sh
cargo xtask spout2
cargo xtask unity-exporter-package
cargo xtask release-package --version <version>
cargo xtask release-audit --version <version>
cargo xtask package-render-smoke
```

`release-package` は既定で build と package staging を実行し、`release-packages/un-avatar-<version>.zip` と `release-packages/un-avatar-<version>.zip.sha256.txt` を作る。zip 作成前に packaged Renderer の windowless startup smoke を実行し、zip 作成後に Renderer / Supervisor executable、license files、Unity Exporter、Spout2 runtime（`--skip-spout2` なしの場合）の必須 entry も検査する。
`release-audit` は既存の portable zip / checksum sidecar / VCC package zip / VCC repo listing と VCC listing の name / version / URL suffix を再ビルドせずに再検査する。portable zip は clean temp directory へ展開し、必須実行ファイル / license / Unity Exporter / Spout2 runtime が通常ファイルとして取り出せること、zip 内の README / LICENSE / third-party notices が現行 source と一致すること、さらに VCC zip の必須 entry が `target/unity/vcc-staging` の生成元と一致し、zip 内 `package.json` の name / version / release asset URL suffix と VCC repo listing の package URL が正しいことも確認する。`local/release-work/` に GitHub Release draft や manual checklist がある場合は、それらの hash / required text / Candidate Build も追加で確認する。
`package-render-smoke` は `target/package/un-avatar/un-avatar-renderer` を使い、window を開かず fixture glTF manifest を `--validate-startup` で検査する。実 profile / `.unavatar` も `cargo xtask package-render-smoke --manifest <path> --wardrobe-set <id>` で同じ packaged Renderer から windowless 検査できる。
`unity-exporter-package` は Unity Editor を起動せず、`unity/un-avatar-unity-exporter` を `target/unity/un-avatar-unity-exporter` へ UPM package layout としてコピーする。

VCC Package Manager 向け Unity Exporter は、GitHub Release 作成後に次を実行する。

```sh
cargo xtask unity-exporter-vcc --version <version>
```

`unity-exporter-vcc` は `target/unity/vcc/network.usagi.un-avatar.unity-exporter-<version>.zip` と `docs/vcc/index.json` を生成する。生成後に VCC zip 内の `package.json`、主要 Editor scripts、native `unavatar_fpng.dll`、license file の必須 entry と `zipSHA256` を検査する。既定の download URL は `https://github.com/usagi/un-avatar/releases/download/<version>/...` で、UN Avatar の git tag / release title と同じく `v` prefix は付けない。生成した zip を同じ GitHub Release asset に添付し、更新された `docs/vcc/index.json` を commit / push すれば、VCC の Package Manager で更新候補として表示される。

### Windows 配布方針

v2 の Windows 配布正本は portable zip とする。Installer（MSI / NSIS / WiX / cargo-wix 等）は v2 では未対応・対応未定であり、release pipeline の必須成果物にしない。

Authenticode 署名も v2 では未対応・対応未定とする。自己署名証明書は Windows 一般ユーザー向けの信頼問題を解決せず、証明書導入を求める運用は逆に負担になるため採用しない。UN Avatar は OSS / MIT として、配布物の透明性は source、build 手順、release notes、hash / checksum の公開で担保する。

## v2 リリース候補前の手動確認

v2 は `.unavatar` / VRC 由来機能を含むため、VRM だけでなく軽量 VRC model と重い wardrobe model を確認対象に入れる。
実機 GUI / tray / Spout2 / migration の作業証跡や GitHub Releases 用の草稿は、公開 docs ではなく `local/release-work/` に置く。このディレクトリは `.gitignore` 済みで、`release-audit` はファイルが存在する場合だけ追加検証する。

- `cargo xtask ci`
- `cargo xtask release-package --version <version>` が portable zip、sha256 sidecar を生成し、packaged Renderer smoke と zip entry 検査を通すこと
- `cargo xtask release-audit --version <version>` が portable zip checksum sidecar、clean unpack、source docs freshness、VCC zip staging freshness、VCC package manifest、VCC package URL consistency、VCC zipSHA256、VCC listing name / version / URL suffix、必須 entry 検査を通すこと。`local/release-work/` に release notes draft や manual checklist がある場合は、それらの hash / required text / Candidate Build も追加で検査する。
- `cargo xtask unity-exporter-vcc --version <version>` が VCC zip、repo listing、zipSHA256、必須 Unity Exporter entry 検査を通すこと
- current Unity Exporter で再出力した `target/tmp/usagi.unavatar`、`target/tmp/blanca.unavatar`、`target/tmp/mizuki.unavatar` が `cargo xtask unphysics-exporter-audit` を通し、UNPhysics / UNDynamics sourceParams の必須 term 欠落が無いこと
- 同じ3件が `cargo xtask unphysics-importer-audit --require-node-constraints --require-parent-node-constraints` を通し、sourceParams が Importer/lowering 後の runtime dynamics group / response group まで消失せず到達し、node constraint / parent constraint counts も同じ import report から取得できること
- 同じ3件が `cargo xtask unphysics-response-audit --require-visual-response-evidence` を通し、UNPhysics response の `rest_response` / `shape_preservation` / `bounce_response` / `damping_half_life_ms` / `motion_coupling` が soft / firm profile override で分離し、active response group が weighted visible skin joint または visible mesh subtree へ届くこと。summary は mode ごとに `top` / `top_visual` / `top_nonvisual` と `top_group` / `top_visual_group` / `top_nonvisual_group` を出し、可視 target へ届くカテゴリ・source group と非可視制御・interaction 系に偏るカテゴリ・source group を分けて読めること
- `cargo xtask unphysics-motion-audit` が solver motion trace で soft / firm tuning、cloth、ears preset、bounce、source-authored shape intent と soft response の分離による step 出力差を確認すること
- 同じ3件が `cargo xtask unphysics-motion-trace-audit --require-known-finding-visibility --require-no-visible-findings` を通し、実 `.unavatar` 上でカテゴリ別および source_id group 別の motion lag、回復後 lag、自然静止 baseline との差分、残留 motion、`findingKinds` / `findingTop` / `visualFindingTop` / `visualFindings` / `nonvisualFindings` / `unknownVisibilityFindings` 分類、`top` / `top_visual` / `top_nonvisual` / `top_group` / `top_visual_group` / `top_nonvisual_group` 要約を取得できること。xtask は CLI を `--require-motion-evidence` 付きで呼ぶため、`missing_motion_evidence` が空でない場合は release gate 失敗である。可視 target を持たない制御・interaction 系は `nonvisual_control_motion` として分け、override seed にしないこと。可視 target ありの motion finding は `visualFindings > 0`、finding の可視性分類漏れは `unknownVisibilityFindings > 0` として失敗扱いにすること
- Supervisor diagnostics から dynamics tuning を作る場合、まず `match_overrides` の semantic match rule または exact `source_id` match rule として扱い、`group_overrides` は source id 単位の final pin が本当に必要な場合だけ使うこと。diagnostics seed は特定衣装名の runtime 分岐や自動 pin にしないこと
- collider projection と mesh cloth assist の実経路確認が必要な wardrobe/profile では `cargo xtask unphysics-importer-audit --wardrobe-set <SET_ID> --require-mesh-cloth-assist-candidates <avatar.unavatar>` を通し、mesh cloth assist sample / candidate / seed candidate 件数と candidate 最大 region を取得できること。続けて `cargo xtask unphysics-vertex-probe-audit --wardrobe-set <SET_ID> --apply-mesh-cloth-assist --require-mesh-cloth-assist-changes --require-collision-projections --require-collision-projection-sources --require-collision-projection-paths --require-collider-summaries --require-projecting-collider-summaries <avatar.unavatar>` を通し、runtime collider count、mesh cloth assist 変更頂点数、collision projection 件数、probe 対象 mesh の weighted dynamic source 数、probe source に限定した projection 件数、probe source に限定した collider candidate / penetrating / projecting summary 件数、projection source 種類数、projection collider path 数、collider path summary 件数、projection 付き collider path summary 件数、projection_count 最大 collider path を取得できること。選択 mesh 自身の dynamics source が実 projection まで届いたことを gate したい場合だけ `--require-probe-collision-projections` を追加し、全体 simulation の projection を選択 mesh の collider evidence と混同しないこと
- `cargo xtask package-render-smoke --manifest target/tmp/mizuki-split-data-bc7-unorm.toml --wardrobe-set field_drape` が packaged Renderer で代表 wardrobe set の windowless startup validation を通すこと
- `cargo xtask package-render-smoke --manifest target/tmp/mizuki-split-data-bc7-unorm.toml --wardrobe-set noble1` が packaged Renderer で別系統の代表 wardrobe set の windowless startup validation を通すこと
- `cargo xtask run --release` で Supervisor が起動し、profile 作成 / 編集 / Renderer 起動 / 停止ができること
- Renderer 直接起動用 shortcut、taskbar launcher、Renderer tray からの停止 / Supervisor 起動が機能すること
- profile UI のユーザー向け物理名は `UNPhysics` / `UNDynamics` とし、`SpringBone` / `PhysBone` は source diagnostics や互換文脈以外に出さないこと
- `model1` 相当の VRM で UNMF/Z、Perfect Sync、手足、UNPhysics が動くこと
- 軽量 VRC model で PhysBone 由来 UNDynamics と Perfect Sync / ShapeKey が動くこと
- `mizuki-split` class の `.unavatar` で Base と代表 wardrobe set を起動し、loading / texture / mesh / shader / first-frame / fps の summary を保存すること
- `cargo xtask run-renderer --release --profile mizuki-split --wardrobe-set <set> -- --bench-frames 180 --no-fps-title` と `cargo xtask summarize-renderer-log` で hot path の比較 TSV を残すこと
- `キャッシュ準備` は通常起動と別操作として機能し、完了 summary が processed / compressed texture cache と pipeline cache の結果を示すこと
- `.unavatar` の unsupported / approximate / diagnostics-only 機能は CLI diagnose、Renderer runtime status、Supervisor diagnostics のいずれかで観測できること
- full Animator graph、dynamic reactive mesh gating、PhysBone suffix value emission、VRC Constraints solver integration は未完了領域として隠さないこと

## v1 リリース前の手動確認

最低限、次を確認する。

- `cargo xtask ci`
- `cargo xtask run --release`
- model1 / model2 相当のプロファイル起動
- texture compression `balanced` / GPU encoder / 空の `UN_AVATAR_TEXTURE_CACHE_DIR` で model1 / model2 相当がReadyまで進むこと。旧 `auto` / `advanced` は `balanced` alias として読む。
- Profiles から Renderer 起動、停止、複製、削除
- 実行中 Renderer への runtime 設定反映
- 透過 ON/OFF と、再起動が必要な設定の表示
- Spout2 Sender が実アバター描画開始後に出ること
- スクリーンショット保存と保存先フォルダー open
- アバターサムネイル icon cache
- README / LICENSE / `THIRD_PARTY_NOTICES.md` / `LICENSES/third-party-licenses.md` が配布物へ入ること

## フォーマット

- Rust: `cargo fmt`
- Text / Markdown: UTF-8、LF
- ユーザー向け文書は日本語を基本にし、プロトコル名、crate 名、API 名などは公式表記を使う。

## 設計文書の扱い

公開時点の正本は現行コード、[`../README.md`](../README.md)、[`roadmap.md`](roadmap.md)、[`runtime-mvp.md`](runtime-mvp.md)。古い設計メモと食い違う場合は現行実装を優先する。
