<script lang="ts">
  import { _ } from "svelte-i18n";
  import { formatUptime } from "./formatting";

  export let name: string;
  export let pid: number | null;
  export let manifestPath: string | null;
  export let uptimeSecs: number;
</script>

<section class="renderer-info-card">
  <div class="renderer-info-card-heading">
    <h3>{$_("renderers.details.profile_card")}</h3>
    <span>{$_("renderers.summary.pid")} {pid ?? "--"}</span>
  </div>
  <dl class="renderer-card-kv">
    <dt>{$_("renderers.summary.profile")}</dt>
    <dd>{name}</dd>
    <dt>{$_("renderers.summary.path")}</dt>
    <dd>{manifestPath ?? $_("renderers.details.manifest_temporary")}</dd>
    <dt>{$_("renderers.metrics.uptime")}</dt>
    <dd>{formatUptime(uptimeSecs)}</dd>
  </dl>
</section>
