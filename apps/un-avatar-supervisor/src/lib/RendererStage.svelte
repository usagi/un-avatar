<script lang="ts">
	import { _ } from "svelte-i18n";
	import { basename } from "./formatting";
	import { rendererHealthKind, type RuntimeStageStatusData } from "./runtimeLabels";
	import { rendererStateClass } from "./runtimeState";
	import RendererStageActions from "./RendererStageActions.svelte";
	import RendererStageChips from "./RendererStageChips.svelte";
	import type { RendererStageProfile, RendererStageView } from "./rendererTypes";

	export let renderer: RendererStageView;
	export let runtimeStatus: RuntimeStageStatusData | null;
	export let profile: RendererStageProfile | null;
	export let iconUrl: string;
	export let busy = false;
	export let rendererStateLabel: (state: string) => string;
	export let onViewProfile: (profileId: string) => void;
	export let onActivateRenderer: (rendererId: number) => void | Promise<void>;
	export let onResetCamera: (rendererId: number) => void | Promise<void>;
	export let onCaptureScreenshot: (rendererId: number) => void | Promise<void>;
	export let onRestartRenderer: (rendererId: number) => void | Promise<void>;
	export let onStopRenderer: (rendererId: number) => void | Promise<void>;

	$: health = rendererHealthKind(renderer, runtimeStatus);
</script>

<section class={`renderer-stage renderer-stage-health-${health}`} aria-label={$_("renderers.details.stage_aria")}>
	<div class="renderer-stage-identity">
		<div class="stage-avatar">
			<img src={iconUrl} alt="" />
		</div>
		<span
			class={renderer.state === "Running"
				? "storage-badge storage-user renderer-stage-state"
				: `${rendererStateClass(renderer.state)} renderer-stage-state`}>{rendererStateLabel(renderer.state)}</span
		>
	</div>
	<div class="stage-main">
		<div class="stage-title-row">
			<div>
				<h2>{renderer.name}</h2>
				<p>{basename(renderer.avatar_path)}</p>
			</div>
			<RendererStageActions
				{renderer}
				{profile}
				{busy}
				{onViewProfile}
				{onActivateRenderer}
				{onResetCamera}
				{onCaptureScreenshot}
				{onRestartRenderer}
				{onStopRenderer}
			/>
		</div>
		<RendererStageChips {renderer} {runtimeStatus} />
	</div>
</section>
