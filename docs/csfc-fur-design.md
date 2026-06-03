# CSFC Fur design

Status: implementation design for UNAvatar High-tier Fur.

CSFC means Compute Surface Fur Cards. It is the primary UNAvatar Fur implementation direction for wgpu. CBF (Compute Barycentric Fur) remains a reference model for lilToon's geometry-shader `AppendFur()` path, but UNAvatar does not need to implement CBF as the main runtime path.

## Goals

- Accept existing lilToon Fur materials without requiring author changes.
- Match lilToon FurTwoPass Scene view quality as the first compatibility baseline.
- Use compute to exceed geometry-shader limits: adaptive density, stable sampling, camera/budget awareness, and future simulation.
- Keep Low/Compatible instanced shell available for fallback hardware and diagnostics.
- Keep the High path incremental: first static generated cards, then animated/skinned update, then physics, then strand/groom.

## Non-goals

- Literal geometry-shader emulation as the final algorithm.
- Per-frame CPU mesh expansion.
- Fur-specific AO maps or new mandatory authored textures.
- Changing imported lilToon material semantics before the renderer can honor them.

## User-facing modes

Internal implementation uses CSFC for all High Fur modes. UI labels should describe intent, not engine internals.

- `lilToon-compatible`
  - Uses lilToon parameters as the source of truth.
  - Density and card placement are tuned to resemble lilToon's Fur / FurCutout / FurTwoPass look.
  - Conservative budget and stable migration behavior.

- `UNAvatar standard`
  - Default recommendation.
  - Uses lilToon inputs but rebalances density by triangle area, UV density, mask, length, camera distance, and quality budget.
  - Aims for better apparent quality per cost than lilToon's fixed barycentric GS samples.

- `UNAvatar high-quality`
  - Heavier version of standard.
  - Higher card count, better distribution, stronger anti-alias coverage, better sorting/lighting, and optional physics.

- `Strand/Groom`
  - Future mode.
  - Persistent strand buffers, compute simulation, ribbon or strand rendering.

## lilToon parameter mapping

CSFC must preserve lilToon authoring inputs.

Core Fur inputs:

- `_UseFur`: enables Fur generation.
- `_FurLayerNum`: compatibility density hint, not literal shell count in CSFC.
- `_FurVector`: base tangent-space direction and length.
- `_FurVectorScale`: normal-map/vector texture scale.
- `_FurVectorTex`: direction modulation.
- `_VertexColor2FurVector`: future input if vertex color import is available.
- `_FurLengthMask`: scalar length mask.
- `_FurGravity`: world/downward bend factor.
- `_FurRandomize`: deterministic direction jitter.
- `_FurNoiseMask`: alpha breakup / strand coverage noise.
- `_FurMask`: density/visibility mask.
- `_FurRootOffset`: root width / alpha shape.
- `_FurAO`: shell/card AO amount.
- `_FurCutoutLength`: pre-pass length shortening for FurTwoPass.
- `_FurRimColor`, `_FurRimFresnelPower`, `_FurRimAntiLight`: Fur rim.

Render-state inputs:

- Fur mode: Fur, FurCutout, FurTwoPass.
- Fur cull, blend, ZWrite, ZTest, ColorMask, AlphaToMask.
- ForwardAdd Fur behavior can be approximated with existing lighting first, then improved when additional light support exists.

AO note:

- lilToon AO Map belongs to the normal Shadow settings. CSFC may use the shared AO/shadow path after that exists, but it must not invent a Fur-only AO map.

## Runtime architecture

CSFC has four stages.

1. Source capture
   - Keep source vertex/index buffers in GPU-readable form.
   - Keep per-primitive metadata: vertex range, index range, material index, node/skin palette, morph metadata, bounds, source triangle count.
   - Keep source material Fur parameters in a compact uniform/storage block.

2. Sample planning
   - Compute or update per-triangle metrics:
     - world area or approximate skinned area
     - UV area / UV density
     - average FurMask
     - average FurLengthMask
     - distance and projected area
     - material quality scale
   - Convert metrics into a card budget per triangle.
   - Prefix-sum card counts into output offsets.

