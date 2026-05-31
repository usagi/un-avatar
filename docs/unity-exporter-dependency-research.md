# Unity Exporter Dependency Research

作成日: 2026-05-31

この文書は `.unavatar` exporter v0.1 のために、Unity 側で使う GLB writer / bake / variant 抽出の依存候補を調査したメモである。

## 結論

v0.1 prototype は package 内蔵の最小 GLB writer に固定する。UnityGLTF は比較対象として調査したが、v0.1 では使用しない。

- UnityGLTF は MIT license、UPM package、Unity 2021.3 以降を対象にしており、VRC avatar project の Unity 2022.3 系に載せやすい。
- Exporter が `GLTFRoot` / node / material / texture へ callback または export plugin で触れるため、`UN_avatar` root extension と node/material mapping を注入しやすい。
- `SaveGLB` / `SaveGLBToByteArray` / `SaveGLBToStream` があり、`.unavatar` を「GLB 2.0 として保存し、拡張子だけ `.unavatar`」にしやすい。
- ただしユーザーに別 package install を要求しないため、初期 exporter は自前 writer に固定する。

glTFast は fallback / 比較対象にする。GLB export と品質は強いが、調査時点の source package は Unity 6000.0 を要求し、custom extension 追加口が UnityGLTF より内部実装寄りだった。v0.1 の目的は「瑞希 + Modular Avatar 衣装で検証できる exporter」を早く作ることなので、初手の主軸にはしない。

UniVRM / UniGLTF は VRM / MToon / VRMC_materials_mtoon の参照実装として扱う。VRC / Modular Avatar / lilToon 衣装 variant exporter の GLB 基盤としては主軸にしない。

Modular Avatar は exporter 内で再実装しない。MA / NDMF の manual bake API を使って bake 済み avatar を生成し、その結果を GLB として export する。衣装 variant の候補は bake 前の MA MenuItem / ObjectToggle / active state / VRC Expression Menu から読む。

## 候補比較

| 候補 | License | Unity package | Export | Custom extension | 判断 |
| --- | --- | --- | --- | --- | --- |
| UnityGLTF | MIT | `org.khronos.unitygltf`, Unity 2021.3 | GLB / glTF export | callback / plugin で `GLTFRoot` を編集可能 | v0.1 不採用 / 将来再評価 |
| glTFast | Apache-2.0 | `com.atteneder.gltfast`, 調査時点 package は Unity 6000.0 | GLB / glTF export | schema / extension registry は内部寄り | fallback / 比較対象 |
| UniVRM / UniGLTF | MIT | UniGLTF / VRM / VRM10 packages | VRM / glTF export | VRM/MToon 参照には強い | 参照実装扱い |
| Minimal C# GLB writer | 自前 | exporter package 内 | 必要 subset のみ | 完全制御 | v0.1 標準経路 |

## UnityGLTF 調査メモ

Source:

- https://github.com/KhronosGroup/UnityGLTF
- `package.json`
- `Runtime/Scripts/GLTFSceneExporter.cs`
- `Runtime/Scripts/Plugins/Core/GltfExportPlugin.cs`

確認した点。

- `package.json` は `org.khronos.unitygltf`、Unity `2021.3`、説明に import / export と plugin 拡張を含む。
- `LICENSE` は MIT。
- `ExportContext` は `BeforeSceneExport` / `AfterSceneExport` / `AfterNodeExport` / material / texture / mesh callbacks を持つ。
- `GLTFExportPluginContext` は `BeforeSceneExport` / `AfterSceneExport` / `AfterNodeExport` を override できる。
- `SaveGLB` / `SaveGLBToByteArray` / `SaveGLBToStream` がある。

v0.1 で不採用にした理由。

1. ユーザーに追加 package install を要求したくない。
2. Exporter UI に複数 writer の選択肢を出すと、初見ユーザーの判断負荷が増える。
3. v0.1 の検証対象は wardrobe capture と `.unavatar` metadata であり、writer 差し替えは後から再評価できる。
4. 生成 GLB の JSON chunk post-process は built-in writer でも同じ方式で実現できる。

注意点。

