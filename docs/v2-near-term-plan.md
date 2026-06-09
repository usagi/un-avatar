# UNAvatar v2 近々の仮計画

この文書は、lilToon-like AudioLink 初期対応後の短期作業順を固定する。

## 現在位置

- AudioLink は v2 初期範囲として十分に完了した扱いにする。
- lilToon-like rendering は互換性優先を維持する。今後の見た目調整は、lilToon 本家実装または具体的な観測差分を根拠にする。
- lilToon 互換が成立したので、MToon / lilToon を別 renderer として並べるのではなく、UNToon semantic material と dynamic variant planning へ整理する。正本は [`untoon-dynamic-variant-architecture.md`](untoon-dynamic-variant-architecture.md)。
- 次の大きな価値は VRC model import / runtime behavior。具体的には wardrobe 高速切替、expression、animation-driven toggle、後続の SpringBone / PhysBone runtime dynamics。
- これらを足す前に、runtime state が読みにくくならない程度のリファクタリングと最適化を行う。

## 近々の順序

1. 現状の v2 renderer / runtime 実装をほどほどにリファクタリングし、最適化する。
2. VRC import base の `.unavatar` skinning / morph を既存 GPU skinning / morph pipeline に接続・検証し、UNToon dynamic variant planning の resource reservation に接続する。
3. VRM SpringBone / VRC PhysBone source を U.N. dynamics runtime model へ lower する正規化境界を設計し、runtime model view に接続する。
4. renderer 再起動なしの Wardrobe hot switch を実装する。
5. VRC Expression Menu、toggle、hotkey、将来の ring menu emulation 向け runtime action model を作る。
6. action model の上に imported animation / expression / material / visibility evaluation を足す。
7. wardrobe と animation state の所有関係が明確になってから PhysBone behavior implementation を進める。
8. instant switching が正しく安定してから、お着替え transition effect を足す。

## リファクタリング / 最適化範囲

この段階では中程度に留める。美観だけを理由に、動いている subsystem を大きく作り直さない。

現在の進捗:

- `UnaRuntimeModel` / `UnaRuntimeModelMut` は scene、humanoid、expression、runtime dynamics を読む境界として導入済み。
- renderer、skeleton retarget、CLI diagnose は、frame loop / solver / diagnostics で source-format field を直接読む箇所を減らし、runtime accessor 経由へ寄せている。
- `HumanoidRetargetContext` は `UnaRuntimeRetargetInputs` から構築できるようになり、renderer の retarget runtime は document source field ではなく runtime model view から compile する。
- `UnaRuntimeDynamics` / `UnaRuntimeDynamicsMut` は SpringBone / PhysBone source settings を直接渡す逃げ道を閉じ、groups / colliders / counts / dynamic node iterator / source id enable mutation の view として solver / renderer / wardrobe importer に渡す。Dynamics enable mutation は source group の authored default ではなく `UnaRuntimeState.dynamics_enabled_overrides` へ書く。
- まだ `UnaDocument` 自体は source data と runtime state を同居させる transitional container であり、Wardrobe hot switch 前に resolved wardrobe state / active asset groups / action state / runtime parameter values の所有境界をさらに分ける。
- scene node は source node id と runtime resolved node id を別フィールドとして保持し、runtime node target は source id 優先のまま resolved id / path / index fallback へ解決できる。MA Replace Object のような resolver 派生 node は source id を authored target として残し、resolved id を cache / diagnostics 用に付与する。
- `.unavatar` / glTF / GLB import は、root `UN_avatar` extension に Modular Avatar payload がある場合は resolver を正本にし、payload がない別アーマチュア衣装は Humanoid 同名骨 fallback で retarget する。同名 Humanoid 接続点にぶら下がる non-Humanoid 補助骨 subtree は world pose を保って主 armature へ reparent する。ただし fallback は constraints、PhysBone behavior、blendshape / material side effects、曖昧な重複骨名の完全解決までは復元しない。
- 2026-06-09 時点の目視確認では、これまで見つかっていた `mizuki-split.unavatar` の visual regression は期待動作まで解決済み。`usagi.unavatar` は Perfect Sync 対応 sample として表情 / blendshape と sparse MA payload export の検証対象にする。

優先領域:

- immutable source package data と runtime state を分ける。
  - `.unavatar` / glTF source data
  - resolved wardrobe state
  - pose、morph、material、expression、action state、dynamics state
  - GPU resources / cache
