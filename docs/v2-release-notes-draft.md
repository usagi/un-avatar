# U.N. Avatar 2.0.0 Release Notes Draft

U.N. Avatar 2.0.0 is the v2 release focused on bringing VRC / Unity avatar workflows into the standalone U.N. Avatar Renderer while keeping the existing VRM workflow lightweight.

## Highlights

- VRM(0/1), glTF, and `.unavatar` avatars can be rendered from Supervisor-managed profiles.
- VRC / Unity avatars can be exported from Unity as `.unavatar` packages using U.N. Avatar Exporter.
- v2 adds lilToon, PhysBone-derived dynamics, Modular Avatar-derived Wardrobe operations, and UNAnimator runtime actions.
- Wardrobe sets can be switched from Renderer tray or Supervisor while the Renderer is running.
- Output modes include Window Preview, transparent / click-through windows, screenshots, and Spout2 Sender output.

## How To Use

- For VRM: start `un-avatar-supervisor.exe`, create a profile, select a `.vrm`, then launch the Renderer.
- For VRC / Unity avatars: install U.N. Avatar Exporter in Unity, export the avatar as `.unavatar`, select that `.unavatar` in a Supervisor profile, then launch the Renderer.
- For Wardrobe: capture `1. Base -> 2. Wardrobe Sets -> 3. Export` in the Unity Exporter, then switch Wardrobe sets from Renderer tray or Supervisor.
- For motion input: send UNMF/Z from U.N. Motion, or use any app that can send VMC/UDP.

See `README.md` and `docs/v2-getting-started.md` for the user-facing setup flow.

## Known Limitations

- U.N. Avatar v2 is not a complete VRChat client or full VRC SDK runtime implementation.
- Full Animator graph style frame evaluation is not implemented.
- Dynamic reactive mesh gating is not implemented; static resolver-compatible mesh operations are diagnostics / static import scope only.
- PhysBone interaction suffix value emission such as `_IsGrabbed` / `_IsPosed` is not implemented.
- Direct grabbing / posing evaluator and VRC Constraints solver integration are not implemented.
- Some Modular Avatar and lilToon behaviors are approximated or diagnostics-only when the authored behavior depends on runtime systems outside the v2 renderer scope.
- Contacts parameter emission is opt-in and diagnostics-driven; it is not enabled silently by default.
- Installer, auto-update, and Authenticode signing are outside v2. The Windows portable zip is the v2 distribution source of truth.

## Downloads

- Portable Windows zip: `release-packages/un-avatar-2.0.0.zip`
- zip SHA-256: `6e703cdc73f2aa807e5bd3a43f00e40cc012a7fe436e2e93f94944c8276c5a41`
- Portable zip checksum sidecar: `release-packages/un-avatar-2.0.0.zip.sha256.txt`
- Unity Exporter VCC package: `target/unity/vcc/network.usagi.un-avatar.unity-exporter-2.0.0.zip`
- VCC zip SHA-256: `f6c6e7e93814c4cc947cac66a3784d7feff6d888db9e55bf39978249624c71af`

Attach both the portable zip and the Unity Exporter VCC package to the same GitHub Release. Commit the generated `docs/vcc/index.json` so VCC can discover the Unity Exporter package.

## Verification

The following commands passed in the release-prep workspace for the recorded candidate package:

```sh
npm run check
cargo xtask fmt
cargo xtask release-guard
cargo xtask unity-exporter-vcc --version 2.0.0
cargo xtask release-package --version 2.0.0
cargo xtask release-audit --version 2.0.0
cargo xtask package-render-smoke --manifest target/tmp/mizuki-split-data-bc7-unorm.toml --wardrobe-set field_drape
cargo xtask package-render-smoke --manifest target/tmp/mizuki-split-data-bc7-unorm.toml --wardrobe-set noble1
cargo xtask ci
git diff --check
```

The release tooling verifies required portable zip entries, README-linked docs, Spout2 payload entries unless explicitly skipped, packaged Renderer startup smoke, VCC package entries, checksum sidecar consistency, VCC listing name / version / URL suffix / `zipSHA256`, the hashes recorded in this release-notes draft, and the Candidate Build artifact paths / hashes recorded in the manual release checklist.

Manual release evidence is tracked in `docs/v2-manual-release-checklist.md`. The checklist covers clean unzip smoke, Supervisor profile workflow, Renderer tray, Spout2 modes, Wardrobe hot switching, UNAnimator actions, migration, and final clean-machine checks.

State the unsupported v2 areas explicitly in the published release text instead of implying silent VRChat parity.
