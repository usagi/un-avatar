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
4. Choose `Wardrobe` for a full wardrobe-capable `.unavatar`, or `Current to Base Only` to write the current scene state as Base while keeping captured wardrobe settings untouched.
5. For `Wardrobe`, set the avatar to the base appearance and click `Capture Current As Base`.
6. Change the scene to an outfit appearance, enter a set name, then click `Capture Current As New Set`.
7. Repeat or duplicate captured sets for additional outfits.
8. Select a set row to apply it and copy its name into `Set Name`.
9. Adjust the scene, edit `Set Name` if needed, then use the row `Update` button to overwrite that set.
10. Use `Base` or select a set row to restore a captured state in the Unity scene.
11. Use `Restore from .unavatar` to restore Base operations and captured wardrobe sets from an existing `.unavatar`.
12. Click `Export`. Validation runs automatically; the standalone `Validate` button is available only in Developer mode.

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

The exporter writes a built-in-writer GLB and patches the root glTF JSON with a `UN_avatar` extension. Modular Avatar is not baked into the mesh at export time; supported component data is carried as metadata for the runtime resolver.

`Wardrobe` is the v2 target mode. It stores all captured wardrobe sets in one `.unavatar` using authored capture diffs. `Current to Base Only` is a lightweight mode that writes the current scene state as Base and ignores captured wardrobe sets for that export. Full FX Animator evaluation is intentionally out of scope for this prototype.
