# U.N. Avatar v2 Getting Started

この文書は、U.N. Avatar v2 を初めて使う人向けの手順です。
概要は `README.md`、ここでは実際の操作を説明します。

## まず知っておくこと

WindowsのSupervisor設定画面には **Microsoft Edge WebView2 Runtime** が必要です。未導入の環境では、[Microsoft公式ページ](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section)からEvergreen Runtimeを導入して再起動してください。Edgeブラウザー本体とは別の実行環境です。これはSupervisorの画面表示の要件で、Rendererの描画要件とは区別します。

U.N. Avatar は、アバターを単体 Renderer で表示し、Window Preview や Spout2 で配信ソフトへ出力します。

- VRM は、そのまま Supervisor のプロファイルへ登録して使います。
- VRC / Unity アバターは、Unity Editor の U.N. Avatar Exporter で `.unavatar` にしてから使います。
- `.unavatar` は、VRC / Unity アバターを U.N. Avatar で使うための配信用アバターパッケージです。
- Wardrobe は、衣装・小物・見た目プリセットを `.unavatar` に保存し、Renderer 起動後に切り替える機能です。

```text
VRC / Unity avatar in Unity
  -> U.N. Avatar Exporter
  -> .unavatar
  -> U.N. Avatar Supervisor / Renderer
  -> Window / Spout2
  -> OBS or streaming software
```

U.N. Avatar Renderer は、実行時に Unity Editor や VRChat client を必要としません。

## 操作する対象を確認する

既存の環境を更新する場合は、先に[更新・バックアップ・復元](update-and-recovery.md)で設定と配布物を退避します。

プロファイルは保存する設定のまとまり、Rendererは実際にアバターを表示している個体です。同じプロファイルから複数のRendererを起動できます。

- 設定を変更するときは「設定の編集対象」を確認します。「識別情報・保存先」を開くとIDと保存先を確認できます。同名のプロファイルは名前だけで判断しないでください。
- 「プロファイルと稼働」で対象を選び、稼働欄の「このプロファイルを起動」を押すと、その保存設定でRendererを起動します。個体ごとの停止も同じ稼働欄から行えます。「全稼働個体」は、別プロファイルを含めて起動済みのRendererをまとめて操作する画面です。
- 表示・撮影などの操作対象にRendererを選んでも、設定の保存先は切り替わりません。
- 設定の反映範囲は項目により異なります。保存状態に加え、稼働個体への反映結果や「次回起動」の表示を確認してください。同じプロファイルを使う複数個体へ反映される設定もあります。
- 設定の場所が分からないときは「機能マップ」で機能名を探し、設定を開く対象のプロファイルを選びます。ここで対象を選ぶだけでは稼働中の個体は切り替わりません。

### 同じ名前のプロファイルを同時に使うとき

たとえば、設定Aを編集したまま、設定Bから起動したRendererを停止することもできます。名前だけでなく、次の表示を確認してください。

| 行いたいこと | 確認する表示 |
| --- | --- |
| 保存する設定を編集する | 左側の「編集する保存設定」とプロファイルID |
| 選択したプロファイルでRendererを起動する | 「プロファイルと稼働」の選択対象と、隣の稼働欄の名前 |
| 「全稼働個体」からRendererを起動する | 上部の起動対象選択にある名前と `ID:` |
| 動いているRendererを操作する | 稼働一覧のプロファイルIDとPID、および選択後の操作対象の個体番号 |

同じプロファイルIDで複数のRendererが動いている場合は、PIDと個体番号で区別します。個体の停止と、共有しているプロファイル設定の編集は反映範囲が異なります。カメラなどの設定編集は同じプロファイルの複数個体へ反映されるため、1個体だけを選択したことを理由に、その個体だけの設定変更だとは判断しないでください。

以下の掲載画像には旧配置の画面も含まれます。現行版で最初に起動するときは「プロファイルと稼働」を開き、対象を選んで「このプロファイルを起動」を押してください。

