<script lang="ts">
	import { _ } from "svelte-i18n";
	import { localizedSettingSummary } from "./profileStageSummary";
	import type { RendererReadySetting } from "./profileTypes";

	export let setting: RendererReadySetting;
</script>

<div class="stage-chip-row">
	<span>{localizedSettingSummary(setting, $_)}</span>
	<span>{setting.render_backend}</span>
	<span>{setting.texture_resolution_limit} textures</span>
</div>
