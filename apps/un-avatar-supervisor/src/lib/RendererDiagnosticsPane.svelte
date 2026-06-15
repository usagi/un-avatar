<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Power } from "lucide-svelte";
	import type { RendererPaneActions } from "./rendererPaneActions";
	import type { RendererDiagnosticsData, RendererRuntimeDiagnosticsData } from "./rendererTypes";

	export let renderer: RendererDiagnosticsData;
	export let runtimeStatus: RendererRuntimeDiagnosticsData | null;
	export let busy = false;
	export let onSetDynamicsEnabled: RendererPaneActions["onSetDynamicsEnabled"];

	const sampleLimit = 4;

	$: rendererRunning = renderer.pid != null;
	$: dynamicsGroups = runtimeStatus?.dynamics_groups ?? [];
	$: canSetDynamicsEnabled = runtimeStatus?.control_capabilities?.includes("set_dynamics_enabled") ?? false;

	function sampledLines<T>(items: T[], label: (item: T) => string): string {
		const lines = items.slice(0, sampleLimit).map(label);
		const omitted = items.length - lines.length;
		if (omitted > 0) lines.push(`... ${omitted} more`);
		return lines.join("\n");
	}

	function sampledStrings(items: string[]): string {
		return sampledLines(items, (item) => item);
	}

	function groupLabel(group: RendererRuntimeDiagnosticsData["dynamics_groups"][number]): string {
		const path = group.root_path ?? group.source_id ?? `#${group.index}`;
		const state = group.effective_enabled ? "on" : "off";
		const sourceState = group.authored_enabled ? "on" : "off";
		const override = group.runtime_enabled_override == null ? "" : `, override=${group.runtime_enabled_override}`;
		const parameter = group.interaction_parameter ? `, param=${group.interaction_parameter}` : "";
		const writeback = group.writeback_mode ? `, writeback=${group.writeback_mode}` : "";
		const translationCandidates =
			group.translation_writeback_candidate_count == null
				? ""
				: `, translationCandidates=${group.translation_writeback_candidate_count}`;
		const translationTargets =
			group.translation_writeback_target_count == null ? "" : `, translationTargets=${group.translation_writeback_target_count}`;
		return `${path} (${group.source_kind}, ${state}, source=${sourceState}${override}, bones=${group.bone_count}${writeback}${translationCandidates}${translationTargets}${parameter})`;
	}

	function interactionHookLabel(hook: RendererRuntimeDiagnosticsData["dynamics_interaction_hooks"][number]): string {
		const path = hook.root_path ?? hook.source_id ?? `#${hook.group_index}`;
		const state = hook.effective_enabled ? "on" : "off";
		const parameter = hook.parameter ? `, param=${hook.parameter}` : "";
		const suffixCount = hook.suffix_parameters?.length ?? 0;
		return `${path} (${hook.source_kind}, ${state}, grab=${hook.allow_grabbing}, pose=${hook.allow_posing}${parameter}, suffixes=${suffixCount}${hook.metadata_only ? ", metadata-only" : ""})`;
	}

	function colliderLabel(collider: RendererRuntimeDiagnosticsData["dynamics_colliders"][number]): string {
		const path = collider.node_path ?? `#${collider.node}`;
		return `${path} (${collider.source_kind}, ${collider.shape}, r=${collider.radius})`;
	}

	function contactDeclarationLabel(declaration: RendererRuntimeDiagnosticsData["contact_parameter_declarations"][number]): string {
		const path = declaration.node_path ?? `#${declaration.node}`;
		return `${declaration.parameter} @ ${path}`;
	}

	function contactProbeLabel(probe: RendererRuntimeDiagnosticsData["contact_probes"][number]): string {
		const receiver = probe.receiver_node_path ?? `#${probe.receiver_node}`;
		const sender = probe.sender_node_path ?? `#${probe.sender_node}`;
		return `${probe.parameter}: ${receiver} <- ${sender} (${probe.would_emit ? "would emit" : "idle"})`;
	}

	function contactEmissionLabel(emission: RendererRuntimeDiagnosticsData["contact_parameter_emissions"][number]): string {
		const receiver = emission.receiver_node_path ?? `#${emission.receiver_node}`;
		return `${emission.parameter}: ${emission.value} @ ${receiver} (${emission.emitted ? "emitted" : "reset"})`;
	}

	function constraintRefLabel(ref: RendererRuntimeDiagnosticsData["dynamics_constraint_refs"][number]): string {
		const target = ref.target_path ?? `#${ref.target_node}`;
		return `${ref.constraint_type ?? "constraint"} @ ${target} (${ref.source_kind})`;
	}

	function runtimeActionLabel(action: RendererRuntimeDiagnosticsData["runtime_actions"][number]): string {
		const label = action.label || action.action_id;
		const state = action.current_condition_state ?? "unconditioned";
		const parameters = action.condition_parameter_names?.join(",") || action.parameter_name || "-";
		const targetCounts = [
			action.target_writes?.length ? `writes=${action.target_writes.length}` : null,
			action.node_visibility_effects?.length ? `nodes=${action.node_visibility_effects.length}` : null,
			action.material_property_effects?.length ? `matProps=${action.material_property_effects.length}` : null,
			action.material_slot_effects?.length ? `matSlots=${action.material_slot_effects.length}` : null,
			action.expression_weight_effects?.length ? `expr=${action.expression_weight_effects.length}` : null,
			action.dynamics_enabled_effects?.length ? `dyn=${action.dynamics_enabled_effects.length}` : null,
		].filter(Boolean);
		const targets = targetCounts.length ? ` ${targetCounts.join(",")}` : "";
		return `${label}: ${state} [${parameters}]${targets}`;
	}

	function menuActionCandidateLabel(candidate: RendererRuntimeDiagnosticsData["menu_action_candidates"][number]): string {
		const label = candidate.menu_path?.length ? candidate.menu_path.join(" / ") : candidate.menu_label || candidate.action_label;
		const wardrobe = candidate.wardrobe_set_ids?.length ? ` wardrobe=${candidate.wardrobe_set_ids.join(",")}` : "";
		const effects = candidate.effect_kinds
			? Object.entries(candidate.effect_kinds)
					.map(([kind, count]) => `${kind}=${count}`)
					.join(",")
			: `effects=${candidate.effect_count}`;
		return `${label}: ${candidate.parameter_name}=${candidate.parameter_value} (${candidate.match_kind}, ${effects}${wardrobe}${candidate.menu_path_truncated ? ", truncated" : ""})`;
	}

	function wardrobeMenuCandidateLabel(candidate: RendererRuntimeDiagnosticsData["menu_wardrobe_candidates"][number]): string {
		const label = candidate.menu_path?.length ? candidate.menu_path.join(" / ") : candidate.menu_label || candidate.wardrobe_set_id;
		return `${label}: wardrobe=${candidate.wardrobe_set_id} action=${candidate.action_id}${candidate.menu_path_truncated ? " truncated" : ""}`;
	}

	function runtimeParameterDefinitionLabel(definition: RendererRuntimeDiagnosticsData["runtime_parameter_definitions"][number]): string {
		const current = definition.current_value == null ? "-" : definition.current_value;
		return `${definition.name}: sources=${definition.source_kinds?.join(",") ?? "-"} values=${definition.value_samples?.join(",") ?? "-"} current=${current}${definition.transient ? " transient" : ""}`;
	}

	function runtimeParameterConflictLabel(conflict: RendererRuntimeDiagnosticsData["runtime_parameter_conflicts"][number]): string {
		return `${conflict.name}: ${conflict.reason} owners=${conflict.owner_keys?.join(",") ?? "-"} sources=${conflict.source_kinds?.join(",") ?? "-"}`;
	}

	function runtimeActionCollisionLabel(
		collision: RendererRuntimeDiagnosticsData["runtime_action_target_write_collisions"][number]
	): string {
		return `${collision.target_kind}:${collision.target_key} owners=${collision.owner_keys.length} actions=${collision.action_ids.join(",")}`;
	}

	function runtimeActionRestoreReadinessLabel(
		readiness: RendererRuntimeDiagnosticsData["runtime_action_restore_readiness"][number]
	): string {
		return `${readiness.owner_key}:${readiness.effect_kind} ${readiness.target_kind}:${readiness.target_key} ready=${readiness.ready} reason=${readiness.reason}`;
	}

	function runtimeActionRestoreBaselineLabel(
		candidate: RendererRuntimeDiagnosticsData["runtime_action_restore_baseline_candidates"][number]
	): string {
		return `${candidate.owner_key}:${candidate.effect_kind} ${candidate.target_kind}:${candidate.target_key} value=${JSON.stringify(candidate.baseline_value)}`;
	}

	function runtimeActionRestoreCaptureLabel(
		entry: RendererRuntimeDiagnosticsData["runtime_action_restore_baseline_capture_plan"][number]
	): string {
		return `${entry.owner_key} ${entry.target_kind}:${entry.target_key} value=${JSON.stringify(entry.baseline_value)} actions=${entry.source_action_ids.join(",")}`;
	}

	function runtimeActionRestoreApplyLabel(entry: RendererRuntimeDiagnosticsData["runtime_action_restore_apply_plan"][number]): string {
		return `${entry.owner_key} state=${entry.condition_state ?? "none"} ${entry.target_kind}:${entry.target_key} ready=${entry.ready} reason=${entry.reason} baseline=${JSON.stringify(entry.baseline_value)} current=${JSON.stringify(entry.current_value)}`;
	}

	function wardrobeResidencyLabel(upload: NonNullable<RendererRuntimeDiagnosticsData["wardrobe_asset_upload"]>): string {
		const pendingCube = upload.pending_cube_texture_upload_count ?? 0;
		const inactiveCube = upload.inactive_cube_textures_used_by_active_draw_count ?? 0;
		const activeCubeDraws = upload.active_draws_using_inactive_cube_texture_count ?? 0;
		const groups = upload.active_asset_groups?.length ? upload.active_asset_groups.join(",") : "-";
		const missingGroups = upload.missing_active_asset_groups?.length ? upload.missing_active_asset_groups.join(",") : "-";
		const meshBytes =
			upload.total_draw_mesh_buffer_bytes == null
				? "-"
				: `${upload.resident_draw_mesh_buffer_bytes ?? 0}/${upload.total_draw_mesh_buffer_bytes}`;
		const inactiveImages = upload.inactive_image_textures_used_by_active_draw?.length
			? upload.inactive_image_textures_used_by_active_draw.join(",")
			: "-";
		const inactiveMaterials = upload.inactive_material_slots_used_by_active_draw?.length
			? upload.inactive_material_slots_used_by_active_draw.join(",")
			: "-";
		const imagePreviewSuffix = upload.inactive_image_textures_used_by_active_draw_truncated ? ",..." : "";
		const materialPreviewSuffix = upload.inactive_material_slots_used_by_active_draw_truncated ? ",..." : "";
		return [
			`active=${runtimeStatus?.active_wardrobe_set ?? "-"}`,
			`mode=${upload.mode}`,
			`groups=${groups}`,
			`missing=${missingGroups}`,
			`allResident=${upload.all_resident}`,
			`scopedUpload=${upload.scoped_upload_supported}`,
			`resident source mesh=${upload.resident_mesh_primitive_count}/${upload.owned_mesh_primitive_count}`,
			`material=${upload.resident_material_count}/${upload.owned_material_count}`,
			`image=${upload.resident_image_count}/${upload.owned_image_count}`,
			`dynamics=${upload.resident_dynamics_count}/${upload.owned_dynamics_count}`,
			`draw mesh=${upload.resident_draw_mesh_primitive_count}/${upload.total_draw_mesh_primitive_count}`,
			`meshBytes=${meshBytes}`,
			`image=${upload.resident_image_texture_count}/${upload.total_image_texture_count}`,
			`material=${upload.resident_material_slot_count}/${upload.total_material_slot_count}`,
			`pending image=${upload.pending_image_texture_upload_count}`,
			`cube=${pendingCube}`,
			`material=${upload.pending_material_slot_upload_count}`,
			`activeGaps imageDraws=${upload.active_draws_using_inactive_image_texture_count}`,
			`cubeDraws=${activeCubeDraws}`,
			`materialDraws=${upload.active_draws_using_inactive_material_slot_count}`,
			`inactiveActive image=${upload.inactive_image_textures_used_by_active_draw_count}[${inactiveImages}${imagePreviewSuffix}]`,
			`cube=${inactiveCube}`,
			`material=${upload.inactive_material_slots_used_by_active_draw_count}[${inactiveMaterials}${materialPreviewSuffix}]`,
			`lastLoad mesh=${upload.last_mesh_buffer_scoped_load_count}/${upload.last_mesh_buffer_scoped_unload_count}`,
			`image=${upload.last_image_texture_scoped_load_count}/${upload.last_image_texture_scoped_unload_count}`,
			`cube=${upload.last_cubemap_scoped_load_count}/${upload.last_cubemap_scoped_unload_count}`,
			`material=${upload.last_material_slot_scoped_upload_count}`,
		].join(" ");
	}

	function toggleDynamicsGroup(group: RendererRuntimeDiagnosticsData["dynamics_groups"][number]): void {
		if (!canSetDynamicsEnabled || !group.source_id) return;
		void onSetDynamicsEnabled(renderer.id, group.source_id, !group.effective_enabled);
	}
