# UNPhysics / UNDynamics v2 Design

UNPhysics は、UNAvatar が扱う avatar physics 全体の製品・設計上の総称とする。
v2 の実装対象は、その中の bone dynamics layer である UNDynamics に限定する。

## Naming

- `UNPhysics`: avatar physics 全体の umbrella。bone dynamics、contacts、constraints、将来の cloth / soft body などを含められる名前。
- `UNDynamics`: v2 で実装する source-neutral runtime layer。VRM SpringBone と VRC PhysBone をここへ lower する。
- `UNEvaluation`: wardrobe / action / animation / parameter / contact 由来の runtime state を評価する層。module 名は `runtime_eval`。詳細は [`unevaluation-v2.md`](unevaluation-v2.md)。
- `SpringBone`: VRM / UniVRM 由来の source format または既存実装資産を指す名前に限定する。
- `PhysBone`: VRC component 由来の source format を指す名前に限定する。

Solver / renderer / wardrobe runtime の public boundary では、可能な限り `UNDynamics` の語彙を使う。
`SpringBoneSimulator` など既存名は互換 shim として段階的に残してよいが、新しい behavior を SpringBone 固有 API に直接足さない。
SpringBone を v2 physics model の基準にはしない。v1 solver は実装資産であり、UNDynamics runtime model の設計基準ではない。

## Design Boundary

VRM SpringBone、VRC PhysBone、VRC PhysBone Collider、VRC Contacts、VRC Constraints は source metadata として保存できる。
ただし runtime solver は source kind を分岐条件にせず、正規化済み UNDynamics model を読む。

UNDynamics の中核概念:

- `UnaDynamicsGroup`: stable source id、enabled state、source provenance、solver parameters、chains、writeback mode、colliders、limits、interactions を束ねる runtime group。
- `UnaDynamicsChain`: 親から子へ向かう node chain。PhysBone endpointPosition は synthetic endpoint chain node として lower できる。
- `UnaDynamicsCollider`: sphere / capsule / inside bounds など source-neutral collider。
- `UnaDynamicsLimit`: angle / stretch など chain motion constraints。v2 初期は angle cone を現 solver backend が扱える拘束として近似反映し、stretch は metadata / diagnostics に留める。
- `UnaDynamicsInteraction`: grabbing / posing など interaction capability metadata。v2 初期は runtime action / diagnostics のために保持する。
- `UnaDynamicsContact`: VRC Contacts を source-neutral event / proximity metadata として保持する。
- `UnaDynamicsConstraintRef`: VRC Constraints や Modular Avatar resolver が残す参照関係を、bone dynamics rebuild / reset の判断材料として保持する。v2 初期では solver 入力ではないが、global dynamics reset 対象 node には含める。runtime action / wardrobe による source-scoped enable 切替では、対象 source group の dynamic nodes と重なる constraint ref だけを reset 対象に含める。CLI diagnose / renderer runtime status / Supervisor diagnostics は VRC constraint ref が metadata/reset ref only であることを warning として出す。

既存 v1 SpringBone solver / collider code は実装資産として再利用する。
ただし v2 の設計上は、SpringBone solver に PhysBone feature を直接追加するのではなく、UNDynamics が出す runtime terms を solver backend が解く、と扱う。
UNDynamics は解く対象を定義する層であり、group / chain / collider / force / drag / pull / limit / interaction / contact / constraint intent を source-neutral に持つ。
Solver backend は解き方を定義する層であり、Verlet、PBD / XPBD 的な拘束解法、反復回数、安定化、衝突解決順などのアルゴリズムを持つ。
このため v2 初期で v1 solver 由来の backend を使っても、PhysBone source を SpringBone source へ変換したとは呼ばない。未対応 term は source metadata / normalized metadata / diagnostics に残し、SpringBone semantics へ丸めたことにしない。

## Source Data And Runtime State

Source data:

- `.unavatar` / glTF extension の `dynamics[]` は authored source payload と provenance を保持する。
- VRM SpringBone の authored values、VRC PhysBone の component fields、PhysBone Collider、Contacts、Constraints 参照は失わない。
- asset group ownership、source node id、component path は source package data に属する。