## 髪・服・体の揺れを調整する

「機能マップ」で「揺れ」「布の拘束」「追従補助」「衝突相手」を検索し、対象プロファイルの「髪・服・体の揺れ」を開きます。既存の全体ON/OFF、カテゴリ・部位・グループ別の調整に加え、その下の3つの見出しを展開して詳細を編集できます。必要な欄だけを開いて基本設定と見比べられます。

| 詳細欄 | 調整すること | 保存後の反映 |
| --- | --- | --- |
| 布の形を保つ拘束 | 接続辺や近接する点の距離、拘束の強さ、最小の辺数 | 同じプロファイルの稼働Rendererへ物理設定を送ります。反映失敗が表示された場合は、その個体への再適用を確認します。 |
| 布メッシュの追従補助 | 身体の動きに偏る布のウェイト、補助の上限、対象メッシュの語句 | モデル読込時に処理するため、Rendererの再起動が必要です。同じプロファイルで複数個体を動かしている場合は、各個体を再起動して揃えます。 |
| 衝突相手を追加するルール | 揺れる部位と衝突相手を、名前に含まれる語句で指定 | 同じプロファイルの稼働Rendererへ物理設定を送ります。 |

数値は欄に記載された単位で入力します。全体の揺れをOFFにしても詳細の値は保持されます。追従補助の対象メッシュ欄を空にすると、布カテゴリの語句を使います。

布の拘束は初期状態では無効です。「布の拘束を使う」をONにしたときだけ適用されます。対象となる布の骨やメッシュのつながりがないモデルでは、ONにしても拘束は生成されません。追従補助もモデルのウェイトに応じて働くため、設定をONにしたことだけで見た目が変わるとは限りません。

個別グループや条件指定の「モデル由来の抵抗倍率」は、モデルが持つ抵抗へ掛ける倍率です。1は元の強さ、0はその抵抗を無効にします。「減衰時間」とは別の設定で、0にしてもすべての減衰がなくなるわけではありません。未設定の欄は上位設定を引き継ぎます。入力欄を消して確定すると0になるため、未設定へ戻す場合はその上書きルールを削除してください。ルール内のほかの値も一緒に削除されるので、残したい調整がある場合は削除前に確認します。


衝突ルールを追加すると、最初は未完成の入力になります。「揺れる部位」と「衝突相手」の両方に語句を入力してください。未完成のルールは保存されず、保存できていない変更として表示されます。別プロファイルへ移って戻っても入力は保持されるため、続きを入力するか、そのルールを削除できます。各語句は1行に1つ入力します。

物理詳細の数値欄は、確定時に許容範囲へ補正し、補正後の値を表示します。空欄も範囲の下限に補正されるため、変更したくない場合は元の値を入力してください。

設定は欄の変更確定時に保存されます。編集対象の名前・IDと、保存失敗や再起動待ちの表示を確認してください。保存済みであることと、表示中の全個体へ反映されたことは別です。

## VRM を使う

1. `un-avatar-supervisor.exe` を起動します。
2. 左側メニューから `プロファイル` を開きます。
3. `+` ボタンで新しいプロファイルを作ります。
4. アバターファイルとして `.vrm` を選びます。
5. モーション入力、出力、ウィンドウ、カメラなどを必要に応じて設定します。
6. `起動` ボタンで Renderer を起動します。
7. U.N. Motion や VMC 対応アプリから UNMF/Z または VMC/UDP でモーションを送ります。

## VRC / Unity アバターを使う

VRC / Unity アバターは、先に Unity Editor で `.unavatar` を作ります。

### 1. Exporter を導入する

推奨手順:

