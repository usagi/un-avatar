<script lang="ts">
  import { settingSummary } from "./profileLabels";
  import type { RendererReadySetting } from "./profileTypes";

  export let setting: RendererReadySetting;
</script>

<div class="stage-chip-row">
  <span>{settingSummary(setting)}</span>
  <span>{setting.render_backend}</span>
  <span>{setting.texture_resolution_limit} textures</span>
</div>
