<script lang="ts">
	import { invoke } from "@tauri-apps/api/core";
	import type {
		AnimatorActionMode,
		AvatarFileSetting,
		ProfileSettingValue,
		UnavatarAnimatorActionCandidate,
		UnavatarAnimatorActionPage,
		UnavatarWardrobeOptions,
	} from "./profileTypes";
	import { _ } from "svelte-i18n";
	import { ChevronLeft, ChevronRight, FolderOpen, RefreshCw, Search, SlidersHorizontal } from "lucide-svelte";
	import { hasTauriRuntime } from "./environment";

	export let setting: AvatarFileSetting;
	export let wardrobeOptions: UnavatarWardrobeOptions | null = null;
	export let busy = false;
	export let onBrowseAvatar: () => void | Promise<void>;
	export let onReviewMetadata: () => void | Promise<void>;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
	export let onActivate: () => void = () => {};

	$: isUnavatar = (setting.avatar_path ?? "").trim().toLowerCase().endsWith(".unavatar");
	$: supportsUnanimator = /\.(unavatar|vrm|glb)$/i.test((setting.avatar_path ?? "").trim());

	let animatorPanelOpen = false;
	let animatorQuery = "";
	let animatorOffset = 0;
	let animatorPage: UnavatarAnimatorActionPage | null = null;
	let animatorCandidates: UnavatarAnimatorActionCandidate[] = [];
	let animatorFilteredCandidates: UnavatarAnimatorActionCandidate[] = [];
	let animatorVisibleCandidates: UnavatarAnimatorActionCandidate[] = [];
	let animatorMatchedCount = 0;
	let animatorBusy = false;
	let animatorError = "";
	let animatorLoadKey = "";
	let animatorRequestId = 0;
	const animatorPageLimit = 80;
	const animatorLoadLimit = 2000;
	const animatorLoadTimeoutMs = 12000;

	$: if (animatorPanelOpen) {
		const key = `${setting.manifest_path}\n${setting.avatar_path ?? ""}`;
		if (key !== animatorLoadKey) {
			animatorLoadKey = key;
			animatorOffset = 0;
			animatorCandidates = [];
			animatorPage = null;
			animatorError = "";
		}
	}

	function setAnimatorPanelOpen(open: boolean): void {
		if (animatorPanelOpen === open) return;
		animatorPanelOpen = open;
		animatorOffset = 0;
		if (!open) {
			animatorBusy = false;
			animatorRequestId = animatorRequestId + 1;
		}
	}

	$: animatorFilteredCandidates = filterAnimatorCandidates(animatorCandidates, animatorQuery);
	$: animatorVisibleCandidates = animatorFilteredCandidates.slice(animatorOffset, animatorOffset + animatorPageLimit);
	$: animatorMatchedCount = animatorFilteredCandidates.length;

	function selectedModeFor(id: string): AnimatorActionMode {
		return setting.animator_actions.find((action) => action.id === id)?.mode ?? "off";
	}

	async function refreshAnimatorPage(): Promise<void> {
		if (!supportsUnanimator || !setting.avatar_path || !hasTauriRuntime()) {
			animatorPage = null;
			animatorCandidates = [];
			return;
		}
		const requestId = ++animatorRequestId;
		animatorBusy = true;
		animatorError = "";
		try {
			const page = await withTimeout(
				invoke<UnavatarAnimatorActionPage>("read_unavatar_animator_action_page", {
					path: setting.avatar_path,
					manifestPath: setting.manifest_path,
					query: "",
					offset: 0,
					limit: animatorLoadLimit,
				}),
				animatorLoadTimeoutMs
			);
			if (requestId !== animatorRequestId) return;
			animatorCandidates = page.candidates;
			animatorPage = {
				...page,
				matched_count: page.candidates.length,
				offset: animatorOffset,
				limit: animatorPageLimit,
				candidates: [],
			};
		} catch (error) {
			if (requestId !== animatorRequestId) return;
			animatorError = String(error);
			animatorPage = null;
			animatorCandidates = [];
		} finally {
			if (requestId === animatorRequestId) {
				animatorBusy = false;
			}
		}
	}

	function setAnimatorQuery(value: string): void {
		animatorQuery = value;
		animatorOffset = 0;
	}

	function filterAnimatorCandidates(candidates: UnavatarAnimatorActionCandidate[], query: string): UnavatarAnimatorActionCandidate[] {
		const words = query
			.trim()
			.toLowerCase()
			.split(/\s+/)
			.filter(Boolean);
		if (!words.length) return candidates;
		return candidates.filter((candidate) => {
			const haystack = [candidate.label, candidate.controller, candidate.layer, candidate.state_path, candidate.id]
				.join(" ")
				.toLowerCase();
			return words.every((word) => haystack.includes(word));
		});
	}

	function withTimeout<T>(promise: Promise<T>, timeoutMs: number): Promise<T> {
		return new Promise((resolve, reject) => {
			const timer = window.setTimeout(() => reject(new Error($_("profiles.editor.unanimator_load_timeout"))), timeoutMs);
			promise.then(
				(value) => {
					window.clearTimeout(timer);
					resolve(value);
				},
				(error) => {
					window.clearTimeout(timer);
					reject(error);
				}
			);
		});
	}

	async function updateAnimatorAction(id: string, mode: AnimatorActionMode): Promise<void> {
		const next = setting.animator_actions.filter((action) => action.id !== id);
		if (mode !== "off") {
			next.push({ id, mode });
		}
		if (animatorPage) {
			animatorCandidates = animatorCandidates.map((candidate) =>
				candidate.id === id ? { ...candidate, selected_mode: mode } : candidate
			);
			animatorPage = {
				...animatorPage,
				selected_count: next.length,
			};
		}
		await onUpdateSettingValue("animator.actions", next);
	}