- [Add to VCC](vcc://vpm/addRepo?url=https%3A%2F%2Fusagi.github.io%2Fun-avatar%2Fvcc%2Findex.json) を開いて repository を追加します。

リンクで開けない場合は、VRChat Creator Companion (VCC) の `Settings > Packages > Add Repository` に次の URL を追加します。

```text
https://usagi.github.io/un-avatar/vcc/index.json
```

- 追加後、対象 project の Package Manager で `U.N. Avatar Unity Exporter` を探してインストールします。

他の方法:

- Unity Editor の `Window > Package Manager` で `Add package from git URL` を選び、`https://github.com/usagi/un-avatar.git?path=/unity/un-avatar-unity-exporter` を入力してインストールします。
- Unity Editor の `Window > Package Manager` で `Add package from disk` を選び、U.N. Avatar 配布パッケージの `unity/un-avatar-unity-exporter/package.json` を指定してインストールします。

### 2. `.unavatar` を出力する

1. Unity Editor で `Tools > U.N. Avatar > Export .unavatar` を開きます。
2. `Avatar Root` に Hierarchy からアバターのルート GameObject を指定します。
3. `Output` に `.unavatar` を出力するパスを設定します。
4. 操作パネルの `1. Base -> 2. Wardrobe Sets -> 3. Export` の順に操作します。

![Unity Editor の U.N. Avatar Exporter 画面。1 は Tools メニュー、2 は Avatar Root と Output、3 は Base / Wardrobe Sets / Export の操作パネル。](assets/v2-getting-started/exporter-workflow.png)

> 図1:
>
> - 1: `Tools > U.N. Avatar > Export .unavatar` から Exporter を開きます。
> - 2: `Avatar Root` と `Output` を指定します。
> - 3: `Base`、`Wardrobe Sets`、`Export` の順に操作します。

Wardrobe を使わない場合は、Export Mode を `Current to Base Only` にして `3. Export` へ進みます。

Wardrobe を使う場合は、Export Mode を `Wardrobe` にして、`1. Base` と `2. Wardrobe Sets` で衣装や小物などの状態を保存してから `3. Export` で出力します。

### 3. Supervisor で起動する

1. `un-avatar-supervisor.exe` を起動します。
2. 左側メニューから `プロファイル` を開きます。
3. `+` ボタンで新しいプロファイルを作ります。
4. アバターファイルとして出力した `.unavatar` を選びます。
5. 必要に応じて出力モード、カメラ、モーション入力を設定します。
6. `起動` ボタンで Renderer を起動します。

![Supervisor の .unavatar 確認画面。1 はプロファイルアイコンに使う切り抜き範囲、2 はズームと位置調整、3 は Wardrobe set 選択、4 は確認して読み込むボタン。](assets/v2-getting-started/supervisor-unavatar-confirm.png)

> 図2:
>
> - 1: プロファイルアイコンに使う切り抜き範囲です。
> - 2: ズームと位置を調整します。
> - 3: プレビューする Wardrobe set を選びます。
> - 4: 権利確認を読んだうえで `.unavatar` を読み込みます。

## Wardrobe を使う

Wardrobe を使うと、Renderer を起動したまま衣装や小物の状態を切り替えられます。

### Base を保存する

`Base` は、お着替え元にする基本状態です。

1. 素体や基本衣装など、最初の状態にしたい GameObject を有効にします。
2. 不要な衣装や小物は Inspector の GameObject active toggle で無効にします。
3. `Capture Current As Base` で状態を保存します。

変更したくなったら、同じボタンで再度保存できます。

### Wardrobe Sets を保存する

`Sets` は、お着替えバリエーションです。

1. 衣装や小物の GameObject active state を切り替えます。
2. 必要に応じて素体側の衣装や blendshape を調整します。
3. `Capture Current As Set` で状態を保存します。

![Exporter の Wardrobe 操作。1 は Base 保存、2 は Wardrobe Sets の保存と管理、3 は .unavatar export。](assets/v2-getting-started/exporter-wardrobe-sets.png)

> 図3:
>
> - 1: 現在の Unity scene 状態を `Base` として保存します。
> - 2: 衣装や小物の状態を `Wardrobe Sets` として保存・更新します。
> - 3: 保存した Base / Sets を含む `.unavatar` を出力します。

保存済み set は、必要に応じて `Update`、`Duplicate`、`Remove` できます。Set は複数作れます。

Base / Sets の名称ボタン部分をクリックすると、Unity scene のアバター状態を保存済み状態へ切り替えられます。

### Modular Avatar 対応衣装の例

素体に Modular Avatar 対応衣装をぶら下げた Hierarchy では、次のような流れで設定できます。

1. 素体のみ有効、追加衣装は無効の状態で Base を保存します。
2. 追加衣装を有効にします。
3. 素体側の不要な衣装を無効にします。
4. 必要に応じて blendshape で素体の状態を調整します。
5. その状態を Set として保存します。
6. 別の衣装や小物の状態を作り、さらに Set として保存します。

Modular Avatar 非対応の衣装でも、GameObject active state を使って同じように Wardrobe set として扱えます。

Renderer 起動後は、Windows tray の Renderer メニューから Wardrobe を切り替えられます。`現在の衣装をプロファイルへ保存` を使うと、次回起動時の既定 Wardrobe set として保存できます。

![Renderer tray の Wardrobe メニュー。1 は Wardrobe メニュー、2 は切り替え候補、3 は現在の衣装をプロファイルへ保存。](assets/v2-getting-started/tray-wardrobe-menu.png)

> 図4:
>
> - 1: Renderer tray から `ワードローブ` メニューを開きます。
> - 2: 配信中に切り替える Wardrobe set を選びます。
> - 3: 現在の衣装を次回起動時の既定値としてプロファイルへ保存します。


### キーボードやMIDIで衣装・アクションを切り替える

Supervisorの「プロファイル」で編集対象を選び、「アバター」を開きます。衣装は「ワードローブ」の各行、アクションはUNAnimatorの各アクションの割当欄を使います。モデルによって使える衣装・アクションは異なります。

- キーボードは「キー割り当て」で指定します。
- MIDIはデバイス名、チャンネル（1〜16）、ノート番号（0〜127）を指定します。キャプチャボタンを押してから鍵盤やパッドを押す方法もあります。
- ノート番号が未入力の間、先に入力したデバイス名やチャンネルは編集中の値として保持されます。この段階では割当は保存されていません。アプリを終了する前にノート番号まで入力し、保存結果を確認してください。
- 範囲外の番号や空欄は、既存割当を削除する操作にはなりません。解除する場合は、その行の「MIDI 割当を解除」を使います。キーボード割当や別の衣装・アクションのMIDI割当は残ります。

保存した割当は対象プロファイルの設定です。別プロファイルを使うRendererの設定にはなりません。実機で試す際は、操作対象とMIDIの送信チャンネル・ノート番号を確認してください。


## モーション入力

Renderer 起動後、U.N. Motion から UNMF/Z を送ると表情、姿勢、手足などを動かせます。

### 別PCのU.N. Motionへ接続する

同じPCでは、profileのMotionにあるUNMF/Z Subscriberを有効にし、受信IDを
U.N. Motion側と同じ値（通常は`un-motion/frame`）にします。`LAN接続`はOFFのままで構いません。
既存どおりZenohの自動探索で接続します。

LAN内の別PCでは、U.N. Motion側で「LAN内の別PCから直接接続を許可」を有効にし、
U.N. Avatar側の対象受信IDで`LAN接続`をONにします。U.N. Motionに表示されたIPv4アドレスと
port（既定`47447`）をそれぞれ入力します。LAN接続を追加しても同一PCの自動探索は維持されます。

複数のU.N. Motionから同じ受信IDを受け取る場合は、受信IDを追加し、同じIDへそれぞれの
IPv4アドレスとportを設定します。Rendererは一つのZenoh sessionで全接続先へ接続し、
同じIDへPublishされたUNMotionFrameを一つの購読として受信します。

接続できない場合は、U.N. Motionを実行しているPCのFirewallで表示されたTCP portへの受信を許可します。
インターネットへのport転送は不要です。

U.N. Motion なしでも、VMC/UDP を送信できる既存アプリを使えます。用途に合わせて Supervisor のプロファイルで入力方式を設定してください。

## 日常運用

Supervisor は、プロファイルの作成、設定、確認、ショートカット作成を行う管理画面です。Renderer は、配信時にアバターを表示して Window Preview や Spout2 へ出力する単体プロセスです。

設定が固まったプロファイルは、Supervisor を毎回開かずにショートカットやピン留めから直接 Renderer として起動できます。配信前の確認や設定変更が必要なときは、Supervisor に戻ってプロファイルを編集します。

Renderer 単独起動中の操作は、Windows タスクトレイの Renderer アイコンを右クリックして行います。Tray メニューから Wardrobe、出力モード、ウィンドウ、カメラ、Supervisor を開く、Renderer の終了などを操作できます。

## OBS / Spout2 出力

Spout2 を使う場合は、Renderer の出力モードを `Spout2 + Preview` または `Spout2 Only (HIDE)` にします。OBS 側では Spout2 Capture source を追加して、U.N. Avatar の Spout2 sender を選びます。背景を透過したい場合は、Spout2 Capture の `Composite mode` を `Premultiplied Alpha` にします。

![OBS の Spout2 Capture 例。1 は配信画面へ入った U.N. Avatar 出力、2 は Spout2 Capture source、3 は source 一覧。](assets/v2-getting-started/obs-spout2-capture.png)

> 図5:
>
> - 1: OBS preview に入った U.N. Avatar の Spout2 出力です。
> - 2: `Spout2 Capture` source を選択しています。
> - 3: source 一覧で Spout2 Capture を管理します。

<video src="assets/v2-getting-started/wardrobe-switch-demo.webm" controls muted playsinline width="960"></video>

動画が表示されない場合は、[wardrobe-switch-demo.webm](assets/v2-getting-started/wardrobe-switch-demo.webm) または [wardrobe-switch-demo.mp4](assets/v2-getting-started/wardrobe-switch-demo.mp4) を直接開いてください。

## 困ったとき

- Renderer が起動しない場合は、Supervisor のプロファイルでアバターファイルと出力設定を確認します。
- VRC / Unity アバターが読み込めない場合は、Unity Editor で `.unavatar` を出力し直します。
- Wardrobe が期待通りに切り替わらない場合は、Unity Editor で `Base` と `Wardrobe Sets` を保存し直してから再度エクスポートします。
- Spout2 が見えない場合は、Renderer の出力モードが `Spout2 + Preview` または `Spout2 Only (HIDE)` になっているか確認します。

## `.unavatar` の扱い

`.unavatar` には、avatar mesh、texture、material metadata、lilToon parameters、PhysBone 由来 dynamics、Expression Menu / Animator 由来 action、Wardrobe set などが含まれます。

第三者への共有や配布は、必ず元アバター、衣装、テクスチャ等の利用規約に従ってください。U.N. Avatar は、ユーザー自身が正規に利用できるアセットをローカル環境で配信等に利用することを主目的とします。

## 図・動画で使用した素材

この文書の図と動画には、U.N. Avatar の操作例として次の素材を使用しています。各素材の権利はそれぞれの権利者に帰属します。

- アバターモデル: [オリジナル3Dモデル「瑞希」メニューギミック搭載](https://booth.pm/ja/items/5132797) / IKUSIA
- 衣装: [Noble Trace - Classic](https://booth.pm/ja/items/6786314) / VELLIE
- 衣装: [Field Drape](https://booth.pm/ja/items/8362173) / CYCR (Cyber Critter)
- ゲーム画面: [魔王城ものがたり](https://store.steampowered.com/app/4212210/Demon_Castle_Story/?l=japanese) / (C) KAIROSOFT CO.,LTD. All Rights Reserved.
