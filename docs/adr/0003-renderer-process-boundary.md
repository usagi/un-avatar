# ADR 0003: Renderer Process Boundary

## Status

Accepted.

## Context

The renderer owns wgpu surfaces, GPU resources, live avatar state, Spout2 output, and external model data. These areas can fail independently of the GUI and may need multiple simultaneous instances.

## Decision

UN Avatar uses a Supervisor GUI process and one or more renderer child processes.

- The GUI/Supervisor launches, stops, restarts, and monitors renderer processes.
- Renderer processes do not depend on Tauri or GUI APIs.
- Renderer startup is described by a manifest plus future IPC commands.
- The first GUI MVP is a compact renderer launcher/control surface, not a full editor.

## Consequences

Renderer configuration must be serializable and process-friendly. Runtime state and high-frequency rendering remain outside the GUI process.
