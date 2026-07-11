<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Power, SlidersHorizontal } from "lucide-svelte";
	import type { DynamicsGroupOverrideSeed, DynamicsMatchOverrideSeed, RendererPaneActions } from "./rendererPaneActions";
	import type { RendererDiagnosticsData, RendererRuntimeDiagnosticsData } from "./rendererTypes";

	export let renderer: RendererDiagnosticsData;
	export let runtimeStatus: RendererRuntimeDiagnosticsData | null;
	export let busy = false;
	export let onSetDynamicsEnabled: RendererPaneActions["onSetDynamicsEnabled"];
	export let onAddDynamicsMatchOverride: RendererPaneActions["onAddDynamicsMatchOverride"];
	export let onAddDynamicsGroupOverride: RendererPaneActions["onAddDynamicsGroupOverride"];

	const sampleLimit = 4;

	$: rendererRunning = renderer.pid != null;
	$: dynamicsGroups = runtimeStatus?.dynamics_groups ?? [];
	$: dynamicsResponseCategories = runtimeStatus?.dynamics_response_categories ?? [];
	$: dynamicsResponseGroups = runtimeStatus?.dynamics_response_groups ?? [];
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
		const response = group.source_id ? responseGroupForSource(group.source_id) : undefined;
		const responseSummary = response
			? `, category=${response.category}, rest=${response.average_rest_response?.toFixed(3) ?? response.average_pull.toFixed(3)}, shape=${response.average_shape_preservation.toFixed(3)}, follow=${response.average_parent_motion_follow.toFixed(3)}`
			: "";
		const responseOverride =
			response?.matched_overrides?.length || response?.group_override_applied
				? `, tuned=${[...(response.matched_overrides ?? []), ...(response.group_override_applied ? ["exact"] : [])].join("|")}`
				: "";
		const translationCandidates =
			group.translation_writeback_candidate_count == null
				? ""
				: `, translationCandidates=${group.translation_writeback_candidate_count}`;
		const translationTargets =
			group.translation_writeback_target_count == null ? "" : `, translationTargets=${group.translation_writeback_target_count}`;
		const visibility =
			group.visual_target == null
				? ""
				: `, visual=${group.visual_target}, skinnedJoints=${group.skinned_joint_count ?? 0}, meshSubtrees=${group.mesh_subtree_node_count ?? 0}`;
		return `${path} (${group.source_kind}, ${state}, source=${sourceState}${override}, bones=${group.bone_count}${visibility}${responseSummary}${responseOverride}${writeback}${translationCandidates}${translationTargets}${parameter})`;
	}

	function responseCategoryLabel(category: RendererRuntimeDiagnosticsData["dynamics_response_categories"][number]): string {
		const fmt = (value: number) => value.toFixed(3);
		const fmtRange = (average: number, min?: number, max?: number) =>
			min == null || max == null ? fmt(average) : `${fmt(average)}[${fmt(min)}..${fmt(max)}]`;
		const compliance =
			category.xpbd_group_count > 0
				? ` xpbd=${category.xpbd_group_count} compliance=${category.average_xpbd_compliance.toFixed(5)}`
				: "";
		const visibility =
			category.visual_target_group_count == null && category.visible_skinned_joint_count == null
				? ""
				: ` visualGroups=${category.visual_target_group_count ?? 0} nonvisualGroups=${category.nonvisual_group_count ?? 0} visibleJoints=${category.visible_skinned_joint_count ?? 0} meshSubtrees=${category.visible_mesh_subtree_node_count ?? 0}`;
		const matched = category.matched_override_group_count ? ` matched=${category.matched_override_group_count}` : "";
		const exact = category.group_override_group_count ? ` exact=${category.group_override_group_count}` : "";
		const rest = category.average_rest_response ?? category.average_pull;
		const bounce = category.average_bounce_response ?? category.average_spring;
		const shape = category.average_shape_preservation;
		const orient = category.average_orientation_follow ?? shape * category.average_parent_motion_follow;
		const damping = category.average_damping_half_life_ms == null ? "" : ` damp=${fmt(category.average_damping_half_life_ms)}`;
		const stretch =
			category.average_max_stretch_response == null || category.average_max_stretch_response === 0
				? ""
				: ` stretch=${fmtRange(category.average_max_stretch_response, category.min_max_stretch_response, category.max_max_stretch_response)}`;
		const squish =
			category.average_max_squish_response == null || category.average_max_squish_response === 0
				? ""
				: ` squish=${fmtRange(category.average_max_squish_response, category.min_max_squish_response, category.max_max_squish_response)}`;
		const stretchMotion =
			category.average_stretch_motion_response == null || (stretch === "" && squish === "")
				? ""
				: ` stretchMotion=${fmtRange(category.average_stretch_motion_response, category.min_stretch_motion_response, category.max_stretch_motion_response)}`;
		return `${category.category}: groups=${category.group_count} joints=${category.joint_count}${visibility}${matched}${exact}${compliance} rest=${fmtRange(rest, category.min_rest_response, category.max_rest_response)} shape=${fmtRange(shape, category.min_shape_preservation, category.max_shape_preservation)} bounce=${fmtRange(bounce, category.min_bounce_response, category.max_bounce_response)}${stretch}${squish}${stretchMotion} drag=${fmt(category.average_drag_force)}${damping} follow=${fmtRange(category.average_parent_motion_follow, category.min_parent_motion_follow, category.max_parent_motion_follow)} orient=${fmt(orient)}`;
	}

	function responseGroupLabel(group: RendererRuntimeDiagnosticsData["dynamics_response_groups"][number]): string {
		const fmt = (value: number) => value.toFixed(3);
		const fmtRange = (average: number, min?: number, max?: number) =>
			min == null || max == null ? fmt(average) : `${fmt(average)}[${fmt(min)}..${fmt(max)}]`;
		const compliance = group.solver === "xpbd" ? ` compliance=${group.xpbd_compliance.toFixed(5)}` : "";
		const rest = group.average_rest_response ?? group.average_pull;
		const bounce = group.average_bounce_response ?? group.average_spring;
		const damping = group.average_damping_half_life_ms == null ? "" : ` damp=${fmt(group.average_damping_half_life_ms)}`;
		const overrides = group.matched_overrides?.length ? ` overrides=${group.matched_overrides.join(",")}` : "";
		const exact = group.group_override_applied ? " exact=true" : "";
		const invalidRegexes = group.invalid_match_regexes?.length ? ` invalidRegex=${group.invalid_match_regexes.join(" | ")}` : "";
		const stretch =
			group.average_max_stretch_response == null || group.average_max_stretch_response === 0
				? ""
				: ` stretch=${fmtRange(group.average_max_stretch_response, group.min_max_stretch_response, group.max_max_stretch_response)}`;
		const squish =
			group.average_max_squish_response == null || group.average_max_squish_response === 0
				? ""
				: ` squish=${fmtRange(group.average_max_squish_response, group.min_max_squish_response, group.max_max_squish_response)}`;
		const stretchMotion =
			group.average_stretch_motion_response == null || (stretch === "" && squish === "")
				? ""
				: ` stretchMotion=${fmtRange(group.average_stretch_motion_response, group.min_stretch_motion_response, group.max_stretch_motion_response)}`;
		return `${group.source_id}: ${group.category} joints=${group.joint_count} solver=${group.solver}${compliance} rest=${fmtRange(rest, group.min_rest_response, group.max_rest_response)} shape=${fmtRange(group.average_shape_preservation, group.min_shape_preservation, group.max_shape_preservation)} bounce=${fmtRange(bounce, group.min_bounce_response, group.max_bounce_response)}${stretch}${squish}${stretchMotion} drag=${fmt(group.average_drag_force)}${damping} follow=${fmtRange(group.average_parent_motion_follow, group.min_parent_motion_follow, group.max_parent_motion_follow)} orient=${fmt(group.average_orientation_follow)}${overrides}${exact}${invalidRegexes}`;
	}

	function interactionHookLabel(hook: RendererRuntimeDiagnosticsData["dynamics_interaction_hooks"][number]): string {
		const path = hook.root_path ?? hook.source_id ?? `#${hook.group_index}`;
		const state = hook.effective_enabled ? "on" : "off";
		const parameter = hook.parameter ? `, param=${hook.parameter}` : "";
		const suffixCount = hook.suffix_parameters?.length ?? 0;
		return `${path} (${hook.source_kind}, ${state}, grab=${hook.allow_grabbing}, pose=${hook.allow_posing}${parameter}, suffixes=${suffixCount}${hook.metadata_only ? ", metadata-only" : ""})`;
	}

	function colliderLabel(collider: RendererRuntimeDiagnosticsData["dynamics_colliders"][number]): string {
		const path = collider.collider_path || collider.node_path || `#${collider.node}`;
		const node = collider.node_path && collider.node_path !== path ? `, node=${collider.node_path}` : "";
		const source = collider.source_id ? `, source=${collider.source_id}` : "";
		const inside = collider.inside_bounds ? ", inside" : "";
		return `${path} (${collider.source_kind}, ${collider.shape}, r=${collider.radius}${inside}${source}${node})`;
	}

	function projectionLabel(status: RendererRuntimeDiagnosticsData): string {
		const top =
			status.dynamics_collision_projection_top_collider_path && status.dynamics_collision_projection_top_collider_count != null
				? `top=${status.dynamics_collision_projection_top_collider_path}:${status.dynamics_collision_projection_top_collider_count}`
				: "top=-";
		const sources = status.dynamics_collision_projection_source_ids.length
			? `sources=${status.dynamics_collision_projection_source_ids.join(",")}`
			: "sources=-";
		const paths = status.dynamics_collision_projection_collider_paths.length
			? `paths=${status.dynamics_collision_projection_collider_paths.join(",")}`
			: "paths=-";
		const counts = status.dynamics_collision_projection_collider_path_counts.length
			? `counts=${status.dynamics_collision_projection_collider_path_counts.map((entry) => `${entry.key}:${entry.count}`).join(",")}`
			: "counts=-";
		return `projections=${status.dynamics_collision_projection_count} ${top} ${sources} ${paths} ${counts}`;
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
			`visiblePromote=${upload.last_visible_draw_residency_promotion_count ?? 0}`,
			`lastLoad mesh=${upload.last_mesh_buffer_scoped_load_count}/${upload.last_mesh_buffer_scoped_unload_count}`,
			`image=${upload.last_image_texture_scoped_load_count}/${upload.last_image_texture_scoped_unload_count}`,
			`cube=${upload.last_cubemap_scoped_load_count}/${upload.last_cubemap_scoped_unload_count}`,
			`material=${upload.last_material_slot_scoped_upload_count}`,
		].join(" ");
	}

	function formatBytes(bytes: number | null | undefined): string {
		if (!Number.isFinite(bytes ?? NaN)) return "-";
		const value = Math.max(0, bytes ?? 0);
		if (value >= 1024 * 1024 * 1024) return `${(value / (1024 * 1024 * 1024)).toFixed(1)} GiB`;
		if (value >= 1024 * 1024) return `${(value / (1024 * 1024)).toFixed(0)} MiB`;
		if (value >= 1024) return `${(value / 1024).toFixed(0)} KiB`;
		return `${value} B`;
	}

	function wardrobeTransitionProgressLabel(upload: NonNullable<RendererRuntimeDiagnosticsData["wardrobe_asset_upload"]>): string {
		const progress = upload.transition_progress;
		if (!progress?.active) return "inactive";
		const total = progress.total_work_bytes || 0;
		const remaining = progress.remaining_work_bytes || 0;
		const percent = total > 0 ? Math.max(0, Math.min(100, (1 - remaining / total) * 100)) : 0;
		return [
			`${percent.toFixed(0)}%`,
			`work=${formatBytes(remaining)}/${formatBytes(total)}`,
			`tex=${progress.image_total - progress.image_remaining}/${progress.image_total}`,
			`mesh=${progress.mesh_total - progress.mesh_remaining}/${progress.mesh_total}`,
			`cube=${progress.cube_total - progress.cube_remaining}/${progress.cube_total}`,
			`material=${progress.material_total - progress.material_remaining}/${progress.material_total}`,
			`drawRemain=${progress.draw_resource_remaining}`,
			`budget=${formatBytes(progress.image_upload_budget_bytes)}`,
			`step=${progress.last_step_ms}ms`,
			`ram=${progress.process_ram_mb ?? "-"}MB`,
			`pressure=${progress.memory_pressure}`,
		].join(" ");
	}

	function toggleDynamicsGroup(group: RendererRuntimeDiagnosticsData["dynamics_groups"][number]): void {
		if (!canSetDynamicsEnabled || !group.source_id) return;
		void onSetDynamicsEnabled(renderer.id, group.source_id, !group.effective_enabled);
	}

	function normalizedGroupRestResponse(group: RendererRuntimeDiagnosticsData["dynamics_groups"][number]): number | undefined {
		const authoredRest = group.pull;
		if (!Number.isFinite(authoredRest)) return undefined;
		return Math.max(0, Math.min(1, authoredRest <= 1 ? authoredRest : authoredRest / 60));
	}

	function responseGroupForSource(sourceId: string): RendererRuntimeDiagnosticsData["dynamics_response_groups"][number] | undefined {
		return dynamicsResponseGroups.find((response) => response.source_id === sourceId);
	}

	function inferredBounceScale(response: RendererRuntimeDiagnosticsData["dynamics_response_groups"][number]): number | undefined {
		const sourceBounce = response.average_spring;
		const finalBounce = response.average_bounce_response ?? sourceBounce;
		if (!Number.isFinite(sourceBounce) || sourceBounce <= Number.EPSILON || !Number.isFinite(finalBounce)) return undefined;
		return Math.max(0, Math.min(4, finalBounce / sourceBounce));
	}

	function dynamicsGroupOverrideSeed(group: RendererRuntimeDiagnosticsData["dynamics_groups"][number]): DynamicsGroupOverrideSeed {
		const response = group.source_id ? responseGroupForSource(group.source_id) : undefined;
		if (response) {
			return {
				solver: response.solver,
				damping_half_life_ms: response.average_damping_half_life_ms,
				rest_response: response.average_rest_response ?? response.average_pull,
				shape_preservation: response.average_shape_preservation,
				bounce_scale: inferredBounceScale(response),
				stretch_range_scale:
					response.average_max_stretch_response == null && response.average_max_squish_response == null
						? undefined
						: Math.max(response.average_max_stretch_response ?? 0, response.average_max_squish_response ?? 0),
				stretch_motion: response.average_stretch_motion_response,
				motion_coupling: response.average_parent_motion_follow,
				xpbd_compliance: response.solver === "xpbd" ? response.xpbd_compliance : undefined,
			};
		}
		return {
			rest_response: normalizedGroupRestResponse(group),
		};
	}

	function addDynamicsGroupOverride(group: RendererRuntimeDiagnosticsData["dynamics_groups"][number]): void {
		if (group.visual_target === false) return;
		if (!group.source_id) return;
		void onAddDynamicsGroupOverride(renderer.id, group.source_id, dynamicsGroupOverrideSeed(group));
	}

	function normalizedMatchCandidate(value: string | undefined): string {
		return (value ?? "")
			.trim()
			.toLowerCase()
			.replace(/[\s-]+/g, "_");
	}

	function sourceLeaf(value: string | undefined): string {
		return (value ?? "").split(/[/:]/).filter(Boolean).pop() ?? "";
	}

	function usefulMatchCandidate(value: string): boolean {
		if (value.length < 3) return false;
		if (/^bone(?:[._]\d+)?$/.test(value)) return false;
		return !["root", "armature", "hips", "spine", "chest", "neck", "head"].includes(value);
	}

	function dynamicsMatchOverrideSeed(group: RendererRuntimeDiagnosticsData["dynamics_groups"][number]): DynamicsMatchOverrideSeed | null {
		if (group.visual_target === false) return null;
		const candidates = [
			normalizedMatchCandidate(group.comment),
			normalizedMatchCandidate(sourceLeaf(group.source_id)),
			normalizedMatchCandidate(sourceLeaf(group.root_path)),
		].filter((value, index, values) => usefulMatchCandidate(value) && values.indexOf(value) === index);
		const base = dynamicsGroupOverrideSeed(group);
		if (candidates.length === 0) {
			if (!group.source_id) return null;
			return {
				...base,
				name: sourceLeaf(group.source_id) || group.source_id,
				source_id: group.source_id,
				source_id_contains: [],
			};
		}
		return {
			...base,
			name: candidates[0],
			source_id_contains: [candidates[0]],
		};
	}

	function addDynamicsMatchOverride(seed: DynamicsMatchOverrideSeed | null): void {
		if (!seed) return;
		void onAddDynamicsMatchOverride(renderer.id, seed);
	}

	function dynamicsMatchOverrideLabel(seed: DynamicsMatchOverrideSeed | null): string {
		if (seed && seed.source_id && seed.source_id_contains.length === 0) {
			return $_("renderers.details.dynamics_add_exact_match_override");
		}
		return $_("renderers.details.dynamics_add_match_override");
	}

	function dynamicsMatchOverrideTitle(
		group: RendererRuntimeDiagnosticsData["dynamics_groups"][number],
		seed: DynamicsMatchOverrideSeed | null
	): string {
		if (seed && seed.source_id && seed.source_id_contains.length === 0) {
			return $_("renderers.details.dynamics_add_exact_match_override_hint", {
				values: { source: seed.source_id },
			});
		}
		return group.comment || group.source_id || group.root_path || `#${group.index}`;
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
				<dt>Wardrobe transition</dt>
				<dd class="stderr-block">{wardrobeTransitionProgressLabel(runtimeStatus.wardrobe_asset_upload)}</dd>
			{/if}
			<dt>{$_("renderers.details.diag_scene_constraints")}</dt>
			<dd>
				{$_("renderers.details.diag_scene_constraints_value", {
					values: {
						nodes: runtimeStatus.scene_node_constraint_count,
						parents: runtimeStatus.scene_parent_constraint_count,
						sources: runtimeStatus.scene_parent_constraint_source_count,
						multiSource: runtimeStatus.scene_parent_constraint_multi_source_count,
					},
				})}
			</dd>
			<dt>{$_("renderers.details.diag_dynamics_summary")}</dt>
			<dd>
				{$_("renderers.details.diag_dynamics_summary_value", {
					values: {
						groups: runtimeStatus.dynamics_group_count,
						enabled: runtimeStatus.dynamics_enabled_group_count,
						sourceEnabled: runtimeStatus.dynamics_source_enabled_group_count,
						overrides: runtimeStatus.dynamics_enabled_override_count,
						colliders: runtimeStatus.dynamics_collider_count,
						surfaceConstraints: runtimeStatus.dynamics_surface_constraint_count,
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
			{#if runtimeStatus.dynamics_collision_projection_count > 0}
				<dt>{$_("renderers.details.diag_dynamics_projections")}</dt>
				<dd class="stderr-block">{projectionLabel(runtimeStatus)}</dd>
			{/if}
			{#if dynamicsResponseCategories.length}
				<dt>{$_("renderers.details.diag_dynamics_response_categories")}</dt>
				<dd class="stderr-block">{sampledLines(dynamicsResponseCategories, responseCategoryLabel)}</dd>
			{/if}
			{#if dynamicsResponseGroups.length}
				<dt>{$_("renderers.details.diag_dynamics_response_groups")}</dt>
				<dd class="stderr-block">{sampledLines(dynamicsResponseGroups, responseGroupLabel)}</dd>
			{/if}
			{#if runtimeStatus.dynamics_groups.length}
				<dt>{$_("renderers.details.diag_dynamics_groups")}</dt>
				<dd class="diagnostics-action-list">
					{#each dynamicsGroups as group}
						{@const matchOverrideSeed = dynamicsMatchOverrideSeed(group)}
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
							<button
								type="button"
								disabled={busy || matchOverrideSeed == null}
								title={dynamicsMatchOverrideTitle(group, matchOverrideSeed)}
								onclick={() => addDynamicsMatchOverride(matchOverrideSeed)}
							>
								<SlidersHorizontal size={13} />
								<span>{dynamicsMatchOverrideLabel(matchOverrideSeed)}</span>
							</button>
							<button
								type="button"
								disabled={busy || !group.source_id || group.visual_target === false}
								title={group.source_id ?? group.root_path ?? `#${group.index}`}
								onclick={() => addDynamicsGroupOverride(group)}
							>
								<SlidersHorizontal size={13} />
								<span>{$_("renderers.details.dynamics_add_override")}</span>
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
				<dt>{$_("renderers.details.diag_unanimator_menu_candidates")}</dt>
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
				<dd class="stderr-block">
					{sampledLines(runtimeStatus.runtime_action_restore_apply_plan, runtimeActionRestoreApplyLabel)}
				</dd>
			{/if}
		{/if}
	</dl>
</div>
