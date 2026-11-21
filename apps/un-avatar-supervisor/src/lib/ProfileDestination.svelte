<script lang="ts">
 import { _ } from "svelte-i18n";
 import type { AvatarSetting } from "./profileTypes";
 export let profileId: string;
 export let profile: AvatarSetting | null = null;
</script>
<section class="profile-destination" aria-label={$_("profiles.destination.title")}>
 <span>{$_("profiles.destination.title")}</span>
 <strong>{profile?.name || profileId}</strong>
 <details>
  <summary>{$_("profiles.destination.details")}</summary>
  <dl><dt>{$_("profiles.destination.id")}</dt><dd>{profile?.id || profileId}</dd>
   {#if profile?.manifest_path}<dt>{$_("profiles.destination.file")}</dt><dd>{profile.manifest_path}</dd>{/if}
  </dl>
 </details>
</section>
<style>
 .profile-destination { margin: 0.75rem 1.5rem; padding: 0.75rem 1rem; border: 1px solid var(--border, #8888); border-radius: 0.5rem; }
 .profile-destination > span { font-size: 0.85em; margin-right: 0.75rem; }
 details { margin-top: 0.4rem; }
 dl { margin: 0.5rem 0 0; } dt { font-size: 0.85em; } dd { margin: 0 0 0.5rem; overflow-wrap: anywhere; }
</style>
