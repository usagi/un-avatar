# U.N. Avatar v2 Release Notes Draft

This is the working release-note source for the first v2 beta. It records what can be said from current automated / windowless evidence and what still needs real GUI confirmation before publishing.

## Release Scope

- Portable Windows zip is the v2 distribution source of truth. Installer and Authenticode signing are not part of v2.
- Supervisor Console manages profiles, launch actions, renderer controls, profile icon crop, cache warmup, diagnostics, and packaged profile workflows.
- Renderer supports VRM / glTF / `.unavatar`, UNMF/Z and VMC/UDP motion input, GPU skinning / morph, UNPhysics / UNDynamics, UNToon material semantics, window preview, screenshots, and Spout2 output.
- `.unavatar` v2 covers Wardrobe set import, runtime wardrobe hot switch, VRC Expression Menu derived runtime actions, Modular Avatar derived visibility / material / dynamics operations where they can be lowered to renderer actions, and scoped asset residency for heavy wardrobe models.
- Renderer tray exposes runtime output modes plus Wardrobe and UNAnimator action surfaces from normalized renderer status. Supervisor uses the same renderer control path for matching actions.
- Unity Exporter can be packaged as a VCC / VPM package from this repository and published beside the portable zip release asset.

## Current Verification Evidence

These commands passed in the release-prep workspace for the recorded candidate package:

```sh
npm run check
cargo xtask fmt
cargo test -p xtask -- --nocapture
cargo test -p un-avatar-render-wgpu renderer_tray -- --nocapture
cargo xtask unity-exporter-vcc --version 2.0.0-beta-2
cargo xtask release-package --version 2.0.0-beta-2
cargo xtask release-audit --version 2.0.0-beta-2
cargo xtask package-render-smoke --manifest target/tmp/mizuki-split-data-bc7-unorm.toml --wardrobe-set field_drape
cargo xtask package-render-smoke --manifest target/tmp/mizuki-split-data-bc7-unorm.toml --wardrobe-set noble1
cargo xtask ci
git diff --check
```

Generated artifact evidence from the latest successful local packaging run:

- `release-packages/un-avatar-2.0.0-beta-2.zip`
- zip SHA-256: `4adf65a8a8611bb931a2c1a45bb3b6c1fa58f689a8102c71a2fa3a005a7026fc`
- sidecar: `release-packages/un-avatar-2.0.0-beta-2.zip.sha256.txt`
- VCC package: `target/unity/vcc/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip`
- VCC zip SHA-256: `7a6a8578387cf5e7536f3746bb9f1446837171eb26fe20518b960c4305f81ce1`

The release tooling verifies required portable zip entries, Spout2 payload entries unless explicitly skipped, packaged renderer startup smoke, VCC package entries, checksum sidecar consistency, VCC listing name / version / URL suffix / `zipSHA256`, the hashes recorded in this release-notes draft, and the Candidate Build artifact paths / hashes recorded in the manual release checklist.

The recorded package includes post-candidate fixes for wardrobe transition rest-pose preparation, startup progress vs wardrobe-changing billboard separation for Spout2 output, startup progress shader separation from wardrobe transition art, startup scene-state naming cleanup, Renderer tray UNAnimator naming cleanup, extended function-key binding regression coverage, and Supervisor UNAnimator list truncation disclosure.

## Known Limitations

- U.N. Avatar v2 is not a complete VRChat client or full Animator Controller emulator.
- Full Animator graph style frame evaluation is not implemented.
- Dynamic reactive mesh gating is not implemented; static resolver-compatible mesh operations are diagnostics / static import scope only.
- PhysBone interaction suffix value emission such as `_IsGrabbed` / `_IsPosed` is not implemented.
- Direct grabbing / posing evaluator and VRC Constraints solver integration are not implemented.
- Some Modular Avatar and lilToon behaviors are approximated or diagnostics-only when the authored behavior depends on runtime systems outside the v2 renderer scope.
- Contacts parameter emission is opt-in and diagnostics-driven; it is not enabled silently by default.
- Installer, auto-update, and Authenticode signing are outside v2.

## Manual Release Checks Still Required

- Real GUI `mizuki-split` wardrobe hot switch from `noble1` and `field_drape` to Base / noble sets, including no black materials, no missing hair / clothing, and UNDynamics still moving after switching.
- Spout2 Only, Spout2 + Preview, Window Preview, minimized preview, preview restore order, OBS / Spout receiver behavior, and startup progress overlay not being published to Spout2 while wardrobe-changing billboard still is.
- Renderer tray operation from the actual Windows tray UI: output modes, Wardrobe, UNAnimator action toggles, Open Supervisor, and Quit this Renderer.
- Supervisor `.unavatar` review flow with bounded metadata and real preview imagery.
- Supervisor profile icon / cache warmup / shortcut / launcher workflow as one user-facing profile preparation path.
- v1 to v2 migration through actual save / duplicate / thumbnail / path updates, confirming legacy root keys normalize to v2 sections.
- Final release package download / unzip check on a clean machine or clean Windows user profile.

## Publishing Notes

- Use release tags and titles without a `v` prefix, for example `2.0.0-beta-2`.
- Attach both the portable zip and the VCC package zip to the same GitHub Release.
- Commit the generated `docs/vcc/index.json` so VCC can discover the Unity Exporter package.
- Publish the portable zip checksum sidecar and include the SHA-256 values in the release text.
- State the unsupported v2 areas explicitly instead of implying silent VRChat parity.