Runtime state:

- active wardrobe set、active asset groups、runtime action parameter、dynamics enabled override は runtime state に属する。
- `dynamicsEnable` は authored default を書き換えず、stable dynamics id に対する runtime override として扱う。Renderer runtime control の `set_dynamics_enabled` command も同じ runtime override 経路を使い、`sourceId` / `enabled` で個別 group を切り替える。`set_all_dynamics_enabled` は QA 用に全 runtime dynamics group の override を一括設定する。Supervisor diagnostics の group toggle / all toggle はこれら command の QA 用 UI であり、source data や authored default は変更しない。
- solver state は source scene を直接 mutate せず、resolved scene / pose buffer / runtime dynamics view から構築する。
- runtime state の owner policy と continuous evaluator は UNEvaluation が扱う。UNDynamics solver は評価済み enabled state / parameters / colliders を読むだけで、wardrobe / action / contact の優先順位を決めない。

Stretch / writeback policy:

- Stretch は source-specific PhysBone feature ではなく、UNDynamics chain limit と solver writeback の能力として扱う。
- 現 solver backend は tail position を解いた後、ボーン local transform へ主に rotation を書き戻す。`rest_local_translation` と `rest_local_scale` は保持されるため、tail 解に stretch を許しても実ボーン長へ反映できない。
- v2 初期で stretch を実装する場合、まず solver output を `rotation_only` と `rotation_translation` のどちらで書き戻すかを group / joint 単位で明示する。実ボーンを伸縮させる対象は synthetic endpoint child、または skinned mesh を壊さない leaf / accessory chain に限定し、Humanoid / skinned deformation の基準ボーンには既定で translation stretch を書かない。solver は scene skin joint set を見て、`rotation_translation` group でも skinned joint child は translation writeback candidate から除外する。CLI diagnose / renderer runtime status / Supervisor diagnostics は group ごとの `translation_writeback_candidate_count` / `translation_writeback_target_count` と全体の `dynamics_rotation_translation_writeback_group_count` / `dynamics_translation_writeback_candidate_count` / `dynamics_translation_writeback_target_count` / `dynamics_stretch_translation_writeback_target_group_count` を公開する。
- `max_stretch` は rest tail length に対する上限倍率として正規化する。値は solver constraint の許容長に使うが、writeback mode が `rotation_only` の group では diagnostics metadata に留める。Unity Exporter は PhysBone の `maxStretch` / `maxSquish` / `stretchMotion` または対応 curve が authored されている場合、`.unavatar` dynamics entry へ `writebackMode: "rotation_translation"` を付けて source intent を渡す。`.unavatar` importer は `writebackMode` / `writeback_mode` を runtime group の `writeback_mode` へ lower するが、現 solver backend では `rotation_translation` をまだ実装せず、CLI diagnose / renderer runtime status warning で unsupported として扱う。
- SpringBone 的な固定長拘束を単に緩めるだけの実装は不可とする。現 solver backend は joint ごとの tail particle と local transform 書き戻し対象が一段ずれる箇所を持つため、stretch 実装は particle / writeback target map を明示してから行う。solver は 2-node leaf chain の terminal child と、multi-node chain の next chain node を writeback target として区別し、multi-node 末端の imaginary tail を最後の実 node へ二重割当しない。視覚上の伸縮、collider 判定、次 joint の parent pose、reset 時の rest pose 復帰が同じ policy で説明できる必要がある。

Lowering:

- VRM SpringBone source は UNDynamics group / chain / collider / parameter view へ lower する。
- VRC PhysBone source も同じ UNDynamics view へ lower する。source-specific fields は `sourceParams` と normalized metadata の両方で保持してよい。
- VRC PhysBone authored default は、現 solver で衣装を壊す可能性がある間は安全側で既定 OFF とし、wardrobe / action で runtime override できる。CLI diagnose / renderer runtime status は runtime dynamics group が存在するのに effective enabled group が 0 の場合、物理が source として保存されていても現在の runtime state では solver 対象が無いことを warning として出す。
Source importer は authored fields を UNDynamics terms へ写像する責務を持つ。Solver backend は source kind を知らず、UNDynamics runtime view と評価済み runtime state だけを読む。

