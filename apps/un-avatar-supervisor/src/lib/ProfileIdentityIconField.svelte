<script lang="ts">
  import { _ } from "svelte-i18n";
  import { FolderOpen } from "lucide-svelte";
  import type { IdentitySetting, ProfileSettingValue } from "./profileTypes";

  export let setting: Pick<IdentitySetting, "icon_path" | "avatar_path">;
  export let busy = false;
  export let onBrowseIcon: () => void | Promise<void>;
  export let onApplyAvatarThumbnail: () => void | Promise<void>;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

<div class="path-field icon-path-field" data-hint={$_("profiles.hints.identity.icon")}
  ><span>{$_("profiles.editor.icon")}</span><input
    aria-label={$_("profiles.editor.icon")}
    value={setting.icon_path ?? ""}
    disabled={busy}
    placeholder={$_("profiles.editor.default_icon")}
    onchange={(event) =>
      onUpdateSettingValue(
        "icon_path",
        (event.currentTarget as HTMLInputElement).value,
      )}
  />
  <button
    type="button"
    class="field-button"
    disabled={busy}
    onclick={() => onBrowseIcon()}
    ><FolderOpen size={15} />{$_("profiles.editor.browse")}</button
  >
  <button
    type="button"
    class="field-button thumbnail-icon-button"
    disabled={busy || !setting.avatar_path}
    onclick={() => onApplyAvatarThumbnail()}
    title={$_("profiles.editor.load_avatar_thumbnail_icon_hint")}
    >{$_("profiles.editor.load_avatar_thumbnail_icon")}</button
  ></div
>
