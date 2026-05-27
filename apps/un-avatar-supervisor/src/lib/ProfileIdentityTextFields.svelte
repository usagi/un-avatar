<script lang="ts">
  import { _ } from "svelte-i18n";
  import type { IdentitySetting, ProfileSettingValue } from "./profileTypes";

  export let setting: Pick<IdentitySetting, "group" | "notes">;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

<label data-hint={$_("profiles.hints.identity.group")}
  >{$_("profiles.editor.group")}<input
    value={setting.group}
    onchange={(event) =>
      onUpdateSettingValue(
        "profile.group",
        (event.currentTarget as HTMLInputElement).value,
      )}
  /></label
>
<label class="notes-field" data-hint={$_("profiles.hints.identity.notes")}
  >{$_("profiles.editor.notes")}<textarea
    value={setting.notes ?? ""}
    onchange={(event) =>
      onUpdateSettingValue(
        "profile.notes",
        (event.currentTarget as HTMLTextAreaElement).value,
      )}
  ></textarea></label
>
