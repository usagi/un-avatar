# ADR 0002: VRM MToon / UNToon Runtime Policy

## Status

Accepted. Updated for v2 direction.

## Context

VRM avatars often use MToon as their visual identity. Treating VRM materials as Simple, Unlit, or generic Lambert loses important shade, rim, matcap, alpha, and outline behavior.

v1 implemented this as an MToon-like renderer. v2 extends the same runtime asset into UNToon, but the compatibility target changes: lilToon-compatible expression is the primary target, and MToon is an input profile that should be converted into that broader UNToon material model.

## Decision

VRM materials default to toon rendering in UN Avatar. In v2, the internal target is UNToon/lilToon-compatible, not a strict MToon-shaped model.

- VRM0 `materialProperties` and VRM1 `VRMC_materials_mtoon` are parsed as MToon source profile data.
- MToon source profile data may still be stored in legacy `UnaMtoonMaterial` fields while the implementation migrates, but runtime rendering uses the v2-UNToon semantic material converted from that source profile.
- lilToon source material state takes priority for `.unavatar` imports. MToon-like behavior must not constrain lilToon-compatible UNToon features.
- MToon / VRM input should be converted into equivalent UNToon v2 coefficients where possible. Temporary v1 MToon rendering regressions are acceptable during the migration when they unblock correct lilToon-compatible behavior.
- Toon materials use dedicated shader entries and pipelines, not a Simple shader fallback.
- MASK discard uses alpha only.
- VRM0 outline width is converted to renderer meters by scaling `_OutlineWidth` by `0.01`.
- Eye, iris, pupil, highlight, and equivalent localized material names may be relaxed from MASK to Opaque when needed to avoid invisible eyes.
- Authored / Override controls for outline, rim, matcap, and related effects are profile controls, not the material model itself. Their v1 behavior was provisional and may be redesigned around per-material UNToon state plus explicit profile overrides.

## Consequences

VRM rendering quality remains important, but v2 quality work should first make UNToon a lilToon-compatible material path that can also represent MToon. Simple/base-color rendering remains only as a diagnostic mode.
