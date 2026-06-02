# Modular Avatar Compatibility Plan

U.N. Avatar v2 の `Wardrobe (Split)` は、Unity Editor で wardrobe set ごとに Modular Avatar bake を繰り返すのではなく、`.unavatar` に source graph と Modular Avatar 由来の解決情報を保持し、Runtime 側で選択 set の render graph / cache を生成する方針とする。

この文書は Modular Avatar 対応の正本 checklist である。個別モデルの見た目だけに合わせた例外処理を入れず、Modular Avatar 本家の処理単位を確認し、Exporter capture / Runtime resolver / Renderer cache のどこで再現するかを明示する。

## Reference

- Modular Avatar reference: `bdunderscore/modular-avatar`, MIT License
- Local source during v2 development: `C:\Users\the\tmp\modular-avatar`
- Current sample issue: `mizuki-split` / `field_drape` / `Mizuki&Rurune_Field Drape_Hair/Hair_Base`

## Scope

`Wardrobe (Split)` では、Runtime が Unity / VRC SDK / NDMF に依存しない。Unity 固有の SerializedObject、Prefab、component reference 解決は Exporter が `.unavatar` に保存する。Runtime は保存済みの構造化 payload だけを読み、rendering に必要な scene graph、skin、mesh、material、visibility、dynamics を解決する。

## Layers

1. Exporter capture: Modular Avatar component と Unity object reference を `.unavatar` の stable node id / path / scalar payload へ正規化する。
2. Runtime resolver: capture payload、wardrobe operations、source scene graph から selected set の render graph を構築する。
3. Renderer cache: resolver 後の mesh / skin / material / dynamics を GPU upload / lazy unload する。
4. Diagnostics: unsupported / approximate / lost feature を report に出し、見た目破綻を原因単位で追えるようにする。

## Status Legend

`[ ]`: not started
`[~]`: partial / approximate
`[x]`: implemented and regression-checked against sample wardrobe states
`[defer]`: intentionally deferred with reason

`[~]` は必ず `done` と `remaining` を分ける。何が保存済みで、何が Runtime 未解決なのかを曖昧にしない。

## Current Finding: Hair_Base

`field_drape` の `Hair_Base` は、alpha / visibility ではなく Modular Avatar Bone Proxy 未適用による transform hierarchy 問題として扱う。

- `.unavatar` 現状:
  - node: `Mizuki&Rurune_Field Drape_Hair/Hair_Base`
  - mesh node world: origin
  - skin skeleton: `Mizuki&Rurune_Field Drape_Hair/cycr_scalp_Root`
  - dominant joint: `cycr_scalp_Root` 100%
  - `cycr_scalp_Root` and `Cycr_Hair_Root` have `ModularAvatarBoneProxy` components targeting avatar `Head`.
  - Runtime が Bone Proxy の reparenting を適用しないと、衣装側 scalp root が avatar head hierarchy に入らず、頭部追従ではなく bind pose 位置の頭皮 mesh として見える。
- Unity / Modular Avatar の期待:
  - Bone Proxy processor が proxy transform を target bone の子へ reparent し、attachment mode に応じて local transform を調整する。
  - `AsChildKeepWorldPose` では reparent 前の world pose を保つ。

この問題は `Hair_Base` 単体の特殊修正ではなく、Modular Avatar bake 相当 resolver の Bone Proxy 対応として直す。MergeArmature / MeshRetargeter は別の衣装 armature 対応として継続する。

## Compatibility Checklist

### Component Discovery / Serialization

- `[~]` component catalog: Modular Avatar component type, enabled state, target node id, path, and raw scalar fields
  - done: Exporter stores `UN_avatar.modularAvatar.components[*]` and report `modularAvatar` with type, target, enabled state, public fields, and component count.
  - remaining: unsupported / approximate classification per component type, private serialized fields where needed, and schema tests.
- `[~]` object reference resolution: `AvatarObjectReference`, direct object reference, humanoid bone reference, sub-path
  - done: Exporter serializes `AvatarObjectReference.referencePath`, direct target object, resolved target, and Transform/GameObject references as stable node id + path.
  - remaining: humanoid bone references, ambiguous duplicate path diagnostics, and component-specific reference schemas.
- `[ ]` execution order model: NDMF / Modular Avatar pass order summary
  - required: Runtime resolver の処理順を固定し、MergeArmature と BoneProxy の依存関係を壊さない。

### Armature / Skin / Mesh Retargeting

- `[~]` MA Merge Armature: bone mapping, prefix/suffix, mangleNames, lock mode
  - done: Exporter stores MergeArmature public fields and resolved source bone -> target bone mappings.
  - remaining: Runtime resolver must rewrite renderer.bones and inverse bind matrices with the MA MeshRetargeter-equivalent formula.
  - sample: `Color  1`, `Color  13`, `B_White&Brown` armature merge.
- `[ ]` MeshRetargeter bindpose rewrite
  - reference formula: `newBindTarget.worldToLocalMatrix * originalBone.localToWorldMatrix * originalBindPose`
  - required: glTF coordinate conversion 後の同等式を exporter/runtime のどちらで適用するか固定する。
- `[ ]` retained merged bones / transform lookthrough
  - required: components, constraints, PhysBone roots, and rootBone offset cases where MA keeps intermediate bones.
