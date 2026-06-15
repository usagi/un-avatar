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
`release-audit` は既存の portable zip / checksum sidecar / VCC package zip / VCC repo listing / release notes draft / manual release checklist の hash と VCC listing の name / version / URL suffix を再ビルドせずに再検査する。portable zip は clean temp directory へ展開し、必須実行ファイル / license / Unity Exporter / Spout2 runtime が通常ファイルとして取り出せること、zip 内の README / LICENSE / third-party notices が現行 source と一致すること、release notes draft に v2 release text の必須事項が残っていること、さらに VCC zip の必須 entry が `target/unity/vcc-staging` の生成元と一致し、zip 内 `package.json` の name / version / release asset URL suffix と VCC repo listing の package URL が正しいことも確認する。GitHub Release draft、manual checklist、VCC `zipSHA256` / package URL、README 更新後の package 作り直し忘れ確認に使う。
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
実機 GUI / tray / Spout2 / migration の証跡は [`v2-manual-release-checklist.md`](v2-manual-release-checklist.md) に沿って残す。

- `cargo xtask ci`
- `cargo xtask release-package --version <version>` が portable zip、sha256 sidecar を生成し、packaged Renderer smoke と zip entry 検査を通すこと
- `cargo xtask release-audit --version <version>` が portable zip checksum sidecar、clean unpack、source docs freshness、release notes required text、VCC zip staging freshness、VCC package manifest、VCC package URL consistency、VCC zipSHA256、VCC listing name / version / URL suffix、release notes draft hash、manual checklist Candidate Build、必須 entry 検査を通すこと
- `cargo xtask unity-exporter-vcc --version <version>` が VCC zip、repo listing、zipSHA256、必須 Unity Exporter entry 検査を通すこと
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
