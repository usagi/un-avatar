<script lang="ts">
  import { _ } from "svelte-i18n";
  import {
    expressionOverrideCount,
    expressionOverrideValue,
    filteredExpressionPresets,
    type ExpressionOverrides,
  } from "./rendererExpressions";
  import RendererExpressionHeader from "./RendererExpressionHeader.svelte";
  import RendererExpressionSearchField from "./RendererExpressionSearchField.svelte";
  import RendererExpressionRow from "./RendererExpressionRow.svelte";

  export let rendererId: number | null = null;
  export let rendererPid: number | null = null;
  export let busy = false;
  export let expressionPresets: string[] = [];
  export let expressionOverrides: ExpressionOverrides = {};
  export let expressionFilter = "";
  export let onClearExpressionOverrides: (rendererId: number) => void | Promise<void>;
  export let onSetExpressionOverride: (
    rendererId: number,
    preset: string,
    weight: number,
  ) => void;

  $: visibleExpressionPresets = filteredExpressionPresets(
    expressionPresets,
    expressionFilter,
  );
  $: activeExpressionCount = expressionOverrideCount(
    expressionOverrides,
    rendererId,
  );
  $: hasRunningRenderer = rendererId != null && rendererPid != null;
</script>

<div class="renderer-pane-scroll renderer-expression-pane">
  {#if expressionPresets.length}
    <RendererExpressionHeader
      {rendererId}
      {busy}
      expressionPresetCount={expressionPresets.length}
      {activeExpressionCount}
      {hasRunningRenderer}
      {onClearExpressionOverrides}
    />
    <RendererExpressionSearchField bind:expressionFilter />
    <div class="expression-grid">
      {#each visibleExpressionPresets as preset}
        {@const weight = expressionOverrideValue(
          expressionOverrides,
          rendererId,
          preset,
        )}
        <RendererExpressionRow
          {rendererId}
          {preset}
          {weight}
          {hasRunningRenderer}
          {onSetExpressionOverride}
        />
      {/each}
    </div>
    {#if visibleExpressionPresets.length === 0}
      <p class="empty">{$_("renderers.details.expression_filter_empty")}</p>
    {/if}
  {:else}
    <p class="empty">{$_("renderers.details.expressions_empty")}</p>
  {/if}
</div>