3. Card generation
   - Generate root/tip vertices into a fur vertex storage buffer.
   - Generate indices into a fur index storage buffer, or use an indirect draw layout that avoids rewriting indices when possible.
   - Each generated card carries:
     - root position
     - tip position
     - normal / tangent or card frame
     - UV
     - root-tip coordinate (`furLayer`)
     - material/draw id
     - stable random seed
     - coverage / length / mask values

4. Rendering
   - Draw generated cards with Fur-specific pipelines:
     - Fur pre coverage pass for FurTwoPass / cutout stability.
     - Transparent Fur pass.
   - Use the existing toon fragment logic where possible, but split Fur-specific alpha/AO/rim from ordinary toon so normal toon pipeline requirements do not grow accidentally.

## Source data model

The source mesh path currently stores CPU-expanded primitive data and creates vertex/index buffers for vertex-shader skinning. CSFC needs additional GPU-readable source buffers.

Recommended source buffers:

- `SourceVertex`
  - position
  - normal
  - tangent
  - uv
  - joints
  - weights

- `SourceTriangle`
  - three source vertex indices
  - primitive/draw id
  - optional precomputed local area
  - optional precomputed UV area

- `FurPrimitiveMeta`
  - material/draw index
  - triangle range
  - source vertex range
  - skin palette index
  - morph range
  - max cards
  - quality limits

- `FurMaterialGpu`
  - lilToon Fur parameters
  - render mode flags
  - quality expression mode
  - texture binding indices if using bindless-like arrays later, or per-draw bind groups first.

Initial implementation can avoid bindless complexity by generating Fur per draw/material batch with the existing texture bind group. Later, grouped multi-draw compute can reduce dispatch overhead.

## Sample allocation

CSFC should not use `_FurLayerNum` as a literal generated count. It is a compatibility hint.

Suggested formula:

```text
base = mode_density(_FurLayerNum, expression_mode)
area = sqrt(world_area / target_world_area)
uv = sqrt(uv_area / target_uv_area)
mask = average(_FurMask)
length = average(_FurLengthMask) * _FurVector.w
screen = projected_area_factor(camera, bounds)
budget = global_quality_budget_remaining

cards = clamp(round(base * area_weight * uv_weight * mask * length_weight * screen_weight), min_cards, max_cards)
```

Compatibility mode:

- Bias sample locations toward lilToon's barycentric sets.
- Preserve stable density for migration even if area/UV weights would choose a very different count.
- `_FurLayerNum` 1/2/3 maps to increasingly dense budgets.

Standard mode:

- Use blue-noise or low-discrepancy sample placement over triangle area.
- Adapt density strongly by projected area and mask/length.
- Enforce a global per-frame card budget.

High-quality mode:

- More samples.
- Better temporal stability.
- Optional card sorting or approximate order-independent coverage.

## Sample placement

Placement must be stable across frames.

Initial option:

- Per-triangle deterministic low-discrepancy sequence.
- Seed from mesh id, primitive id, triangle id, material id.
- Convert 2D random pair to barycentric coordinates.
- Reject or downweight samples where mask/length are low.

Compatibility option:

- Start with lilToon barycentric samples for low counts.
- Fill additional CSFC samples around them using deterministic jitter.

Future option:

- Persistent per-surface sample buffer so samples survive topology-stable updates and physics can accumulate history.

## Card shape

First implementation:

- A fur card is a small camera-facing or frame-facing quad from root to tip.
- Root is on the surface sample.
- Tip is root plus fur vector after vector texture, length mask, randomize, gravity, and optional `_FurCutoutLength` pre scaling.
- Width is derived from local triangle size, density, `furLayer`, and quality mode.

Compatibility target:

- Cards should visually approximate lilToon's generated root-tip segments.
- `furLayer` is 0 at root and 1 at tip.
- Fragment alpha uses the same Fur alpha formulas as lilToon for pre/transparent modes.

