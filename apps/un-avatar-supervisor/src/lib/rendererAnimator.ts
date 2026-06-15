import type { RendererRuntimeActionStatus, RendererRuntimeMenuActionCandidateStatus } from "./rendererTypes";

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

export function animatorItemCount(
	expressionPresets: readonly string[],
	menuActionCandidates: readonly RendererRuntimeMenuActionCandidateStatus[],
	runtimeActions: readonly RendererRuntimeActionStatus[]
): number {
	return (
		expressionPresets.length +
		menuActionCandidates.filter(animatorCandidateVisible).length +
		runtimeActions.filter(animatorFallbackActionVisible).length
	);
}
