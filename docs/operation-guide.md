# U.N. Avatar 操作ガイド

## はじめて使う場合

1. [Windows版のZIP](https://github.com/usagi/un-avatar/releases/latest)をすべて展開し、`un-avatar-supervisor.exe` を起動します。
2. 左側メニューの `プロファイルと稼働` を開き、`新規` で新しいプロファイルを作ります。既存のものを使う場合は、一覧から選びます。
3. アバターファイルとして `.vrm` または `.unavatar` を選びます。
4. 編集対象の名前を確認し、そのプロファイルの稼働欄から Renderer を起動します。Renderer は、アバターを実際に表示する処理です。
5. U.N. Motion や VMC 対応アプリから UNMF/Z または VMC/UDP でモーションを送ります。

`全稼働個体` は、起動済みの Renderer をまとめて確認・操作する画面です。最初の設定と起動は `プロファイルと稼働` で進められます。プロファイルを選び直すだけでは、起動済みの Renderer は切り替わりません。

同じPCのU.N. Motionは、UNMF/Zの接続方法を既定の「Auto（同じPC）」のまま利用できます。
LAN内の別PCへ接続する場合は「Address（LAN）」を選び、U.N. Motionに表示された
`IP:port`を入力します。送信側PCでは、指定したTCP待受ポートの受信をFirewallで許可する必要があります。

VRC / Unity アバターを使う場合は、先に U.N. Avatar Exporter で `.unavatar` を作ります。`.unavatar` は、VRC / Unity アバターを U.N. Avatar で使うための配信用アバターパッケージです。Exporter の導入、`1. Base -> 2. Wardrobe Sets -> 3. Export` の操作、Wardrobe の作り方は [v2 Getting Started](v2-getting-started.md) を参照してください。

最初は Supervisor でプロファイルを作成・調整します。設定が固まったら、ショートカットやピン留めから特定プロファイルの Renderer を直接起動できます。Renderer 単独起動中の Wardrobe、出力、ウィンドウ、カメラなどの操作は、Windows タスクトレイの Renderer アイコンを右クリックして行います。

## 複数プロファイル・Renderer を使う場合

更新前の設定退避と旧版へ戻す方法は [更新・バックアップ・復元](update-and-recovery.md) を参照してください。EXEを別フォルダーに置くだけでは、設定の保存先は分離されません。

プロファイルは保存する設定、Renderer はその設定で起動したアバター表示です。プロファイルを選んで編集することと、稼働一覧で操作する Renderer を選ぶことは別の操作です。同じ名前を使う場合は、プロファイルの識別情報と Renderer の番号も確認してください。

- 設定変更は編集対象のプロファイルに保存されます。稼働個体がなければ、次回起動に使われます。
- カメラ・物理・モーションなど即時反映に対応した設定は、同じプロファイルを使う稼働個体が対象です。個体を一つ選んだだけで、その個体専用の設定になるわけではありません。
- 現在のウィンドウ設定の即時反映は同じプロファイルの先頭個体が対象で、背景色は全個体が対象です。起動時に読み込む項目は再起動が必要です。画面の反映範囲・再起動待ち表示を確認してください。
- 稼働一覧でのカメラやウィンドウの操作と、プロファイルへの保存は区別されています。今の状態を次回も使う場合は、該当する「プロファイルへ保存」操作を使います。保存設定から戻す操作もあります。
- 「保存済み・反映失敗」は、設定ファイルの保存失敗とは異なります。表示された個体と操作を確認してください。

ここでの説明は、このリポジトリの現行実装に対応します。配布済みの旧版とは表示や対応範囲が異なる場合があります。

### 保存できなかった変更を確認する

「保存できていない変更」には、対象プロファイルと保存できなかった項目・入力値が表示されます。別のプロファイルを選んでも、この入力の保存先は元の対象です。入力の保持は現在のウィンドウ内だけなので、保存の問題を残したまま強制終了しないでください。

- 保存内容が別の操作で変わった場合は「最新の保存内容を確認」を開き、保持された入力と比較します。確認だけでは上書きしません。必要な値を、その対象の設定欄へ改めて入力してください。
- その他の保存失敗では、原因を解消して「この値で保存を再試行」を使います。成功したか不明な場合は、先に保存内容を確認してください。
- プリセットなどの一括変更は「一括変更の保存確認」にまとまって残ります。最新の設定と比較して必要な変更を行い、不要になった記録は「保持した一括変更を破棄」で消します。この操作は保存済み設定を元に戻す操作ではありません。

保存待ち・保存失敗の入力があるプロファイルは、複製・削除の前にその入力を解決してください。複製は保存済み設定をコピーするための操作です。別プロファイルの未保存入力によって、保存済みプロファイルの複製・削除が止まることはありません。

## U.N. Avatar Exporter を VCC に追加する

VCC の custom repository として追加すると、Unity project の Package Manager から `U.N. Avatar Unity Exporter` を導入できます。

- [公式 Web の Add to VCC](https://usagi.github.io/un-avatar/) を使う
- または VCC の `Settings > Packages > Add Repository` に次の URL を追加する

```text
https://usagi.github.io/un-avatar/vcc/index.json
```

## できること

- VRM / glTF / `.unavatar` ベースのアバターを wgpu renderer (Vulkan / DX12) で描画。
- Supervisor Console で複数のプロファイルと Renderer を管理。
- VRC / Unity アバターを `.unavatar` に変換し、Unity なしの Renderer runtime で利用。
- UNMF/Z と VMC/UDP のモーション入力を受け取り、姿勢、表情、手指、UNPhysics / UNDynamics を反映。
- VRM SpringBone と VRC PhysBone を U.N. Avatar の dynamics として扱う。
- VRM MToon と VRC lilToon を UNToon material として扱う。
- Modular Avatar 由来の衣装・小物・見た目プリセットの切り替えを Wardrobe として扱う。Modular Avatar 非対応の衣装も、GameObject active state を使って Wardrobe set にできます。
- VRC Expression Menu / Animator 由来の操作を UNAnimator runtime action として扱う。
- グローバルなキー / マウス / MIDI 割り当てで Wardrobe / UNAnimator action を実行。
- 透過ウィンドウ、クリック透過、最前面表示、背景色、Spout2 Sender、スクリーンショット保存に対応。

## VRC / Unity アバターの流れ

U.N. Avatar Renderer は Unity Editor や VRChat client を実行時に必要としません。Unity project 上の VRC / Unity アバターを Exporter で `.unavatar` に変換し、Renderer がそれを読み込みます。

```text
Unity project with VRC / Unity avatar
  -> U.N. Avatar Exporter
  -> .unavatar
  -> U.N. Avatar Supervisor / Renderer
  -> Window / Spout2
  -> OBS or streaming software
```

`.unavatar` には avatar mesh、texture、material metadata、lilToon parameters、PhysBone 由来 dynamics、Expression Menu / Animator 由来 action、Wardrobe set などが含まれます。第三者への共有や配布は、必ず元アバター、衣装、テクスチャ等の利用規約に従ってください。

## Wardrobe / Runtime Actions

Wardrobe set が含まれる `.unavatar` は、Renderer 起動後に Renderer tray や Supervisor から衣装・小物・見た目プリセットを切り替えられます。VRC Expression Menu / Animator 由来の操作は、配信用に使う runtime action として UNAnimator にまとめて扱います。

v2 は VRChat client や VRC SDK の完全な runtime 互換実装ではありません。VRC / Unity アバターを U.N. Avatar の独立 Renderer で配信利用しやすくするため、material、dynamics、Wardrobe、action を `.unavatar` と runtime status へ正規化して扱います。

## U.N. Motion と組み合わせる例

[U.N. Motion](https://github.com/usagi/un-motion) は Web カメラや VMC 入力からモーションを作るアプリで、U.N. Avatar はそのモーションを受けてアバターを表示できます。

```text
Web camera / VMC app
  -> U.N. Motion
  -> UNMF/Z or VMC/UDP
  -> U.N. Avatar Renderer
  -> Window / Spout2
  -> OBS or streaming software
```

U.N. Motion なしでも、VMC/UDP を送信できる既存アプリや [UNMF/Z](https://github.com/usagi/un-motion-frame) を扱うアプリから U.N. Avatar を使えます。

## Windowsへの登録を行わずに起動する

ポータブル利用や自動運用で、スタートメニューとジャンプリストを更新したくない場合は、
`un-avatar-supervisor.exe --no-shell-integration` で起動できます。
この起動では自動ショートカット登録・ジャンプリスト更新・SupervisorのWindowsアプリID設定を行わず、
画面からのデスクトップショートカット作成も理由を表示して拒否します。
Windows起動メニューへの追加・ピン変更・一括解除も、保存する前に同じ理由で拒否します。
Windowsの通知トーストも送信せず、アプリ内の通知履歴は残します。
既存のショートカットやジャンプリストは削除しません。通常の起動方法では従来どおり更新します。

このオプションは今回のSupervisorプロセスにだけ適用されます。プロファイルの保存先、通信ポート、
Rendererの起動やGPU利用を分離する機能ではありません。複数の環境を同時に使う場合は、
それらの設定も別途分離してください。

## 対応環境

- Windows 10 / 11
- Supervisor の画面表示には Microsoft Edge WebView2 Runtime
- Windows x64 配布版には Microsoft Visual C++ v14 再頒布可能パッケージ（x64）
- Vulkan または DX12 対応 GPU
- Spout2 出力は Windows 版の標準リリースパッケージで利用可能

ZIPはフォルダー全体を展開し、Supervisor・Renderer・同梱DLLを同じ配置のまま使ってください。
`VCRUNTIME140.dll` などが見つからないと表示された場合は、[Microsoft公式のVisual C++ v14配布ページ](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist)から最新のx64版をインストールまたは修復してください。
WebView2 Runtimeがない場合は、[Microsoft公式のWebView2 Runtime導入案内](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution)を参照してください。これらのランタイムはportable ZIPには含まれません。

配布版の診断ログは `%APPDATA%/UN Avatar/logs` に保存されます。「ログ」画面から保存・保存先の表示ができます。診断レポートは同じ設定フォルダー内の `diagnostics` に保存されます。スクリーンショットの既定保存先は写真フォルダー内の `un-avatar`、写真フォルダーを取得できない場合は `%APPDATA%/UN Avatar/screenshots` です。

## ドキュメント

- [v2 Getting Started](v2-getting-started.md): 初回起動、VRC Exporter、Wardrobe の操作手順
- [Unity Exporter](unity-exporter-v0.1.md): Unity project から `.unavatar` を出力する Exporter の境界
- [Third-party Licenses](third-party-licenses.md): Spout2 などの third-party licenses
- [Documentation Index](README.md): 詳しい仕様、設計メモ、開発者向け情報の索引

## 相対パスで指定したモデルとアイコン

配布版のSupervisorでは、相対パスは設定ファイルの隣にあるファイルを優先し、見つからない場合はSupervisorのEXEがあるフォルダーを参照します。絶対パスは指定先をそのまま使用します。モデル一式を移す際は、モデルが参照するテクスチャなども一緒に移してください。

