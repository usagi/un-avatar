<p align="center">
  <img src="assets/brand/un-avatar-artwork-supervisor.png" width="160" alt="U.N. Avatar">
</p>

# U.N. Avatar

U.N. Avatar は VRM(0/1) と VRC 向け Unity アバターを、配信向けに軽快に表示する「仮想アバターレンダラー」です。姿勢や表情の入力を U.N. Motion からの UNMF/Z や VMC 対応で受け、透過ウィンドウ、スクリーンショット保存、Spout2 出力などを行えます。

v2 では従来の .vrm モデルと MToon / SpringBone 対応に加えて、VRC 向けモデルと lilToon / PhysBone 、 Modular Avatar ベースの U.N. Avatar 独自のお着替え機能 Wardrobe を扱えます。VRC モデルは Unity Editor に追加する U.N. Avatar Exporter パッケージで `.unavatar` 形式にエクスポートして使います。

## はじめて使う場合の簡単な手順

1. Supervisor Console を起動します。(`un-avatar-supervisor.exe`)
2. 画面左側メニューからプロファイルのタブを開き、`+` ボタンで新しいプロファイルを作ります。
3. アバターファイルとして `.vrm` または `.unavatar` を開きます。
4. 各種設定を行い、「起動」ボタンで Renderer を起動します。

### VRCモデルを使う場合の U.N. Avatar Exporter の導入方法と簡単な操作手順

- VRChat Creator Companion (VCC) のパッケージマネージャーから U.N. Avatar Exporter を探してインストール。

他の方法として次の方法でも可能です:

- 別法1: Unity Editor の Window > Package Manager で「Add package from git URL」を選び、`https://github.com/usagi/un-avatar-exporter.git` を入力してインストール。
- 別法2: Unity Editor の Window > Package Manager で「Add package from disk」を選び、U.N. Avatar 配布パッケージの `unity/un-avatar-unity-exporter` フォルダーの `un-avatar-exporter/package.json` を指定してインストール。

#### 操作手順

1. Unity Editor で Tools > U.N. Avatar > Exporter .unavatar を開きます。
2. Avatar Root に Hierarchy からアバターのルート GameObject を指定（ドラッグ＆ドロップ）します。
3. Output に `.unavatar` を出力するパスを設定します。
4. 操作パネルの 1. Base → 2. Wardrobe Sets → 3. Export の順に操作して `.unavatar` を出力します。

お着替え機能 Wardrobe を使わない場合は Export Mode を「Current to Base Only」にして 3. Export へ進めます。 Wardrobe を使う場合は Export Mode を「Wardrobe」にして、1. Base と 2. Wardrobe Sets で衣装や小物などの状態を保存してから 3. Export で出力します。

##### お着替え機能 Wardrobe 設定

お着替え機能 Wardrobe で設定した Base / Sets はアバターのレンダリング中にいつでも切り替え（お着替え）可能になります。

Base / Sets はお着替えの状態を作り保存します。それぞれの状態ごとにHierarchyで有効/無効としたいオブジェクトをGameObject Active Toggle （Inspector のチェックボックス）で切り替えた状態を作り U.N. Avatar Exporter の操作パネルで保存します。

- **Base**
  - アバターのお着替え元とする基本的な衣装やアイテムが有効な状態かつ不要な衣装やアイテムはInspectorで「Capture Current As Base」ボタンから状態を保存します。変更したくなったら同じボタンで再度保存できます。
- **Sets**
  - お着替えバリエーションごとに「Capture Current As Set」ボタンから状態を保存します。変更したくなったら Update 、複製したくなったら Duplicate 、削除したくなったら Remove ボタンを使います。
  - Set は幾つでも作れます。

素体に Modular Avatar 対応衣装をぶら下げた状態の Hierarchy では、素体のみ有効、ぶら下げた追加衣装は無効の状態で Base を保存し、衣装を有効にして素体側の衣装を適切に無効にしたり、ブレンドシェイプで素体の状態を調整した状態で Set を追加保存、また別の衣装の状態を作って Set を追加保存、のように操作して衣装状態を設定します。

Tips:

- Base / Sets の名称ボタン部分をクリックすると、アバターの衣装状態が保存された状態に切り替えられ何かと便利です。

## 想定ユーザー

- U.N. Motion や VMC 送信対応アプリと組み合わせて、Web カメラや各種入力デバイスからのモーションでアバターを動かしたい VStreamer / VTuber / virtual avatar user
- アバターを透過ウィンドウや Spout2 で配信ソフトへ載せたい民
- VRM / MToon / SpringBone だけでなく、VRC 向け Unity avatar / lilToon / PhysBone を使いたい民
- UNMF/Z、VRM、`.unavatar`、表示の確認、ツール連携、研究用途で renderer を直接扱いたい研究者 / 開発者

