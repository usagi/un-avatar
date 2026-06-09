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

2026-06-09 時点で、これまで見つかっていた visual regression はユーザー目視確認により期待動作まで解決済み。

## Compatibility Checklist

### Component Discovery / Serialization

- `[~]` component catalog: Modular Avatar component type, enabled state, target node id, path, and raw scalar fields
  - done: Exporter stores `UN_avatar.modularAvatar.components[*]` and report `modularAvatar` with type, target, enabled state, public fields, and component count. Runtime importer and CLI diagnose report component support classification counts for resolver-supported, runtime-action-supported, unsupported, and disabled components, including type counts and disabled type counts. Enabled unsupported components are recorded as warning diagnostics and lost features so imports become PartialSuccess instead of silently hiding missing MA behavior; CLI diagnose also surfaces those import warnings in its human-readable warning list.
  - remaining: exporter-side unsupported / approximate classification per component type, private serialized fields where needed, and schema tests.
- `[~]` object reference resolution: `AvatarObjectReference`, direct object reference, humanoid bone reference, sub-path
  - done: Exporter serializes `AvatarObjectReference.referencePath`, direct target object, resolved target, and Transform/GameObject references as stable node id + path. Import reports now emit path diagnostics for exact duplicate scene paths, normalized ambiguous paths, and `.unavatar` registry paths that resolve to multiple scene nodes.
  - remaining: humanoid bone references and component-specific reference schemas.
- `[~]` execution order model: NDMF / Modular Avatar pass order summary
  - done: Runtime resolver order follows the relevant MA transform passes for current support: MeshSettings, ReplaceObject, MergeArmature / MeshRetargeter, then BoneProxy with prepass-captured world pose.
  - remaining: extend pass ordering as ReplaceObject, MeshCutter, MaterialSetter, dynamics, and late transform features are implemented.

### Armature / Skin / Mesh Retargeting

- `[~]` MA Merge Armature: bone mapping, prefix/suffix, mangleNames, lock mode
  - done: Exporter stores MergeArmature public fields and resolved source bone -> target bone mappings. Runtime resolver rewrites `UnaSkin.joint_nodes` and inverse bind matrices from saved source bone -> target bone mappings.
  - remaining: retained merged bones / transform lookthrough, nested topology order, and components / constraints / PhysBone cases where MA preserves intermediate bones.
  - sample: `Color  1`, `Color  13`, `B_White&Brown` armature merge.
- `[~]` non-MA fitted outfit armature fallback
  - done: When a `.unavatar` wardrobe outfit contains a separate fitted armature but no Modular Avatar payload, Runtime maps same-name outfit Humanoid joints to the avatar Humanoid joints, rewrites skin joint bindposes, and reparents unmapped auxiliary bone subtrees directly below the matched avatar Humanoid bone while preserving world pose. This covers common non-MA fitted outfit roots such as sleeve, stocking, breast, skirt, bag, and accessory helper bones that sit below `Hips` / `Chest` / `Head` / arm / leg bones.
  - remaining: this is a conservative fallback, not a full Modular Avatar bake. It does not infer constraints, custom component semantics, PhysBone behavior, material side effects, or ambiguous duplicated bone names beyond same-name Humanoid connection points.
  - sample: `usagi.unavatar` `LittleWriter` / `A_Brown&Gold` is a non-MA wardrobe outfit with its own `A_Brown&Gold/Armature`; current fallback detects same-name Humanoid connections and moves auxiliary roots such as `ShirtRoot`, `SleeveRoot_*`, `StockingsRoot_*`, and accessory roots under the avatar armature.
- `[~]` MeshRetargeter bindpose rewrite
  - reference formula: `newBindTarget.worldToLocalMatrix * originalBone.localToWorldMatrix * originalBindPose`
  - done: Runtime applies the equivalent formula after glTF import using `UnaSceneSnapshot` world matrices.
  - remaining: renderer rootBone / localBounds scale adjustment and fixture coverage for multi-bone wardrobe assets.
