<script lang="ts">
  import { _ } from "svelte-i18n";

  export let disabled = false;
  export let onSaveWindow: () => void;
  export let onRestoreWindow: () => void;
</script>

<div class="runtime-button-row">
  <button
    {disabled}
    onclick={onSaveWindow}
    title={$_("renderers.details.save_window_title")}
    >{$_("renderers.details.save_to_profile")}</button
  >
  <button
    {disabled}
    onclick={onRestoreWindow}
    title={$_("renderers.details.restore_window_title")}
    >{$_("renderers.details.restore_from_profile")}</button
  >
</div>
