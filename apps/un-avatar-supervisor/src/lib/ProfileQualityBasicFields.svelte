<script lang="ts">
  import { _ } from "svelte-i18n";
  import ProfileSelectField from "./ProfileSelectField.svelte";
  import type { ProfileSettingValue, QualitySetting } from "./profileTypes";
  import { AA_MODE_OPTIONS, RENDER_BACKEND_OPTIONS } from "./qualityOptions";

  export let setting: Pick<QualitySetting, "aa" | "render_backend">;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

<ProfileSelectField
  label={$_("profiles.editor.anti_aliasing")}
  hint={$_("profiles.hints.quality.anti_aliasing")}
  value={setting.aa}
  disabled={busy}
  options={AA_MODE_OPTIONS}
  onChange={(value) => onUpdateSettingValue("render_quality.aa", value)}
/>
<ProfileSelectField
  label={$_("profiles.editor.render_backend")}
  hint={$_("profiles.hints.quality.render_backend")}
  value={setting.render_backend}
  disabled={busy}
  options={RENDER_BACKEND_OPTIONS}
  onChange={(value) =>
    onUpdateSettingValue("render_quality.render_backend", value)}
/>
