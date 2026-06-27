<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
	import ProfileSelectField from "./ProfileSelectField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import {
		DYNAMICS_BOUNCE_FIELD,
		DYNAMICS_DAMPING_FIELD,
		DYNAMICS_MATCH_OVERRIDE_TEMPLATES,
		DYNAMICS_MATCH_OVERRIDE_FIELD,
		DYNAMICS_MOTION_COUPLING_FIELD,
		DYNAMICS_SHAPE_FIELD,
		DYNAMICS_STRETCH_MOTION_FIELD,
		DYNAMICS_STRETCH_RANGE_FIELD,
		DYNAMICS_VERLET_FIELDS,
		DYNAMICS_XPBD_FIELDS,
		type DynamicsMatchOverrideSetting,
	} from "./dynamicsPresets";

	export let overrides: DynamicsMatchOverrideSetting[] = [];
	export let dynamicsEnabled = false;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	const solverOptions = [
		["verlet", "profiles.editor.dynamics_mode_standard"],
		["xpbd", "profiles.editor.dynamics_mode_extended"],
	] as const;

	$: disabled = !dynamicsEnabled || busy;

	function listText(values: string[] | undefined): string {
		return (values ?? []).join(", ");
	}

	function textList(value: string): string[] {
		return value
			.split(",")
			.map((item) => item.trim())
			.filter(Boolean);
	}

	function cleanOverride(override: DynamicsMatchOverrideSetting): DynamicsMatchOverrideSetting {
		const next: DynamicsMatchOverrideSetting = { ...override };
		next.name = next.name?.trim() || undefined;
		next.source_id = next.source_id?.trim() || undefined;
		next.source_id_contains = (next.source_id_contains ?? []).map((item) => item.trim()).filter(Boolean);
		next.source_id_regex = (next.source_id_regex ?? []).map((item) => item.trim()).filter(Boolean);
		if (next.source_id_contains.length === 0) delete next.source_id_contains;
		if (next.source_id_regex.length === 0) delete next.source_id_regex;
		return next;
	}

	function persist(next: DynamicsMatchOverrideSetting[]): void {
		const clean = next
			.map(cleanOverride)
			.filter(
				(override) =>
					(override.source_id?.length ?? 0) > 0 ||
					(override.source_id_contains?.length ?? 0) > 0 ||
					(override.source_id_regex?.length ?? 0) > 0
			);
		void onUpdateSettingValue(DYNAMICS_MATCH_OVERRIDE_FIELD, clean as Record<string, unknown>[]);
	}

	function updateOverride(index: number, patch: Partial<DynamicsMatchOverrideSetting>): void {
		const next = overrides.map((override, itemIndex) => {
			if (itemIndex !== index) return { ...override };
			const updated = { ...override, ...patch };
			if (patch.solver === "xpbd") {
				updated.xpbd_compliance ??= 0.025;
				updated.constraint_iterations ??= 4;
			} else if (patch.solver === "verlet") {
				delete updated.xpbd_compliance;
				delete updated.constraint_iterations;
			}
			return updated;
		});
		persist(next);
	}

	function addOverride(): void {
		persist([
			...overrides,
			{
				name: $_("profiles.editor.dynamics_match_override_new_name"),
				source_id_contains: ["new_match"],
				source_id_regex: [],
				solver: "verlet",
				rest_response: 0.08,
				shape_preservation: 0.04,
				motion_coupling: 0.5,
			},
		]);
	}

	function addTemplate(templateOverride: DynamicsMatchOverrideSetting): void {
		persist([
			...overrides,
			{
				...templateOverride,
				source_id_contains: [...(templateOverride.source_id_contains ?? [])],
				source_id_regex: [...(templateOverride.source_id_regex ?? [])],
			},
		]);
	}

	function removeOverride(index: number): void {
		persist(overrides.filter((_, itemIndex) => itemIndex !== index));
	}

	function moveOverride(index: number, direction: -1 | 1): void {
		const target = index + direction;
		if (target < 0 || target >= overrides.length) return;
		const next = overrides.map((override) => ({ ...override }));
		const [item] = next.splice(index, 1);
		next.splice(target, 0, item);
		persist(next);
	}