- `[~]` same-name Humanoid armature fallback
  - done: when a `.unavatar` contains skinned outfit armatures but no MA MergeArmature payload for those skins, importer retargets joints whose node names match the primary `UN_avatar.humanoid` bones and rewrites inverse bind matrices with the same MeshRetargeter formula.
  - scope: this is a compatibility fallback for sparse / non-MA captures such as `usagi.unavatar`, not a substitute for Modular Avatar bake semantics.
  - remaining: non-Humanoid accessory bones, constraints, PhysBone roots, and material / blendshape side effects still require explicit MA or VRC payload.
- `[ ]` retained merged bones / transform lookthrough
  - required: components, constraints, PhysBone roots, and rootBone offset cases where MA keeps intermediate bones.
- `[~]` rootBone / localBounds / probeAnchor retarget
  - done: Importer preserves glTF `skin.skeleton` as `UnaSkin.skeleton_node`. Runtime MeshRetargeter retargets `skeleton_node` through MergeArmature mappings. Runtime MeshSettings applies RootBone to selected subtree skins, stores ProbeAnchor / localBounds on renderer nodes, and renderer debug/draw metadata can observe them.
  - remaining: full renderer culling/cache policy for localBounds and exact rootBone-vs-probeAnchor reference handling.
- `[ ]` nested MergeArmature topology
  - required: parent/child merge order, cycle diagnostics.
- `[ ]` Visible Head Accessory
  - required: head-visible accessory mesh handling and proxy head bone behavior.

### Reparenting / Proxy / Object Replacement

- `[~]` MA Bone Proxy
  - done: Exporter stores public fields and resolved target node. Runtime resolver applies reparenting for `AsChildAtRoot`, `AsChildKeepWorldPose`, `AsChildKeepPosition`, `AsChildKeepRotation`, `matchScale`, and MA-like duplicate child name suffixing. Nested proxies use a MA-like prepass that captures every proxy world pose before any proxy reparenting. Numeric tests cover duplicate target child names and missing target reporting.
  - remaining: regression-check `field_drape` visually and add broader fixture coverage for component interactions.
- `[~]` MA Replace Object
  - done: Runtime resolver moves the replacement object into the original object's parent slot, migrates original children under the replacement while preserving world pose, hides the original node, and remaps Runtime node references such as skin joints, skin root, probe anchor, and node constraints to the replacement. Replacement nodes get a derived resolved node id without changing their source node id.
  - remaining: Exporter schema parity for object/component reference remap diagnostics and recursive / conflicting replacement fixture coverage.
- `[ ]` MA Move Independently
  - required: grouped transform parent remapping, if it affects visible render graph.
- `[ ]` MA World Fixed Object / World Scale Object
  - required: classify as runtime transform/dynamics concern or unsupported render-only feature.

### Mesh Settings / Geometry Mutation

- `[~]` MA Mesh Settings
  - done: Exporter serializes `Bounds` as structured center/extents/size. Runtime applies RootBone override to skin skeleton metadata, stores ProbeAnchor / Bounds on target renderer nodes, and exposes them in renderer debug metadata.
  - remaining: exact renderer culling behavior, inverted root bone behavior, and unskinned mesh conversion.
- `[~]` MA Mesh Cutter
  - done: Mesh Cutter / VertexFilter component payloads are classified as resolver-capable metadata and diagnose exposes a common vertex filter group representation with target, combine mode, and blendshape / mask / bone / axis filter summaries when the source payload carries them. Runtime resolver applies blendshape-based Mesh Cutter filters by selecting vertices whose morph target position delta exceeds the MA threshold and removing any triangle that references a selected vertex, cloning shared meshes before mutation.
  - remaining: mask / bone / axis filter execution, generated mesh cache key detail, and dynamic reactive gating beyond enabled static payloads.
