<script lang="ts">
	import { locale } from "svelte-i18n";
	import { findAvatarFeatures, type AvatarFeature } from "./avatarFeatureCatalog";
	let {
		profiles,
		selectedProfileId = $bindable(),
		query = $bindable(""),
		onOpen,
	}: {
		profiles: { id: string; name: string }[];
		selectedProfileId: string | null;
		query?: string;
		onOpen: (feature: AvatarFeature) => void;
	} = $props();
	const text = (ja: string, en: string) => ($locale?.startsWith("ja") ? ja : en);
	const matches = $derived(findAvatarFeatures(query));
	const groups = [
		{ id: "source", ja: "1. 姿と動きを用意", en: "1. Avatar and motion" },
		{ id: "look", ja: "2. 見た目と動きを整える", en: "2. Shape your presence" },
		{ id: "delivery", ja: "3. 見せる・配信へ届ける", en: "3. Frame and deliver" },
		{ id: "operation", ja: "構成と動作を管理", en: "Manage your setup" },
	];
</script>

<section class="feature-map">
	<header>
		<div>
			<h2>{text("機能マップ", "Feature map")}</h2>
			<p>{text("できることと、そのつながりから操作場所を選べる。", "Find controls by what they do and how they connect.")}</p>
		</div>
		<label
			>{text("機能・設定を探す", "Find a feature or setting")}<input
				type="search"
				bind:value={query}
				placeholder="Spout2, 揺れ, camera…"
			/></label
		>
	</header>
	<div class="target">
		<label
			>{text("設定を開く対象", "Profile to configure")}<select bind:value={selectedProfileId} disabled={!profiles.length}
				>{#if !profiles.length}<option value="">{text("プロファイル未作成", "No profiles yet")}</option>{/if}{#each profiles as p (p.id)}<option value={p.id}>{p.name} · {p.id}</option>{/each}</select
			></label
		><span>{text("対象の選択では実行個体は切り替わらない。", "Selecting a profile does not switch running instances.")}</span>
	</div>
	<div class="groups">
		{#each groups as group}{@const items = matches.filter((f) => f.group === group.id)}{#if items.length}<section
					class:operations={group.id === "operation"}
				>
					<h3>{text(group.ja, group.en)}</h3>
					<div class="features">
						{#each items as feature}<button data-map-feature={feature.id} onclick={() => onOpen(feature)}
								><strong>{text(feature.ja, feature.en)}</strong><span class="map-search-terms">{feature.terms}</span></button
							>{/each}
					</div>
				</section>{/if}{/each}
	</div>
	{#if !matches.length}<p role="status">
			{text("一致する機能がない。別の名前や用途でも探せる。", "No matching feature. Try another name or purpose.")}
		</p>{/if}
</section>

<style>
	.feature-map {
		width: 100%;
		box-sizing: border-box;
		padding: 24px;
		display: grid;
		gap: 22px;
		align-content: start;
	}
	header {
		display: flex;
		justify-content: space-between;
		gap: 24px;
		align-items: start;
	}
	h2 {
		margin: 0;
		font-size: 26px;
	}
	p,
	.target span {
		color: var(--muted);
	}
	label {
		display: grid;
		gap: 8px;
		font-size: 13px;
	}
	input,
	select {
		background: var(--panel);
		color: var(--text);
		border: 1px solid var(--border);
		border-radius: 8px;
		padding: 10px;
		min-width: 220px;
	}
	.target {
		display: flex;
		gap: 20px;
		align-items: end;
		padding: 16px;
		border: 1px solid var(--border);
		border-radius: 12px;
	}
	.target span {
		font-size: 13px;
	}
	.groups {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 22px;
	}
	h3 {
		font-size: 16px;
		margin: 0 0 14px;
		color: var(--text);
	}
	.features {
		display: grid;
		gap: 10px;
	}
	.features button {
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: 12px;
		color: var(--text);
		padding: 18px;
		text-align: left;
		display: grid;
		gap: 12px;
		cursor: pointer;
	}
	.features button:hover {
		border-color: var(--accent);
	}
	strong {
		font-size: 15px;
	}
	button span {
		font-size: 12px;
		line-height: 1.65;
		color: var(--muted);
		overflow-wrap: anywhere;
	}
	.operations {
		grid-column: 1/-1;
	}
	.operations .features {
		grid-template-columns: repeat(auto-fit, minmax(210px, 1fr));
	}
	@media (max-width: 1000px) {
		.groups {
			grid-template-columns: 1fr 1fr;
		}
	}
	@media (max-width: 700px) {
		.groups {
			grid-template-columns: 1fr;
		}
		header,
		.target {
			flex-direction: column;
			align-items: stretch;
		}
		.feature-map {
			padding: 16px;
		}
	}
</style>
