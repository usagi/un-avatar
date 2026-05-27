<script lang="ts">
  import { _ } from "svelte-i18n";

  export let runningCount = 0;
  export let issueCount = 0;
  export let profileCount = 0;
  export let profileGroupCount = 0;
</script>

<div class="stage-metrics">
  <span><strong>{runningCount}</strong><small>{$_("renderers.ready.running")}</small></span>
  <span><strong>{issueCount}</strong><small>{$_("renderers.ready.issues")}</small></span>
  <span><strong>{profileCount}</strong><small>{$_("renderers.ready.profiles")}</small></span>
  <span><strong>{profileGroupCount}</strong><small>{$_("renderers.ready.groups")}</small></span>
</div>
