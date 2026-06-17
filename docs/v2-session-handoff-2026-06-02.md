# v2 Session Handoff - 2026-06-02

This note is for continuing UNAvatar v2 development in a fresh Codex session.

## Core Direction

- v2 UNToon is lilToon-compatible first.
- v1 MToon-like code is reference/legacy, not the architectural base for v2.
- During v2 development, `.unavatar` / lilToon input should prefer `UnaLilToonLikeMaterial`.
- `.vrm` / MToon input can continue to use legacy MToon-like behavior until the v2 conversion layer is proven.
- Do not add new lilToon features by forcing them into MToon parameters. Add them to `UnaLilToonLikeMaterial`.

## Current Working Sample

- Main test profile: `mizuki-split`
- Sample file path used during development:
  - `C:\Users\the\tmp\un-avatar\target\tmp\mizuki-split.unavatar`
- Unity project location:
  - `C:\Users\the\AppData\Local\VRChatProjects\mizuki`
- Main visual references were Base / original / noble1 / noble13 wardrobe states.

## What Was Implemented In This Session

### Material Architecture

- Added/expanded `UnaLilToonLikeMaterial` as v2 source of truth.
- Renderer now uses lilToon-like material data for `.unavatar` material paths where present.
- Renamed renderer-side Toon pipeline terms away from MToon-centered names where practical.

### Exporter / Importer

- Unity exporter keeps source bytes and raw material params.
- Exporter writes extra texture indices for lilToon-compatible slots.
- glTF importer maps `UN_avatar_material` extras into `UnaLilToonLikeMaterial`.

### Implemented / Partially Connected lilToon Features

- Shadow:
  - `_UseShadow`
  - `_ShadowColor`
  - `_ShadowColorTex`
  - `_ShadowStrength`
  - `_ShadowBorder`
  - `_ShadowBlur`
  - `_ShadowBorderRange`
  - `_ShadowMainStrength`
  - `_ShadowEnvStrength`
  - `_ShadowBorderColor`
  - `_ShadowStrengthMask`
  - `_ShadowBorderMask`
  - `_ShadowBlurMask`
  - `_ShadowNormalStrength`
  - `_ShadowReceive` is stored only. It needs a Unity-style shadow attenuation input before shader connection.
- MatCap:
  - `_UseMatCap`
  - `_MatCapTex`
  - `_MatCapColor`
  - `_MatCapMainStrength`
  - `_MatCapBlend`
  - `_MatCapBlendMode`
  - `_MatCapEnableLighting`
  - `_MatCapBlendMask`
- Reflection / Specular:
  - `_UseReflection`
  - `_Smoothness`
  - `_SmoothnessTex`
  - `_Metallic`
  - `_MetallicGlossMap`
  - `_Reflectance`
  - `_ApplySpecular`
  - `_SpecularToon`
  - `_SpecularBorder`
  - `_SpecularBlur`
  - `_ApplyReflection`
  - `_ReflectionColor`
  - `_ReflectionColorTex`
  - `_ReflectionCubeTex` source asset import
  - `_ReflectionBlendMode`
- Rim:
  - `_UseRim`
  - `_RimColor`
  - `_RimColorTex`
  - `_RimMainStrength`
  - `_RimBorder`
  - `_RimBlur`
  - `_RimFresnelPower`
  - `_RimEnableLighting`
  - `_RimBlendMode`
  - `_UseRimShade`
  - `_RimShadeColor`
- Emission:
  - `_UseEmission`
  - `_EmissionColor`
  - `_EmissionMainStrength`
  - `_EmissionMap`
  - `_EmissionBlend`
  - `_EmissionBlendMode`
- Outline:
  - `_UseOutline`
  - `_OutlineWidth`
  - `_OutlineColor`
  - `_OutlineTex` is stored but not fully sampled as outline color yet.
  - `_OutlineWidthMask`
  - `_OutlineFixWidth` is stored only.
  - `_OutlineEnableLighting`
  - `_OutlineZBias` is stored only.
- Alpha:
  - `_AlphaMaskMode`
  - `_AlphaMask`
  - `_AlphaMaskScale`
  - `_AlphaMaskValue`

## Important Docs

- Main compatibility tracker:
  - `docs/untoon-liltoon-compatibility.md`
- v2 roadmap:
  - `docs/v2-roadmap.md`

`docs/untoon-liltoon-compatibility.md` uses:

- `[ ]`: not started
- `[~]`: partial / approximate
- `[x]`: implemented and regression-checked against sample wardrobe states
- `[defer]`: intentionally deferred

Rule: every `[~]` item must include sub-level `done:` and `remaining:` lines.

## Verification Commands Used

Run these after continuing material/shader changes:

```powershell
cargo fmt
cargo check -p un-avatar-render-wgpu
cargo test -p un-avatar-io-gltf -- --nocapture
cargo test -p un-avatar-render-wgpu -- --nocapture
@'
from pathlib import Path
lines=Path('docs/untoon-liltoon-compatibility.md').read_text(encoding='utf-8').splitlines()
missing=[]
for i,line in enumerate(lines):
    if line.startswith('- `[~]`'):
        block=[]; j=i+1
        while j < len(lines) and not lines[j].startswith('- `[') and not lines[j].startswith('### '):
            block.append(lines[j]); j+=1
        text='\n'.join(block)
        if 'done:' not in text or 'remaining:' not in text:
            missing.append((i+1,line))
for ln,line in missing: print(f'{ln}: {line}')
print(f'missing={len(missing)}')
'@ | python -
```

Most recent verification before this handoff:

- `cargo check -p un-avatar-render-wgpu`: passed
- `cargo test -p un-avatar-io-gltf -- --nocapture`: 19 passed
- `cargo test -p un-avatar-render-wgpu -- --nocapture`: 76 passed
- compatibility checklist partial-status check: `missing=0`

## Good Next Steps

1. Launch `mizuki-split` and visually compare Base / original / noble1 / noble13 after the recent shader additions.
2. Continue `docs/untoon-liltoon-compatibility.md` top-down rather than tuning screenshots blindly.
3. Candidate next slices:
   - per-texture UV set / UV mode and `_ST` handling beyond main texture
   - `_MatCapNormalStrength`
   - `_SpecularNormalStrength` / `_ReflectionNormalStrength`
   - `_ReflectionCubeEnableLighting`
   - render queue / stencil / color mask reporting
   - emission gradation
4. Keep `_ShadowReceive` stored-only until UNAvatar has an explicit shadow attenuation/light visibility input.

## Warnings

- Do not reintroduce hardcoded compatibility paths for old experimental `mizuki-split.unavatar` exports.
- Do not treat v1 MToon-like parameters as the v2 design target.
- Avoid optimizing `.unavatar` texture bytes inside the Unity exporter. Source bytes should remain faithful; optimization belongs in a later external optimizer.
- Wardrobe Split is the preferred direction over baked wardrobe export unless a later experiment proves otherwise.
