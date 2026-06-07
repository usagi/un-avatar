# Humanoid Retarget Basis Notes

This note fixes the current UNMotion -> Humanoid node basis contract used by
`crates/un-avatar-skeleton/src/humanoid_retarget.rs`.

`cargo xtask retarget-audit` is the CPU-side regression probe for this contract.
It imports the real `target/tmp/model1.vrm`, VRM1 fixture, and
`target/tmp/mizuki-split.unavatar`, applies the same UNMotion samples, and
compares Humanoid successor world axes without launching the renderer.

## Target Basis

| target document | importer/exporter basis | root normalization | rotation conversion for body/root | translation conversion |
| --- | --- | --- | --- | --- |
| VRM0 | legacy VRM0 glTF | scene roots are premultiplied by Y 180 deg in `normalize_scene_basis_for_vrm` | `(-x, -y, z, w)` | `(x, y, -z)` |
| VRM1 | VRM1 / glTF | none | `(x, -y, -z, w)` | `(-x, y, z)` |
| `.unavatar` | Unity local TRS exported as glTF: position `(-x, y, z)`, rotation `(x, -y, -z, w)` | none | `(x, -y, -z, w)` for Humanoid body/root | `(-x, y, z)` |

VRM0 and VRM1 can disagree in local space for the same UNMotion sample. They
must agree after root/basis normalization in world space. This is covered by
`unmotion_left_upper_arm_matches_vrm0_vrm1_and_equivalent_unavatar_world_axis`
and the arm-raise regression
`unavatar_unmotion_upper_arm_raise_matches_vrm1_world_axis`.

## `.unavatar` Limb Axis Adapter

Unity/VRC avatars may have Humanoid limb transforms whose local child chain uses
`+Y`, while UNMotion limb samples are produced against canonical axes. The
adapter is only for `.unavatar` Humanoid body limb bones:

| bone group | canonical source axis after `.unavatar` target-basis conversion |
| --- | --- |
| LeftShoulder / LeftUpperArm / LeftLowerArm / LeftHand | `+X` |
| RightShoulder / RightUpperArm / RightLowerArm / RightHand | `-X` |
| LeftUpperLeg / LeftLowerLeg / RightUpperLeg / RightLowerLeg | `-Y` |
| LeftFoot / RightFoot | `+Z` |

The model-side rest bone axis is not raw `child.translation`. It is
`rest_rotation * child.translation` in the bone parent space. The chosen child
must be the Humanoid successor (`Shoulder -> UpperArm -> LowerArm -> Hand` and
`UpperLeg -> LowerLeg -> Foot -> Toes`) when available. For `Hand` the middle
proximal finger is used as the palm-forward successor, and for `Foot` a direct
child whose normalized name contains `toe` is preferred when the Humanoid Toes
bone is absent. Arbitrary first child is wrong for VRC avatars where shoulder
ribbons, sleeve helper bones, hand frills, leg frills, or other decoration bones
can precede the actual Humanoid chain. The adapter first rotates the model rest
axis onto the canonical source axis, then applies the already
target-basis-converted sample. This is not a coordinate-frame conjugation:
`inverse(adapter) * sample_rotation * adapter` turns a `.unavatar` `+Y` arm raise
into a 90 degree sideways rotation. The resulting parent-space delta is
converted back to the glTF local node space:

```text
target_axis = rest_rotation * child_translation
adapter = rotation_arc(target_axis, canonical_source_axis)
parent_space_delta = sample_rotation * adapter
local_delta = inverse(rest_rotation) * parent_space_delta * rest_rotation
node_rotation = rest_rotation * local_delta
```

## Hand And Finger Caveat

Body `LeftHand` / `RightHand` owns the hand bone when present and uses the same
`.unavatar` limb axis adapter as the arm chain. `HandMotion.wrist` must not
overwrite it in the same frame. If a frame has no body `LeftHand` / `RightHand`
sample and the fallback `HandMotion.wrist` path is used, the wrist rotation is
adapted with the same side-specific hand axis as the body hand bone.

