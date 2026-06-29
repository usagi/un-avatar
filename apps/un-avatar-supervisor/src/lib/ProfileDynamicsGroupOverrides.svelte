<script lang="ts">
	import { _ } from "svelte-i18n";
	import ProfileNumberInputField from "./ProfileNumberInputField.svelte";
	import ProfileSelectField from "./ProfileSelectField.svelte";
	import type { ProfileSettingValue } from "./profileTypes";
	import {
		DYNAMICS_BOUNCE_FIELD,
		DYNAMICS_DAMPING_FIELD,
		DYNAMICS_GROUP_OVERRIDE_FIELD,
		DYNAMICS_MOTION_COUPLING_FIELD,
		DYNAMICS_SHAPE_FIELD,
		DYNAMICS_STRETCH_MOTION_FIELD,
		DYNAMICS_STRETCH_RANGE_FIELD,
		DYNAMICS_VERLET_FIELDS,
		DYNAMICS_XPBD_FIELDS,
		type DynamicsGroupOverrideSetting,
	} from "./dynamicsPresets";

	export let overrides: DynamicsGroupOverrideSetting[] = [];
	export let dynamicsEnabled = false;
	export let busy = false;
	export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;

	const solverOptions = [
		["verlet", "profiles.editor.dynamics_mode_standard"],
		["xpbd", "profiles.editor.dynamics_mode_extended"],
	] as const;

	$: disabled = !dynamicsEnabled || busy;

	function updateOverride(index: number, patch: Partial<DynamicsGroupOverrideSetting>): void {
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
		void onUpdateSettingValue(DYNAMICS_GROUP_OVERRIDE_FIELD, next as Record<string, unknown>[]);
	}

	function removeOverride(index: number): void {
		const next = overrides.filter((_, itemIndex) => itemIndex !== index);
		void onUpdateSettingValue(DYNAMICS_GROUP_OVERRIDE_FIELD, next as Record<string, unknown>[]);
	}
</script>

{#if overrides.length > 0}
	<div class="profile-wide-field dynamics-overrides">
		<div class="field-note-block">
			<div class="field-note">{$_("profiles.editor.dynamics_group_overrides")}</div>
			<div>{$_("profiles.editor.dynamics_group_overrides_hint")}</div>
		</div>
		<div class="dynamics-override-list">
			{#each overrides as override, index}
				<div class="dynamics-override-row">
					<div class="dynamics-override-title">
						<strong>{override.source_id}</strong>
						<span>{override.solver ?? "verlet"}</span>
					</div>
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
						{$_("profiles.editor.dynamics_group_override_remove")}
					</button>
				</div>
			{/each}
		</div>
	</div>
{/if}
