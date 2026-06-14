# v2 UI / GUI Operation Plan

This document fixes the v2 user-operation shape before release polish.
The source policy is [`v2-near-term-plan.md`](v2-near-term-plan.md), especially Output / preview and Renderer tray / Supervisor operation.

## Product Surfaces

### Supervisor

Supervisor is the management and authoring UI.

- First-run setup and profile creation.
- Profile editing while watching a Renderer.
- Multi-renderer launch, focus, restart, screenshot, diagnostics, and telemetry comparison.
- Cache warmup, desktop shortcut creation, Start Menu / taskbar launcher shortcut creation, and future Jump List registration.
- Migration from v1 user profiles into v2 schema.

Supervisor must not be required for mature daily operation once a profile is prepared.

### Renderer Tray

The Renderer tray icon is the stable runtime operation surface for each Renderer process.

- One tray icon per Renderer.
- Header identifies avatar / profile and pid.
- Output operations are local to that Renderer: Window Preview, Spout2 + Preview, Spout2 Only, and explicit Spout2 resolution presets.
- Window operations are local preview operations: focus, hide, always-on-top, and input passthrough where supported.
- UNPhysics, wardrobe, VRC menu actions, camera reset, Open Supervisor, and Quit this Renderer use existing normalized runtime commands.
- Open Supervisor carries the Renderer profile manifest when available. A cold Supervisor start should select that profile; an already-running Supervisor should receive an event and switch the Profiles view to the same profile without launching another Renderer.
- Tray refresh reads throttled runtime snapshots; it is not a per-frame UI.
- Wardrobe / VRC menu actions should remain reachable from the tray without an app-side fixed count cap. Very large menus may be long, but hiding available actions behind "more in Supervisor" breaks the tray's role as the reliable runtime operation surface.

Renderer tray commands must map to the same control path as Supervisor runtime buttons. If a command cannot be represented by the current `RendererControlEvent`, add the control event first instead of adding a tray-only side path.

### Launcher / Shortcuts

Shortcut and launcher UX is a bridge from prepared profiles to daily operation.

- Desktop shortcut launches a selected profile directly through the Renderer path.
- Start Menu / taskbar launcher shortcut uses a stable launcher identity and AppUserModelID.
- Renderer processes set the stable Renderer AppUserModelID themselves. Shortcut metadata alone is not the source of truth for taskbar grouping, tray-adjacent operation, or standalone Renderer launches.
- Launching the pinned app without a profile opens or focuses Supervisor.
- Launching a profile task starts or focuses the corresponding Renderer according to that profile's multiple-renderer policy when Supervisor is already running and can observe managed Renderer state. If Supervisor is not running, v2 launches a standalone Renderer and exits; focusing an already-running standalone Renderer is a future registry/discovery task. The single-instance handoff must route profile tasks through the same launch path instead of spawning a tray-only duplicate Renderer.
- Jump List tasks should expose all visible profile launch tasks and Open Supervisor. Do not impose an app-side fixed count cap.

### Supervisor Profile Workflow

The profile stage presents shortcut, launcher, cache warmup, launch, and live renderer actions as one v2 workflow.

- `Prepare Daily Use` groups cache warmup, desktop shortcut creation, and taskbar launcher setup. It answers "how do I make this profile pleasant to use next time?"
- `Check Now` launches the selected profile from Supervisor so the user can verify appearance, physics, output, and motion before turning it into daily operation.
- `Live Renderer` appears when the selected profile already has a Renderer. It exposes inspect, activate, and screenshot actions without making the user search the Renderers tab first.
- These actions are not developer utilities. They are the main reason to open Supervisor after a profile exists: prepare, verify, and hand the profile to direct Renderer / tray operation.
- Running Renderer controls in Supervisor may group or scroll wardrobe / VRC menu candidates, but must not impose an app-side fixed candidate cap. If an action is available from the normalized runtime status, the user should be able to invoke it without switching surfaces.

### `.unavatar` Asset Review

The `.unavatar` rights / asset review dialog is part of profile creation, not a decorative metadata page.

