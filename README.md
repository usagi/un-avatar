<p align="center">
  <img src="assets/brand/un-avatar-artwork-supervisor.png" width="160" alt="U.N. Avatar">
</p>

# U.N. Avatar

U.N. Avatar はアバターを軽快にレンダリングする「仮想アバターレンダラー」アプリです。姿勢や表情の入力を U.N. Motion からの UNMF/Z や VMC 対応で受け、ウィンドウ表示、スクリーンショット保存、Spout2 出力などを行えます。

## 想定ユーザー

- U.N. Motion や VMC 送信対応アプリと組み合わせて、Web カメラや各種入力デバイスからのモーションでアバターを動かしたい VStreamer / VTuber / virtual avatar user
- アバターを透過ウィンドウや Spout2 で配信ソフトへ載せたい民
- 軽快な単体アバターレンダラーを求める民
- UNMF/Z や VRM / MToon 表示の検証、ツール連携、研究用途で renderer を直接扱いたい研究者 / 開発者

## できること

- Supervisor Console で複数のプロファイルと Renderer を統合的に管理。
- Renderer でプロファイルごとの設定に基づいたアバターを描画。
- VRM / glTF ベースのアバターを wgpu renderer (backend: Vulkan / DX12) で描画、出力。
- UNMF/Z と VMC/UDP のモーション入力を受け取り、姿勢、表情、手指、SpringBone を反映。
- プロファイルごとにアバターファイル、モーション入力、出力、描画品質、ルック、ウィンドウ、カメラ、ライティングを設定。
- 透過ウィンドウ、クリック透過、最前面表示、背景色、XYZ 軸表示、簡易コライダー表示などの便利機能。
- Spout2 Sender として OBS などへアバター映像を転送。
- スクリーンショットをユーザーの Pictures 配下に保存。

## ここが嬉しい

- 軽量で高速: Renderer は Supervisor と別プロセスで動作し、GPU skinning / GPU morph / texture cache / texture compression などで実行時の負荷を抑えます。
- 配信に使いやすい: 透過ウィンドウと Spout2 出力を標準的な出力として対応、配信アプリとの組み合わせが容易で軽快です。
- 複数運用しやすい: 複数のプロファイルを設定分けし、複数の Renderer を同時に稼働可能なので、複数アバターを同時に使ったり、同じモーションで別アバターを同時表示したりできます。
- 見た目を整えやすい: AA、texture policy、背景色、肌色合わせ、Bloom、SSAO、Contact shadow、Outline、MatCap、Specular、Rim、Lighting などをプロファイルごとに設定できます。
- 安定性と軽快さ: Rust と wgpu で堅牢さと一般性を両立したネイティブコードの Renderer プロセス本体は軽量で効率よく安定して動作し、Tauri/WebView で作られた Supervisor GUI プロセスは扱いやすく高度な表現でプロファイルや Renderer の管理を便利に行えます。

## 一般的な使い方

1. Supervisor Console (un-avatar-supervisor.exe) を起動します。
2. Profiles でプロファイルを作成します。
3. アバターファイル、モーション入力、出力、ウィンドウ、カメラなどを設定します。
4. Profiles または Renderers から Renderer を起動します。
5. U.N. Motion などから UNMF/Z または VMC/UDP でモーションを送ります。
6. 必要に応じて透過ウィンドウ、Spout2、背景色、ライティング、ルック、スクリーンショットを調整します。

## U.N. Motion と組み合わせて OBS などで配信する場合の例

[U.N. Motion](https://github.com/usagi/un-motion) は Web カメラや VMC 入力からモーションを作るアプリで、U.N. Avatar はそのモーションを受けてアバターを表示できます。

典型的には次の構成です。

```text
Web camera / VMC app
  -> U.N. Motion
  -> UNMF/Z or VMC/UDP
  -> U.N. Avatar Renderer
  -> Window / Spout2
  -> OBS or streaming software
```

※U.N. Motion なしでも、VMC/UDP を送信できる既存アプリや、[UNMF/Z](https://github.com/usagi/un-motion-frame) を扱うアプリから U.N. Avatar を使えます。

## Spout2 について

Windows 版の標準リリースパッケージには Spout2 runtime が同梱され、Spout2 Sender 出力を利用できます。
Spout2 は BSD 2-Clause License の第三者コンポーネントです。配布物にはライセンス表示が含まれています。

## 対応環境

- Windows 10 / 11
- Vulkan または DX12 対応 GPU
- Spout2 出力は Windows 版の標準リリースパッケージで利用可能

## 開発者向け

この repository には Supervisor GUI、Renderer runtime、VRM / glTF / UNA I/O、motion adapter、plugin host、release tooling が含まれています。

よく使うコマンド:

```sh
cargo xtask ci
cargo xtask build
cargo xtask run
cargo xtask run --release
cargo xtask release-package --version 1.0.0
```

Renderer だけを smoke test する場合:

```sh
cargo xtask render-smoke
cargo xtask unity-exporter-package
```

Supervisor の UI を編集する場合:

```sh
cd apps/un-avatar-supervisor
npm install
npm run dev
```

注意: workspace の default member は Supervisor 側です。開発中に Renderer 変更も含めて起動確認する場合は、`cargo run` / `cargo build` ではなく `cargo xtask build` または `cargo xtask run` を使ってください。

## ドキュメント

- [Documentation Index](docs/README.md): 公開文書と開発メモの索引
- [Roadmap](docs/roadmap.md): 実装状況、v1 境界、次の候補
- [v2 Roadmap](docs/v2-roadmap.md): `.unavatar` / VRC Unity Exporter を中核にした v2 計画
- [Runtime MVP](docs/runtime-mvp.md): VRM / VMC / MToon / wgpu / Spout2 runtime
- [Profile Settings UI v1 Design](docs/profile-settings-ui-v1-design.md): Profiles / Renderers UI の情報設計メモ
- [Render Quality Plan](docs/render-quality-plan.md): AA、mipmap、texture compression、描画品質
- [Development Guidelines](docs/development-guidelines.md): 開発時の確認方針
- [Third-party Licenses](docs/third-party-licenses.md): Spout2 などの third-party licenses

## 関連プロジェクト

- [U.N. Motion](https://github.com/usagi/un-motion): Web カメラや VMC 入力からモーションを作るモーションキャプチャアプリ
- [U.N. Motion Frame](https://github.com/usagi/un-motion-frame): U.N. Motion Frame / Zenoh (UNMF/Z) プロトコルの定義
- [U.N. Virtual Avatar Connect](https://github.com/usagi/un-virtual-avatar-connect): 仮想アバターと周辺アプリをつなぐデータフロー駆動のブリッジアプリ
- [U.N. Virtual Eye Tracker](https://github.com/usagi/un-virtual-eye-tracker): 仮想アイトラッカー

## License

[MIT](LICENSE)

Third-party components keep their own licenses. See [docs/third-party-licenses.md](docs/third-party-licenses.md).

## Author

[usagi / USAGI.NETWORK](https://usagi.network)
