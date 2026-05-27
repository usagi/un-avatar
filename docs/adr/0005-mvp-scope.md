# ADR 0005: Runtime MVP Scope

## Status

Accepted.

## Context

UN Avatar has a broad long-term scope. The current working implementation has already formed a valuable vertical slice around VRM, VMC, MToon, wgpu, and Spout2.

## Decision

The first runtime MVP is the VRM / VMC / MToon / wgpu / Spout2 slice.

MVP includes VRM0/VRM1 loading, rest-pose-aware Humanoid retargeting, VMC Marionette input, MToon rendering, +Z-front orbit camera, Windows Spout2 output, and diagnostics for material/morph/VMC/skinning issues.

MVP excludes full Supervisor GUI, FBX/USD/`.blend`/VRC bridge completion, full video recording, GPU morph/physics rewrite, and Spout2 GPU texture path as a requirement.

## Consequences

New work should first protect and verify this vertical slice. Broader architecture remains valid, but should not delay MVP stabilization.
