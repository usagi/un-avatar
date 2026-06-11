<script lang="ts">
	import { _ } from "svelte-i18n";
	import type { ProfileSectionNavItem } from "./profileStageTypes";

	export let items: ProfileSectionNavItem[];
	export let activeSectionId: ProfileSectionNavItem["id"];
	export let onSelect: (sectionId: ProfileSectionNavItem["id"]) => void;
</script>

<nav class="profile-section-nav" aria-label="Profile setting sections">
	{#each items as item}
		<button
			type="button"
			class:active={activeSectionId === item.id}
			data-section={item.id}
			data-hint={$_("profiles.sections.nav_hint", {
				values: { section: $_(item.labelKey) },
			})}
			onclick={() => onSelect(item.id)}
		>
			<strong>{$_(item.labelKey)}</strong>
			{#if item.scopeKey}
				<small>{$_(item.scopeKey)}</small>
			{/if}
		</button>
	{/each}
</nav>
