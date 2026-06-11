import { basename } from "./formatting";
import { cameraSummaryLabel, lightingSummaryLabel, lookSummaryLabel, motionLabel, outputLabel, windowLabel } from "./profileLabels";
import type { ProfileSummaryItem } from "./profileSummary";
import { qualitySummaryLabel } from "./runtimeLabels";

type Translate = (key: string) => string;

export type ProfileStageSummarySetting = Parameters<typeof motionLabel>[0] &
	Parameters<typeof outputLabel>[0] &
	Parameters<typeof qualitySummaryLabel>[0] &
	Parameters<typeof lightingSummaryLabel>[0] &
	Parameters<typeof lookSummaryLabel>[0] &
	Parameters<typeof windowLabel>[0] &
	Parameters<typeof cameraSummaryLabel>[0] & {
		avatar_path: string | null;
	};

export function profileStageSummaryItems(setting: ProfileStageSummarySetting, translate: Translate): ProfileSummaryItem[] {
	return [
		{
			section: "avatar",
			label: translate("profiles.sections.avatar"),
			value: basename(setting.avatar_path),
		},
		{
			section: "motion",
			label: translate("profiles.sections.motion"),
			value: motionLabel(setting),
		},
		{
			section: "output",
			label: translate("profiles.sections.output"),
			value: outputLabel(setting),
		},
		{
			section: "quality",
			label: translate("profiles.sections.quality"),
			value: qualitySummaryLabel(setting),
		},
		{
			section: "lighting",
			label: translate("profiles.editor.lighting"),
			value: lightingSummaryLabel(setting),
		},
		{
			section: "look",
			label: translate("profiles.sections.look"),
			value: lookSummaryLabel(setting),
		},
		{
			section: "window",
			label: translate("profiles.sections.window"),
			value: windowLabel(setting),
		},
		{
			section: "camera",
			label: translate("profiles.sections.camera"),
			value: cameraSummaryLabel(setting),
		},
	];
}
