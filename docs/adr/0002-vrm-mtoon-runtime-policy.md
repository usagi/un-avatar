# ADR 0002: VRM MToon Runtime Policy

## Status

Accepted.

## Context

VRM avatars often use MToon as their visual identity. Treating VRM materials as Simple, Unlit, or generic Lambert loses important shade, rim, matcap, alpha, and outline behavior.

## Decision

VRM materials default to MToonLike rendering in UN Avatar.

- VRM0 `materialProperties` and VRM1 `VRMC_materials_mtoon` are parsed into runtime MToon parameters.
- MToon uses dedicated shader entries and pipelines, not a Simple shader fallback.
- MASK discard uses alpha only.
- VRM0 outline width is converted to renderer meters by scaling `_OutlineWidth` by `0.01`.
- Eye, iris, pupil, highlight, and equivalent localized material names may be relaxed from MASK to Opaque when needed to avoid invisible eyes.

## Consequences

VRM rendering quality is part of the MVP acceptance surface. Simple/base-color rendering remains only as a diagnostic mode.
