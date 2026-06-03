# lilToon Fur technical target

Status: investigation note for UNAvatar Fur implementation.

## Source baseline

- Upstream local repo: `C:\Users\the\tmp\lilToon`, tag `2.3.2`, commit `56d5095`.
- Official docs identify Fur as a dedicated high-cost rendering mode with normal/vector, length mask, gravity, randomize, noise, mask, AO, mesh type, layer count, root width, and contact controls.
- Official shader-structure docs list `lts_fur.shader` and related Fur variants as exceptional shaders with their own passes, not just ordinary lilToon material variants.

## What lilToon Fur actually is

lilToon Fur is not a classic uniform shell-only technique.

The core implementation is a geometry-shader fur generator:

- `lil_common_vert_fur.hlsl` computes per-vertex fur vectors in object/tangent space, blends `_FurVector`, optional vertex color, optional `_FurVectorTex`, `_FurVectorScale`, `_FurVector.w`, `_FurCutoutLength` for pre-pass, world transform, gravity, optional contact deformation, and randomization.
- `geom()` receives each triangle and emits generated line/card-like pairs through `AppendFur()`.
- `AppendFur()` emits an inner vertex with `furLayer = 0` and an outer vertex offset by the interpolated fur vector with `furLayer = 1`.
- `_FurLayerNum` does not mean shell instance count. It selects a fixed set of barycentric sample positions inside the triangle:
  - 1: triangle vertices.
  - 2: vertices plus edge midpoints.
  - 3: above plus additional interior/biased barycentric samples.
  - one final repeated vertex sample is appended before `RestartStrip()`.
- This is why Unity Scene view shows many fine strands from a low-poly mesh: the shader creates additional per-triangle fur segments/cards, not merely expanded copies of the original mesh.

There is also `lil_common_vert_fur_thirdparty.hlsl`, a FakeFur path based on UnlitWF/UnToon. It generates triangle-center/interpolated fur strips in a loop over `_FurLayerNum`, but the default lilToon path used by current Fur shaders is the `AppendFur()` geometry path.

## Fragment behavior

`lil_pass_forward_fur.hlsl` has a Fur-specific fragment path:

- Runs normal lilToon main color and lighting setup.
- Applies `OVERRIDE_FUR` from `lil_common_frag.hlsl`.
- Computes:
  - `furLayerShift = furLayer - furLayer * _FurRootOffset + _FurRootOffset`
  - noise from `_FurNoiseMask`
  - alpha from noise and `furLayerShift`
  - mask multiplication from `_FurMask`
  - alpha multiplication into `fd.col.a`
  - Fur AO into `fd.col.rgb`
- Cutout / pre path uses `fd.col.a = saturate(fd.col.a * 5.0 - 2.0)` and discards zero alpha.
- Transparent path clips by `_Cutoff`.
- Adds Fur rim contribution using `input.furLayer`, `_FurRimColor`, `_FurRimFresnelPower`, `_FurRimAntiLight`, light color, and view angle.

Important distinction:

- For `LIL_RENDER == 1` or `LIL_FUR_PRE`, alpha uses the cubic `furLayerShift * abs^3 + 0.25` formula and shell-style AO based on `fwidth(input.furLayer)`.
- For transparent Fur, alpha uses the square formula and a different noise-driven AO expression.

## Pass model

lilToon has several Fur rendering modes:

- Fur: transparent-ish Fur pass, no Fur ZWrite, no AlphaToMask.
- FurCutout: cutout Fur, Fur ZWrite on, AlphaToMask on.
- FurTwoPass: combines:
  - `FORWARD_FUR_PRE`: `LIL_FUR_PRE`, ZWrite On, `Blend One Zero`, AlphaToMask On, cutout-like pre coverage.
  - `FORWARD_FUR`: transparent Fur, configurable Fur blending, typically ZWrite Off.
  - ForwardAdd Fur pre and Fur passes for additive lights.

This is a quality/stability design, not just a style toggle. The pre pass gives stable coverage/depth and the transparent pass softens the result.

## UNAvatar target

The current UNAvatar instanced-shell path is not the correct quality target. It can remain as Low/Compatible, but it should not define Fur completion.

## Compute direction

UNAvatar targets wgpu, so geometry shaders and tessellation shaders are not available as implementation primitives. That constraint changes the design goal.

We do not need to stop at a compute port of lilToon's geometry shader. Such a port is useful as a reference model, but it inherits the constraints of the original shader stage.

Terminology:

- CBF: Compute Barycentric Fur.
  - A compute translation of lilToon's geometry-shader `AppendFur()` method.
  - Triangle-local.
  - Uses fixed barycentric sample sets.
  - Reproduces GS-style generation density.
  - Strongly tied to source mesh topology.
  - Mostly local output control.
  - Kept as a theoretical/reference compatibility model, not the primary UNAvatar implementation target.

- CSFC: Compute Surface Fur Cards.
  - UNAvatar's primary compute fur-card direction.
  - Evaluates triangle area, UV density, fur mask, length mask, camera distance, and quality budget.
  - Allocates generated fur-card count from those signals.
  - Places samples over the surface with a stable distribution.
  - Generates fur cards/segments from those samples.
  - Can emulate lilToon-style density where compatibility matters, while also allowing better quality/performance tradeoffs than CBF.

Reasoning:

- CBF is valuable for understanding lilToon's look, but it preserves geometry-shader limitations.
- Compute has global budgeting and arbitrary generation logic, so matching a GS algorithm exactly is not automatically the best result.
- CSFC can spend density where it matters: large triangles, high visible area, dense UV/mask detail, long fur, close camera distance.
- CSFC can avoid spending density where it is wasted: tiny distant triangles, masked-out regions, short/zero-length fur, visually hidden regions.
- Therefore CSFC is expected to match or exceed lilToon Scene view quality at lower or more controllable cost than a literal CBF implementation.

User-facing modes should not expose CBF vs CSFC as engine internals. The modes should describe intent:

- lilToon-compatible expression:
  - Uses lilToon parameters directly.
  - Prioritizes compatibility and predictable migration.
  - Internally implemented with CSFC tuned toward lilToon-like placement, density, alpha, and two-pass behavior.

- UNAvatar standard expression:
  - Default recommended balance.
  - Uses the same lilToon inputs, but allocates density by area/UV/mask/length/camera/budget.
  - Targets better apparent quality per cost than lilToon GS.

- UNAvatar high-quality expression:
  - Heavier option after the base CSFC path is stable.
  - Increases sample quality, card shaping, sorting/coverage quality, and lighting fidelity.

- Future strand/groom expression:
  - Persistent strand buffers.
  - Compute simulation.
  - Ribbon/card/strand rendering.
  - Physics such as root fixed, tip dynamics, wind, gravity, motion inertia, and collision.

Target tiers:

- Low / Compatible:
  - Keep instanced shell for portability and old hardware.
  - Make it materially correct enough: imported parameters, textures, alpha/noise/mask/root/AO/rim, two-pass-style pre coverage where possible.
  - Accept that it will not match Unity Scene quality on sparse fur meshes.

- High:
  - Implement CSFC, not literal CBF, as the primary high-tier Fur path.
  - Compute output should be able to emulate the default `AppendFur()` look when the user selects lilToon-compatible expression, but the internal generation model is budgeted surface sampling:
    - source triangles -> area/UV/mask/length/camera/budget weighted sample counts
    - samples -> inner/outer generated fur-card vertices
    - generated indices matching the selected expression mode's density and card-shape target
    - per-generated vertex `furLayer` / root-tip coordinate
    - interpolated UV, normal, tangent/fur vector, joints/weights or already-skinned position
  - Run per-frame when bones/morphs/material animation affect the fur source.
  - Render generated buffers through a Fur-specific toon fragment path.
  - Add FurTwoPass-equivalent rendering: pre cutout/AlphaToMask-style coverage plus transparent Fur pass.
  - Match `_FurVectorTex`, `_FurLengthMask`, `_FurNoiseMask`, `_FurMask`, `_FurRootOffset`, `_FurAO`, `_FurCutoutLength`, `_FurRimColor`, `_FurRimFresnelPower`, `_FurRimAntiLight`, blend/ZWrite/ZTest/cull controls.

- Future:
  - Compute tessellation/subdivision before Fur generation where needed.
  - Compute strand/groom mode as an UNAvatar extension beyond lilToon compatibility.

## Acceptance goal

UNAvatar Fur is not complete until the High path reaches at least Unity Editor Scene view quality for known lilToon FurTwoPass assets such as `mizuki-split`.

The first visual milestone is:

- Fur silhouette is made of fine generated fur segments, not a smooth inflated shell.
- sparse source mesh still produces dense fur.
- Fur length/mask/noise produces strand breakup comparable to Unity.
- Two-pass Fur avoids both hard cutout-only aliasing and transparent-shell blobbing.
- The result is equal to or better than lilToon 2.3.2 Scene view on the comparison target.
