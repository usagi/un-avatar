<script lang="ts">
	import { invoke } from "@tauri-apps/api/core";
	import type {
		AnimatorActionMode,
		AnimatorBindingSetting,
		AvatarFileSetting,
		ProfileSettingValue,
		UnavatarAnimatorActionCandidate,
		UnavatarAnimatorActionPage,
		UnavatarWardrobeOptions,
		WardrobeBindingSetting,
		WardrobeSetOption,
	} from "./profileTypes";
	import { _ } from "svelte-i18n";
	import { ChevronLeft, ChevronRight, FolderOpen, Keyboard, RefreshCw, Search, SlidersHorizontal } from "lucide-svelte";
	import { hasTauriRuntime } from "./environment";
	import { traceAsync, traceFrontendEvent } from "./frontendTrace";

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
	const animatorTransitionDefaultMs = 250;
	let animatorValueDrafts: Record<string, number> = {};
	let animatorTransitionMsDrafts: Record<string, number> = {};
	let keyboardCaptureSetId: string | null = null;
	let keyboardCaptureActionId: string | null = null;
	let midiCaptureSetId: string | null = null;
	let midiCaptureActionId: string | null = null;
	let midiCaptureError = "";
	let midiCaptureRequestId = 0;

	type MidiNoteCaptureResult = {
		device: string;
		channel: number;
		note: number;
	};

	$: wardrobeRows = wardrobeSettingRows(wardrobeOptions);

	$: if (animatorPanelOpen) {
		const key = `${setting.manifest_path}\n${setting.avatar_path ?? ""}`;
		if (key !== animatorLoadKey) {
			animatorLoadKey = key;
			animatorOffset = 0;
			animatorCandidates = [];
			animatorPage = null;
			animatorError = "";
			animatorValueDrafts = {};
			animatorTransitionMsDrafts = {};
		}
	}

	function setAnimatorPanelOpen(open: boolean): void {
		traceFrontendEvent("unanimator:panel", { open, avatarPath: setting.avatar_path ?? "" });
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

	function selectedValueFor(id: string): number {
		const draft = animatorValueDrafts[id];
		if (typeof draft === "number" && Number.isFinite(draft)) return Math.min(1, Math.max(0, draft));
		const value = setting.animator_actions.find((action) => action.id === id)?.value;
		return typeof value === "number" && Number.isFinite(value) ? Math.min(1, Math.max(0, value)) : 1;
	}

	function selectedTransitionCurveFor(id: string): string {
		return setting.animator_actions.find((action) => action.id === id)?.transition_curve ?? "none";
	}

	function selectedTransitionMsFor(id: string): number {
		const draft = animatorTransitionMsDrafts[id];
		if (typeof draft === "number" && Number.isFinite(draft)) return Math.min(3000, Math.max(0, Math.round(draft)));
		const value = setting.animator_actions.find((action) => action.id === id)?.transition_ms;
		return typeof value === "number" && Number.isFinite(value) ? Math.min(3000, Math.max(0, Math.round(value))) : 0;
	}

	async function refreshAnimatorPage(): Promise<void> {
		traceFrontendEvent("unanimator:refresh:enter", {
			avatarPath: setting.avatar_path ?? "",
			manifestPath: setting.manifest_path,
		});
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
				traceAsync(
					"invoke:read_unavatar_animator_action_page",
					() =>
						invoke<UnavatarAnimatorActionPage>("read_unavatar_animator_action_page", {
							path: setting.avatar_path,
							manifestPath: setting.manifest_path,
							query: "",
							offset: 0,
							limit: animatorLoadLimit,
						}),
					{ manifestPath: setting.manifest_path }
				),
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
			traceFrontendEvent("unanimator:refresh:ok", {
				total: page.total_count,
				candidates: page.candidates.length,
			});
		} catch (error) {
			if (requestId !== animatorRequestId) return;
			animatorError = String(error);
			animatorPage = null;
			animatorCandidates = [];
			traceFrontendEvent("unanimator:refresh:error", { error: String(error) });
		} finally {
			if (requestId === animatorRequestId) {
				animatorBusy = false;
				traceFrontendEvent("unanimator:refresh:finally", { busy: animatorBusy });
			}
		}
	}

	function setAnimatorQuery(value: string): void {
		traceFrontendEvent("unanimator:query", { value });
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

	function animatorCandidateRowKey(candidate: UnavatarAnimatorActionCandidate, index: number): string {
		return `${candidate.id}:${animatorOffset + index}`;
	}

	function animatorCandidateFor(id: string): UnavatarAnimatorActionCandidate | null {
		return animatorCandidates.find((candidate) => candidate.id === id) ?? null;
	}

	function animatorSelectedLabel(id: string): string {
		return animatorCandidateFor(id)?.label ?? id;
	}

	function animatorSelectedDetails(id: string): string {
		const candidate = animatorCandidateFor(id);
		if (!candidate) {
			return `${selectedModeFor(id)} / ${selectedValueFor(id).toFixed(2)}`;
		}
		return `${candidate.controller} / ${candidate.effect_count} effects / ${candidate.condition_count} conditions`;
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

	async function updateAnimatorAction(id: string, mode: AnimatorActionMode, value = selectedValueFor(id)): Promise<void> {
		const existing = setting.animator_actions.find((action) => action.id === id);
		const next = setting.animator_actions.filter((action) => action.id !== id);
		if (mode !== "off") {
			next.push({
				id,
				mode,
				value: Math.min(1, Math.max(0, value)),
				transition_curve: existing?.transition_curve ?? null,
				transition_ms: existing?.transition_ms ?? null,
			});
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

	async function updateAnimatorActionValue(id: string, value: number): Promise<void> {
		const nextValue = Math.min(1, Math.max(0, value));
		animatorValueDrafts = { ...animatorValueDrafts, [id]: nextValue };
		try {
			await updateAnimatorAction(id, selectedModeFor(id) === "off" ? "toggle" : selectedModeFor(id), nextValue);
		} finally {
			const nextDrafts = { ...animatorValueDrafts };
			delete nextDrafts[id];
			animatorValueDrafts = nextDrafts;
		}
	}

	function draftAnimatorActionValue(id: string, value: number): void {
		animatorValueDrafts = { ...animatorValueDrafts, [id]: Math.min(1, Math.max(0, value)) };
	}

	async function updateAnimatorTransition(id: string, curve: string, durationMs = selectedTransitionMsFor(id)): Promise<void> {
		const mode = selectedModeFor(id) === "off" ? "toggle" : selectedModeFor(id);
		const next = setting.animator_actions.filter((action) => action.id !== id);
		const normalizedCurve = curve === "none" ? null : curve;
		const requestedMs = Math.min(3000, Math.max(0, Math.round(durationMs)));
		const normalizedMs = normalizedCurve ? requestedMs || animatorTransitionDefaultMs : null;
		next.push({
			id,
			mode,
			value: selectedValueFor(id),
			transition_curve: normalizedCurve,
			transition_ms: normalizedMs && normalizedMs > 0 ? normalizedMs : null,
		});
		if (normalizedCurve) {
			animatorTransitionMsDrafts = { ...animatorTransitionMsDrafts, [id]: normalizedMs ?? requestedMs };
		}
		try {
			await onUpdateSettingValue("animator.actions", next);
		} finally {
			const nextDrafts = { ...animatorTransitionMsDrafts };
			delete nextDrafts[id];
			animatorTransitionMsDrafts = nextDrafts;
		}
	}

	function draftAnimatorTransitionMs(id: string, value: number): void {
		animatorTransitionMsDrafts = { ...animatorTransitionMsDrafts, [id]: Math.min(3000, Math.max(0, Math.round(value))) };
	}

	function wardrobeSettingRows(options: UnavatarWardrobeOptions | null): WardrobeSetOption[] {
		if (!options?.available) return [];
		return [{ id: "", name: options.base_label || "Base" }, ...options.sets];
	}

	function wardrobeShortcutFor(setId: string): string {
		return (
			setting.wardrobe_bindings.find((binding) => binding.set_id === setId && binding.kind === "keyboard")?.binding ??
			setting.wardrobe_shortcuts.find((shortcut) => shortcut.set_id === setId)?.shortcut ??
			""
		);
	}

	function wardrobeMidiBindingFor(setId: string): WardrobeBindingSetting {
		return (
			setting.wardrobe_bindings.find((binding) => binding.set_id === setId && binding.kind === "midi_note") ?? {
				set_id: setId,
				kind: "midi_note",
				device: "",
				channel: 1,
				note: null,
			}
		);
	}

	function animatorShortcutFor(actionId: string): string {
		return (
			setting.animator_bindings.find((binding) => binding.action_id === actionId && binding.kind === "keyboard")?.binding ?? ""
		);
	}

	function animatorMidiBindingFor(actionId: string): AnimatorBindingSetting {
		return (
			setting.animator_bindings.find((binding) => binding.action_id === actionId && binding.kind === "midi_note") ?? {
				action_id: actionId,
				kind: "midi_note",
				device: "",
				channel: 1,
				note: null,
			}
		);
	}

	async function updateWardrobeDefault(setId: string): Promise<void> {
		await onUpdateSettingValue("wardrobe_set", setId);
	}

	async function updateWardrobeShortcut(setId: string, shortcut: string): Promise<void> {
		const normalized = shortcut.trim();
		const next: WardrobeBindingSetting[] = setting.wardrobe_bindings.filter(
			(item) => !(item.set_id === setId && item.kind === "keyboard")
		);
		if (normalized) {
			next.push({ set_id: setId, kind: "keyboard", binding: normalized });
		}
		await onUpdateSettingValue("wardrobe.bindings", next);
	}

	async function updateWardrobeMidi(setId: string, patch: Partial<WardrobeBindingSetting>): Promise<void> {
		const current = wardrobeMidiBindingFor(setId);
		const nextBinding: WardrobeBindingSetting = {
			...current,
			...patch,
			set_id: setId,
			kind: "midi_note",
		};
		const next = setting.wardrobe_bindings.filter((item) => !(item.set_id === setId && item.kind === "midi_note"));
		const channel = Number(nextBinding.channel ?? 0);
		const note = Number(nextBinding.note ?? -1);
		if (channel >= 1 && channel <= 16 && note >= 0 && note <= 127) {
			next.push({
				...nextBinding,
				device: nextBinding.device?.trim() || null,
				channel,
				note,
			});
		}
		await onUpdateSettingValue("wardrobe.bindings", next);
	}

	async function updateAnimatorShortcut(actionId: string, shortcut: string): Promise<void> {
		const normalized = shortcut.trim();
		const next: AnimatorBindingSetting[] = setting.animator_bindings.filter(
			(item) => !(item.action_id === actionId && item.kind === "keyboard")
		);
		if (normalized) {
			next.push({ action_id: actionId, kind: "keyboard", binding: normalized });
		}
		await onUpdateSettingValue("animator.bindings", next);
	}

	async function updateAnimatorMidi(actionId: string, patch: Partial<AnimatorBindingSetting>): Promise<void> {
		const current = animatorMidiBindingFor(actionId);
		const nextBinding: AnimatorBindingSetting = {
			...current,
			...patch,
			action_id: actionId,
			kind: "midi_note",
		};
		const next = setting.animator_bindings.filter((item) => !(item.action_id === actionId && item.kind === "midi_note"));
		const channel = Number(nextBinding.channel ?? 0);
		const note = Number(nextBinding.note ?? -1);
		if (channel >= 1 && channel <= 16 && note >= 0 && note <= 127) {
			next.push({
				...nextBinding,
				device: nextBinding.device?.trim() || null,
				channel,
				note,
			});
		}
		await onUpdateSettingValue("animator.bindings", next);
	}

	async function captureWardrobeMidi(setId: string): Promise<void> {
		if (!hasTauriRuntime()) return;
		const requestId = ++midiCaptureRequestId;
		midiCaptureSetId = setId;
		midiCaptureError = "";
		try {
			const result = await invoke<MidiNoteCaptureResult>("capture_midi_note_binding", { timeoutMs: 10_000 });
			if (requestId !== midiCaptureRequestId) return;
			await updateWardrobeMidi(setId, {
				device: result.device,
				channel: result.channel,
				note: result.note,
			});
		} catch (error) {
			if (requestId !== midiCaptureRequestId) return;
			midiCaptureError = String(error);
		} finally {
			if (requestId === midiCaptureRequestId) {
				midiCaptureSetId = null;
			}
		}
	}

	async function captureAnimatorMidi(actionId: string): Promise<void> {
		if (!hasTauriRuntime()) return;
		const requestId = ++midiCaptureRequestId;
		midiCaptureActionId = actionId;
		midiCaptureError = "";
		try {
			const result = await invoke<MidiNoteCaptureResult>("capture_midi_note_binding", { timeoutMs: 10_000 });
			if (requestId !== midiCaptureRequestId) return;
			await updateAnimatorMidi(actionId, {
				device: result.device,
				channel: result.channel,
				note: result.note,
			});
		} catch (error) {
			if (requestId !== midiCaptureRequestId) return;
			midiCaptureError = String(error);
		} finally {
			if (requestId === midiCaptureRequestId) {
				midiCaptureActionId = null;
			}
		}
	}

	function cancelWardrobeMidiCapture(): void {
		midiCaptureRequestId += 1;
		midiCaptureSetId = null;
		midiCaptureActionId = null;
	}

	function formatCapturedKeyboardEvent(event: KeyboardEvent): string {
		const parts: string[] = [];
		if (event.ctrlKey) parts.push("Ctrl");
		if (event.altKey) parts.push("Alt");
		if (event.shiftKey) parts.push("Shift");
		if (event.metaKey) parts.push("Win");
		const key = event.key.length === 1 ? event.key.toUpperCase() : event.key;
		if (!["Control", "Alt", "Shift", "Meta"].includes(key)) {
			parts.push(key === " " ? "Space" : key);
		}
		return parts.length > 0 ? parts.join("+") : "";
	}

	async function captureKeyboard(event: KeyboardEvent): Promise<void> {
		if (keyboardCaptureSetId === null && keyboardCaptureActionId === null) return;
		event.preventDefault();
		event.stopPropagation();
		if (event.key === "Escape") {
			keyboardCaptureSetId = null;
			keyboardCaptureActionId = null;
			return;
		}
		const binding = formatCapturedKeyboardEvent(event);
		if (!binding || ["Ctrl", "Alt", "Shift", "Win"].includes(binding)) return;
		const setId = keyboardCaptureSetId;
		const actionId = keyboardCaptureActionId;
		keyboardCaptureSetId = null;
		keyboardCaptureActionId = null;
		if (setId !== null) {
			await updateWardrobeShortcut(setId, binding);
		} else if (actionId !== null) {
			await updateAnimatorShortcut(actionId, binding);
		}
	}
</script>

<svelte:window onkeydown={captureKeyboard} />

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
		<div class="wardrobe-profile-panel">
			<div class="wardrobe-profile-header">
				<span>{$_("profiles.editor.wardrobe")}</span>
				<small>{$_("profiles.editor.wardrobe_profile_hint")}</small>
			</div>
			<div class="wardrobe-transition-settings">
				<label>
					<span>{$_("profiles.editor.wardrobe_changing_anchor")}</span>
					<select
						value={setting.wardrobe_billboard_anchor || "neck"}
						disabled={busy}
						onchange={(event) =>
							onUpdateSettingValue("wardrobe.transition.billboard_anchor", (event.currentTarget as HTMLSelectElement).value)}
					>
						<option value="head">Head</option>
						<option value="neck">Neck</option>
						<option value="spine">Spine</option>
					</select>
				</label>
				<label>
					<span>{$_("profiles.editor.wardrobe_changing_y_offset")}</span>
					<input
						type="number"
						min="-1000"
						max="1000"
						step="1"
						value={setting.wardrobe_billboard_y_offset_mm ?? 0}
						disabled={busy}
						onchange={(event) =>
							onUpdateSettingValue(
								"wardrobe.transition.billboard_y_offset_mm",
								Number((event.currentTarget as HTMLInputElement).value),
							)}
					/>
				</label>
			</div>
			<div class="wardrobe-set-list">
				{#each wardrobeRows as set (set.id)}
					<div class="wardrobe-set-row">
						<label class="wardrobe-default-choice">
							<input
								type="radio"
								name={`wardrobe-default-${setting.manifest_path}`}
								checked={(setting.wardrobe_set ?? "") === set.id}
								disabled={busy}
								onchange={() => updateWardrobeDefault(set.id)}
							/>
							<span>{$_("profiles.editor.wardrobe_default")}</span>
						</label>
						<div class="wardrobe-set-label">
							<strong>{set.name || set.id || "Base"}</strong>
							{#if set.id}<small>{set.id}</small>{/if}
						</div>
						<label class="wardrobe-shortcut-field">
							<span>{$_("profiles.editor.wardrobe_shortcut")}</span>
							<div class="binding-input-row">
								<input
									value={wardrobeShortcutFor(set.id)}
									placeholder={$_("profiles.editor.wardrobe_shortcut_placeholder")}
									disabled={busy}
									onchange={(event) => updateWardrobeShortcut(set.id, (event.currentTarget as HTMLInputElement).value)}
								/>
								<button
									type="button"
									class="icon-button binding-capture-button"
									disabled={busy}
									title={$_("profiles.editor.binding_capture")}
									onclick={() => (keyboardCaptureSetId = set.id)}><Keyboard size={15} /></button
								>
							</div>
						</label>
						<div class="wardrobe-midi-binding">
							<span>{$_("profiles.editor.wardrobe_midi_note")}</span>
							<input
								value={wardrobeMidiBindingFor(set.id).device ?? ""}
								placeholder={$_("profiles.editor.wardrobe_midi_device")}
								disabled={busy}
								onchange={(event) => updateWardrobeMidi(set.id, { device: (event.currentTarget as HTMLInputElement).value })}
							/>
							<input
								type="number"
								min="1"
								max="16"
								value={wardrobeMidiBindingFor(set.id).channel ?? 1}
								disabled={busy}
								aria-label={$_("profiles.editor.wardrobe_midi_channel")}
								onchange={(event) => updateWardrobeMidi(set.id, { channel: Number((event.currentTarget as HTMLInputElement).value) })}
							/>
							<input
								type="number"
								min="0"
								max="127"
								value={wardrobeMidiBindingFor(set.id).note ?? ""}
								placeholder={$_("profiles.editor.wardrobe_midi_note_number")}
								disabled={busy}
								aria-label={$_("profiles.editor.wardrobe_midi_note_number")}
								onchange={(event) => updateWardrobeMidi(set.id, { note: Number((event.currentTarget as HTMLInputElement).value) })}
							/>
							<button
								type="button"
								class="icon-button binding-capture-button"
								disabled={busy || midiCaptureSetId !== null}
								title={$_("profiles.editor.midi_capture")}
								onclick={() => captureWardrobeMidi(set.id)}><SlidersHorizontal size={15} /></button
							>
						</div>
					</div>
				{/each}
			</div>
		</div>
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
			{#if setting.animator_actions.length > 0}
				<div class="animator-selected-panel">
					<div class="animator-result-header">
						<span>{$_("profiles.editor.unanimator_selected_actions")}</span>
					</div>
					<div class="animator-action-list animator-selected-list">
						{#each setting.animator_actions as action (action.id)}
							<div class="animator-action-row">
								<div class="animator-action-label">
									<strong>{animatorSelectedLabel(action.id)}</strong>
									<small>{animatorSelectedDetails(action.id)}</small>
								</div>
								<select
									value={selectedModeFor(action.id)}
									disabled={busy}
									onchange={(event) =>
										updateAnimatorAction(action.id, (event.currentTarget as HTMLSelectElement).value as AnimatorActionMode)}
								>
									<option value="off">{$_("profiles.editor.unanimator_mode_off")}</option>
									<option value="toggle">{$_("profiles.editor.unanimator_mode_toggle")}</option>
									<option value="one_shot">{$_("profiles.editor.unanimator_mode_one_shot")}</option>
								</select>
								<div class="animator-action-value">
									<input
										type="range"
										min="0"
										max="1"
										step="0.01"
										value={selectedValueFor(action.id)}
										disabled={busy || selectedModeFor(action.id) === "off"}
										oninput={(event) => draftAnimatorActionValue(action.id, Number((event.currentTarget as HTMLInputElement).value))}
										onchange={(event) => updateAnimatorActionValue(action.id, Number((event.currentTarget as HTMLInputElement).value))}
									/>
									<input
										type="number"
										min="0"
										max="1"
										step="0.01"
										value={selectedValueFor(action.id).toFixed(2)}
										disabled={busy || selectedModeFor(action.id) === "off"}
										onchange={(event) => updateAnimatorActionValue(action.id, Number((event.currentTarget as HTMLInputElement).value))}
									/>
								</div>
								<div class="animator-transition-row">
									<select
										value={selectedTransitionCurveFor(action.id)}
										disabled={busy || selectedModeFor(action.id) === "off"}
										onchange={(event) => updateAnimatorTransition(action.id, (event.currentTarget as HTMLSelectElement).value)}
									>
										<option value="none">{$_("profiles.editor.unanimator_transition_none")}</option>
										<option value="linear">Linear</option>
										<option value="ease_in">Ease In</option>
										<option value="ease_out">Ease Out</option>
										<option value="ease_in_out">Ease InOut</option>
									</select>
									<input
										type="range"
										min="0"
										max="1000"
										step="10"
										value={selectedTransitionMsFor(action.id)}
										disabled={busy || selectedTransitionCurveFor(action.id) === "none"}
										oninput={(event) => draftAnimatorTransitionMs(action.id, Number((event.currentTarget as HTMLInputElement).value))}
										onchange={(event) =>
											updateAnimatorTransition(action.id, selectedTransitionCurveFor(action.id), Number((event.currentTarget as HTMLInputElement).value))}
									/>
									<input
										type="number"
										min="0"
										max="3000"
										step="10"
										value={selectedTransitionMsFor(action.id)}
										disabled={busy || selectedTransitionCurveFor(action.id) === "none"}
										aria-label={$_("profiles.editor.unanimator_transition_ms")}
										onchange={(event) =>
											updateAnimatorTransition(action.id, selectedTransitionCurveFor(action.id), Number((event.currentTarget as HTMLInputElement).value))}
									/>
								</div>
								<div class="animator-binding-row">
									<div class="binding-input-row">
										<input
											value={animatorShortcutFor(action.id)}
											placeholder={$_("profiles.editor.wardrobe_shortcut_placeholder")}
											disabled={busy}
											onchange={(event) => updateAnimatorShortcut(action.id, (event.currentTarget as HTMLInputElement).value)}
										/>
										<button
											type="button"
											class="icon-button binding-capture-button"
											disabled={busy}
											title={$_("profiles.editor.binding_capture")}
											onclick={() => (keyboardCaptureActionId = action.id)}><Keyboard size={15} /></button
										>
									</div>
									<div class="wardrobe-midi-binding">
										<span>{$_("profiles.editor.wardrobe_midi_note")}</span>
										<input
											value={animatorMidiBindingFor(action.id).device ?? ""}
											placeholder={$_("profiles.editor.wardrobe_midi_device")}
											disabled={busy}
											onchange={(event) => updateAnimatorMidi(action.id, { device: (event.currentTarget as HTMLInputElement).value })}
										/>
										<input
											type="number"
											min="1"
											max="16"
											value={animatorMidiBindingFor(action.id).channel ?? 1}
											disabled={busy}
											aria-label={$_("profiles.editor.wardrobe_midi_channel")}
											onchange={(event) => updateAnimatorMidi(action.id, { channel: Number((event.currentTarget as HTMLInputElement).value) })}
										/>
										<input
											type="number"
											min="0"
											max="127"
											value={animatorMidiBindingFor(action.id).note ?? ""}
											placeholder={$_("profiles.editor.wardrobe_midi_note_number")}
											disabled={busy}
											aria-label={$_("profiles.editor.wardrobe_midi_note_number")}
											onchange={(event) => updateAnimatorMidi(action.id, { note: Number((event.currentTarget as HTMLInputElement).value) })}
										/>
										<button
											type="button"
											class="icon-button binding-capture-button"
											disabled={busy || midiCaptureSetId !== null || midiCaptureActionId !== null}
											title={$_("profiles.editor.midi_capture")}
											onclick={() => captureAnimatorMidi(action.id)}><SlidersHorizontal size={15} /></button
										>
									</div>
								</div>
							</div>
						{/each}
					</div>
				</div>
			{/if}
			{#if animatorPanelOpen}
				<div class="animator-profile-browser">
					<form
						class="animator-search-row"
						onsubmit={(event) => {
							event.preventDefault();
							void refreshAnimatorPage();
						}}
					>
						<label class="animator-search-field"
							><Search size={15} /><input
								value={animatorQuery}
								placeholder={$_("profiles.editor.unanimator_search")}
								oninput={(event) => setAnimatorQuery((event.currentTarget as HTMLInputElement).value)}
							/></label
						>
						<button type="submit" class="field-button" disabled={busy || animatorBusy}
							><RefreshCw size={15} />{$_("profiles.editor.unanimator_load_candidates")}</button
						>
					</form>
					<div class="animator-load-row">
						<span>{$_("profiles.editor.unanimator_load_hint")}</span>
					</div>
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
							{#each animatorVisibleCandidates as candidate, candidateIndex (animatorCandidateRowKey(candidate, candidateIndex))}
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
									<div class="animator-action-value">
										<input
											type="range"
											min="0"
											max="1"
											step="0.01"
											value={selectedValueFor(candidate.id)}
											disabled={busy || selectedModeFor(candidate.id) === "off"}
											oninput={(event) => draftAnimatorActionValue(candidate.id, Number((event.currentTarget as HTMLInputElement).value))}
											onchange={(event) => updateAnimatorActionValue(candidate.id, Number((event.currentTarget as HTMLInputElement).value))}
										/>
										<input
											type="number"
											min="0"
											max="1"
											step="0.01"
											value={selectedValueFor(candidate.id).toFixed(2)}
											disabled={busy || selectedModeFor(candidate.id) === "off"}
											onchange={(event) => updateAnimatorActionValue(candidate.id, Number((event.currentTarget as HTMLInputElement).value))}
										/>
									</div>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			{/if}
		</div>
	{/if}
</section>

{#if keyboardCaptureSetId !== null || keyboardCaptureActionId !== null}
	<div class="binding-capture-backdrop" role="presentation">
		<div class="binding-capture-modal" role="dialog" aria-modal="true">
			<Keyboard size={24} />
			<strong>{$_("profiles.editor.binding_capture_title")}</strong>
			<span>{$_("profiles.editor.binding_capture_hint")}</span>
			<button
				type="button"
				class="field-button"
				onclick={() => {
					keyboardCaptureSetId = null;
					keyboardCaptureActionId = null;
				}}>{$_("profiles.editor.cancel")}</button
			>
		</div>
	</div>
{/if}

{#if midiCaptureError}
	<div class="profile-inline-note profile-inline-note-warning">{midiCaptureError}</div>
{/if}

{#if midiCaptureSetId !== null || midiCaptureActionId !== null}
	<div class="binding-capture-backdrop" role="presentation">
		<div class="binding-capture-modal" role="dialog" aria-modal="true">
			<SlidersHorizontal size={24} />
			<strong>{$_("profiles.editor.midi_capture_title")}</strong>
			<span>{$_("profiles.editor.midi_capture_hint")}</span>
			<button type="button" class="field-button" onclick={cancelWardrobeMidiCapture}>{$_("profiles.editor.cancel")}</button>
		</div>
	</div>
{/if}
