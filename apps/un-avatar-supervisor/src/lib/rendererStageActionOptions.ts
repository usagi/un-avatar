import { Camera, ExternalLink, RefreshCw, RotateCcw, Square, type Icon as LucideIcon } from "lucide-svelte";

export type RendererStageActionKey = "activate" | "resetCamera" | "screenshot" | "restart" | "stop";

export type RendererStageActionOption = {
	key: RendererStageActionKey;
	icon: typeof LucideIcon;
	labelKey: string;
	danger?: boolean;
};

export const RENDERER_STAGE_ACTIONS: readonly RendererStageActionOption[] = [
	{
		key: "activate",
		icon: ExternalLink,
		labelKey: "renderers.toolbar.activate",
	},
	{
		key: "resetCamera",
		icon: RotateCcw,
		labelKey: "renderers.toolbar.reset_view",
	},
	{
		key: "screenshot",
		icon: Camera,
		labelKey: "renderers.toolbar.screenshot",
	},
	{
		key: "restart",
		icon: RefreshCw,
		labelKey: "renderers.toolbar.restart",
	},
	{
		key: "stop",
		icon: Square,
		labelKey: "renderers.toolbar.stop",
		danger: true,
	},
];