Typed finger joints are intentionally separate from body and wrist conversion.
VRM0 keeps UNMotion finger rotations direct, matching the 1.0.0
`model1.vrm` behavior. The previous VRM1-oriented audit made this look wrong,
but that inverted the four long fingers relative to 1.0.0. `.unavatar`
therefore must not apply the body/root Unity-to-glTF quaternion conversion to
`HandFinger`. Instead, `.unavatar` finger joints use a separate rest-axis
adapter that maps the actual finger child axis, commonly `+Y` in Unity/VRC
exports, onto the canonical side axis (`left = +X`, `right = -X`), then flips
the non-thumb Z curl so Unity/VRC exports follow the VRM0/1.0.0 curl direction.

Thumb joints are not the same as the four long fingers. UNMotion typed
`HandMotion` emits thumb flexion as yaw, and proximal CMC yaw already includes
the UNMotion rest-open / spread term. The `.unavatar` adapter therefore must
not remove that rest-open term. `.unavatar` thumb intermediate / distal also do
not use the long-finger child-axis adapter: live UNMF/Z carries thumb yaw
directly, and adapting those joints as long fingers turns small open-hand
residual yaw into visible over-curl. `cargo xtask retarget-audit` compares
`model1.vrm`, a VRM1 fixture, and `mizuki-split.unavatar` for the same
`curl=0.0 -> 0.8` UNMF/Z samples. The audit checks both successor-axis movement
and the parent joint's world-basis movement; successor-axis-only agreement is
not enough because skinning consumes the full joint matrix.

## Runtime Cache Boundary

Retargeting still has format-specific code in `humanoid_retarget.rs`; this is
not the final architecture. The current cleanup boundary is
`RetargetFrameContext` plus `RetargetRestCache`. `RetargetFrameContext` carries
the per-frame motion coordinate space, target basis, and optional rest cache so
body, hand, finger, and face-head retargeting do not pass those pieces around as
separate ad hoc arguments.

For `.unavatar + UNMotion` frames, `UnavatarRetargetAdapter` owns the compiled
format-specific adapter data. Its `RetargetRestCache` stores rest local
rotations/translations, rest-parent indices, and rest-world rotations, and its
node-index rest axis table stores the adapter target axes. Other formats do not
build this adapter. This removes per-bone recomputation of rest `Mat4`
decomposition, `scene_parent_indices`, and `scene_world_matrices` from the
`.unavatar` limb/finger adapters and makes the future split clearer:

- import / model-compile layer: derive rest topology, rest-world rotations, and
  source-format basis adapters
- runtime layer: apply UNMotion to compiled canonical Humanoid data without
  rediscovering model topology

The next structural step is to move `TargetHumanoidBasis` and the
`.unavatar`-only adapter decisions into a compiled model-retarget context rather
than looking at the original document format during every frame application.
`HumanoidRetargetContext` is that first compiled context. The renderer builds it
when motion receivers are started, next to the immutable rest-node snapshot, and
then calls `apply_un_motion_frame_to_document_with_context` for each pending
motion frame. It owns `RuntimeRetargetData`, the compiled runtime lookup bundle
for profile keys, base transforms, body/finger node bindings, and expression
name matching. This keeps the legacy `apply_un_motion_frame_to_document_with_rest`
API available for tests and tools while avoiding repeated target-basis detection
and repeated `.unavatar` rest-cache construction in the renderer hot path.
It also stores base TRS for the root and Humanoid-profile nodes, so the renderer
hot path can write rest-relative local transforms without decomposing each rest
node matrix again.
It also precomputes normalized Humanoid profile keys so fallback key matching
does not scan and normalize the whole profile for every bone and finger lookup.
Body Humanoid bone node bindings are also compiled into a fixed bone-index
array, so body pose application does not resolve profile strings or scan node
binding lists for each received bone sample.
Body, face-head, and hand-wrist application share the node-index transform path;
only compatibility fallback paths resolve profile strings at runtime.
Expression preset name matching is also compiled into exact-casefold and
normalized lookup tables, avoiding catalog scans for each PerfectSync sample.
Typed finger profile keys are static table lookups, not `format!` allocations,
so the per-frame hand path does not allocate strings for each finger segment.
The context also stores finger segment node bindings, including successor node
indices, in fixed left/right x finger x segment arrays. Runtime hand application
therefore no longer resolves finger profile keys, successor profile keys, or map
lookups for each finger joint.
For `.unavatar + UNMotion`, body limb and typed finger adapters use the
compiled adapter's single node-index lookup instead of rediscovering Humanoid
successors or first-child fallback axes for every frame.
