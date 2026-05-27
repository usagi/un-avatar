<script lang="ts">
  import type { ProfileSettingValue } from "./profileTypes";
  import { _ } from "svelte-i18n";
  import ProfileSpringBoneOverrideRow from "./ProfileSpringBoneOverrideRow.svelte";
  import type { SpringBoneCategoryOverrideSetting } from "./springBonePresets";


  export let overrides: SpringBoneCategoryOverrideSetting[] = [];
  export let springBonesEnabled = false;
  export let busy = false;
  export let onUpdateSettingValue: (
    field: string,
    value: ProfileSettingValue,
  ) => void | Promise<void>;
</script>

<div class="profile-wide-field spring-bone-overrides">
  <div class="field-note">{$_("profiles.editor.spring_bone_category_overrides")}</div>
  <div class="spring-bone-override-list">
    {#each overrides as override}
      <ProfileSpringBoneOverrideRow
        {override}
        {springBonesEnabled}
        {busy}
        {onUpdateSettingValue}
      />
    {/each}
  </div>
</div>
