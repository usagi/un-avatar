# UN Avatar Runtime MVP

この文書は、現在の実装で先に厚くなっている **VRM / VMC / MToon / wgpu / Spout2** の垂直スライスを、当面の製品中核として固定するための正本である。

この文書は、v1 の安定対象になった **VRM / VMC / MToon / wgpu / Spout2** runtime の境界をまとめる。

## 1. MVPの目的

最初の安定対象は、外部ゲームエンジンに依存しないVRMアバター表示ランタイムである。

MVPで保証したいこと。

- VRM0 / VRM1 を読み込み、Humanoid・表情・MToon・SpringBone情報を保持できる
- SpringBone の体幹・頭部・腕・手への明らかなめり込みは、[`bone-based-colliders-v1.md`](bone-based-colliders-v1.md) のボーンベースコライダー方針で軽減できる
- VMC Marionette入力を受け、UNMotionFrame経由でHumanoid姿勢と表情を適用できる
- MToonをSimple/Unlit代替ではなく、MToonパラメータ駆動の専用シェーダで描画する
- UN Avatar内部表示座標系を **右手座標 / Y-up / +Z-front** に固定する
- WindowsではSpout2へ送出できる
- 診断CLIとdebug flagsで、モデル・材質・VMC・モーフ・スキニングを切り分けられる

## 2. MVPで扱わないもの

以下は設計上の前提を崩さない範囲で後回しにする。

- FBX / USD / `.blend` / VRC prefab bridgeの本格実装
- 動画録画・FFmpeg出力
- 複数renderer instanceの完成版GUI
- 高度なmaterial editor
- GPU morph / GPU physicsへの全面移行
- Spout2 GPU texture共有の最終実装
- NDI出力の標準対応。SDK・runtime・商標表示・再配布条件がSpout2より重いため、現時点では積極対応せず将来調査枠に置く
- Zenoh / NNG等の本格IPC transport

ただし、これらを後で足せるように、rendererはGUIから独立し、IOはcoreから分離し、Spout2やbridgeはfeature/package境界に置く。

## 3. 座標系ポリシー

UN Avatarランタイム内部は、右手座標 / Y-up / +Z-front に固定する。

モデルロード時に行うこと。

- VRM0はロード時にrootをY軸180度回転して +Z-front 表示へ正規化する
- VRM1はglTF/VRM1側の向きを前提に、追加回転を最小化する
- カメラ初期位置は +Z 側から原点付近を見る

VMC入力適用時に行うこと。

- VMC入力はUNMotionFrameの `CoordinateSpace::Vmc` として扱う
- VRM0 / VRM1でHumanoid基底が異なるため、translation / quaternion変換をtarget VRM flavorごとに分ける
- Humanoid bone rotationはrest poseを消さず、rest local rotationに入力rotationを掛ける
- VRM1 node constraintはHumanoid適用後にrest pose基準で評価する

## 4. MToonポリシー

VRMは既定でMToonLikeとして描画する。

MToonで保持・反映するもの。

- shade color / shade texture
- shading shift / toony / GI equalization
- matcap
- rim lighting
- emissive
- outline width / outline color / outline lighting mix
- alphaMode: Opaque / Mask / Blend
- VRM0 `materialProperties` と VRM1 `VRMC_materials_mtoon`

注意点。

- VRM0 `_OutlineWidth` はメートル換算として `0.01` を掛ける
- MToon MASK discardはRGBではなくalphaのみで判定する
- 目・虹彩・ハイライトらしい材質名は、モデル差異によりMASKで消えやすいため緩和対象にする
- `KHR_materials_unlit` のまま残ったVRM材質も、VRM文脈ではMToonLikeへ寄せる

## 5. Spout2ポリシー

Windows配布ではSpout2を標準機能として扱う。

- `cargo xtask spout2` でSpout2を取得・CMake Releaseビルドする
- `cargo xtask package` は既定で `cargo xtask spout2` を実行し、Spout2込みの配布レイアウトを作る
- `cargo xtask release-package` は `target/package/un-avatar` を `release-packages/un-avatar-<version>.zip` に固める
- 配布物には package root の `Spout.dll` と `LICENSES/Spout2-BSD-2-Clause.txt` を含める
- renderer起動前にSupervisorが package root を `PATH` へ追加する
- 開発時に手動で `spout-sdk` featureを使う場合は `SPOUT2_SDK_DIR` / `SPOUT2_LIB_DIR` / `PATH` を明示する