- wardrobe visibility と morph change を renderer control、VRC menu action、shortcut、将来の animation evaluation から再利用できる形にする。
- render thread の work は bounded / nonblocking に保つ。AudioLink で固定した方針を skinning、animation、physics にも適用する。
- 生成 fallback resources、bind groups、optional material textures 周辺の brittle な indexing assumption は、実害が見える箇所から減らす。
- refactor 中も lilToon compatibility behavior を維持する。既知の mismatch 修正に必要でない semantic rewrite は避ける。
- 広い snapshot churn より、state resolution、resource indexing、command application の focused test を優先する。

## Runtime Dynamics Normalization

SpringBone / PhysBone は source format ごとの physics component ではなく、U.N. Avatar の runtime dynamics model へ正規化してから solver / renderer へ渡す。

初期方針:

- VRM SpringBone と VRC PhysBone は source metadata を保持しつつ、実行時には共通の U.N. dynamics group / chain / collider / parameter view へ lower する。
- v1 で実装済みの SpringBone solver / collider 実装は利用してよい。ただし入力は VRM SpringBone 生データではなく、正規化済み runtime dynamics state とする。
- VRC PhysBone は v2 初期では完全再現を狙わず、既存 SpringBone-like runtime primitives へ近似変換する。
- 正規化境界は現在進めている runtime model view の一部として扱う。形式別の VRM / VRC / Unity component 判定を frame loop や solver 内へ散らさない。
- solver state は source scene を直接 mutate せず、resolved runtime state と pose buffer を入力にする方向へ寄せる。

この段階でやること:

- `UnaDocument` / `.unavatar` / VRM source から dynamics source を読み、runtime dynamics view の最小形を決める。
- Unity Exporter は現在有効な VRC PhysBone component を `.unavatar` `dynamics[]` へ近似出力し、Runtime importer が SpringBone-like group へ lower する。
- 現在対応済み: VRC PhysBone `rootTransform` / `ignoreTransforms` / `multiChildType=Ignore` / `endpointPosition` / `radius` / `pull` / `spring` / `stiffness` / `gravity` / `allowCollision=false` / stable source id / limit metadata / interaction metadata の最小抽出と lower、source collider metadata 保存、branch root の複数 group 化、wardrobe `dynamicsEnable` による runtime group enable override、CLI diagnostics。VRC PhysBone は source metadata / action target として保持するが、現行 SpringBone-like solver では衣装を壊す可能性があるため authored default は既定 OFF とする。
- 残り: VRC PhysBone limit solver behavior / detailed collision behavior / grabbing / posing の挙動再現、animation state と連動した runtime enable state。
- Wardrobe / action / animation が dynamics enabled state を切り替えられるよう、source data と runtime state の所有関係を明記する。
- PhysBone behavior の詳細再現は Wardrobe hot switch と action model の後まで待つ。

## Wardrobe Hot Switch Target

リファクタリング後の最初の機能ターゲットは、renderer を再起動せずに `wardrobe_set` を切り替えること。

前提として、startup import 時点の `.unavatar` skinning / morph は共通 `UnaSceneSnapshot` 経由で GPU pipeline に接続済みとする。Wardrobe の `blendShapeWeight` operation は scene primitive の default morph weights を変えるため、hot switch は document revision を進め、draw 側の default morph weights を再読込し、既存 uploaded morph weights を invalidation する。通常フレームでは scene default morph の再走査を行わない。

初期 behavior:

- 選択された wardrobe operations を runtime resolved state へ適用する。
- process reload なしで visible draw set、関連 morph weights、material overrides を更新する。
- 可能な範囲で upload 済み asset を再利用する。
- runtime status に active set を出す。
- hot switch path が成熟するまでは、現在の startup path を fallback として残す。

MVP control command:

```json
{"command":"set_wardrobe","set_id":"field_drape"}
```

この command は既に attach 済みの document を base wardrobe state へ戻してから対象 `.unavatar` wardrobe operation を適用し、document revision を進める。対象 set だけを現在状態へ重ねると、前回 set の visibility / morph default が累積してしまうため禁止。draw transform / visibility / morph default は次の frame update で反映する。成功時は runtime status の `active_wardrobe_set` を更新する。初期実装では新規 GPU resource が必要な material / mesh を lazy upload せず、startup 時に読み込まれた resource set の範囲で切り替える。

現在対応済み:

- `set_wardrobe` runtime control command は正規化済み set id を受け、適用失敗理由を control response に返す。
- renderer は wardrobe 適用後に document revision を進め、draw transform / visibility / scene morph default / runtime requirements を次 frame で再読込する。
- renderer は wardrobe / runtime action の material slot 差し替え後、draw material uniform だけでなく material / outline material bind group も再生成し、startup 時に upload 済みの texture / sampler / cube map resource へ texture slot を再束縛する。
- `UnaRuntimeState.active_wardrobe_set` と `active_asset_groups` は wardrobe 適用成功時の resolved runtime state として更新され、`UnaRuntimeState.last_action_id` と `parameter_values` は runtime action 成功時だけ更新される。`UnaRuntimeState.dynamics_enabled_overrides` は wardrobe / runtime action の dynamics enable state を保持する。runtime status は document state から `active_wardrobe_set` / `active_asset_groups` / `last_action_id` / `runtime_parameter_values` を公開し、dynamics は effective enabled count、source authored enabled count、runtime override count を分けて出す。
- `dynamicsEnable` は `UnaRuntimeDynamicsMut` 経由で runtime dynamics enable override を切り替え、source group の authored default を直接変更しない。適用件数と missing dynamics id は renderer log で観測できる。

後回し:

- wardrobe asset group 単位の lazy GPU upload / unload。現在は選択 set の `assetGroups`、最小 resolver cache key、source asset group ownership count、active group から算出した scoped resident mesh/material/image/dynamics count、renderer draw residency count、image texture slot residency count、inactive image slot を参照する draw count、active draw が参照する inactive image slot count、material slot residency count を runtime state / renderer status / CLI diagnose / wardrobe apply report で観測する段階。material slot hot switch は upload 済み texture / sampler を再束縛するが、実 GPU resource の scoped upload / unload はまだ `all-resident` fallback。
- crossfade、dissolve、sparkle などのお着替え effect。
- set ごとの physics reset / blend。
- user-facing ring-menu UI。

## VRC Action Model Target

最初から VRC Animator Controller の完全 clone を作らない。

最初の model は、複数入力から叩ける action を表す。

- Expression Menu item
- keyboard shortcut / Function key
- Supervisor control
- 将来の ring-menu UI
- 将来の animation event / parameter change

初期 action effects:

- node / subtree visibility
- wardrobe set selection
- expression / morph weight
- material color、emission、scalar override
- dynamics enable / disable marker

この action model の上に、後から VRC Expression Menu の Toggle、Button、SubMenu、simple Puppet controls を載せる。

現在対応済み:

- `UnaRuntimeActionSet` / `UnaRuntimeAction` / trigger / effect schema を core に追加した。
- `.unavatar` wardrobe sets は、base set を除き `WardrobeSet` effect を持つ runtime action candidate へ import される。
- `.unavatar` variants のうち ObjectToggle / active-state 由来の node visibility operations は `NodeVisibility` effect を持つ runtime action candidate へ import される。metadata だけの MenuItem は effect を確定できないため source payload に残す。
- `.unavatar` variants の material color / scalar / slot、expression weight、dynamics enable operations は `MaterialColor` / `MaterialScalar` / `MaterialSlot` / `ExpressionWeight` / `DynamicsEnabled` effect を持つ runtime action candidate へ import される。
- CLI diagnose は runtime action 件数、trigger 件数、effect 件数、trigger / effect kind 内訳、action id / label を観測できる。
- renderer runtime control は `activate_action` を受け、`action_id`、`supervisor_command`、`expression_menu_path`、または `parameter_name` + `parameter_value` で action を解決する。`set_parameter` は action の有無に関係なく runtime parameter state を更新し、matching `ParameterValue` action がある場合は同じ effect 適用経路を使う。`WardrobeSet` effect は既存 hot switch 経路、`DynamicsEnabled` effect は runtime dynamics mutation 経路、`ExpressionWeight` effect は既存 expression override 経路、`NodeVisibility` effect は runtime scene visibility mutation 経路で適用する。`MaterialColor` / `MaterialScalar` effect は PBR 共通値の初期範囲として base color、emissive、alpha、metallic、roughness/smoothness、alpha cutoff を runtime material mutation 経路で適用し、`MaterialSlot` effect は runtime mesh primitive の material slot を差し替えて draw material uniform を再同期する。
- effect 付き `.unavatar` variant に MenuItem / Expression Menu metadata operation が同居する場合は、runtime action の `ExpressionMenu` trigger path へ metadata path を取り込む。metadata-only MenuItem は引き続き action 化しない。

次の段階:

- VRC Expression Menu metadata から action label/path をより正確に取り込む。Modular Avatar MenuItem metadata は effect source が確定できるものから順次 action 化する。CLI diagnose は action kind count に加え、NodeVisibility / MaterialSlot action の主要 target を表示できる。Menu Item / Menu Group / Menu Installer / Menu Install Target は metadata component として分類し、保存済み label / control / parameter / target / install target を diagnose で観測できる。
- Modular Avatar Object Toggle は structured component payload から `NodeVisibility` action へ import される。Material Setter の direct renderer slot payload は scene-aware renderer reference resolver を通して `MaterialSlot` action へ import され、Material Swap の scene-aware From / To slot expansion も null material slot を含めて `MaterialSlot` action へ import される。component / fields / menuItem に明示された Expression Menu path metadata は `ExpressionMenu` trigger へ取り込み、明示 MenuItem control parameter/value metadata は `ParameterValue` trigger として保持する。MenuItem `subParameters` は puppet 系 control metadata として runtime action condition / CLI diagnose に保持するが、値付き trigger にはしない。Action label は component name / displayName に加え、MenuItem name / displayName / label と Control name を fallback として取り込む。MenuItem の Expression Menu path は明示 `menuPath` / `expressionMenuPath` / `path` payload がある場合だけ取り込み、階層を推測して合成しない。QuickSwapMode は本家 Inspector の `To` material 候補選択補助であり runtime reaction 登録には使われないため、runtime emulation 対象外とする。Generic wardrobe material color / scalar / slot operations も hot switch で適用され、CLI diagnose / wardrobe probe で material apply counts を観測できる。runtime action trigger 評価は core query helper に統合済み。CLI diagnose は MenuItem parameter/value から effect-backed runtime action への対応と `WardrobeSet` ids を `menu_action_candidates` / `menu_wardrobe_candidates` として公開し、nested menu path も保持する。CLI diagnose は wardrobe asset group summary と missing group warning を出す。Renderer status は wardrobe asset upload plan を公開し、ownership metadata count、active groups の scoped resident count、renderer draw residency count、image texture slot residency count、inactive image slot を参照する draw count、active draw が参照する inactive image slot count、material slot residency count を出すが、実 resource は scoped upload/unload 未実装時は `all-resident` と明示する。残りは external menu asset expansion / UI consumption、scoped upload / unload 対応。
- Runtime action は trigger/effect とは別に source component id、MenuItem parameter/value、`Inverted`、解決できた component source node と active parent nodes を condition metadata として保持する。`set_parameter` は condition metadata を trigger より優先して action を選び、parameter/value と `Inverted` の組み合わせを本家 ReactiveObject の 0.005 幅に合わせて判定する。親ノード付き reactive action は current runtime scene の visible state を gate として評価する。現時点では inherited active-state 条件の完全評価、continuous evaluator、inactive-state default restore はまだ未実装。
- Mesh Cutter / Shape Changer / VertexFilter component payload は metadata として保持しつつ MeshCutter / ShapeChanger を resolver-capable に分類する。CLI diagnose は target、combine mode、blendshape / mask / bone / axis filter summary を観測できる。Runtime resolver は blendshape-based delete filters を、MA と同じく morph delta threshold で頂点選択し、該当頂点を含む三角形を削除する。mask / bone / axis filter と動的 reactive gating はまだ未実装。
- Modular Avatar / VRC Expression Menu metadata から取り込んだ material parameter 名を、必要に応じて lilToon 専用 parameter へ拡張する。

## PhysBone Placement

PhysBone behavior implementation は runtime state cleanup、runtime dynamics normalization、Wardrobe hot switch の後に置く。

理由:

- PhysBone roots、colliders、enabled state は active wardrobe と animation state に依存する。
- scene source data を直接 mutate する solver は、hot switch と相性が悪い。
- 初期実装では VRC PhysBone parameters を既存 SpringBone-like runtime primitives へ lower してよい。ただし source data ではなく resolved runtime dynamics view を入力にする。
- 現在は exporter/importer が PhysBone source を runtime dynamics group / collider data へ lower し、endpointPosition は leaf root の synthetic child として正規化し、normalized collider data も保持する。`allowCollision=false` は source collider を solver へ渡さない。limit / interaction metadata は runtime dynamics group に保持するが solver / interaction 挙動にはまだ反映しない。VRC PhysBone group は authored default として既定 OFF で、wardrobe `dynamicsEnable` と runtime action `DynamicsEnabled` は runtime state override だけを切り替える。CLI diagnose と renderer runtime status は effective enabled と source authored enabled を分けて観測でき、renderer status は runtime override count も公開する。残りは limits solver behavior、detailed collision behavior、grabbing/posing behavior。

## この段階の非目標

- VRChat client 完全再現。
- FX Layer / Animator Controller 完全互換。
- Poiyomi 互換。
- style-only cleanup のための lilToon-like rendering rewrite。
- instant switching が安定する前の完璧な wardrobe transition effect。