- UnityGLTF package の依存は軽くない。Newtonsoft JSON、ShaderGraph、Mathematics、Collections などが入る。
- exporter package の manifest には UnityGLTF を dependency として書かない。UnityGLTF が導入済みでも v0.1 exporter は使わない。
- `UN_avatar` extension は GLB post-process で注入する。将来、node index 対応や material extension が必要になったら UnityGLTF plugin 方式を再評価する。

## glTFast 調査メモ

Source:

- https://github.com/atteneder/glTFast
- `package.json`
- `Documentation~/ExportRuntime.md`
- `Runtime/Scripts/Export/GameObjectExport.cs`
- `Runtime/Scripts/Export/GltfWriter.cs`

確認した点。

- `LICENSE.md` は Apache-2.0。
- `package.json` は調査時点で `com.atteneder.gltfast` version `6.19.0`、Unity `6000.0`。
- `GameObjectExport` は `SaveToFileAndDispose` / `SaveToStreamAndDispose` を提供する。
- docs では `SaveToStreamAndDispose` は self-contained GLB のみと明記されている。
- `GltfWriter` は `RegisterExtensionUsage` と `extensionsUsed` / `extensionsRequired` の生成を持つが、独自 root extension を外部から素直に注入する公開 API は見つけにくい。

判断。

- GLB export 品質と validator 連携は魅力的。
- ただし v0.1 では Unity 2022.3 VRC project での friction と custom `UN_avatar` injection の確認コストが大きい。
- built-in writer の品質が不足する場合に、glTFast 旧 version または Unity Cloud glTFast を再評価する。

## UniVRM / UniGLTF 調査メモ

Source:

- https://github.com/vrm-c/UniVRM
- `LICENSE.txt`
- `README.md`
- `Packages/UniGLTF/package.json`
- `Packages/VRM10/Runtime/IO/Material/...`

確認した点。

- MIT license。
- README は VRM が glTF 2.0 extension であり、UniVRM が VRM 0.x / VRM 1.0 / glTF の import/export を扱うことを明記している。
- VRM10 MToon exporter は `VRMC_materials_mtoon` の実装参考になる。

判断。

- MToon / VRM の正規化や `.vrm` 内包・移行方針の参考にする。
- VRC / MA / lilToon exporter の GLB writer としては、依存範囲と目的がやや違う。

## Modular Avatar / NDMF 調査メモ

Source:

- https://github.com/bdunderscore/modular-avatar
- `package.json`
- `COPYING.md`
- `Editor/AvatarProcessor.cs`
- `Runtime/ModularAvatarMenuItem.cs`
- `Runtime/ReactiveObjects/ModularAvatarObjectToggle.cs`

確認した点。

- Modular Avatar は基本 MIT。ただし Editor/images は公式 package 再配布向けの別注意がある。
- package は Unity `2022.3`、VPM dependencies として NDMF を要求する。
- `nadena.dev.modular_avatar.core.editor.AvatarProcessor.ProcessAvatar(GameObject)` は `ndmf.AvatarProcessor.ProcessAvatar` へ委譲する公開 entrypoint。
- `ModularAvatarMenuItem` は portable control を持ち、Toggle / Button / SubMenu / Puppet 系の menu 情報を読める。
- `ModularAvatarObjectToggle` は対象 object と active state の list を持つ。

v0.1 での使い方。

1. 選択 avatar root を複製する。
2. 複製側に MA / NDMF `ProcessAvatar` を実行する。
3. bake 前 root から variant 候補を抽出する。
4. bake 後 root から最終 mesh / transform / material / PhysBone 近似情報を抽出する。
5. variant operation はまず `nodeVisibility` に限定する。

注意点。

- MA editor assembly は VRC SDK / NDMF / VRC Dynamics 参照を持つ。Exporter は「MA がある時だけ強く連携」し、ない時は current state export に落とせる構成が望ましい。
- MA の full reactive object / animator 解析は v0.1 の非目標。MenuItem + ObjectToggle + active state を優先する。

## v0.1 実装順

1. Unity package skeleton を作る。
2. built-in GLB writer と GLB post-processor を入れる。
3. EditorWindow: avatar root、export path、mode、validate/export button。
4. MA installed detection と bake wrapper。
5. built-in writer で `.glb` bytes を出す。
6. `UN_avatar` root extension に manifest / humanoid / node map / variants を入れる。
7. `.unavatar` と `.unavatar.report.json` を保存する。
8. U.N. Avatar 側 validator / loader と往復する。