- `[~]` MA Shape Changer
  - done: Shape Changer delete-shape payloads are represented as blendshape vertex filters with threshold metadata in diagnose. Runtime resolver applies delete-shape payloads with the same blendshape vertex selection and triangle removal path as Mesh Cutter.
  - remaining: Set-mode morph defaults, Blendshape Sync propagation, and dynamic reactive gating beyond enabled static payloads.
- `[~]` MA Remove Vertex Color
  - done: Runtime resolver applies `Mode=Remove` to renderer meshes under the nearest Remove Vertex Color component, honors nested `DontRemove`, strips `colors_0`, clones shared mesh buffers before mutation so subtree-external renderers keep their vertex colors, and reports removed node / primitive counts.
  - remaining: exporter schema parity for every serialized mode spelling and fixture coverage through full `.unavatar` import/export.
- `[~]` vertex filters: by blendshape, mask, bone, axis
  - done: common filter representation exists in core and diagnose metadata for blendshape, mask, bone, and axis filters.
  - remaining: resolver-side vertex selection and mesh mutation.

### Materials / Reactive Objects

- `[~]` MA Object Toggle
  - done: exporter extracts some toggle/menu candidates as variants. Runtime action importer also lowers structured `ModularAvatarObjectToggle` component payloads into `NodeVisibility` effects, using scene-aware object reference resolution when a scene snapshot is available and preserving explicit MenuItem path / parameter triggers. Runtime actions now preserve condition metadata for source component id, MenuItem parameter/value, MenuItem `subParameters`, `Inverted`, component source node, and active parent nodes. `set_parameter` selects actions through parameter condition metadata first, including inverted conditions, and gates parented reactive actions on the current runtime visibility of their active parent chain.
  - remaining: full reactive object integration with inherited active-state conditions beyond the captured parent chain, inactive-state default restore, and continuous animator-style evaluation.
- `[~]` MA Material Swap
  - done: Runtime action importer expands structured Material Swap root / From / To payload into `MaterialSlot` effects by matching current scene primitive material slots under the selected root. Null `From` / `To` materials are represented as empty material slots. Explicit component / fields / menuItem expression menu path metadata is imported as an `ExpressionMenu` trigger; explicit MenuItem control parameter/value metadata is preserved as a `ParameterValue` trigger, and MenuItem `subParameters` are retained as puppet metadata. Renderer control accepts `set_parameter`, records runtime parameter state even without a matching action, and applies matching `ParameterValue` actions through the same runtime action path.
  - note: upstream `QuickSwapMode` is an Inspector candidate-selection helper for editing the `To` material; runtime reaction registration consumes the serialized `Swaps` From / To list, so no runtime QuickSwap emulation is planned.
  - remaining: asset group lazy upload / texture references, and full continuous parameter evaluator behavior beyond direct `set_parameter` updates.
- `[~]` MA Material Setter
  - done: Runtime action importer resolves structured Material Setter renderer references against the scene when available, then lowers object / material index / material payload into `MaterialSlot` effects for direct renderer slot replacement. Explicit component / fields / menuItem expression menu path metadata is imported as an `ExpressionMenu` trigger; explicit MenuItem control parameter/value metadata is preserved as a `ParameterValue` trigger, and MenuItem `subParameters` are retained as puppet metadata on runtime action conditions / CLI diagnose. Renderer `set_parameter` can drive matching material setter actions without using explicit action ids and still persists unmatched parameter state for later evaluators.
  - remaining: full reactive object integration with continuous parameter evaluator behavior, broader component reference diagnostics, and material property override mapping beyond slot replacement.
