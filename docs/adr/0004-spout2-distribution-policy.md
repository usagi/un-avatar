# ADR 0004: Spout2 Distribution Policy

## Status

Accepted.

## Context

Spout2 is the practical Windows path for OBS-facing low-latency texture output. The selected Rust integration dynamically links to `Spout.dll`, so DLL discovery must happen before renderer process startup.

## Decision

Spout2 is included in standard Windows distribution builds.

- `cargo xtask spout2` fetches and builds Spout2.
- `cargo xtask package` stages `Spout.dll` in the package root.
- BSD-2-Clause license notices are included under `LICENSES/`.
- Build provenance is written as `LICENSES/spout2-build-info.txt`.
- The Supervisor prepends the package root to the renderer process `PATH` before startup.

## Consequences

User-selected external DLL directories are a development/emergency fallback, not the default product path. Runtime DLL selection after process startup is not reliable for `spout-rs`.