## できること

- Supervisor Console で複数のプロファイルと Renderer を統合的に管理。
- Renderer でプロファイルごとの設定に基づいたアバターを描画。
- VRM / glTF / `.unavatar` ベースのアバターを wgpu renderer (backend: Vulkan / DX12) で描画、出力。
- VRC / Unity avatar を U.N. Avatar Exporter で `.unavatar` に変換し、Unity なしの Renderer runtime で利用。
- UNMF/Z と VMC/UDP のモーション入力を受け取り、姿勢、表情、手指、UNPhysics / UNDynamics を反映。
- VRM SpringBone と VRC PhysBone を独自の互換レイヤーを通し差異を吸収して UNPhysics / UNDynamics システムで統合的に扱う。
- VRM Mtoon と VRC lilToon を独自の互換レイヤーを通し差異を吸収して UNToon システムで統合的に扱う。
- Modular Avatar の互換レイヤーを通し、素体と衣装の組み合わせを Wardrobe / MergeArmature / BoneProxy / ObjectToggle で統合的に扱う。ついでに Modular Avatar 非対応の衣装も Wardrobe で同様に扱える。
- プロファイルごとにアバターファイル、モーション入力、出力、描画品質、ルック、ウィンドウ、カメラ、ライティングを設定。
- 透過ウィンドウ、クリック透過、最前面表示、背景色、XYZ 軸表示、簡易コライダー表示などの便利機能。
- Spout2 Sender として OBS などへアバター映像を転送。
- スクリーンショットをユーザーの Pictures 配下に保存。

## ここが嬉しい

- 軽量で高速: Renderer は Supervisor と別プロセスで動作し、GPU skinning / GPU morph / texture cache / texture compression などで実行時の負荷を抑えます。
- 配信に使いやすい: 透過ウィンドウと Spout2 出力を標準的な出力として対応、配信アプリとの組み合わせが容易で軽快です。
- 複数運用しやすい: 複数のプロファイルを設定分けし、複数の Renderer を同時に稼働可能なので、複数アバターを同時に使ったり、同じモーションで別アバターを同時表示したりできます。
- VRC モデルも使いやすい: Unity Exporter が VRC avatar、lilToon material、PhysBone、Expression Menu / Animator、wardrobe set を `.unavatar` にまとめ、Renderer はそれを Unity なしで扱えます。
- 見た目を整えやすい: AA、texture policy、背景色、肌色合わせ、Bloom、SSAO、Contact shadow、シルエットアウトライン、Lighting などをプロファイルごとに設定できます。MatCap、Specular、Rim、material outline などはモデル authored value を UNToon material として尊重します。
- 安定性と軽快さ: Rust と wgpu で堅牢さと一般性を両立したネイティブコードの Renderer プロセス本体は軽量で効率よく安定して動作し、Tauri/WebView で作られた Supervisor GUI プロセスは扱いやすく高度な表現でプロファイルや Renderer の管理を便利に行えます。

## 一般的な使い方

1. Supervisor Console (un-avatar-supervisor.exe) を起動します。
2. Profiles でプロファイルを作成します。
3. VRM を使う場合は `.vrm`、VRC / Unity avatar を使う場合は Exporter で作成した `.unavatar` をアバターファイルに設定します。
4. モーション入力、出力、ウィンドウ、カメラなどを設定します。
5. Profiles または Renderers から Renderer を起動します。
6. U.N. Motion などから UNMF/Z または VMC/UDP でモーションを送ります。
7. 必要に応じて透過ウィンドウ、Spout2、背景色、ライティング、ルック、wardrobe set、スクリーンショットを調整します。

## VRC / Unity アバターを使う場合

U.N. Avatar Renderer は Unity Editor や VRChat client を実行時に必要としません。VRC 向け avatar は、Unity Editor 上の U.N. Avatar Exporter で `.unavatar` に変換してから使います。

```text
Unity project with VRC avatar
  -> U.N. Avatar Exporter
  -> .unavatar
  -> U.N. Avatar Supervisor / Renderer
  -> Window / Spout2
  -> OBS or streaming software
```

`.unavatar` には、avatar mesh、texture、material metadata、lilToon-compatible parameters、PhysBone 由来 dynamics、Expression Menu / Animator 由来 action、wardrobe set などが含まれます。第三者への共有や配布は、必ず元アバター、衣装、テクスチャ等の利用規約に従ってください。

## Wardrobe / Runtime Actions

`.unavatar` に wardrobe set が含まれている場合、Renderer 起動後に Renderer tray や Supervisor から衣装・小物・見た目プリセットを切り替えられます。VRC Expression Menu / Animator 由来の操作は、配信用に使う runtime action として UNAnimator にまとめて扱います。