</script>

<div class="profile-wide-field dynamics-overrides">
	<div class="field-note-block">
		<div class="field-note">{$_("profiles.editor.dynamics_match_overrides")}</div>
		<div>{$_("profiles.editor.dynamics_match_overrides_hint")}</div>
	</div>
	<div class="dynamics-override-list">
		<div class="dynamics-template-actions">
			<span>{$_("profiles.editor.dynamics_match_templates")}</span>
			{#each DYNAMICS_MATCH_OVERRIDE_TEMPLATES as template}
				<button type="button" class="secondary" {disabled} onclick={() => addTemplate(template.override)}>
					{$_(template.labelKey)}
				</button>
			{/each}
		</div>
		{#each overrides as override, index}
			<div class="dynamics-override-row">
				<div class="dynamics-override-title">
					<strong>{override.name || override.source_id || override.source_id_contains?.join(", ") || override.source_id_regex?.join(", ")}</strong>
					<span>{override.solver ?? "verlet"}</span>
				</div>
				<div class="dynamics-override-actions">
					<button type="button" class="secondary" disabled={disabled || index === 0} onclick={() => moveOverride(index, -1)}>
						{$_("profiles.editor.dynamics_match_override_move_up")}
					</button>
					<button
						type="button"
						class="secondary"
						disabled={disabled || index === overrides.length - 1}
						onclick={() => moveOverride(index, 1)}
					>
						{$_("profiles.editor.dynamics_match_override_move_down")}
					</button>
				</div>
				<label data-hint={$_("profiles.editor.dynamics_match_name_hint")}>
					{$_("profiles.editor.dynamics_match_name")}
					<input
						value={override.name ?? ""}
						{disabled}
						onchange={(event) => updateOverride(index, { name: (event.currentTarget as HTMLInputElement).value })}
					/>
				</label>
				<label data-hint={$_("profiles.editor.dynamics_match_source_id_hint")}>
					{$_("profiles.editor.dynamics_match_source_id")}
					<input
						value={override.source_id ?? ""}
						{disabled}
						onchange={(event) => updateOverride(index, { source_id: (event.currentTarget as HTMLInputElement).value })}
					/>
				</label>
				<label data-hint={$_("profiles.editor.dynamics_match_contains_hint")}>
					{$_("profiles.editor.dynamics_match_contains")}
					<input
						value={listText(override.source_id_contains)}
						{disabled}
						onchange={(event) =>
							updateOverride(index, { source_id_contains: textList((event.currentTarget as HTMLInputElement).value) })}
					/>
				</label>
				<label data-hint={$_("profiles.editor.dynamics_match_regex_hint")}>
					{$_("profiles.editor.dynamics_match_regex")}
					<input
						value={listText(override.source_id_regex)}
						{disabled}
						onchange={(event) =>
							updateOverride(index, { source_id_regex: textList((event.currentTarget as HTMLInputElement).value) })}
					/>
				</label>
				<ProfileSelectField
					label={$_("profiles.editor.dynamics_mode")}
					value={override.solver ?? "verlet"}
					{disabled}
					options={solverOptions}
					onChange={(solver) => updateOverride(index, { solver })}
				/>
				<ProfileNumberInputField
					label={$_(DYNAMICS_DAMPING_FIELD.labelKey)}
					hint={$_(DYNAMICS_DAMPING_FIELD.hintKey)}
					value={override.damping_half_life_ms}
					min={DYNAMICS_DAMPING_FIELD.min}
					max={DYNAMICS_DAMPING_FIELD.max}
					step={DYNAMICS_DAMPING_FIELD.step}
					placeholder="-"
					{disabled}
					onChange={(value) => updateOverride(index, { damping_half_life_ms: value })}
				/>
				<ProfileNumberInputField
					label={$_(DYNAMICS_BOUNCE_FIELD.labelKey)}
					hint={$_(DYNAMICS_BOUNCE_FIELD.hintKey)}
					value={override.bounce_scale}
					min={DYNAMICS_BOUNCE_FIELD.min}
					max={DYNAMICS_BOUNCE_FIELD.max}
					step={DYNAMICS_BOUNCE_FIELD.step}
					placeholder="-"
					{disabled}
					onChange={(value) => updateOverride(index, { bounce_scale: value })}
				/>
				<ProfileNumberInputField
					label={$_(DYNAMICS_MOTION_COUPLING_FIELD.labelKey)}
					hint={$_(DYNAMICS_MOTION_COUPLING_FIELD.hintKey)}
					value={override.motion_coupling}
					min={DYNAMICS_MOTION_COUPLING_FIELD.min}
					max={DYNAMICS_MOTION_COUPLING_FIELD.max}
					step={DYNAMICS_MOTION_COUPLING_FIELD.step}
					placeholder="-"
					{disabled}
					onChange={(value) => updateOverride(index, { motion_coupling: value })}
				/>
				<ProfileNumberInputField
					label={$_(DYNAMICS_STRETCH_RANGE_FIELD.labelKey)}
					hint={$_(DYNAMICS_STRETCH_RANGE_FIELD.hintKey)}
					value={override.stretch_range_scale}
					min={DYNAMICS_STRETCH_RANGE_FIELD.min}
					max={DYNAMICS_STRETCH_RANGE_FIELD.max}
					step={DYNAMICS_STRETCH_RANGE_FIELD.step}
					placeholder="-"
					{disabled}
					onChange={(value) => updateOverride(index, { stretch_range_scale: value })}
				/>
				<ProfileNumberInputField
					label={$_(DYNAMICS_STRETCH_MOTION_FIELD.labelKey)}
					hint={$_(DYNAMICS_STRETCH_MOTION_FIELD.hintKey)}
					value={override.stretch_motion}
					min={DYNAMICS_STRETCH_MOTION_FIELD.min}
					max={DYNAMICS_STRETCH_MOTION_FIELD.max}
					step={DYNAMICS_STRETCH_MOTION_FIELD.step}
					placeholder="-"
					{disabled}
					onChange={(value) => updateOverride(index, { stretch_motion: value })}
				/>
				<ProfileNumberInputField
					label={$_(DYNAMICS_SHAPE_FIELD.labelKey)}
					hint={$_(DYNAMICS_SHAPE_FIELD.hintKey)}
					value={override.shape_preservation}
					min={DYNAMICS_SHAPE_FIELD.min}
					max={DYNAMICS_SHAPE_FIELD.max}
					step={DYNAMICS_SHAPE_FIELD.step}
					placeholder="-"
					{disabled}
					onChange={(value) => updateOverride(index, { shape_preservation: value })}
				/>
				{#each DYNAMICS_VERLET_FIELDS as field}
					<ProfileNumberInputField
						label={$_(field.labelKey)}
						hint={$_(field.hintKey)}
						value={override.rest_response}
						min={field.min}
						max={field.max}
						step={field.step}
						placeholder="-"
						{disabled}
						onChange={(value) => updateOverride(index, { rest_response: value })}
					/>
				{/each}
				{#if (override.solver ?? "verlet") === "xpbd"}
					{#each DYNAMICS_XPBD_FIELDS as field}
						<ProfileNumberInputField
							label={$_(field.labelKey)}
							hint={$_(field.hintKey)}
							value={override[field.key]}
							min={field.min}
							max={field.max}
							step={field.step}
							placeholder="-"
							{disabled}
							onChange={(value) => updateOverride(index, { [field.key]: value })}
						/>
					{/each}
				{/if}
				<button type="button" class="secondary" {disabled} onclick={() => removeOverride(index)}>
					{$_("profiles.editor.dynamics_match_override_remove")}
				</button>
			</div>
		{/each}
		<button type="button" class="secondary" {disabled} onclick={addOverride}>
			{$_("profiles.editor.dynamics_match_override_add")}
		</button>
	</div>
</div>
