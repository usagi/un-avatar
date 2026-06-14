<script lang="ts">
	import { _ } from "svelte-i18n";
	import { Moon, Sun } from "lucide-svelte";
	import type { ThemeMode } from "./appSettings";

	export let mode: ThemeMode;
	export let className = "segmented-control";
	export let title: string | undefined = undefined;
	export let onChange: (mode: ThemeMode) => void;
</script>

<div class={className} {title} aria-label={$_("settings.theme.label")}>
	<button class:active={mode === "system"} onclick={() => onChange("system")}>{$_("theme.system")}</button>
	<button class:active={mode === "light"} onclick={() => onChange("light")}><Sun size={15} />{$_("theme.light")}</button>
	<button class:active={mode === "dark"} onclick={() => onChange("dark")}><Moon size={15} />{$_("theme.dark")}</button>
</div>