v2 は VRChat client や VRC SDK の完全な runtime 互換実装ではありません。VRC avatar を U.N. Avatar の独立 Renderer で配信利用しやすくするため、material、dynamics、wardrobe、action を `.unavatar` と runtime status へ正規化して扱います。

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

この repository には Supervisor GUI、Renderer runtime、VRM / glTF / `.unavatar` I/O、Unity Exporter、motion adapter、release tooling が含まれています。

よく使うコマンド:

```sh
cargo xtask ci
cargo xtask build
cargo xtask run
cargo xtask run --release
cargo xtask release-package --version <version>
cargo xtask release-audit --version <version>
cargo xtask package-render-smoke
cargo xtask package-render-smoke --manifest <path> --wardrobe-set <set-id>
cargo xtask unity-exporter-vcc --version <version>
```

Renderer だけを smoke test する場合:

```sh
cargo xtask render-smoke
```

`package-render-smoke --manifest <path> --wardrobe-set <set-id>` は packaged Renderer で実 avatar manifest と代表 wardrobe set の起動検証を windowless に実行します。
`release-audit --version <version>` は既存の portable zip、checksum sidecar、VCC package zip、`docs/vcc/index.json`、`docs/v2-release-notes-draft.md`、`docs/v2-manual-release-checklist.md` の hash / 必須 entry / VCC listing name・version・URL 整合を再ビルドなしで検査します。

Supervisor の UI を編集する場合:

```sh
cd apps/un-avatar-supervisor
npm install
npm run dev
```

注意: workspace の default member は Supervisor 側です。開発中に Renderer 変更も含めて起動確認する場合は、`cargo run` / `cargo build` ではなく `cargo xtask build` または `cargo xtask run` を使ってください。

## ドキュメント

- [Documentation Index](docs/README.md): 公開文書と開発メモの索引
- [v2 Roadmap](docs/v2-roadmap.md): `.unavatar` / VRC Unity Exporter を中核にした v2 計画
- [Unity Exporter](docs/unity-exporter-v0.1.md): Unity project から `.unavatar` を出力する Exporter の境界
- [v2 UI / GUI Operation Plan](docs/v2-ui-gui-operation-plan.md): Renderer tray、Supervisor、wardrobe、UNAnimator の操作方針
- [UNPhysics / UNDynamics v2](docs/unphysics-undynamics-v2.md): SpringBone / PhysBone 由来 dynamics の正規化方針
- [Runtime MVP](docs/runtime-mvp.md): VRM / VMC / MToon / wgpu / Spout2 runtime の歴史的な土台
- [Roadmap](docs/roadmap.md): v1 安定対象と v2 への接続
- [Render Quality Plan](docs/render-quality-plan.md): AA、mipmap、texture compression、描画品質
- [Development Guidelines](docs/development-guidelines.md): 開発時の確認方針
- [Third-party Licenses](docs/third-party-licenses.md): Spout2 などの third-party licenses

## 関連プロジェクト

- [U.N. Motion](https://github.com/usagi/un-motion): Web カメラや VMC 入力からモーションを作るモーションキャプチャアプリ
- [U.N. Motion Frame](https://github.com/usagi/un-motion-frame): U.N. Motion Frame / Zenoh (UNMF/Z) プロトコルの定義
- [U.N. Virtual Avatar Connect](https://github.com/usagi/un-virtual-avatar-connect): 仮想アバターと周辺アプリをつなぐデータフロー駆動のブリッジアプリ
- [U.N. Virtual Eye Tracker](https://github.com/usagi/un-virtual-eye-tracker): 仮想アイトラッカー

## Acknowledgements

U.N. Avatar の toon rendering、material compatibility、Wardrobe / avatar assembly behavior は独立した Rust / wgpu / WGSL / Unity Exporter 実装ですが、互換性検証と behavior の理解にあたり、MIT License で公開されている次の先行プロジェクトを重要な参考実装として扱います。

- [lilToon](https://github.com/lilxyzw/lilToon): UNToon v2 / lilToon-compatible rendering の主要な参考実装
- [MToon](https://github.com/Santarh/MToon): VRM / MToon material compatibility の参考実装
- [Modular Avatar](https://github.com/bdunderscore/modular-avatar): `.unavatar` Wardrobe / MergeArmature / BoneProxy / ObjectToggle behavior の主要な参考実装

これらの project 名は互換性の説明と謝辞のために記載しています。U.N. Avatar は各 project の公式派生物または公式実装ではありません。

## License

[MIT](LICENSE)

Third-party components keep their own licenses. See [docs/third-party-licenses.md](docs/third-party-licenses.md).

## Author

[usagi / USAGI.NETWORK](https://usagi.network)
