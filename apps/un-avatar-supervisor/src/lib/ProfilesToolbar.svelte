<script lang="ts">
  import { _ } from "svelte-i18n";
  import { Copy, ExternalLink, Plus, Trash2 } from "lucide-svelte";

  export let busy = false;
  export let selectedSettingId: string | null = null;
  export let deleteHoldTargetId: string | null = null;
  export let deleteHoldProgress = 0;
  export let onNew: () => void | Promise<void>;
  export let onDuplicate: (settingId: string | null) => void | Promise<void>;
  export let onStartDeleteHold: (settingId: string | null) => void;
  export let onCancelDeleteHold: () => void;
  export let onOpenFolder: () => void | Promise<void>;
</script>

<div class="toolbar">
  <button
    class="primary"
    data-hint={$_("profiles.hints.toolbar.new")}
    disabled={busy}
    onclick={() => onNew()}
    ><Plus size={16} />{$_("profiles.actions.new")}</button
  >
  <button
    data-hint={$_("profiles.hints.toolbar.duplicate")}
    disabled={!selectedSettingId || busy}
    onclick={() => onDuplicate(selectedSettingId)}
    ><Copy size={16} />{$_("profiles.actions.duplicate")}</button
  >
  <button
    class="danger hold-delete"
    data-hint={$_("profiles.hints.toolbar.delete")}
    disabled={!selectedSettingId || busy}
    style={`--hold-progress: ${deleteHoldTargetId === selectedSettingId ? deleteHoldProgress : 0}`}
    onpointerdown={() => onStartDeleteHold(selectedSettingId)}
    onpointerup={onCancelDeleteHold}
    onpointercancel={onCancelDeleteHold}
    onpointerleave={onCancelDeleteHold}
    onkeydown={(event) => {
      if (event.key === " " || event.key === "Enter") {
        event.preventDefault();
        onStartDeleteHold(selectedSettingId);
      }
    }}
    onkeyup={onCancelDeleteHold}
    ><span class="hold-fill"></span><Trash2 size={16} /><span
      >{$_("profiles.actions.delete")}</span
    ></button
  >
  <button
    data-hint={$_("profiles.hints.toolbar.open_folder")}
    disabled={busy}
    onclick={() => onOpenFolder()}
    ><ExternalLink size={16} />{$_("profiles.actions.open_folder")}</button
  >
</div>
