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
			{texturePolicyLabel(runtimeStatus, {
				pending: $_("renderers.summary.pending"),
				connected: $_("renderers.summary.connected"),
				cacheOn: $_("renderers.summary.cache_on"),
				cacheOff: $_("renderers.summary.cache_off"),
				cacheUnknown: $_("renderers.summary.cache_unknown"),
			})}
			<small
				>{textureSummaryLabel(runtimeStatus, {
					pending: $_("renderers.summary.pending"),
					connected: $_("renderers.summary.connected"),
					textureImages: $_("renderers.summary.texture_images"),
					textureResized: $_("renderers.summary.texture_resized"),
					textureCompressed: $_("renderers.summary.texture_compressed"),
					textureFallback: $_("renderers.summary.texture_fallback"),
					textureUploaded: $_("renderers.summary.texture_uploaded"),
				})}</small
			>
			<small
				>{textureCacheLabel(runtimeStatus, {
					pending: $_("renderers.summary.pending"),
					connected: $_("renderers.summary.connected"),
					cacheOn: $_("renderers.summary.cache_on"),
					cacheDisabled: $_("renderers.summary.cache_disabled"),
					compressedCache: $_("renderers.summary.compressed_cache"),
				})}</small
			>
		</dd>
	</dl>
</section>
