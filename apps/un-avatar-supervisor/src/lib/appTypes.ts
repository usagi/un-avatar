import type { AppSettings } from "./appSettings";

export type AppSettingValue = AppSettings[keyof AppSettings];

export type AppNotification = {
  id: number;
  level: "info" | "warning" | "error";
  title: string;
  body: string;
  created_at_secs: number;
};

export type NativeNotificationStatus = {
  permission_state: string;
};

export type DiagnosticsExportEntry = {
  path: string;
  archive_path: string | null;
  generated_at_secs: number | null;
  modified_at_secs: number | null;
  size_bytes: number;
  archive_size_bytes: number | null;
};
