import { settingSummary } from "./profileLabels";
import type { ProfileLaunchSetting } from "./profileTypes";

type Translate = (key: string, options?: { values?: Record<string, string | number> }) => string;

export function rendererReadyTitle(
	launchGroupName: string | null,
	launchTargetSetting: ProfileLaunchSetting | null,
	translate: Translate
): string {
	if (launchGroupName) {
		return translate("renderers.ready.group_title", {
			values: { group: launchGroupName },
		});
	}
	return launchTargetSetting?.name ?? translate("renderers.toolbar.no_settings_found");
}

export function rendererReadySubtitle(
	launchGroupName: string | null,
	launchTargetSetting: ProfileLaunchSetting | null,
	launchGroupCount: number,
	translate: Translate
): string {
	if (launchGroupName) {
		return translate("renderers.ready.group_count", {
			values: { count: launchGroupCount },
		});
	}
	return launchTargetSetting ? settingSummary(launchTargetSetting) : translate("renderers.details.none_selected");
}

export const RENDERER_READY_STATS = [
	{
		key: "runningCount",
		labelKey: "renderers.ready.running",
	},
	{
		key: "issueCount",
		labelKey: "renderers.ready.issues",
	},
	{
		key: "profileCount",
		labelKey: "renderers.ready.profiles",
	},
	{
		key: "profileGroupCount",
		labelKey: "renderers.ready.groups",
	},
] as const;