- Counts and previews must be read from actual `.unavatar` metadata. Do not show fake wardrobe, dynamics, contact, Modular Avatar, or preview data.
- Summary counts must accept both direct arrays and explicit count fields. A valid `.unavatar` that stores `dynamics.groupCount`, `contacts.contactCount`, `wardrobe.setCount`, or equivalent object summaries must not regress to `0` just because the full arrays are omitted from the review dialog metadata path.
- `wardrobe.sets[].previewImages[]` are the preferred source for wardrobe-specific sample views. If a package only has root-level `previewImages`, `sampleScreenshots`, `screenshots`, or `previews`, the dialog should still show all available sample views instead of falling back to a fake placeholder or a single image.
- Preview image `width` / `height` metadata should be preserved when available so profile-icon crop masks can match the saved square crop instead of guessing from the dialog frame.
- Metadata reads must not pull the whole GLB BIN payload. The current contract is a bounded JSON chunk, or bounded JSON glTF file fallback, plus referenced preview bufferView ranges only; data URI and external preview image reads are also size-capped before decode / full read. External preview URIs are resolved only as child paths under the avatar file directory, not absolute paths or `..` traversal. Wardrobe set discovery uses the same JSON-chunk path.
- VRM metadata review uses the same bounded image read policy for thumbnails and texture summary probes, so opening the review dialog cannot be forced into unbounded external image reads.
- Future large-wardrobe optimization: keep the set list available immediately, but lazy-load preview images for the selected set on demand. This avoids reading every sample image just to open the rights dialog while preserving wardrobe switching in the UI.

## v2 UI Rules

- Output resolution and preview window size are separate controls. No button may silently resize the preview when the user asked for an output mode.
- `Spout2 Only` means Spout2 enabled and local preview minimized. It does not change Spout2 resolution.
- UNPhysics / UNDynamics are the user-facing physics names. SpringBone / PhysBone are source-format or diagnostics terms.
- UNPhysics solver labels should trust the user while staying readable: `Standard (Verlet/PBD)` for the default Verlet integration plus PBD-style constraint projection path, and `Extended (XPBD)` for compliance / iteration based tuning. Do not describe source-authored model values as an authored solver choice.
- Physics group adjustments are template-backed overrides over model-load resolved groups. v2 may ship the built-in Hair / Ears / Tail / Cloth / Accessory / Other templates, but the target UI model is an arbitrary override list with editable partial-match keywords and parameters. Template add actions such as standard base, animal ears/tail, cloth, and body/skin can seed that list without making model-specific hacks.
- Material authored values stay authored. Supervisor profile UI should not provide broad v1-style controls that push one Outline / Rim / MatCap / Specular / AO value into every material.
- Screen / viewer effects may be profile controls: Silhouette Outline, Bloom, SSAO, contact shadow, color grading, background, capture / output policy.
- Runtime status labels should describe user-visible operation. Prefer `Live` / `No response` style wording over transport-centric `connected` / `disconnected` in Supervisor UI and diagnostics previews.
- Runtime UI should show only operations that can work now, or clearly mark unavailable backend support. Do not advertise headless output until the renderer architecture supports it.

## Implementation Order

1. Keep Renderer tray output behavior consistent with Supervisor runtime output controls.
2. Fill missing tray operations only through existing normalized renderer control events.
3. Keep the Supervisor profile workflow visually coherent as shortcut, launcher, cache warmup, and live renderer actions evolve.
4. Remove development-only compatibility UI labels after v1-to-v2 user migration remains covered.
5. Add diagnostics that compare Supervisor runtime buttons and Renderer tray behavior for the same output and UNPhysics state.

## Non-goals For v2 First Release

- True headless rendering without a native preview surface.
- Full external VRC menu UI parity inside Supervisor.
- Full Animator graph editing.
- VRC Constraints solver integration.
- Per-material authoring UI for every UNToon parameter.
- FEM / SPH physics solvers and UNPhysics worker-thread pose-buffer architecture.
