# UNAvatar v2 AudioLink Profile and Runtime Spec

## Goal

UNAvatar v2 supports lilToon AudioLink-compatible materials without making audio capture a baseline renderer cost.

The default profile behavior is `source = "none"`. In this mode no OS audio device is opened and no CPU FFT worker runs. lilToon-compatible fallback waveforms remain shader-side, matching the upstream `fd.audioLinkValue = 1.0` default and `_AudioLinkDefaultValue` fallback behavior.

## Profile Schema

Persistent profiles and renderer-instance manifests use the same table:

```toml
[audio_link]
source = "none" # "none" | "input_device"
input_device_id = ""
input_device_name_hint = ""
```

Fields:

- `source = "none"`: do not start audio capture. Shader fallback only.
- `source = "input_device"`: allow an OS input device to feed AudioLink, but start capture lazily.
- `input_device_id`: opaque device id returned by the audio backend. It may change when the OS or driver changes.
- `input_device_name_hint`: human-readable fallback used when the exact id is not found.

Invalid or missing `source` values are treated as `none` by profile readers. Renderer manifests reject invalid enum values at deserialization boundaries when possible.

## Backend Plan

Initial implementation:

- Device enumeration and input capture: `cpal`.
- FFT: `realfft` or `rustfft`.
- Worker handoff: existing channel infrastructure first; introduce a ring buffer only if callback pressure requires it.
- GPU input: generate a small VRChat AudioLink-compatible `_AudioTexture`-style texture and bind it to the lilToon-like renderer path.

Windows follow-up:

- System loopback and application-level capture can be added behind a Windows-only `wasapi` backend.
- This is not part of the first implementation because most lilToon avatar wardrobes do not use AudioLink materials.

Deferred processing:

- gain
- normalization / AGC
- smoothing presets
- VST or plugin processing
- virtual mixer / app capture routing

## Lazy Runtime Policy

The audio worker must not start just because the profile has `source = "input_device"`.

Start `cpal` capture and FFT only when all of these are true:

- active profile has `audio_link.source = "input_device"`;
- currently visible wardrobe state contains at least one visible primitive using a lilToon-like material;
- that material has `_UseAudioLink > 0`;
- at least one target toggle is active: `_AudioLink2Main2nd`, `_AudioLink2Main3rd`, `_AudioLink2Emission`, `_AudioLink2EmissionGrad`, `_AudioLink2Emission2nd`, `_AudioLink2Emission2ndGrad`, or `_AudioLink2Vertex`.

Stop capture after the active visible material set no longer needs AudioLink. Use a short debounce window, initially 1-3 seconds, so wardrobe switching does not repeatedly open and close the audio device.

## Shader Contract

The shader path remains lilToon-source-compatible:

- `fd.audioLinkValue` equivalent is `1.0` when AudioLink is disabled.
- `_UseAudioLink` gates AudioLink value calculation.
- `_AudioLinkDefaultValue` fallback works without an external audio texture.
- AudioLink target toggles decide where the value is applied.

External audio texture support must layer on top of this behavior rather than replacing it.

## Non-Goals for v2 Initial AudioLink

- Poiyomi AudioLink compatibility.
- System/game audio capture by default.
- Always-on FFT.
- VST hosting.
- Exact VRChat AudioLink controller UI parity.
