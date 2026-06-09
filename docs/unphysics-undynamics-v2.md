# UNPhysics / UNDynamics v2 Design

UNPhysics は、UNAvatar が扱う avatar physics 全体の製品・設計上の総称とする。
v2 の実装対象は、その中の bone dynamics layer である UNDynamics に限定する。

## Naming

- `UNPhysics`: avatar physics 全体の umbrella。bone dynamics、contacts、constraints、将来の cloth / soft body などを含められる名前。
- `UNDynamics`: v2 で実装する source-neutral runtime layer。VRM SpringBone と VRC PhysBone をここへ lower する。
- `SpringBone`: VRM / UniVRM 由来の source format または既存実装資産を指す名前に限定する。
- `PhysBone`: VRC component 由来の source format を指す名前に限定する。

Solver / renderer / wardrobe runtime の public boundary では、可能な限り `UNDynamics` の語彙を使う。
`SpringBoneSimulator` など既存名は互換 shim として段階的に残してよいが、新しい behavior を SpringBone 固有 API に直接足さない。

## Design Boundary

VRM SpringBone、VRC PhysBone、VRC PhysBone Collider、VRC Contacts、VRC Constraints は source metadata として保存できる。
ただし runtime solver は source kind を分岐条件にせず、正規化済み UNDynamics model を読む。

UNDynamics の中核概念:

- `UnaDynamicsGroup`: stable source id、enabled state、source provenance、solver parameters、chains、colliders、limits、interactions を束ねる runtime group。
- `UnaDynamicsChain`: 親から子へ向かう node chain。PhysBone endpointPosition は synthetic endpoint chain node として lower できる。
- `UnaDynamicsCollider`: sphere / capsule / inside bounds など source-neutral collider。
- `UnaDynamicsLimit`: angle / stretch など chain motion constraints。v2 初期は metadata と diagnostics から始め、solver 反映は別 commit に分ける。
- `UnaDynamicsInteraction`: grabbing / posing など interaction capability metadata。v2 初期は runtime action / diagnostics のために保持する。
- `UnaDynamicsContact`: VRC Contacts を source-neutral event / proximity metadata として保持する将来枠。
- `UnaDynamicsConstraintRef`: VRC Constraints や Modular Avatar resolver が残す参照関係を、bone dynamics rebuild / reset の判断材料として保持する将来枠。

既存 v1 SpringBone solver / collider code は実装資産として再利用する。
ただし v2 の設計上は、SpringBone solver に PhysBone feature を直接追加するのではなく、UNDynamics solver がまず SpringBone-like Verlet/PBD primitive を内包している、と扱う。

## Source Data And Runtime State

Source data:

- `.unavatar` / glTF extension の `dynamics[]` は authored source payload と provenance を保持する。
- VRM SpringBone の authored values、VRC PhysBone の component fields、PhysBone Collider、Contacts、Constraints 参照は失わない。
- asset group ownership、source node id、component path は source package data に属する。

Runtime state:

- active wardrobe set、active asset groups、runtime action parameter、dynamics enabled override は runtime state に属する。
- `dynamicsEnable` は authored default を書き換えず、stable dynamics id に対する runtime override として扱う。
- solver state は source scene を直接 mutate せず、resolved scene / pose buffer / runtime dynamics view から構築する。

Lowering:

- VRM SpringBone source は UNDynamics group / chain / collider / parameter view へ lower する。
- VRC PhysBone source も同じ UNDynamics view へ lower する。source-specific fields は `sourceParams` と normalized metadata の両方で保持してよい。
- VRC PhysBone authored default は、現 solver で衣装を壊す可能性がある間は安全側で既定 OFF とし、wardrobe / action で runtime override できる。

## Implementation Checklist

1. Docs / naming: UNPhysics と UNDynamics の責務、source/runtime 境界、非目標を固定する。
2. Core scaffolding: source-neutral dynamics group / chain / parameter view を追加し、既存 `UnaRuntimeDynamics` から読めるようにする。
3. No-behavior-change bridge: 既存 `UnaSpringBoneSettings` を UNDynamics view へ写し、現 solver の挙動を変えずに tests を通す。
4. Solver naming bridge: `SpringBoneSimulator` を互換名として残しつつ、内部または新 API を UNDynamics solver として呼べる形へ寄せる。
5. Collider path cleanup: solver 入力の collider 構築を source-neutral names に寄せ、`allowCollision=false` / `insideBounds` の扱いを明示する。
6. PhysBone colliders: sphere / capsule / inside bounds の solver 反映を個別 test 付きで追加する。
7. PhysBone limits: angle / stretch limit を solver へ反映する。SpringBone 互換挙動と分けて、UNDynamics limit constraint として実装する。
8. Interactions / Contacts / Constraints: grabbing / posing / contacts / constraints は metadata、diagnostics、runtime action hooks の順に広げる。solver 反映は source evidence と test model が揃ってから行う。
9. Runtime integration: wardrobe hot switch / action / animation state が dynamics enabled state、reset、blend を同じ runtime boundary で扱うようにする。
10. Diagnostics: CLI / renderer status は source counts と effective runtime counts を分けて出し続ける。

## Non Goals For v2 Initial Release

- VRChat client の PhysBone 完全再現。
- SpringBone solver と PhysBone solver の二重運用。
- source kind 分岐を frame loop / renderer / solver に散らす実装。
- モデル固有ハック。
- wardrobe transition effect より前の physics blend 完成。
