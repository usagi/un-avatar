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

`ci` は `fmt --check`、`check --workspace`、`test --workspace`、CLI smoke、renderer smoke を順に実行する。

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
cargo xtask release-package --version 1.0.0
```

`release-package` は既定で build と package staging を実行し、`release-packages/un-avatar-<version>.zip` を作る。
`unity-exporter-package` は Unity Editor を起動せず、`unity/un-avatar-unity-exporter` を `target/unity/un-avatar-unity-exporter` へ UPM package layout としてコピーする。

### Windows 配布方針

v2 の Windows 配布正本は portable zip とする。Installer（MSI / NSIS / WiX / cargo-wix 等）は v2 では未対応・対応未定であり、release pipeline の必須成果物にしない。

Authenticode 署名も v2 では未対応・対応未定とする。自己署名証明書は Windows 一般ユーザー向けの信頼問題を解決せず、証明書導入を求める運用は逆に負担になるため採用しない。UN Avatar は OSS / MIT として、配布物の透明性は source、build 手順、release notes、hash / checksum の公開で担保する。

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
- README / LICENSE / `docs/third-party-licenses.md` が配布物へ入ること

## フォーマット

- Rust: `cargo fmt`
- Text / Markdown: UTF-8、LF
- ユーザー向け文書は日本語を基本にし、プロトコル名、crate 名、API 名などは公式表記を使う。

## 設計文書の扱い

公開時点の正本は現行コード、[`../README.md`](../README.md)、[`roadmap.md`](roadmap.md)、[`runtime-mvp.md`](runtime-mvp.md)。古い設計メモと食い違う場合は現行実装を優先する。
