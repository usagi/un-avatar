import type { AppSettings } from "./appSettings";

export type AppBehaviorToggle = {
	key: keyof AppSettings;
	labelKey: string;
	hintKey: string;
	requiresSystemTray?: boolean;
};

export const APP_BEHAVIOR_TOGGLES: readonly AppBehaviorToggle[] = [
	{
		key: "system_tray_enabled",
		labelKey: "settings.app_behavior.enable_system_tray",
		hintKey: "settings.hints.system_tray",
	},
	{
		key: "minimize_to_tray",
		labelKey: "settings.app_behavior.minimize_to_tray",
		hintKey: "settings.hints.minimize_to_tray",
		requiresSystemTray: true,
	},
	{
		key: "close_to_tray_while_running",
		labelKey: "settings.app_behavior.close_to_tray_while_running",
		hintKey: "settings.hints.close_to_tray_while_running",
		requiresSystemTray: true,
	},
	{
		key: "start_minimized_to_tray",
		labelKey: "settings.app_behavior.start_minimized_to_tray",
		hintKey: "settings.hints.start_minimized_to_tray",
		requiresSystemTray: true,
	},
	{
		key: "crash_notifications",
		labelKey: "settings.app_behavior.crash_notifications",
		hintKey: "settings.hints.crash_notifications",
	},
	{
		key: "stop_all_on_console_exit",
		labelKey: "settings.app_behavior.stop_renderers_on_exit",
		hintKey: "settings.hints.stop_children_on_exit",
	},
	{
		key: "jump_to_renderers_on_quick_run",
		labelKey: "settings.app_behavior.quick_run_jump",
		hintKey: "settings.hints.quick_launch_jump",
	},
	{
		key: "auto_launch_selected_on_startup",
		labelKey: "settings.app_behavior.auto_launch_selected_on_startup",
		hintKey: "settings.hints.auto_launch_selected_on_startup",
	},
	{
		key: "show_developer_controls",
		labelKey: "settings.app_behavior.developer_controls",
		hintKey: "settings.hints.developer_controls",
	},
];