- `[~]` Blendshape Sync
  - required: source renderer/shape to target renderer/shape binding.
  - done: Unity Exporter serializes `BlendshapeBinding` entries as structured `referenceMesh`, `blendshape`, `localBlendshape`, and `remapCurve` payload instead of opaque type-name strings. Runtime resolver applies structured bindings to static source morph defaults, uses `localBlendshape` fallback semantics, evaluates keyframe remap curves with in/out tangents for initial weights, and clones shared target meshes before mutating default morph weights. Runtime expression catalog propagation is supported for no-remap and linear origin remap curves by adding target morph binds to the source expression preset. A freshly re-exported `mizuki-split.unavatar` sample reports `blendshape_sync_applied=18`, `missing=0`, and `unsupported=0`.
  - remaining: non-linear runtime expression / animation curve propagation, weighted tangent / wrap mode curve parity, and recursive sync.
- `[ ]` Sync Parameter Sequence
  - required: classify as VRC expression/parameter feature; defer if not needed for static wardrobe render.

### Menu / Parameters / Animator

- `[~]` MA Menu Item / Menu Group / Menu Installer
  - done: exporter extracts menu item hints. Runtime / diagnose classify Menu Item, Menu Group, Menu Installer, and Menu Install Target as metadata components rather than unsupported behavior, and CLI diagnose reports saved label, control type, parameter/value, component index, hierarchy path, sibling index, target path, menu source, source target, menuToAppend, install target menu, and installer reference from the captured payload. CLI diagnose derives deterministic menu graph candidates from component index, hierarchy path, sibling index, menu source target, and installer references, then promotes them into graph nodes with parent component edges and child component lists for hierarchy-based UI labels. Diagnose also reports installer / install-target edges and marks an installer's target-menu edge as ignored when any Menu Install Target references that installer, matching Modular Avatar's filtering rule. MenuItem parameter/value metadata is matched to effect-backed runtime actions through action conditions first, then parameter triggers, so UI candidates can report the action id, effect kinds, and WardrobeSet ids they would drive. Diagnose now also groups MenuItem -> runtime action -> WardrobeSet matches into `menu_wardrobe_candidates` with menu path labels for wardrobe UI planning.
  - remaining: external menu asset expansion, loop handling, and UI surface beyond diagnose.
- `[~]` MA Parameters
  - done: `ModularAvatarParameters` is classified as metadata and CLI diagnose reports captured `ParameterConfig` entries with name/prefix, remap target, internal/prefix flags, sync type, local-only state, default value, saved state, explicit-default flag, and animator-default override flag.
  - remaining: runtime parameter definition model, conflict merge rules, and expression parameter / animator default application.
- `[defer]` Merge Animator / Merge Motion / MMD Layer Control
  - reason: full FX Animator evaluation is not a v2 initial goal.
  - required before enabling: compatibility report must expose lost animator behavior.

### Dynamics / Constraints / Platform

- `[ ]` MA Convert Constraints
  - required: identify constraints that affect render transform and convert to Runtime-supported node constraints or report unsupported.
- `[ ]` MA Global Collider / PhysBone Blocker
  - required: capture blocker / collider intent and feed resolved state into the U.N. dynamics normalization layer.
  - note: PhysBone behavior is not owned by the Modular Avatar resolver; it is lowered through the SpringBone / PhysBone to U.N. dynamics plan.
- `[ ]` MA Floor Adjuster
  - required: avatar root transform / floor offset handling.
- `[ ]` MA Platform Filter
  - required: platform selection policy for local renderer.
- `[ ]` MA VRChat Settings / Rename VRChat Collision Tags
  - required: metadata/report first; render impact likely deferred.

### Export Format

- `[~]` `UN_avatar.modularAvatar` extension block
  - done: importer reads the root `UN_avatar` extension from `.unavatar`, `.glb`, `.gltf`, and in-memory glTF/GLB inputs, so Modular Avatar source payloads are no longer restricted to `.unavatar` path hints.
  - remaining: required fields `schemaVersion`, `components`, `references`, `armatureMappings`, `meshMutations`, `materialOverrides`, `diagnostics`, plus exporter schema parity.
