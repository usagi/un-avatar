<script lang="ts">
  import { _ } from "svelte-i18n";
  import type {
    RendererDiagnosticsData,
    RendererRuntimeDiagnosticsData,
  } from "./rendererTypes";

  export let renderer: RendererDiagnosticsData;
  export let runtimeStatus: RendererRuntimeDiagnosticsData | null;

  const sampleLimit = 4;

  function groupLabel(group: RendererRuntimeDiagnosticsData["dynamics_groups"][number]): string {
    const path = group.root_path ?? group.source_id ?? `#${group.index}`;
    const state = group.effective_enabled ? "on" : "off";
    return `${path} (${group.source_kind}, ${state}, bones=${group.bone_count})`;
  }

  function colliderLabel(collider: RendererRuntimeDiagnosticsData["dynamics_colliders"][number]): string {
    const path = collider.node_path ?? `#${collider.node}`;
    return `${path} (${collider.source_kind}, ${collider.shape}, r=${collider.radius})`;
  }

  function contactDeclarationLabel(
    declaration: RendererRuntimeDiagnosticsData["contact_parameter_declarations"][number],
  ): string {
    const path = declaration.node_path ?? `#${declaration.node}`;
    return `${declaration.parameter} @ ${path}`;
  }

  function contactProbeLabel(probe: RendererRuntimeDiagnosticsData["contact_probes"][number]): string {
    const receiver = probe.receiver_node_path ?? `#${probe.receiver_node}`;
    const sender = probe.sender_node_path ?? `#${probe.sender_node}`;
    return `${probe.parameter}: ${receiver} <- ${sender} (${probe.would_emit ? "would emit" : "idle"})`;
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

  function runtimeActionCollisionLabel(
    collision: RendererRuntimeDiagnosticsData["runtime_action_target_write_collisions"][number],
  ): string {
    return `${collision.target_kind}:${collision.target_key} owners=${collision.owner_keys.length} actions=${collision.action_ids.join(",")}`;
  }

  function runtimeActionRestoreReadinessLabel(
    readiness: RendererRuntimeDiagnosticsData["runtime_action_restore_readiness"][number],
  ): string {
    return `${readiness.owner_key}:${readiness.effect_kind} ${readiness.target_kind}:${readiness.target_key} ready=${readiness.ready} reason=${readiness.reason}`;
  }

  function runtimeActionRestoreBaselineLabel(
    candidate: RendererRuntimeDiagnosticsData["runtime_action_restore_baseline_candidates"][number],
  ): string {
    return `${candidate.owner_key}:${candidate.effect_kind} ${candidate.target_kind}:${candidate.target_key} value=${JSON.stringify(candidate.baseline_value)}`;
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
      <dt>{$_("renderers.details.diag_dynamics_summary")}</dt>
      <dd>
        {$_("renderers.details.diag_dynamics_summary_value", {
          values: {
            groups: runtimeStatus.dynamics_group_count,
            enabled: runtimeStatus.dynamics_enabled_group_count,
            colliders: runtimeStatus.dynamics_collider_count,
            contacts: runtimeStatus.dynamics_contact_count,
            probes: runtimeStatus.dynamics_contact_probe_count,
            wouldEmit: runtimeStatus.dynamics_contact_probe_would_emit_count,
            constraints: runtimeStatus.dynamics_constraint_ref_count,
          },
        })}
      </dd>
      {#if runtimeStatus.dynamics_groups.length}
        <dt>{$_("renderers.details.diag_dynamics_groups")}</dt>
        <dd class="stderr-block">{runtimeStatus.dynamics_groups.slice(0, sampleLimit).map(groupLabel).join("\n")}</dd>
      {/if}
      {#if runtimeStatus.dynamics_colliders.length}
        <dt>{$_("renderers.details.diag_dynamics_colliders")}</dt>
        <dd class="stderr-block">{runtimeStatus.dynamics_colliders.slice(0, sampleLimit).map(colliderLabel).join("\n")}</dd>
      {/if}
      {#if runtimeStatus.contact_parameter_declarations.length}
        <dt>{$_("renderers.details.diag_contact_parameters")}</dt>
        <dd class="stderr-block">
          {runtimeStatus.contact_parameter_declarations.slice(0, sampleLimit).map(contactDeclarationLabel).join("\n")}
        </dd>
      {/if}
      {#if runtimeStatus.contact_probes.length}
        <dt>{$_("renderers.details.diag_contact_probes")}</dt>
        <dd class="stderr-block">{runtimeStatus.contact_probes.slice(0, sampleLimit).map(contactProbeLabel).join("\n")}</dd>
      {/if}
      {#if runtimeStatus.dynamics_constraint_refs.length}
        <dt>{$_("renderers.details.diag_dynamics_constraints")}</dt>
        <dd class="stderr-block">
          {runtimeStatus.dynamics_constraint_refs.slice(0, sampleLimit).map(constraintRefLabel).join("\n")}
        </dd>
      {/if}
      {#if runtimeStatus.runtime_actions.length}
        <dt>{$_("renderers.details.diag_runtime_actions")}</dt>
        <dd class="stderr-block">{runtimeStatus.runtime_actions.slice(0, sampleLimit).map(runtimeActionLabel).join("\n")}</dd>
      {/if}
      {#if runtimeStatus.runtime_action_target_write_collisions.length}
        <dt>{$_("renderers.details.diag_runtime_action_collisions")}</dt>
        <dd class="stderr-block">
          {runtimeStatus.runtime_action_target_write_collisions.slice(0, sampleLimit).map(runtimeActionCollisionLabel).join("\n")}
        </dd>
      {/if}
      {#if runtimeStatus.runtime_action_restore_readiness.length}
        <dt>{$_("renderers.details.diag_runtime_action_restore")}</dt>
        <dd class="stderr-block">
          {runtimeStatus.runtime_action_restore_readiness.slice(0, sampleLimit).map(runtimeActionRestoreReadinessLabel).join("\n")}
        </dd>
      {/if}
      {#if runtimeStatus.runtime_action_restore_baseline_candidates.length}
        <dt>{$_("renderers.details.diag_runtime_action_restore_baseline")}</dt>
        <dd class="stderr-block">
          {runtimeStatus.runtime_action_restore_baseline_candidates.slice(0, sampleLimit).map(runtimeActionRestoreBaselineLabel).join("\n")}
        </dd>
      {/if}
    {/if}
  </dl>
</div>
