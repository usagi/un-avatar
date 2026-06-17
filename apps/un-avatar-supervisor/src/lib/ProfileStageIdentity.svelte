<script lang="ts">
	import { _ } from "svelte-i18n";

	export let iconUrl: string;
	export let liveRendererCount = 0;
</script>

<div class="profile-stage-identity">
	<div class="profile-stage-art">
		<img src={iconUrl} alt="" />
	</div>
	{#if liveRendererCount > 0}
		<span class="storage-badge storage-user profile-live-count">
			{liveRendererCount === 1
				? $_("profiles.live.running")
				: $_("profiles.live.running_count", {
						values: { count: liveRendererCount },
					})}
		</span>
	{/if}
</div>