- `[~]` stable IDs for source and resolved nodes
  - done: Scene nodes carry source node ids separately from runtime resolved node ids. Runtime node targets can resolve by source id, resolved id, path, or index while preserving source id priority for wardrobe-authored targets. MA Replace Object assigns a derived resolved node id to the replacement node, and CLI diagnose exposes resolved node ids.
  - remaining: exporter schema parity for resolved ids created by other resolver stages and derived mesh/cache node ids.
- `[x]` resolver cache key
  - done: runtime resolver cache key exposes selected wardrobe set, active asset groups, Modular Avatar component payload hash, material source hash, mesh render identity hash, and resolver version through core, CLI diagnose, and renderer status. Mesh render identity includes vertex attribute layout, including vertex color presence, so resolver mesh mutations such as Remove Vertex Color can invalidate downstream mesh caches.
  - note: resolved graph cache storage and full vertex payload cache keys are renderer implementation work, not part of the metadata surface checklist.
- `[~]` Wardrobe asset group lazy GPU upload
  - done: importer reads per-asset group ownership metadata from root / wardrobe source payload into scene source data. CLI diagnose reports both ownership counts, per-group mesh primitive / material / image / dynamics membership, scoped active resident counts, and missing active group ids, including no-scene documents with active groups. Wardrobe apply report exposes the same scoped active resident counts and missing active group ids. Core provides a document-level scoped asset selection query that combines runtime active asset groups with source ownership metadata for renderer / diagnose / future physics use. Renderer draw/material/image residency uses that scoped selection semantics, keeping unowned assets resident and scoping only assets with source ownership. Renderer runtime status exposes a wardrobe asset upload plan with active groups, declared wardrobe asset groups, source asset group ownership counts, scoped resident mesh/material/image/dynamics counts for the active groups, missing active group ids, inactive owned group count, renderer draw residency counts, image texture slot residency counts, draw count referencing inactive image slots, active draw count referencing inactive image/material slots, unique inactive image/material slot count and bounded slot index preview referenced by active draws, material slot residency counts, pending scoped texture/material upload work counts, last hot-switch refresh image/material load/unload transition counts, last 2D image texture scoped load/unload counts, last material slot scoped upload count, and explicit all-resident GPU mode for cubemap resources. Renderer draw residency is now scoped by active asset groups for owned mesh primitives, including hot-switch refresh, while underlying GPU buffers/textures remain resident. Renderer internals now keep full active source asset work lists, full active texture/material gap upload work lists, and hot-switch image/material load/unload transition lists separate from bounded status previews. Hot switch refresh uploads active 2D image texture slots from retained CPU upload payloads, swaps inactive image slots to fallback views while dropping their GPU texture/view, promotes active draw material slots that need scoped residency, and regenerates material / outline material bind groups after material slot and asset residency updates. Unity Exporter report includes bounded renderer asset ownership diagnostics with node id / path / glTF mesh / primitive / material / image indices. Unity Exporter also writes conservative `wardrobe.assetGroupOwnership` entries for renderer and PhysBone source paths whose inferred `outfit:<top>` group is declared by a wardrobe set.
  - remaining: exact ownership for non-outfit groups, actual scoped mesh / texture upload allocation for inactive groups, and unload / eviction.
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
  - done: one-node `AsChildKeepWorldPose` reparent keeps world position and updates parent hierarchy; nested proxy test preserves child world pose when parent proxy snaps to target; numeric coverage now checks `AsChildAtRoot`, `AsChildKeepPosition`, `AsChildKeepRotation`, `AsChildKeepWorldPose`, `matchScale`, duplicate target child names, and missing target reporting.
  - remaining: broader component interaction fixture coverage.
- `[x]` visual regression screenshots / visual checks
  - done: existing `mizuki-split` visual regressions were visually confirmed as resolved on 2026-06-09.

## Near-Term Order

1. Implement renderer scoped upload / unload using Wardrobe asset group ownership metadata.
2. Expand Menu / Parameter runtime surface beyond diagnose, including external menu asset expansion and UI consumption of `menu_wardrobe_candidates`.
