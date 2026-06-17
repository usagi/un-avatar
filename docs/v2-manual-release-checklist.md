# UN Avatar v2 Manual Release Checklist

This checklist is for the final v2 beta candidate pass after automated checks have already passed. Keep screenshots, renderer logs, diagnostics bundles, and short notes beside the release artifact being tested.

## Candidate Build

- Date / operator: 2026-06-17 / Codex local release-prep
- Git commit: `12062bd2cf67`
- Version: `2.0.0-beta-2`
- Portable zip: `release-packages/un-avatar-2.0.0-beta-2.zip`
- Portable zip SHA-256: `57762618a076a7a5b90cd30b07197b637c33fa9b4f4af81b751aafcc7c0ff96d`
- VCC package zip: `target/unity/vcc/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip`
- VCC package SHA-256: `7a6a8578387cf5e7536f3746bb9f1446837171eb26fe20518b960c4305f81ce1`
- `cargo xtask ci` result: passed
- `cargo xtask release-audit --version <version>` result: passed for `2.0.0-beta-2`
- `release-audit` confirms release notes hashes: yes
- `cargo xtask package-render-smoke --manifest target/tmp/mizuki-split-data-bc7-unorm.toml --wardrobe-set field_drape` result: passed; missing counts `0`, scoped missing groups `[]`
- `cargo xtask package-render-smoke --manifest target/tmp/mizuki-split-data-bc7-unorm.toml --wardrobe-set noble1` result: passed; missing counts `0`, scoped missing groups `[]`

This recorded package includes the post-candidate source fixes through the commit above. Refresh this section again if any new source fix lands before publishing.

## Clean Package Smoke

Use a clean unpack directory outside the repository.

1. Unzip `release-packages/un-avatar-<version>.zip`.
2. Start `un-avatar-supervisor.exe` from the unpacked directory.
3. Confirm the packaged `un-avatar-renderer.exe`, `Spout.dll`, `LICENSE`, `README.md`, `THIRD_PARTY_NOTICES.md`, `LICENSES/third-party-licenses.md`, and Unity Exporter package are present.
4. Confirm `cargo xtask release-audit --version <version>` still passes against the repository artifacts.

Evidence:

- Unpack path:
- Screenshot / notes:

## Supervisor Profile Workflow

Confirm the profile stage is one workflow, not scattered utilities.

- `.unavatar` metadata review opens with actual bounded metadata and real preview images.
- Profile icon selection from `.unavatar` sample image is on by default during new avatar acceptance and can be adjusted with crop / zoom / position controls.
- Cache readiness is visible as prepared / stale / missing before pressing launch or shortcut actions.
- Cache warmup completes and reports processed / compressed texture cache plus pipeline cache details when available.
- Desktop shortcut creation updates a direct profile launch shortcut.
- Taskbar launcher setup updates the profile launch list without model-specific naming.
- `Check Now` launches or focuses the profile Renderer.
- `Live Renderer` actions expose inspect / activate / screenshot when the Renderer is already running.

Evidence:

- Diagnostics bundle:
- Screenshot / notes:

## Output Modes

Use one live Renderer and check both Supervisor runtime controls and Renderer tray controls for the same profile.

- Window Preview: preview visible, Spout2 disabled, local preview restored if it had been minimized.
- Spout2 + Preview: Spout2 enabled, preview visible, Spout2 resolution unchanged unless an explicit resolution button is pressed.
- Spout2 Only: Spout2 enabled, preview minimized / hidden for local desktop use, rendering continues in OBS / Spout receiver.
- Startup progress overlay: local preview may show startup progress, but Spout2 receiver must not receive startup progress overlay frames before the avatar is ready.
- Returning from Spout2 Only to Window Preview restores preview before disabling Spout2, so the user is not left with Spout2 off and preview still minimized.
- 720p / 1080p buttons change Spout2 output resolution only; they do not implicitly restore or minimize preview.

Evidence:

- OBS / Spout receiver:
- Runtime status / screenshot:

## Renderer Tray

Confirm the Windows tray icon operates the running Renderer directly.

- Tray header identifies the profile / avatar and process.
- Window Preview, Spout2 + Preview, and Spout2 Only map to the same behavior as Supervisor runtime buttons.
- Wardrobe menu lists all normalized wardrobe candidates needed for the test model, with no fixed app-side cap.
- UNAnimator lists non-wardrobe menu action candidates, uses resolved menu path labels where available, falls back to expression-menu runtime actions when normalized candidates are absent, and toggles active parameter actions back to `0`.
- Open Supervisor focuses or starts Supervisor and selects the relevant profile when possible.
- Quit this Renderer stops only the Renderer represented by that tray icon.

Evidence:

- Tray screenshots / notes:
- Renderer pid:

## `mizuki-split` Wardrobe Hot Switch

Start from both representative sets and switch without restarting the Renderer.

Recommended commands for direct Renderer checks:

```powershell
cargo xtask run-renderer --release --profile mizuki-split --wardrobe-set noble1
cargo xtask run-renderer --release --profile mizuki-split --wardrobe-set field_drape
```

For each start set, switch through Base and representative noble / field sets from Supervisor and Renderer tray:

- No black materials.
- No missing hair / clothing.
- No stale inactive outfit meshes.
- No rest-pose corruption after return: neck, hands, and mirrored / Z-axis-looking transforms remain correct after switching.
- Wardrobe blendshape operation is reflected.
- UNDynamics still moves after switching.
- Wardrobe-changing billboard is visible in OBS / Spout2 during the switch, while startup progress overlay remains preview-local.
- Runtime status reports no scoped missing active groups.
- Hot-switch refresh metrics are present in diagnostics / status: `active_wardrobe_set`,
  `wardrobe_asset_upload.active_asset_groups`, scoped resident counts, pending upload counts, and last scoped load / unload counts.

Evidence:

- Start set:
- Switch sequence:
- Screenshot / log / diagnostics:

## Motion / Physics Sanity

- `model1` VRM: UNMF/Z, Perfect Sync, hands / feet, UNPhysics.
- Lightweight VRC / `.unavatar` model: PhysBone-derived UNDynamics, Perfect Sync / ShapeKey.
- `mizuki-split`: UNDynamics motion survives wardrobe switches and cache-warmed startup.
- Unsupported areas are visible as diagnostics, not silent parity claims: full Animator graph, dynamic reactive mesh gating, PhysBone suffix value emission, VRC Constraints solver integration.

Evidence:

- Motion source:
- Diagnostics / notes:

## Migration

Use copied v1 profile files, not the only local production profile.

- Legacy root keys load: `aa`, `icon_path`, `transparent`, `input_passthrough`, `decorations`, `vmc_address`, `vmc_port`, `spout`, `spring_bones`.
- Saving, duplicating, thumbnail update, and avatar path update normalize to v2 sections.
- `[physics.dynamics.solver]` is canonical after solver/category edits.
- Legacy `[physics.spring_bone]`, solver aliases, and old texture compression names remain read-only compatibility inputs.

Evidence:

- Before manifest:
- After manifest:

## Release Text

Before publishing, make sure the public release text says:

- Portable zip is the Windows distribution source of truth for v2.
- Installer, auto-update, and Authenticode signing are not part of v2.
- Hashes for portable zip and VCC package are listed.
- Known limitations are explicit and match `docs/v2-release-notes-draft.md`.
