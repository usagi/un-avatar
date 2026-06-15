import type { RendererRuntimeActionStatus, RendererRuntimeMenuActionCandidateStatus } from "./rendererTypes";

export type AnimatorTogglePolarity = "on" | "off" | null;

export function animatorCandidateVisible(candidate: RendererRuntimeMenuActionCandidateStatus): boolean {
	if (candidate.available === false) return false;
	if (candidate.wardrobe_set_ids?.length) return false;
	if (candidate.match_kind === "metadata" || candidate.effect_count <= 0) return false;
	if (candidate.match_kind !== "metadata") return true;
	if (candidate.control_type === "Button") return false;
	const path = candidate.menu_path ?? [];
	if (path.length > 2) return false;
	if (path.some((segment) => segment === "Face_Tracking" || segment.includes("VRCFT") || segment.includes("<"))) {
		return false;
	}
	return !candidate.menu_label?.includes("<") && !candidate.menu_label?.includes("VRCFT");
}

export function animatorFallbackActionVisible(action: RendererRuntimeActionStatus): boolean {
	return !action.wardrobe_set_id && Boolean(action.expression_menu_path?.trim());
}

export function animatorNormalizedToggleLabel(label: string): { label: string; polarity: AnimatorTogglePolarity } {
	const trimmed = label.trim();
	const match = trimmed.match(/^(.*?)(?:[\s_:/-]+)?(ON|OFF)$/i);
	if (!match) return { label: trimmed, polarity: null };
	const base = match[1].trim().replace(/[\s_:/-]+$/g, "");
	if (!base) return { label: trimmed, polarity: null };
	return {
		label: base,
		polarity: match[2].toUpperCase() === "OFF" ? "off" : "on",
	};
}

export function animatorToggleStateLabel(active: boolean, polarity: AnimatorTogglePolarity): string {
	if (polarity === "off") return active ? "OFF" : "ON";
	if (polarity === "on") return active ? "ON" : "OFF";
	return active ? "ON" : "OFF";
}

export function animatorItemCount(
	expressionPresets: readonly string[] | null | undefined,
	menuActionCandidates: readonly RendererRuntimeMenuActionCandidateStatus[] | null | undefined,
	runtimeActions: readonly RendererRuntimeActionStatus[] | null | undefined
): number {
	return (
		(expressionPresets ?? []).length +
		(menuActionCandidates ?? []).filter(animatorCandidateVisible).length +
		(runtimeActions ?? []).filter(animatorFallbackActionVisible).length
	);
}
