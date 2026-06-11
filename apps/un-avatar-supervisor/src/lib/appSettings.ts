export type ThemeMode = "system" | "light" | "dark";

export type AppSettings = {
	system_tray_enabled: boolean;
	minimize_to_tray: boolean;
	close_to_tray_while_running: boolean;
	start_minimized_to_tray: boolean;
	crash_notifications: boolean;
	stop_all_on_console_exit: boolean;
	renderer_close_hotkey: string;
	quit_behavior: "ask" | "stop_renderers" | "leave_renderers";
	theme_mode: ThemeMode;
	jump_to_renderers_on_quick_run: boolean;
	auto_launch_selected_on_startup: boolean;
	show_developer_controls: boolean;
	last_selected_setting_id: string | null;
	console_window_x: number | null;
	console_window_y: number | null;
	console_window_width: number | null;
	console_window_height: number | null;
	locale: string;
};

export const defaultAppSettings: AppSettings = {
	system_tray_enabled: false,
	minimize_to_tray: true,
	close_to_tray_while_running: true,
	start_minimized_to_tray: false,
	crash_notifications: true,
	stop_all_on_console_exit: false,
	renderer_close_hotkey: "Escape",
	quit_behavior: "ask",
	theme_mode: "system",
	jump_to_renderers_on_quick_run: false,
	auto_launch_selected_on_startup: false,
	show_developer_controls: false,
	last_selected_setting_id: null,
	console_window_x: null,
	console_window_y: null,
	console_window_width: null,
	console_window_height: null,
	locale: "",
};

function appSettingsFromPartial(parsed: Partial<AppSettings>): AppSettings {
	return {
		...defaultAppSettings,
		...parsed,
		system_tray_enabled:
			typeof parsed.system_tray_enabled === "boolean" ? parsed.system_tray_enabled : defaultAppSettings.system_tray_enabled,
		theme_mode:
			parsed.theme_mode === "system" || parsed.theme_mode === "light" || parsed.theme_mode === "dark"
				? parsed.theme_mode
				: defaultAppSettings.theme_mode,
		quit_behavior:
			parsed.quit_behavior === "ask" || parsed.quit_behavior === "stop_renderers" || parsed.quit_behavior === "leave_renderers"
				? parsed.quit_behavior
				: defaultAppSettings.quit_behavior,
		stop_all_on_console_exit:
			typeof parsed.stop_all_on_console_exit === "boolean"
				? parsed.stop_all_on_console_exit
				: defaultAppSettings.stop_all_on_console_exit,
		renderer_close_hotkey:
			typeof parsed.renderer_close_hotkey === "string" && parsed.renderer_close_hotkey.trim()
				? parsed.renderer_close_hotkey.trim()
				: defaultAppSettings.renderer_close_hotkey,
		jump_to_renderers_on_quick_run:
			typeof parsed.jump_to_renderers_on_quick_run === "boolean"
				? parsed.jump_to_renderers_on_quick_run
				: defaultAppSettings.jump_to_renderers_on_quick_run,
		auto_launch_selected_on_startup:
			typeof parsed.auto_launch_selected_on_startup === "boolean"
				? parsed.auto_launch_selected_on_startup
				: defaultAppSettings.auto_launch_selected_on_startup,
		show_developer_controls:
			typeof parsed.show_developer_controls === "boolean"
				? parsed.show_developer_controls
				: defaultAppSettings.show_developer_controls,
		last_selected_setting_id:
			typeof parsed.last_selected_setting_id === "string" && parsed.last_selected_setting_id.trim()
				? parsed.last_selected_setting_id.trim()
				: null,
		locale: typeof parsed.locale === "string" ? parsed.locale : defaultAppSettings.locale,
	};
}

export function loadAppSettings(storageKey: string, legacyThemeModeStorageKey: string): AppSettings {
	if (typeof window === "undefined") return defaultAppSettings;
	const saved = window.localStorage.getItem(storageKey);
	if (!saved) {
		const legacyTheme = window.localStorage.getItem(legacyThemeModeStorageKey);
		return {
			...defaultAppSettings,
			theme_mode: legacyTheme === "light" || legacyTheme === "dark" ? legacyTheme : "system",
		};
	}
	try {
		return appSettingsFromPartial(JSON.parse(saved) as Partial<AppSettings>);
	} catch {
		return defaultAppSettings;
	}
}

export function saveAppSettings(storageKey: string, legacyThemeModeStorageKey: string, settings: AppSettings): void {
	if (typeof window === "undefined") return;
	window.localStorage.setItem(storageKey, JSON.stringify(settings));
	window.localStorage.removeItem(legacyThemeModeStorageKey);
}
