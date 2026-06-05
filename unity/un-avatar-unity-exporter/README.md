# U.N. Avatar Unity Exporter

This is the v0.1 prototype Unity Editor exporter for `.unavatar`.

## Requirements

- Unity 2022.3 LTS
- Optional: VRC SDK Avatars, Modular Avatar, NDMF, lilToon

The exporter includes a built-in minimal GLB writer. UnityGLTF is not required or used by this prototype.

## Usage

1. Add this package as a local package from `unity/un-avatar-unity-exporter`.
2. Open `Tools > U.N. Avatar > Export .unavatar`.
3. Select the avatar root.
4. Set the avatar to the base appearance and click `Capture Current As Base`.
5. Change the scene to an outfit appearance, enter a set name, then click `Capture Current As New Set`.
6. Repeat or duplicate captured sets for additional outfits.
7. Select a set row to apply it and copy its name into `Set Name`.
8. Adjust the scene, edit `Set Name` if needed, then use the row `Update` button to overwrite that set.
9. Use `Save Draft` to preserve the capture session as JSON.
10. Use `Base` or select a set row to restore a captured state in the Unity scene.
11. Use `Import From .unavatar` to restore Base operations and captured wardrobe sets from an existing `.unavatar`.
12. Run `Validate`, then `Export`.

## Developer Mode

Developer mode is off by default. Turn it on at the bottom of the exporter
window only when diagnostic output or release-gated benchmark tools are needed.

PNG encoder benchmarking is also off by default. Enable `PNG Encoder Benchmark`
inside Developer mode to activate `Tools > U.N. Avatar > Benchmark PNG Encoders`
and the matching in-window run button. The benchmark writes
`un-avatar-png-encoder-benchmark.csv` under the system temp directory. Each row
also decodes the generated PNG and checks that the resulting RGBA pixels match
the benchmark input exactly.

The encoder policy is documented in
`docs/unity-exporter-png-encoding.md`. The native fpng path is only for generated
RAW RGBA that must become PNG; source-backed PNG/JPEG assets are preserved and
not re-encoded.

The exporter writes:

- `avatar.unavatar`
- `avatar.unavatar.report.json`

## Prototype Scope

The first implementation exports a built-in-writer GLB and patches the root glTF JSON with a `UN_avatar` extension. Modular Avatar is not reimplemented. If Modular Avatar is present, the exporter automatically clones the selected avatar and calls the Modular Avatar / NDMF bake entrypoint on the clone before GLB export.

`All Wardrobe Sets In One .unavatar` is the target mode. v0.1 includes `Capture Base` / `Capture Wardrobe Set` diff capture for GameObject active state, Unity Scene Visibility, renderer enabled state, and SkinnedMeshRenderer blendshape weights. Simple candidates from Modular Avatar Object Toggle and Modular Avatar Menu Item metadata are kept as fallback hints. Full FX Animator evaluation is intentionally out of scope for this prototype.
