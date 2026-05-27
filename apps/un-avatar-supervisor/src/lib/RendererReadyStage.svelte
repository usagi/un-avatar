<script lang="ts">
  import { _ } from "svelte-i18n";
  import { basename } from "./formatting";
  import RendererReadyChips from "./RendererReadyChips.svelte";
  import RendererReadyMetrics from "./RendererReadyMetrics.svelte";
  import type { RendererReadySetting } from "./profileTypes";

  export let setting: RendererReadySetting;
  export let iconUrl: string;
  export let launchGroupName: string | null;
  export let launchGroupCount = 0;
  export let runningCount = 0;
  export let issueCount = 0;
  export let profileCount = 0;
  export let profileGroupCount = 0;
</script>

<section class="renderer-stage renderer-stage-ready" aria-label="Launch profile summary">
  <div class="renderer-stage-identity">
    <div class="stage-avatar">
      <img src={iconUrl} alt="" />
    </div>
    <span class="state state-starting renderer-stage-state">{$_("renderers.ready.state")}</span>
  </div>
  <div class="stage-main">
    <div class="stage-title-row">
      <div>
        <h2>{launchGroupName ? $_("renderers.ready.group_title", { values: { group: launchGroupName } }) : setting.name}</h2>
        <p>{launchGroupName ? $_("renderers.ready.group_count", { values: { count: launchGroupCount } }) : basename(setting.avatar_path)}</p>
      </div>
    </div>
    <RendererReadyChips {setting} />
  </div>
  <RendererReadyMetrics
    {runningCount}
    {issueCount}
    {profileCount}
    {profileGroupCount}
  />
</section>
