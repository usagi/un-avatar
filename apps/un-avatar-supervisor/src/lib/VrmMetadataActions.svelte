<script lang="ts">
  import { _ } from "svelte-i18n";

  export let pendingPath: string | null;
  export let hasThumbnail = false;
  export let busy = false;
  export let useThumbnailForProfileIconOnAccept = false;
  export let onClose: () => void | Promise<void>;
  export let onAcceptAndUse: () => void | Promise<void>;
</script>

<footer class="vrm-metadata-actions">
  {#if pendingPath && hasThumbnail}
    <label
      class="vrm-thumbnail-icon-toggle"
      title={$_("vrm_metadata.use_thumbnail_as_profile_icon_hint")}
    >
      <input
        type="checkbox"
        bind:checked={useThumbnailForProfileIconOnAccept}
      />
      <span>{$_("vrm_metadata.use_thumbnail_as_profile_icon")}</span>
    </label>
  {/if}
  <button class="secondary" onclick={() => onClose()}>
    {pendingPath ? $_("common.cancel") : $_("common.close")}
  </button>
  {#if pendingPath}
    <button class="primary" disabled={busy} onclick={() => onAcceptAndUse()}>
      {$_("vrm_metadata.accept_and_use")}
    </button>
  {/if}
</footer>