- `[ ]` rootBone / localBounds / probeAnchor retarget
  - required: SkinnedMeshRenderer local bounds を selected graph に合わせる。
- `[ ]` nested MergeArmature topology
  - required: parent/child merge order, cycle diagnostics.
- `[ ]` Visible Head Accessory
  - required: head-visible accessory mesh handling and proxy head bone behavior.

### Reparenting / Proxy / Object Replacement

- `[~]` MA Bone Proxy
  - done: Exporter stores public fields and resolved target node. Runtime resolver applies reparenting for `AsChildAtRoot`, `AsChildKeepWorldPose`, `AsChildKeepPosition`, `AsChildKeepRotation`, and `matchScale`.
  - remaining: regression-check `field_drape` visually and add broader fixture coverage for nested proxies / duplicate names / missing targets.
- `[ ]` MA Replace Object
  - required: replacement object and child migration as render graph operation.
- `[ ]` MA Move Independently
  - required: grouped transform parent remapping, if it affects visible render graph.
- `[ ]` MA World Fixed Object / World Scale Object
  - required: classify as runtime transform/dynamics concern or unsupported render-only feature.

### Mesh Settings / Geometry Mutation

- `[ ]` MA Mesh Settings
  - required: rootBone override, bounds, probe anchor, inverted root bone behavior.
- `[ ]` MA Mesh Cutter
  - required: mesh vertex / primitive filtering, generated mesh cache key.
- `[ ]` MA Shape Changer
  - required: blendshape-driven mesh filtering or morph defaults.
- `[ ]` MA Remove Vertex Color
  - required: vertex color stripping or material input fallback.
- `[ ]` vertex filters: by blendshape, mask, bone, axis
  - required: common filter representation before Mesh Cutter / Shape Changer.

### Materials / Reactive Objects

- `[~]` MA Object Toggle
  - done: exporter extracts some toggle/menu candidates as variants.
  - remaining: treat these as wardrobe operations source, not full MA bake replacement.
- `[ ]` MA Material Swap
  - required: material slot replacement operation and texture/material asset group references.
- `[ ]` MA Material Setter
  - required: material property override operation with lilToon-compatible parameter mapping.
- `[ ]` Blendshape Sync
  - required: source renderer/shape to target renderer/shape binding.
- `[ ]` Sync Parameter Sequence
  - required: classify as VRC expression/parameter feature; defer if not needed for static wardrobe render.

### Menu / Parameters / Animator

- `[~]` MA Menu Item / Menu Group / Menu Installer
  - done: exporter extracts menu item hints.
  - remaining: preserve menu hierarchy enough to generate wardrobe candidates and runtime UI labels.
- `[ ]` MA Parameters
  - required: parameter definitions, default values, saved/synced metadata where relevant to wardrobe.
- `[defer]` Merge Animator / Merge Motion / MMD Layer Control
  - reason: full FX Animator evaluation is not a v2 initial goal.
  - required before enabling: compatibility report must expose lost animator behavior.

### Dynamics / Constraints / Platform

- `[ ]` MA Convert Constraints
  - required: identify constraints that affect render transform and convert to Runtime-supported node constraints or report unsupported.
- `[ ]` MA Global Collider / PhysBone Blocker
  - required: integrate with VRC PhysBone to U.N. dynamics plan.
- `[ ]` MA Floor Adjuster
  - required: avatar root transform / floor offset handling.
- `[ ]` MA Platform Filter
  - required: platform selection policy for local renderer.
- `[ ]` MA VRChat Settings / Rename VRChat Collision Tags
  - required: metadata/report first; render impact likely deferred.

### Export Format

- `[ ]` `UN_avatar.modularAvatar` extension block
  - required fields: `schemaVersion`, `components`, `references`, `armatureMappings`, `meshMutations`, `materialOverrides`, `diagnostics`.
- `[ ]` stable IDs for source and resolved nodes
  - required: source node id remains wardrobe operation target; resolved graph can create derived node ids/cache keys.
- `[ ]` resolver cache key
  - required: selected wardrobe set, MA component payload hash, mesh/material source hash, runtime resolver version.
- `[ ]` report parity
  - required: Unity Exporter report and Runtime importer report use the same feature names.

### Tests / Regression

- `[ ]` Unity diagnostic probe for selected GameObject
  - required: print component payload, renderer.bones, rootBone, bindposes, material slots before export.
- `[ ]` isolated `.unavatar` fixtures
  - required: Hair_Base-only, Field Drape Hair-only, full field_drape.
- `[ ]` numeric resolver tests
  - required: MergeArmature one-bone retarget, BoneProxy reparent, MeshSettings rootBone override.
- `[~]` BoneProxy numeric test
  - done: one-node `AsChildKeepWorldPose` reparent keeps world position and updates parent hierarchy.
  - remaining: cover all attachment modes and matchScale.
- `[ ]` visual regression screenshots
  - required: front/back/detail views for mizuki field_drape after each resolver milestone.

## Near-Term Order

1. Regression: verify Runtime Bone Proxy resolver removes the `Hair_Base` neck/back artifact in `mizuki-split`.
2. Runtime: implement MergeArmature / MeshRetargeter resolver on scene snapshot before GPU upload.
3. Runtime: implement MeshSettings rootBone / bounds / probeAnchor handling.
4. Add schema / numeric tests for each resolver stage.