現状の送出はCPU readback + `send_image_rgba` を実用候補のfallbackとし、readback は 2-slot ring buffer と非同期 `map_async` でフレームループをブロックしない。低遅延化の次段階は、readback ring 長 / drop policy の実測調整、または Spout2 GPU texture 送信への移行である。

Supervisor runtime status / diagnostics では、Spout2の送信試行数・成功数・失敗数・連続失敗数・readback/send/total時間・sender初期化状態・sender実解像度を観測できるようにする。送信連続失敗、sender未初期化、要求解像度と実sender解像度の不一致はruntime noteとして表に出す。OBS側で受信できない、またはPremultiplied Alpha設定が合わない場合でも、まずrenderer側が送信を継続できているかを切り分けられる状態をMVP完了条件に含める。

## 6. レンダラー構造ポリシー

当面のrenderer crateは、動くMVPを保ちながら次の境界へ寄せる。

- `options`: CLI/manifestから生成される起動設定
- `model_loader`: VRM/glTF判定とImportContext生成
- `camera`: +Z-front基準のorbit camera
- `gpu`: wgpu device/surface/pipeline/render orchestration
- `mesh_pass`: mesh/material/morph/skinning draw resources
- `spout`: Spout2出力

将来のSupervisor化では、`model_loader` はrenderer起動manifestの処理へ移り、GUIはrenderer内部のwinit処理に依存しない。

## 7. 直近の実装優先度

1. MVP正本と実装境界を一致させる
2. renderer入口を `options` / `model_loader` / `camera` に分ける
3. MToon / coordinate / VMC のrender smokeまたはgolden testを作る
4. Spout2のOBS実機確認と低遅延化方針を決める
5. GPU morph / skinningへ進む

## 8. MVP完了条件

MVP完了は、機能追加量ではなく以下の受け入れ条件で判定する。

リリース前の確認は [`development-guidelines.md`](development-guidelines.md) の v1 リリース前チェックを使う。

- `model1.vrm` / `model2.vrm` / `vrm1.vrm` が +Z-front の正面向きで表示される
- VRM0 / VRM1 の腕、前腕、手首がVMCポージング後も消失・左右反転・Z反転しない
- 瞳、虹彩、ハイライトがMToon描画で消えない
- VRM材質がSimple/Unlit fallbackではなくMToonLike専用パイプラインで描画される
- VRM0 outline widthが過大なリングや全身アーティファクトを出さない
- VMC Marionette入力でHumanoid poseと表情weightが更新される
- ボーンベースコライダーが未設定 VRM でも自動生成され、SpringBone の明らかな体めり込みが軽減される
- `cargo test -p un-avatar-render-wgpu` が通る
- `cargo check -p un-avatar-render-wgpu --features spout-sdk` が通る
- `cargo xtask package` がWindowsでSpout2込みの最小配布レイアウトを生成する
- `cargo xtask release-package` がzip形式のリリース成果物を生成する
- 配布物に `Spout.dll`、`LICENSES/Spout2-BSD-2-Clause.txt`、`LICENSES/spout2-build-info.txt` が含まれる

## 9. Renderer Manifest MVP

SupervisorのAvatar SettingsからReveal Fileで直接編集する前提に合わせ、renderer起動manifestはTOMLを正本とする。

最小フィールド。

```toml
title = "UN Avatar"
avatar_path = "target/tmp/model1.vrm"
icon_path = "assets/brand/un-avatar-artwork-renderer.png"
vmc_address = "0.0.0.0:39539"
transparent = true
input_passthrough = false
aa = "off"
clear_color = [0.0, 0.0, 0.0, 0.0]

[render_quality]
aa = "off"
texture_resolution_limit = "off"
texture_compression = "source"
processed_texture_cache = true

[spout]
enabled = true
name = "UN Avatar Spout"
width = 1280
height = 720

[debug]
vmc = true
scene = true
morph = true
```

CLIは当面 `--manifest path` で `.toml` を読み、明示されたCLIオプションで上書きできる。Tauri Supervisorはこのmanifestを生成してrenderer子プロセスへ渡す。