</script>

<section
	class="editor-section section-grid profile-avatar-section"
	data-profile-section="avatar"
	onfocusin={onActivate}
	data-hint={$_("profiles.hints.avatar.section")}
>
	<div class="section-title-row">
		<h3>{$_("profiles.editor.avatar")}</h3>
		<span class="setting-scope">{$_("profiles.editor.launch_time")}</span>
	</div>
	<label class="path-field avatar-path-field"
		><span>{$_("profiles.editor.avatar_file")}</span><input
			value={setting.avatar_path ?? ""}
			disabled={busy}
			onchange={(event) => onUpdateSettingValue("avatar_path", (event.currentTarget as HTMLInputElement).value)}
		/>
		<button class="field-button" disabled={busy} onclick={() => onBrowseAvatar()}
			><FolderOpen size={15} />{$_("profiles.editor.browse")}</button
		>
		{#if setting.avatar_path}
			<button class="field-button metadata-review-button" disabled={busy} onclick={() => onReviewMetadata()}
				>{$_("profiles.editor.review_metadata")}</button
			>
		{/if}</label
	>
	{#if wardrobeOptions?.available}
		<label class="select-field"
			><span>{$_("profiles.editor.wardrobe")}</span><select
				value={setting.wardrobe_set ?? ""}
				disabled={busy}
				onchange={(event) => onUpdateSettingValue("wardrobe_set", (event.currentTarget as HTMLSelectElement).value)}
			>
				<option value="">{wardrobeOptions.base_label || "Base"}</option>
				{#each wardrobeOptions.sets as set}
					<option value={set.id}>{set.name || set.id}</option>
				{/each}
			</select></label
		>
	{:else if isUnavatar}
		<div class="profile-inline-note profile-inline-note-warning">
			{$_("profiles.editor.wardrobe_unavailable")}{wardrobeOptions?.error ? `: ${wardrobeOptions.error}` : ""}
		</div>
	{/if}
	{#if supportsUnanimator}
		<div class="animator-profile-panel">
			<div class="animator-profile-summary">
				<div>
					<span>{$_("profiles.editor.unanimator")}</span>
					<small>
						{#if animatorPage}
							{$_("profiles.editor.unanimator_selected_count", {
								values: { selected: animatorPage.selected_count, total: animatorPage.total_count },
							})}
						{:else}
							{$_("profiles.editor.unanimator_summary_idle", {
								values: { selected: setting.animator_actions.length },
							})}
						{/if}
					</small>
				</div>
				<button
					type="button"
					class="field-button"
					disabled={busy}
					onclick={() => setAnimatorPanelOpen(!animatorPanelOpen)}
					><SlidersHorizontal size={15} />{$_("profiles.editor.configure")}</button
				>
			</div>
			{#if animatorPanelOpen}
				<div class="animator-profile-browser">
					<div class="animator-load-row">
						<button type="button" class="field-button" disabled={busy || animatorBusy} onclick={() => refreshAnimatorPage()}
							><RefreshCw size={15} />{$_("profiles.editor.unanimator_load_candidates")}</button
						>
						<span>{$_("profiles.editor.unanimator_load_hint")}</span>
					</div>
					<label class="animator-search-field"
						><Search size={15} /><input
							value={animatorQuery}
							placeholder={$_("profiles.editor.unanimator_search")}
							oninput={(event) => setAnimatorQuery((event.currentTarget as HTMLInputElement).value)}
						/></label
					>
					<div class="animator-search-hints">
						<span>{$_("profiles.editor.unanimator_search_hint")}</span>
						<button type="button" onclick={() => setAnimatorQuery("expression")}>expression</button>
						<button type="button" onclick={() => setAnimatorQuery("cloth")}>cloth</button>
						<button type="button" onclick={() => setAnimatorQuery("object")}>object</button>
						<button type="button" onclick={() => setAnimatorQuery("hat")}>hat</button>
					</div>
					{#if animatorError}
						<div class="profile-inline-note profile-inline-note-warning">{animatorError}</div>
					{:else if animatorBusy}
						<div class="profile-inline-note">{$_("profiles.editor.loading")}</div>
					{:else if !animatorPage}
						<div class="profile-inline-note">{$_("profiles.editor.unanimator_not_loaded")}</div>
					{:else if animatorPage && !animatorPage.available}
						<div class="profile-inline-note">{$_("profiles.editor.unanimator_unavailable")}</div>
					{:else if animatorPage}
						<div class="animator-result-header">
							<span>
								{$_("profiles.editor.unanimator_result_count", {
									values: { matched: animatorMatchedCount, total: animatorPage.total_count },
								})}
							</span>
							<div class="animator-page-controls">
								<button
									type="button"
									class="icon-button"
									disabled={animatorOffset <= 0}
									onclick={() => (animatorOffset = Math.max(0, animatorOffset - animatorPageLimit))}
									title={$_("profiles.editor.previous_page")}><ChevronLeft size={16} /></button
								>
								<button
									type="button"
									class="icon-button"
									disabled={animatorOffset + animatorPageLimit >= animatorMatchedCount}
									onclick={() => (animatorOffset += animatorPageLimit)}
									title={$_("profiles.editor.next_page")}><ChevronRight size={16} /></button
								>
							</div>
						</div>
						<div class="animator-action-list">
							{#each animatorVisibleCandidates as candidate (candidate.id)}
								<div class="animator-action-row">
									<div class="animator-action-label">
										<strong>{candidate.label}</strong>
										<small>{candidate.controller} / {candidate.effect_count} effects / {candidate.condition_count} conditions</small>
									</div>
									<select
										value={selectedModeFor(candidate.id)}
										disabled={busy}
										onchange={(event) =>
											updateAnimatorAction(candidate.id, (event.currentTarget as HTMLSelectElement).value as AnimatorActionMode)}
									>
										<option value="off">{$_("profiles.editor.unanimator_mode_off")}</option>
										<option value="toggle">{$_("profiles.editor.unanimator_mode_toggle")}</option>
										<option value="one_shot">{$_("profiles.editor.unanimator_mode_one_shot")}</option>
									</select>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			{/if}
		</div>
	{/if}
</section>
