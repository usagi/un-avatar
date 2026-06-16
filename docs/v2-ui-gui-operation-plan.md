# v2 UI / GUI Operation Plan

This document fixes the v2 user-operation shape before release polish.
The source policy is [`v2-near-term-plan.md`](v2-near-term-plan.md), especially Output / preview and Renderer tray / Supervisor operation.

## Product Surfaces

### Supervisor

Supervisor is the management and authoring UI.

- First-run setup and profile creation.
- Profile editing while watching a Renderer. Durable default values belong to the Profiles workflow, not the Renderers tab.
- Multi-renderer launch, focus, restart, screenshot, diagnostics, and telemetry comparison.
- Cache warmup, desktop shortcut creation, Start Menu / taskbar launcher shortcut creation, and future Jump List registration.
- Migration from v1 user profiles into v2 schema.

Supervisor must not be required for mature daily operation once a profile is prepared.

### Supervisor Renderers Tab

The Renderers tab is a live-operation and observation surface for running Renderer processes.

- It should answer "what is this Renderer doing right now?" and "change this running Renderer now."
- It may provide explicit `Save to profile` / `Restore from profile` actions for live state that users naturally tune while watching the result: output mode, preview window geometry, camera, and future visual defaults.
- It should not become the primary editor for profile startup policy, prepared daily-use state, shortcut setup, cache warmup, migration, or avatar-source paths. Those belong to the Profiles workflow.
- Runtime diagnostics, action lists, and telemetry are allowed here because they are process-scoped and often meaningless without a running Renderer.
- If a control edits durable profile data, the UI must say so directly. Silent persistence from a live control is not allowed.

### Renderer Tray

The Renderer tray icon is the stable runtime operation surface for each Renderer process.

- One tray icon per Renderer.
- Header identifies avatar / profile and pid.
- Output operations are local to that Renderer: Window Preview, Spout2 + Preview, Spout2 Only, and explicit Spout2 resolution presets.
- Window operations are local preview operations: focus, hide, always-on-top, and input passthrough where supported.
- UNPhysics, wardrobe, UNAnimator actions, camera reset, Open Supervisor, and Quit this Renderer use existing normalized runtime commands.
- Open Supervisor carries the Renderer profile manifest when available. A cold Supervisor start should select that profile; an already-running Supervisor should receive an event and switch the Profiles view to the same profile without launching another Renderer.
- Profile sync actions may be exposed here for standalone Renderer operation: save current output mode / preview window state to the profile, and restore those values from the profile. These must use the same manifest fields and control events as Supervisor.
- Tray refresh reads throttled runtime snapshots; it is not a per-frame UI.
- Wardrobe / UNAnimator actions should remain reachable from the tray without an app-side fixed count cap. Very large imported menus may be long, but hiding available actions behind "more in Supervisor" breaks the tray's role as the reliable runtime operation surface.
- Wardrobe candidates and non-wardrobe Animator / expression-menu action candidates are both sourced from normalized runtime status. Resolved source menu paths should be preserved in runtime status and used for tray labels when available. The tray may group action kinds separately, but it must not invent model-specific entries or require Supervisor to stay resident for basic menu operation.
- The native tray icon and context menu should run on a dedicated UI worker thread when needed so modal menu tracking cannot stall Renderer drawing or Spout2 publishing.
- Renderer tray labels are localized independently from Supervisor so standalone Renderer operation remains readable without requiring Supervisor to be running.

Renderer tray commands must map to the same control path as Supervisor runtime buttons. If a command cannot be represented by the current `RendererControlEvent`, add the control event first instead of adding a tray-only side path.

Output mode controls must be semantic operations, not a bare Spout2 toggle. `Window Preview`, `Spout2 + Preview`, and `Spout2 Only` each update the necessary Spout2 and window-minimized state together so users cannot accidentally hide the preview by disabling Spout2 while the window remains minimized.

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
- Cache readiness is a visible profile-stage state card, not only a tooltip or secondary button label. Users must be able to tell whether a profile is prepared, stale, or missing warmup before deciding between launch, shortcut, and launcher actions.
- `Check Now` launches the selected profile from Supervisor so the user can verify appearance, physics, output, and motion before turning it into daily operation.
- `Live Renderer` appears when the selected profile already has a Renderer. It exposes inspect, activate, and screenshot actions without making the user search the Renderers tab first.
- These actions are not developer utilities. They are the main reason to open Supervisor after a profile exists: prepare, verify, and hand the profile to direct Renderer / tray operation.
- Running Renderer controls in Supervisor expose wardrobe switching in Controls and expression / Animator actions in the UNAnimator tab. They may group or scroll candidates, but must not impose an app-side fixed candidate cap. If an action is available from the normalized runtime status and is not a known non-user runtime control such as VRCFT metadata, the user should be able to invoke it without switching surfaces.

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
- `VRC Menu` is not a primary user-facing surface name in v2. Source VRC menu metadata is an import path into normalized UNAnimator actions; raw VRC menu diagnostics may remain in diagnostics until the exporter/runtime normalization is complete.

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
