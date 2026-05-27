# ADR 0001: Runtime Coordinate System

## Status

Accepted.

## Context

UN Avatar must load VRM0, VRM1, glTF, and live motion sources without letting each format's coordinate conventions leak into renderer behavior. Earlier debugging showed that implicit flips, root rotations, and VMC conversion rules can stack in confusing ways.

## Decision

UN Avatar runtime display space is fixed to **right-handed / Y-up / +Z-front**.

- The renderer camera starts on the +Z side and looks toward the avatar.
- Model import normalizes source-specific basis differences into this display space.
- VMC input is converted according to the target VRM flavor before applying Humanoid pose.
- Rest pose is preserved when applying Humanoid bone rotations.

## Consequences

All loaders, motion adapters, renderer defaults, screenshots, and tests use the same front direction. Format-specific corrections belong at import or motion adapter boundaries, not in draw code.