UNAvatar target:

- Card width and orientation can improve over lilToon GS.
- Cards may use camera-facing width for silhouette quality, but root orientation must remain surface-stable enough to avoid swimming.

## Pass model

CSFC should implement FurTwoPass semantics explicitly.

- Pre pass:
  - Shorter length using `_FurCutoutLength`.
  - Cutout-like alpha remap.
  - Depth write on when mode requires.
  - AlphaToMask-style coverage where MSAA is available.

- Transparent pass:
  - Full length.
  - Transparent or premultiplied blend according to imported Fur render state.
  - Depth write normally off for Fur/FurTwoPass transparent layer.

- FurCutout:
  - Can use only cutout/pre-style rendering.

This pass model is more important for visual stability than exact CBF topology.

## Update cadence

Static inputs:

- Source topology, source UVs, material texture ids, and local-space precomputed triangle metrics.

Per model load:

- Build source triangle buffer.
- Build static local/UV metrics.
- Allocate maximum expected card buffers from quality caps.

Per frame:

- Update skin palette and morph buffers as today.
- If camera or motion changed, update sample planning if LOD is camera-dependent.
- Dispatch card generation.
- Draw generated cards.

Optimization:

- If camera-independent compatibility mode is selected and skeleton/morphs are static, planning can be cached and only card positions update.
- If a material has no length/mask/vector texture and no animation, more planning can be static.

## Integration plan

Phase 0: keep current Low shell stable.

- Current instanced shell remains a fallback.
- Do not keep expanding it into the quality path.

Phase 1: CSFC CPU prototype for validation only.

- Implement a small non-runtime Rust generator for one primitive or test fixture.
- Verify barycentric placement, count allocation, and buffer size math.
- No renderer integration yet.

Phase 2: GPU compute skeleton.

- Add `csfc_fur.wgsl`.
- Add source triangle/source vertex storage buffers.
- Generate simple root-tip quads without texture sampling.
- Draw generated card buffer for one Fur draw.

Phase 3: lilToon parameter correctness.

- Add `_FurVectorTex`, `_FurLengthMask`, `_FurNoiseMask`, `_FurMask`.
- Add root offset, AO, cutout length, randomize, gravity.
- Add pre / transparent pass split.

Phase 4: adaptive CSFC.

- Add area/UV/mask/length/camera/budget sample allocation.
- Add expression modes.
- Add quality caps and debug counters.

Phase 5: polish.

- Better card orientation.
- Better temporal stability.
- Better sorting/coverage.
- Optional physics hooks.

## Diagnostics

Diagnostics must stay small and targeted.

Useful toggles:

- Force Low shell.
- Force CSFC compatibility placement.
- Force uniform per-triangle card count.
- Show generated card count per material.
- Clamp global Fur budget.
- Disable Fur textures independently: vector, length, noise, mask.
- Freeze sample planning.

Useful counters:

- source Fur draw count
- source triangle count
- generated card count
- generated vertex/index bytes
- compute dispatch count
- cards clipped by mask/length/budget

## Tests

Unit tests:

- `_FurLayerNum` compatibility density maps to expected budget tiers.
- Barycentric sample generation is deterministic.
- Area/UV/mask/length budget allocation is monotonic.
- Buffer size math rejects overflow.
- FurTwoPass mode produces pre and transparent pass requirements.

Shader/pipeline tests:

- CSFC compute shader parses and validates.
- Generated card render pipelines validate.
- Ordinary toon pipelines do not require Fur-only bindings.

Scene tests:

- `mizuki-split` imports Fur texture slots and parameters.
- CSFC generated card count is nonzero for `Mat_Fur` / `Mat_Fur_Hat`.
- Low shell fallback still starts.

Visual acceptance:

- Compare against Unity Scene view on `mizuki-split`.
- First pass acceptance is silhouette and density, not exact lighting.
- Final compatibility acceptance is equal or better than lilToon 2.3.2 Scene view for FurTwoPass targets.
