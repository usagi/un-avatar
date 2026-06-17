# UN Avatar v2 Input Bindings

This document fixes the v2 input binding design for runtime actions that must work while only Renderer is running.

## Policy

- Renderer is the runtime input owner. Bindings must work without Supervisor.
- Supervisor is the profile editing and learn UI owner.
- Renderer tray is the visible runtime operation surface. If a binding exists for a tray action, the tray label should show it, for example `Field Drape (F12)`.
- Global input bindings are accelerators for the same runtime actions exposed by tray/IPC. They are not a separate behavior path.
- Bindings are profile settings. They are read from the Renderer launch manifest.
- Wardrobe set changes and UNAnimator actions use the same binding kinds.

## Initial Binding Kinds

### Keyboard

`keyboard` bindings use Windows global hotkeys on Windows. They do not require the Renderer preview window to be focused.

Examples:

```toml
[[wardrobe.bindings]]
set_id = "field_drape"
kind = "keyboard"
binding = "F12"

[[wardrobe.bindings]]
set_id = ""
kind = "keyboard"
binding = "Ctrl+Alt+B"
```

### MIDI Note

`midi_note` bindings listen to MIDI Note state in Renderer.

- Note On with velocity > 0 is DOWN.
- Note Off is UP.
- Note On with velocity 0 is treated as UP.
- Toggle and one-shot actions fire on the DOWN edge only.
- Velocity is not bound to action value in the v2 initial implementation.
- Control Change is not part of the v2 initial implementation.

Examples:

```toml
[[wardrobe.bindings]]
set_id = "field_drape"
kind = "midi_note"
device = "Launchkey Mini"
channel = 1
note = 60
```

Device matching is name based for the initial implementation. A later implementation may add a stable device fingerprint if Windows MIDI device identity proves too ambiguous.

## Manifest Compatibility

`[wardrobe] shortcuts = [...]` remains accepted as legacy keyboard-only shorthand:

```toml
[wardrobe]
shortcuts = [
  { set_id = "field_drape", shortcut = "F12" },
]
```

New profile writes should use `[[wardrobe.bindings]]`.

UNAnimator actions use `[[animator.bindings]]`:

```toml
[[animator.actions]]
id = "expression:angry"
mode = "toggle"
value = 1.0
transition_curve = "ease_out"
transition_ms = 250

[[animator.bindings]]
action_id = "expression:angry"
kind = "keyboard"
binding = "F8"

[[animator.bindings]]
action_id = "expression:angry"
kind = "midi_note"
device = "Launchkey Mini"
channel = 1
note = 61
```

For `toggle`, Renderer owns the current on/off state while the process is running. If the action is active and it has a runtime parameter, Renderer writes the inactive parameter value. Expression-only profile actions are restored by clearing their expression override.

`transition_curve` and `transition_ms` are optional numeric-action easing fields. They apply only to range-like UNAnimator effects such as expression weights and runtime parameters. Valid curves are:

- `none`
- `linear`
- `ease_in`
- `ease_out`
- `ease_in_out`

Wardrobe set changes do not use this transition model. Wardrobe needs a separate transit system because it involves scoped asset loading, visibility/material swaps, physics reset, and presentation timing.

## Deferred

- Mouse button global bindings.
- MIDI Control Change and continuous value mapping.
- Velocity-to-value mapping.
- Per-binding conflict UI.
- Wardrobe set transit system.
