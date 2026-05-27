<script lang="ts">
  import { _ } from "svelte-i18n";
  import {
    runtimeAaLabel,
    textureCacheLabel,
    texturePolicyLabel,
    textureSummaryLabel,
    type RuntimeQualityStatusLabelData,
  } from "./runtimeLabels";

  export let runtimeStatus: RuntimeQualityStatusLabelData | null;
</script>

<section class="renderer-info-card">
  <div class="renderer-info-card-heading">
    <h3>{$_("renderers.details.rendering_card")}</h3>
    <span>{runtimeAaLabel(runtimeStatus)}</span>
  </div>
  <dl class="renderer-card-kv">
    <dt>{$_("renderers.details.textures")}</dt>
    <dd>
      {texturePolicyLabel(runtimeStatus, $_("renderers.summary.pending"))}
      <small>{textureSummaryLabel(runtimeStatus)}</small>
      <small>{textureCacheLabel(runtimeStatus)}</small>
    </dd>
  </dl>
</section>
