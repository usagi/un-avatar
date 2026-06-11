import type { RendererWindowPatch } from "./rendererTypes";

export type RendererWindowActionPatch = [RendererWindowPatch, string];

export type RendererWindowActionData = {
	decorations: boolean;
	transparent: boolean;
	always_on_top: boolean;
};

export type RendererWindowActionStatus = {
	input_passthrough: boolean;
	minimized: boolean;
};

export function borderlessWindowPatch(renderer: RendererWindowActionData): RendererWindowActionPatch {
	return [{ decorations: !renderer.decorations }, renderer.decorations ? "borderless" : "framed"];
}

export function transparentWindowPatch(renderer: RendererWindowActionData): RendererWindowActionPatch {
	return [{ transparent: !renderer.transparent }, renderer.transparent ? "opaque" : "transparent"];
}

export function clickThroughWindowPatch(status: RendererWindowActionStatus | null): RendererWindowActionPatch {
	const next = !(status?.input_passthrough ?? false);
	return [{ inputPassthrough: next }, next ? "click-through" : "interactive"];
}

export function topmostWindowPatch(renderer: RendererWindowActionData): RendererWindowActionPatch {
	return [{ alwaysOnTop: !renderer.always_on_top }, renderer.always_on_top ? "normal" : "topmost"];
}

export function minimizedWindowPatch(status: RendererWindowActionStatus | null): RendererWindowActionPatch {
	const next = !(status?.minimized ?? false);
	return [{ minimized: next }, next ? "minimized" : "restored"];
}
