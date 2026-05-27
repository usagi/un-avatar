<script lang="ts">
  import { flip } from "svelte/animate";
  import { _ } from "svelte-i18n";
  import type { ProfileListSetting, SettingPointerDrag } from "./profileListTypes";
  import ProfileSettingListRow from "./ProfileSettingListRow.svelte";

  export let settings: ProfileListSetting[];
  export let selectedSettingId: string | null;
  export let draggedSettingId: string | null;
  export let settingPointerDrag: SettingPointerDrag | null;
  export let iconSrc: (path: string | null) => string;
  export let runningCountForManifestPath: (manifestPath: string) => number;
  export let onSelect: (settingId: string) => void;
  export let onBeginDrag: (event: PointerEvent, settingId: string) => void;
</script>

<section class="panel setting-list">
  <h2>{$_("sidebar.profiles")}</h2>
  <div class:drag-active={Boolean(draggedSettingId)} class="setting-list-items">
    {#each settings as setting (setting.id)}
      {@const runningCount = runningCountForManifestPath(setting.manifest_path)}
      <div class="setting-list-row" animate:flip={{ duration: 120 }}>
        {#if draggedSettingId === setting.id && settingPointerDrag?.active}
          <div class="drag-placeholder" aria-hidden="true"></div>
        {/if}
        <ProfileSettingListRow
          {setting}
          selected={selectedSettingId === setting.id}
          dragging={draggedSettingId === setting.id}
          {runningCount}
          {settingPointerDrag}
          {iconSrc}
          {onSelect}
          {onBeginDrag}
        />
      </div>
    {:else}
      <p class="empty">{$_("profiles.editor.no_saved_settings")}</p>
    {/each}
  </div>
</section>
