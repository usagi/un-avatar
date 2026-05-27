import type { ProfileSectionId } from "./profileTypes";

export type ProfileSummaryItem = {
  section: ProfileSectionId;
  label: string;
  value: string;
};