## Implementation Checklist

1. Done: Docs / naming: UNPhysics と UNDynamics の責務、source/runtime 境界、非目標を固定する。
2. Done: Core scaffolding: source-neutral dynamics group / chain / parameter view を追加し、既存 `UnaRuntimeDynamics` から読めるようにする。
3. Done: No-behavior-change bridge: 既存 `UnaSpringBoneSettings` を UNDynamics view へ写し、現 solver の挙動を変えずに tests を通す。
4. Done: Solver naming bridge: `SpringBoneSimulator` を互換名として残しつつ、`DynamicsSimulator` alias と neutral renderer runtime names から呼べる形へ寄せる。
5. Done: Collider path cleanup: solver 入力の collider 構築を source-neutral names に寄せ、`allowCollision=false` / `insideBounds` の扱いを明示する。
6. Done for v2 initial solver: PhysBone colliders: sphere / capsule / inside bounds は UNDynamics collider として solver / debug draw へ接続済み。local source collider は runtime node scale を solver / debug draw の両方で反映する。`.unavatar` importer は exporter が出す string shape name に加えて Unity enum serialized numeric `shapeType` の Sphere / Capsule も受ける。Modular Avatar Global Collider は VRC PhysBone capsule collider intent として lower する。v2 初期の collider detailed behavior は position / rotation / radius / height / insideBounds / shapeType の source-neutral 接続を完成範囲とし、PhysBone particle radius curve、per-segment radius、endpoint edge case は chain / limit / solver writeback task 側で扱う。leaf root の `endpointPosition` は synthetic endpoint child として lower し、non-leaf root で無視される `endpointPosition` は ImportReport warning に出す。
7. In progress: PhysBone limits and per-chain radius: angle limit は UNDynamics cone constraint として solver へ近似反映済み。stretch limit は現 solver が主に回転を書き戻すため metadata / diagnostics に留め、translation / scale 反映設計後に扱う。CLI diagnose / renderer runtime status / Supervisor diagnostics は stretch limit が存在する場合、現 solver では metadata-only であることを warning として出し、lower 済み group では代表的な `source_id@root_path` sample も含める。Unity Exporter は PhysBone radius / force / angle / stretch 系 AnimationCurve metadata を sourceParams に保持する。radius curve は base radius の倍率として chain tail 正規化位置に sample し、source-neutral な per-joint hit radius として solver collider constraint へ近似反映する。CLI diagnose は radius curve が lowering された group では per-joint hit radius 近似であること、group へ lower できない raw source では metadata-only であることを warning として出す。CLI diagnose は raw source limit count と normalized runtime group limit count の両方を angle / stretch に分け、raw source curve count も radius / angle / stretch に分けて観測できる。
8. Done for v2 initial metadata: Interactions / Contacts / Constraints: grabbing / posing は group metadata として保持し、VRC PhysBone `parameter` も interaction metadata として exporter / importer / CLI diagnose / renderer runtime status へ接続済み。PhysBone suffix parameters は runtime parameter definition として宣言し、action/menu parameter との同名共有は conflict diagnostics として観測する。CLI diagnose / renderer runtime status / Supervisor diagnostics は interaction hook の source_id / root_path / allow_grabbing / allow_posing / base parameter / suffix parameters を bounded list として公開する。ただし `_IsGrabbed` / `_IsPosed` などの PhysBone suffix value emission は direct interaction evaluator 実装まで行わない。CLI diagnose / renderer runtime status / Supervisor diagnostics は grabbing / posing hook が現 solver では metadata-only であることを warning として出す。contacts / constraints は source-neutral metadata と runtime / diagnostics counts へ接続済み。Modular Avatar PBBlocker は VRC PhysBone lowering の ignore set に合成し、Modular Avatar Global Collider は resolved root / radius / height / position / rotation から UNDynamics VRC PhysBone capsule collider intent へ lower する。VRC Contact source id は sender / receiver 種別と同一 Transform 上の重複 ordinal で一意化する。Renderer runtime status は dynamics group / collider / contact parameter declaration / contact probe / constraint ref の bounded list も公開する。Contacts parameter emission は [`unevaluation-v2.md`](unevaluation-v2.md) の Phase A-D に従い、v2 初期範囲の metadata + parameter declaration は core runtime view / renderer runtime status / CLI diagnose まで接続済み。core runtime view、CLI diagnose、renderer runtime status は current runtime scene pose の diagnostics-only contact probe / would_emit count も出す。emission が無効な状態で would_emit probe がある場合、warning は receiver/source path、sender source、parameter の bounded sample も含める。opt-in 時だけ同じ current runtime scene pose probe から runtime parameter state へ 1/0 を書く。
9. In progress: Runtime integration: wardrobe hot switch / action state は dynamics enabled override を runtime state に書く。runtime action の source-scoped enable 変更時は対象 source group の dynamic nodes と関連 constraint ref nodes だけを rest pose へ戻し、simulator / collider state を同じ設定で再構築する。Wardrobe hot switch は base reset と複数 operation を含むため、切替単位で全 dynamic / constraint ref nodes を reset して simulator / collider state を再構築する。global dynamics reconfigure も従来通り全 dynamic / constraint ref nodes を reset する。animation state 連動、continuous evaluator、blend は UNEvaluation の設計に従って実装する。
10. Done for current metadata: Diagnostics: CLI diagnose は raw source counts と normalized runtime group counts を分け、raw PhysBone source collider count / unknown shape collider count も runtime lowered collider count と別に公開する。renderer runtime status と Supervisor diagnostics pass-through は effective enabled count、source authored enabled count、runtime override count、runtime limit / interaction / contact / constraint metadata count、contact probe count / would_emit count、node path 付き group / collider / contact / constraint bounded list を公開する。per-group status は `authored_enabled` / `effective_enabled` / `runtime_enabled_override` を分けて出す。metadata-only / disabled-emission warning は代表 source id と node path sample を含める。Supervisor diagnostics は bounded group list から `source_id` 単位で runtime enable override を切り替えられ、QA 用に全 dynamics group の runtime override を一括 enable / disable できる。どちらも source authored default は変更しない。今後新しい solver behavior を足す場合も、source metadata count と runtime effective count を混ぜない。

