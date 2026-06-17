# U.N. Avatar v2 Getting Started

この文書は、U.N. Avatar v2 を初めて使う人向けの手順です。
README は概要、ここでは実際の操作を少し詳しく説明します。

## VRM を使う場合

1. `un-avatar-supervisor.exe` を起動します。
2. 左側メニューから `プロファイル` を開きます。
3. `+` ボタンで新しいプロファイルを作ります。
4. アバターファイルとして `.vrm` を選びます。
5. モーション入力、出力、ウィンドウ、カメラなどを必要に応じて設定します。
6. `起動` ボタンで Renderer を起動します。
7. U.N. Motion や VMC 対応アプリから UNMF/Z または VMC/UDP でモーションを送ります。

## VRC / Unity アバターを使う場合

VRC 向け Unity アバターは、そのまま Renderer に読み込ませるのではなく、Unity Editor 上の U.N. Avatar Exporter で `.unavatar` にエクスポートして使います。以降、この文書では VRC 向け Unity アバターを `VRC / Unity アバター` と呼びます。

```text
Unity project with VRC / Unity avatar
  -> U.N. Avatar Exporter
  -> .unavatar
  -> U.N. Avatar Supervisor / Renderer
  -> Window / Spout2
  -> OBS or streaming software
```

U.N. Avatar Renderer は Unity Editor や VRChat client を実行時に必要としません。

## U.N. Avatar Exporter の導入

推奨手順:

- VRChat Creator Companion (VCC) のパッケージマネージャーから U.N. Avatar Exporter を探してインストールします。

他の方法:

- Unity Editor の `Window > Package Manager` で `Add package from git URL` を選び、`https://github.com/usagi/un-avatar.git?path=/unity/un-avatar-unity-exporter` を入力してインストールします。
- Unity Editor の `Window > Package Manager` で `Add package from disk` を選び、U.N. Avatar 配布パッケージの `unity/un-avatar-unity-exporter/package.json` を指定してインストールします。

## `.unavatar` を出力する

`.unavatar` は、VRC / Unity アバターを U.N. Avatar で使うための配信用アバターパッケージです。

1. Unity Editor で `Tools > U.N. Avatar > Exporter .unavatar` を開きます。
2. `Avatar Root` に Hierarchy からアバターのルート GameObject を指定します。
3. `Output` に `.unavatar` を出力するパスを設定します。
4. 操作パネルの `1. Base -> 2. Wardrobe Sets -> 3. Export` の順に操作して `.unavatar` を出力します。

Wardrobe を使わない場合は、Export Mode を `Current to Base Only` にして `3. Export` へ進めます。
Wardrobe を使う場合は、Export Mode を `Wardrobe` にして、`1. Base` と `2. Wardrobe Sets` で衣装や小物などの状態を保存してから `3. Export` で出力します。

## Wardrobe の考え方

Wardrobe は、衣装・小物・見た目プリセットの状態を `.unavatar` に保存し、Renderer 起動後に切り替えるための機能です。

`Base` / `Sets` は、Unity の Hierarchy で GameObject の active state や blendshape などを調整した状態を保存します。Renderer では保存された状態を衣装セットとして切り替えられます。

### Base

`Base` は、お着替え元にする基本状態です。

1. 素体や基本衣装など、最初の状態にしたい GameObject を有効にします。
2. 不要な衣装や小物は Inspector の GameObject active toggle で無効にします。
3. `Capture Current As Base` で状態を保存します。

変更したくなったら、同じボタンで再度保存できます。

### Sets

`Sets` は、お着替えバリエーションです。

1. 衣装や小物の GameObject active state を切り替えます。
2. 必要に応じて素体側の衣装や blendshape を調整します。
3. `Capture Current As Set` で状態を保存します。

保存済み set は、必要に応じて `Update`、`Duplicate`、`Remove` できます。Set は複数作れます。

Base / Sets の名称ボタン部分をクリックすると、Unity scene のアバター状態を保存済み状態へ切り替えられます。

## Modular Avatar 対応衣装の例

素体に Modular Avatar 対応衣装をぶら下げた Hierarchy では、次のような流れで設定できます。

1. 素体のみ有効、追加衣装は無効の状態で Base を保存します。
2. 追加衣装を有効にします。
3. 素体側の不要な衣装を無効にします。
4. 必要に応じて blendshape で素体の状態を調整します。
5. その状態を Set として保存します。
6. 別の衣装や小物の状態を作り、さらに Set として保存します。

Modular Avatar 非対応の衣装でも、GameObject active state を使って同じように Wardrobe set として扱えます。

## `.unavatar` の扱い

`.unavatar` には、avatar mesh、texture、material metadata、lilToon parameters、PhysBone 由来 dynamics、Expression Menu / Animator 由来 action、Wardrobe set などが含まれます。

第三者への共有や配布は、必ず元アバター、衣装、テクスチャ等の利用規約に従ってください。U.N. Avatar は、ユーザー自身が正規に利用できるアセットをローカル環境で配信等に利用することを主目的とします。
