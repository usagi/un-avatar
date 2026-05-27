import type { ProfileSectionId } from "./profileTypes";
import type { ProfileStageSummarySetting } from "./profileStageSummary";

export type ProfileSectionNavItem = {
  id: ProfileSectionId;
  labelKey: string;
  scopeKey: string | null;
};

export type ProfilePendingRestart = {
  fieldLabel: string;
};

export type ProfileStageSetting = ProfileStageSummarySetting & {
  id: string;
  name: string;
  group: string;
  icon_path: string | null;
};
