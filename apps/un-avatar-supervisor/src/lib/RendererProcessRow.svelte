<script lang="ts">
  import { _ } from "svelte-i18n";
  import { basename } from "./formatting";
  import { motionLabel } from "./profileLabels";
  import RendererProcessMetricsCell from "./RendererProcessMetricsCell.svelte";
  import RendererProcessStatusCell from "./RendererProcessStatusCell.svelte";
  import type { RendererTableView } from "./rendererTypes";
  import {
    rendererHealthKind,
    runtimeOutputLabel,
    type RuntimeTableStatusData,
  } from "./runtimeLabels";

  export let renderer: RendererTableView;
  export let selectedRendererId: number | null;
  export let status: RuntimeTableStatusData | null;
  export let iconSrc: string;
  export let onSelectRenderer: (rendererId: number) => void;
  export let onOpenRendererLog: (rendererId: number) => void;

  $: health = rendererHealthKind(renderer, status);
</script>

<tr
  class={`renderer-health-${health}`}
  class:selected={selectedRendererId === renderer.id}
  onclick={() => onSelectRenderer(renderer.id)}
>
  <RendererProcessStatusCell {renderer} {status} {onOpenRendererLog} />
  <td
    class="process-cell-profile"
    title={`${renderer.name}${renderer.avatar_path ? ` · ${basename(renderer.avatar_path)}` : ""}`}
  >
    <div class="process-profile-summary">
      <img src={iconSrc} alt="" />
      <span>
        <strong>{renderer.name}</strong>
        <small>{basename(renderer.avatar_path)}</small>
      </span>
    </div>
  </td>
  <RendererProcessMetricsCell {status} />
  <td class="process-cell-io">
    <strong>{motionLabel(renderer)}</strong>
    <small>{runtimeOutputLabel(renderer, status)}</small>
  </td>
</tr>
