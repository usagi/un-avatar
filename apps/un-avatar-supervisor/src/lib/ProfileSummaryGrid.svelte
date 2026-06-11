<script lang="ts">
	import type { ProfileSummaryItem } from "./profileSummary";
	import type { ProfileSectionId } from "./profileTypes";

	export let items: ProfileSummaryItem[];
	export let activeSection: ProfileSectionId;
	export let onScrollSection: (section: ProfileSectionId) => void;
</script>

<div class="profile-summary-grid">
	{#each items as item}
		<button type="button" class:active={activeSection === item.section} onclick={() => onScrollSection(item.section)}>
			<strong>{item.value}</strong><small>{item.label}</small>
		</button>
	{/each}
</div>
