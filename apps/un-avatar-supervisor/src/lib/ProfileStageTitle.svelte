<script lang="ts">
	import { _ } from "svelte-i18n";
	import { basename } from "./formatting";
	import type { IdentitySetting } from "./profileTypes";

	export let setting: Pick<IdentitySetting, "name" | "group" | "avatar_path">;
	export let nameShownInTarget = false;

	$: profileGroupLabel = setting.group || $_("profiles.live.solo_profile");
	$: avatarFileName = basename(setting.avatar_path);
</script>

<div>
	{#if !nameShownInTarget}<h2>{setting.name}</h2>{/if}
	<p>{profileGroupLabel} · {avatarFileName}</p>
</div>
