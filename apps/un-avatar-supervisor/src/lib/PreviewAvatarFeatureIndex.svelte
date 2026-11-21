<script lang="ts">
	import { avatarFeatures, type AvatarFeature } from './avatarFeatureCatalog';
	import { _ } from 'svelte-i18n';
	import type { ProfileSectionNavItem } from './profileStageTypes';
	let { language = 'ja-JP', targetName, activeSection, activeDestination, sectionItems, onOpen }: {
		language?: string; targetName: string; activeSection: string; activeDestination: string;
		sectionItems: ProfileSectionNavItem[];
		onOpen: (feature: AvatarFeature) => void | Promise<void>;
	} = $props();
	const ja = $derived(language.startsWith('ja'));
</script>
<nav class="preview-avatar-feature-index" aria-label={ja ? '作業中の機能目次' : 'Feature index while working'}>
	<p>{ja ? '編集する保存設定' : 'Saved configuration'}<strong>{targetName || (ja ? '未選択' : 'None selected')}</strong></p>
	<div class="entries">
	{#each avatarFeatures as feature (feature.id)}
		{@const section = sectionItems.find(item => item.id === feature.section)}
		<button type="button" data-outline-feature={feature.id} title={feature.terms}
			aria-current={feature.section === activeSection || (!feature.section && feature.destination === activeDestination) ? 'location' : undefined}
			onclick={() => void onOpen(feature)}>
			<span>{ja ? feature.ja : feature.en}</span>
			{#if section?.scopeKey}<small class="scope">{$_(section.scopeKey)}</small>{/if}
		</button>
	{/each}
	</div>
	<small>{ja ? '個体の操作は稼働一覧で対象を選択。アプリ設定は全体に適用。' : 'Choose a runtime target in the running list. App preferences apply globally.'}</small>
</nav>
<style>
	nav{border-top:1px solid var(--border);margin-top:8px;padding-top:8px;display:flex;flex-direction:column;gap:3px;flex:1;min-height:170px}
	.entries{display:grid;gap:3px;overflow-y:auto;min-height:60px;overscroll-behavior:contain}
	p,small{flex-shrink:0}
	p{font-size:11px;color:var(--muted);margin:4px 6px 8px;overflow-wrap:anywhere}strong{display:block;color:var(--text);font-size:12px;margin-top:4px}
	button{padding:8px 6px;text-align:left;font-size:12px;border:0;background:transparent;border-radius:6px;color:var(--text)}
	button:hover,button[aria-current]{background:var(--accent-soft);color:var(--accent)}
	button:focus-visible{outline:2px solid var(--accent);outline-offset:-2px}
	small{font-size:10px;line-height:1.5;color:var(--muted);padding:8px 6px}
	small.scope{display:block;padding:2px 0 0;font-size:10px}
</style>