## Current Mainline

Wardrobe hot switch、runtime action、Menu / Parameter candidate、Contacts metadata / opt-in emission diagnostics は v2 初回リリースの physics QA に十分な基盤とする。
以後の主作業は UNDynamics behavior implementation であり、Wardrobe / Menu の richer UI、renderer tray / global shortcut access、broader eviction policy、お着替え transition effect は physics behavior が期待動作に達した後へ送る。

優先順:

1. Stretch を、UNDynamics limit / chain term と solver writeback の問題として設計・実装する。local collider scale、PhysBone collider position / rotation / radius / height / insideBounds、leaf endpointPosition synthetic child、sourceParams.ignoreTransforms で leaf 扱いになる endpointPosition、per-chain radius curve は solver / debug draw / chain lowering の runtime 展開へ反映済み。
2. PhysBone particle radius curve は、source-neutral chain sample metadata と solver hit radius の問題として扱う。v2 初期では `radiusCurve` を base radius 倍率として chain tail ごとに sample し、per-joint `hit_radius_samples` として collider constraint へ渡す。PhysBone 完全互換の粒子半径 semantics は、solver / writeback の検証後に必要範囲だけ拡張する。
3. Grabbing / posing は action hook / diagnostics の最小接続に留め、direct manipulation UI は後段へ送る。CLI diagnose / renderer runtime status は interaction hook の suffix parameter 候補まで公開済み。
4. Contact evaluation は current runtime scene pose を読む初期実装と Sphere / Capsule exact overlap を固定済み。次は dynamic reactive gating を、同じ source-neutral contact view 上で詰める。
5. VRC Constraints solver integration は node constraint / dynamics reset 対象の整理後に扱い、v2 初回では metadata / reset ref を維持する。

## Non Goals For v2 Initial Release

- VRChat client の PhysBone 完全再現。
- SpringBone solver と PhysBone solver の二重運用。
- source kind 分岐を frame loop / renderer / solver に散らす実装。
- モデル固有ハック。
- wardrobe transition effect より前の physics blend 完成。