</script>

<div class="renderer-pane-scroll">
	<dl>
		<dt>{$_("renderers.details.diag_last_stderr")}</dt>
		<dd class="stderr-block">
			{renderer.last_stderr ?? $_("renderers.details.value_none")}
		</dd>
		{#if renderer.stderr_tail?.length}
			<dt>{$_("renderers.details.diag_stderr_tail")}</dt>
			<dd class="stderr-block">
				{renderer.stderr_tail.join("\n")}
			</dd>
		{/if}
		{#if renderer.exit_code != null}
			<dt>{$_("renderers.details.diag_exit_code")}</dt>
			<dd>{renderer.exit_code}</dd>
		{/if}
		{#if runtimeStatus?.note}
			<dt>{$_("renderers.details.diag_runtime_note")}</dt>
			<dd>{runtimeStatus.note}</dd>
		{/if}
		{#if runtimeStatus}
			{#if runtimeStatus.wardrobe_asset_upload}
				<dt>{$_("renderers.details.diag_wardrobe_residency")}</dt>
				<dd class="stderr-block">{wardrobeResidencyLabel(runtimeStatus.wardrobe_asset_upload)}</dd>
			{/if}
			<dt>{$_("renderers.details.diag_dynamics_summary")}</dt>
			<dd>
				{$_("renderers.details.diag_dynamics_summary_value", {
					values: {
						groups: runtimeStatus.dynamics_group_count,
						enabled: runtimeStatus.dynamics_enabled_group_count,
						sourceEnabled: runtimeStatus.dynamics_source_enabled_group_count,
						overrides: runtimeStatus.dynamics_enabled_override_count,
						colliders: runtimeStatus.dynamics_collider_count,
						contacts: runtimeStatus.dynamics_contact_count,
						probes: runtimeStatus.dynamics_contact_probe_count,
						wouldEmit: runtimeStatus.dynamics_contact_probe_would_emit_count,
						emitted: runtimeStatus.dynamics_contact_parameter_emitted_count,
						constraints: runtimeStatus.dynamics_constraint_ref_count,
						writeback: runtimeStatus.dynamics_rotation_translation_writeback_group_count,
						writebackCandidates: runtimeStatus.dynamics_translation_writeback_candidate_count,
						writebackTargets: runtimeStatus.dynamics_translation_writeback_target_count,
						stretchWriteback: runtimeStatus.dynamics_stretch_translation_writeback_group_count,
						stretchWritebackTargets: runtimeStatus.dynamics_stretch_translation_writeback_target_group_count,
					},
				})}
			</dd>
			{#if runtimeStatus.dynamics_warnings.length}
				<dt>{$_("renderers.details.diag_dynamics_warnings")}</dt>
				<dd class="stderr-block">{sampledStrings(runtimeStatus.dynamics_warnings)}</dd>
			{/if}
			{#if runtimeStatus.dynamics_groups.length}
				<dt>{$_("renderers.details.diag_dynamics_groups")}</dt>
				<dd class="diagnostics-action-list">
					{#each dynamicsGroups as group}
						<div class="diagnostics-action-row">
							<code>{groupLabel(group)}</code>
							<button
								type="button"
								class:active={group.effective_enabled}
								disabled={busy || !rendererRunning || !canSetDynamicsEnabled || !group.source_id}
								title={group.source_id ?? group.root_path ?? `#${group.index}`}
								onclick={() => toggleDynamicsGroup(group)}
							>
								<Power size={13} />
								<span>
									{group.effective_enabled
										? $_("renderers.details.dynamics_disable")
										: $_("renderers.details.dynamics_enable")}
								</span>
							</button>
						</div>
					{/each}
				</dd>
			{/if}
			{#if runtimeStatus.dynamics_interaction_hooks.length}
				<dt>{$_("renderers.details.diag_dynamics_interaction_hooks")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.dynamics_interaction_hooks, interactionHookLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.dynamics_colliders.length}
				<dt>{$_("renderers.details.diag_dynamics_colliders")}</dt>
				<dd class="stderr-block">{sampledLines(runtimeStatus.dynamics_colliders, colliderLabel)}</dd>
			{/if}
			{#if runtimeStatus.contact_parameter_declarations.length}
				<dt>{$_("renderers.details.diag_contact_parameters")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.contact_parameter_declarations, contactDeclarationLabel)}
				</dd>
			{/if}
			<dt>{$_("renderers.details.diag_contact_parameter_emission")}</dt>
			<dd>
				{runtimeStatus.contact_parameter_emission_enabled ? "enabled" : "disabled"}
				({runtimeStatus.dynamics_contact_parameter_emitted_count}/{runtimeStatus.dynamics_contact_parameter_emission_count}, reset={runtimeStatus.dynamics_contact_parameter_reset_to_zero_count})
			</dd>
			{#if runtimeStatus.contact_parameter_emissions.length}
				<dt>{$_("renderers.details.diag_contact_parameter_emissions")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.contact_parameter_emissions, contactEmissionLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.contact_probes.length}
				<dt>{$_("renderers.details.diag_contact_probes")}</dt>
				<dd class="stderr-block">{sampledLines(runtimeStatus.contact_probes, contactProbeLabel)}</dd>
			{/if}
			{#if runtimeStatus.dynamics_constraint_refs.length}
				<dt>{$_("renderers.details.diag_dynamics_constraints")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.dynamics_constraint_refs, constraintRefLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.runtime_actions.length}
				<dt>{$_("renderers.details.diag_runtime_actions")}</dt>
				<dd class="stderr-block">{sampledLines(runtimeStatus.runtime_actions, runtimeActionLabel)}</dd>
			{/if}
			{#if runtimeStatus.menu_action_candidates.length}
				<dt>{$_("renderers.details.diag_vrc_menu_actions")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.menu_action_candidates, menuActionCandidateLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.menu_wardrobe_candidates.length}
				<dt>{$_("renderers.details.diag_wardrobe_menu_candidates")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.menu_wardrobe_candidates, wardrobeMenuCandidateLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.runtime_parameter_definitions.length}
				<dt>{$_("renderers.details.diag_runtime_parameters")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.runtime_parameter_definitions, runtimeParameterDefinitionLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.runtime_parameter_conflicts.length}
				<dt>{$_("renderers.details.diag_runtime_parameter_conflicts")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.runtime_parameter_conflicts, runtimeParameterConflictLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.runtime_action_target_write_collisions.length}
				<dt>{$_("renderers.details.diag_runtime_action_collisions")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.runtime_action_target_write_collisions, runtimeActionCollisionLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.runtime_action_restore_readiness.length}
				<dt>{$_("renderers.details.diag_runtime_action_restore")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.runtime_action_restore_readiness, runtimeActionRestoreReadinessLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.runtime_action_restore_baseline_candidates.length}
				<dt>{$_("renderers.details.diag_runtime_action_restore_baseline")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.runtime_action_restore_baseline_candidates, runtimeActionRestoreBaselineLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.runtime_action_restore_baseline_capture_plan.length}
				<dt>{$_("renderers.details.diag_runtime_action_restore_capture")}</dt>
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.runtime_action_restore_baseline_capture_plan, runtimeActionRestoreCaptureLabel)}
				</dd>
			{/if}
			{#if runtimeStatus.runtime_action_restore_apply_plan.length}
				<dt>{$_("renderers.details.diag_runtime_action_restore_apply")}</dt>
				<dd class="stderr-block">{sampledLines(runtimeStatus.runtime_action_restore_apply_plan, runtimeActionRestoreApplyLabel)}</dd>
			{/if}
		{/if}
	</dl>
</div>
