use std::{
	borrow::Cow,
	collections::HashMap,
	env, fs,
	path::{Path, PathBuf},
	process::Command,
	sync::LazyLock,
	sync::{mpsc, Arc, Mutex},
	thread,
	time::Duration,
};

use tray_icon::{
	menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
	Icon as TrayIconImage, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use winit::event_loop::EventLoopProxy;

use crate::{gpu, AvatarWindowOptions, RendererControlEvent, RendererRuntimeSnapshot};

const TRAY_ICON_ID_PREFIX: &str = "un-avatar-renderer-tray";
const SUPERVISOR_OPEN_PROFILE_MANIFEST_ARG: &str = "--open-profile-manifest";
const RENDERER_TRAY_LOCALE_ENV: &str = "UN_AVATAR_LOCALE";
const RENDERER_TRAY_FALLBACK_LOCALE: &str = "ja-JP";
const UNANIMATOR_TRAY_MENU_ID: &str = "renderer:unanimator";
const UNANIMATOR_TRAY_ACTION_ID_PREFIX: &str = "unanimator";

static RENDERER_TRAY_I18N: LazyLock<un_i18n::UnI18nStore> = LazyLock::new(|| {
	let mut store = un_i18n::UnI18nStore::new();
	store.add_locale_toml(
		"ja-JP",
		r#"
show_focus_preview = "プレビューを表示 / 前面へ"
output = "出力: Spout2"
camera = "カメラ"
window_preview = "ウィンドウプレビュー"
spout_preview = "Spout2 + プレビュー"
spout_only = "Spout2 のみ (最小化)"
spout_resolution = "Spout2 解像度 %{width} x %{height}"
spout_output_size = "Spout2 出力: %{width} x %{height}"
spout_output_default = "Spout2 出力: レンダラー既定"
save_output_profile = "Spout2 設定をプロファイルへ保存"
save_window_profile = "位置とサイズをプロファイルへ保存"
restore_window_profile = "位置とサイズをプロファイルから復元"
save_camera_profile = "プロファイルへ保存"
restore_camera_profile = "プロファイルから復元"
save_wardrobe_profile = "現在の衣装をプロファイルへ保存"
wardrobe = "ワードローブ"
base_wardrobe = "Base"
unanimator = "UNAnimator"
unphysics = "UNPhysics"
unphysics_summary = "有効グループ %{enabled} / %{total}"
dynamics_enabled = "有効"
window = "ウィンドウ"
window_hidden = "ウィンドウ非表示"
always_on_top = "常に手前"
input_passthrough = "クリックを透過"
reset_camera = "カメラをリセット"
open_supervisor = "Supervisor を開く"
quit_renderer = "この Renderer を終了"
scene_starting = "起動中"
scene_avatar_scene = "アバター表示中"
scene_loading = "読み込み中"
"#,
	);
	store.add_locale_toml(
		"en-US",
		r#"
show_focus_preview = "Show / Focus Preview"
output = "Output: Spout2"
camera = "Camera"
window_preview = "Window Preview"
spout_preview = "Spout2 + Preview"
spout_only = "Spout2 Only (minimized)"
spout_resolution = "Spout2 %{width} x %{height}"
spout_output_size = "Spout2 output: %{width} x %{height}"
spout_output_default = "Spout2 output: renderer default"
save_output_profile = "Save Spout2 Settings to Profile"
save_window_profile = "Save Position and Size to Profile"
restore_window_profile = "Restore Position and Size from Profile"
save_camera_profile = "Save to Profile"
restore_camera_profile = "Restore from Profile"
save_wardrobe_profile = "Save Current Outfit to Profile"
wardrobe = "Wardrobe"
base_wardrobe = "Base"
unanimator = "UNAnimator"
unphysics = "UNPhysics"
unphysics_summary = "%{enabled} / %{total} effective groups"
dynamics_enabled = "Enabled"
window = "Window"
window_hidden = "Window Hidden"
always_on_top = "Always on Top"
input_passthrough = "Input Passthrough"
reset_camera = "Reset Camera"
open_supervisor = "Open Supervisor"
quit_renderer = "Quit this Renderer"
scene_starting = "starting"
scene_avatar_scene = "avatar scene"
scene_loading = "loading"
"#,
	);
	store
});

#[derive(Clone, Debug)]
pub(crate) enum RendererTrayAction {
	ActivatePreview,
	SetWindowPreview,
	SetSpoutPreview,
	SetSpoutOnly,
	SetSpoutResolution { width: u32, height: u32 },
	SaveOutputToProfile,
	SaveWindowToProfile,
	RestoreWindowFromProfile,
	SaveCameraToProfile,
	RestoreCameraFromProfile,
	SaveWardrobeToProfile,
	SetAlwaysOnTop(bool),
	SetInputPassthrough(bool),
	SetCurrentWardrobeDynamics(bool),
	SetWardrobe(String),
	SetParameter { name: String, value: f32 },
	ActivateAction(String),
	OpenSupervisor,
	ResetCamera,
	Quit,
}

pub(crate) struct RendererTray {
	backend: RendererTrayBackend,
}

enum RendererTrayBackend {
	Worker {
		tx: mpsc::Sender<RendererTrayWorkerCommand>,
		actions: Arc<Mutex<HashMap<String, RendererTrayAction>>>,
	},
	Local {
		icon: TrayIcon,
		actions: HashMap<String, RendererTrayAction>,
		last_menu_key: String,
	},
}

impl RendererTray {
	pub(crate) fn new(
		opts: &AvatarWindowOptions,
		snapshot: &RendererRuntimeSnapshot,
		proxy: EventLoopProxy<RendererControlEvent>,
	) -> Result<Self, String> {
		match Self::new_worker(opts, snapshot, proxy.clone()) {
			Ok(tray) => Ok(tray),
			Err(error) => {
				eprintln!("un-avatar-renderer: renderer tray worker disabled: {error}");
				Self::new_local(opts, snapshot)
			}
		}
	}

	fn new_local(opts: &AvatarWindowOptions, snapshot: &RendererRuntimeSnapshot) -> Result<Self, String> {
		let (menu, actions) = build_menu(opts, snapshot);
		let icon = TrayIconBuilder::new()
			.with_id(tray_icon_id())
			.with_tooltip(tray_tooltip(opts, snapshot))
			.with_icon(load_tray_icon(opts.icon_path.as_deref()).unwrap_or_else(default_tray_icon))
			.with_menu_on_left_click(false)
			.with_menu(Box::new(menu))
			.build()
			.map_err(|error| format!("build renderer tray: {error}"))?;
		Ok(Self {
			backend: RendererTrayBackend::Local {
				icon,
				actions,
				last_menu_key: menu_key(opts, snapshot),
			},
		})
	}

	fn new_worker(
		opts: &AvatarWindowOptions,
		snapshot: &RendererRuntimeSnapshot,
		_proxy: EventLoopProxy<RendererControlEvent>,
	) -> Result<Self, String> {
		let (startup_tx, startup_rx) = mpsc::channel();
		let opts = opts.clone();
		let snapshot = snapshot.clone();
		let actions = Arc::new(Mutex::new(HashMap::new()));
		let worker_actions = Arc::clone(&actions);
		thread::Builder::new()
			.name("un-avatar-renderer-tray".into())
			.spawn(move || renderer_tray_worker(opts, snapshot, worker_actions, startup_tx))
			.map_err(|error| format!("spawn renderer tray worker: {error}"))?;
		let tx = startup_rx
			.recv_timeout(Duration::from_secs(2))
			.map_err(|error| format!("start renderer tray worker: {error}"))??;
		Ok(Self {
			backend: RendererTrayBackend::Worker { tx, actions },
		})
	}

	pub(crate) fn refresh(&mut self, opts: &AvatarWindowOptions, snapshot: &RendererRuntimeSnapshot) {
		match &mut self.backend {
			RendererTrayBackend::Worker { tx, .. } => {
				let _ = tx.send(RendererTrayWorkerCommand::Refresh {
					opts: opts.clone(),
					snapshot: snapshot.clone(),
				});
			}
			RendererTrayBackend::Local {
				icon,
				actions,
				last_menu_key,
			} => {
				let key = menu_key(opts, snapshot);
				let _ = icon.set_tooltip(Some(tray_tooltip(opts, snapshot)));
				if key == *last_menu_key {
					return;
				}
				let (menu, next_actions) = build_menu(opts, snapshot);
				icon.set_menu(Some(Box::new(menu)));
				*actions = next_actions;
				*last_menu_key = key;
			}
		}
	}

	pub(crate) fn action(&self, id: &str) -> Option<RendererTrayAction> {
		match &self.backend {
			RendererTrayBackend::Worker { actions, .. } => actions.lock().ok().and_then(|actions| actions.get(id).cloned()),
			RendererTrayBackend::Local { actions, .. } => actions.get(id).cloned(),
		}
	}
}

impl Drop for RendererTray {
	fn drop(&mut self) {
		if let RendererTrayBackend::Worker { tx, .. } = &self.backend {
			let _ = tx.send(RendererTrayWorkerCommand::Shutdown);
		}
	}
}

enum RendererTrayWorkerCommand {
	Refresh {
		opts: AvatarWindowOptions,
		snapshot: RendererRuntimeSnapshot,
	},
	Shutdown,
}

struct TrayText {
	locale: String,
}

impl TrayText {
	#[cfg(test)]
	fn en() -> Self {
		Self {
			locale: "en-US".to_string(),
		}
	}

	#[cfg(test)]
	fn ja() -> Self {
		Self {
			locale: "ja-JP".to_string(),
		}
	}

	fn resolve() -> Self {
		let locale = env::var(RENDERER_TRAY_LOCALE_ENV)
			.ok()
			.filter(|locale| RENDERER_TRAY_I18N.has_locale(locale))
			.unwrap_or_else(|| un_i18n::resolve_default_locale(&RENDERER_TRAY_I18N, RENDERER_TRAY_FALLBACK_LOCALE));
		Self { locale }
	}

	fn msg(&self, key: &'static str) -> Cow<'static, str> {
		RENDERER_TRAY_I18N
			.messages_for_locale(&self.locale)
			.and_then(|messages| messages.get(key))
			.map(|value| Cow::Owned(value.clone()))
			.unwrap_or(Cow::Borrowed(key))
	}

	fn format_msg(&self, key: &'static str, replacements: &[(&str, String)]) -> String {
		let mut text = self.msg(key).into_owned();
		for (name, value) in replacements {
			text = text.replace(&format!("%{{{name}}}"), value);
		}
		text
	}

	fn show_focus_preview(&self) -> Cow<'static, str> {
		self.msg("show_focus_preview")
	}

	fn output(&self) -> Cow<'static, str> {
		self.msg("output")
	}

	fn camera(&self) -> Cow<'static, str> {
		self.msg("camera")
	}

	fn window_preview(&self) -> Cow<'static, str> {
		self.msg("window_preview")
	}

	fn spout_preview(&self) -> Cow<'static, str> {
		self.msg("spout_preview")
	}

	fn spout_only(&self) -> Cow<'static, str> {
		self.msg("spout_only")
	}

	fn spout_resolution(&self, width: u32, height: u32) -> String {
		self.format_msg("spout_resolution", &[("width", width.to_string()), ("height", height.to_string())])
	}

	fn spout_output_size(&self, width: u32, height: u32) -> String {
		self.format_msg("spout_output_size", &[("width", width.to_string()), ("height", height.to_string())])
	}

	fn spout_output_default(&self) -> Cow<'static, str> {
		self.msg("spout_output_default")
	}

	fn save_output_profile(&self) -> Cow<'static, str> {
		self.msg("save_output_profile")
	}

	fn save_window_profile(&self) -> Cow<'static, str> {
		self.msg("save_window_profile")
	}

	fn restore_window_profile(&self) -> Cow<'static, str> {
		self.msg("restore_window_profile")
	}

	fn save_camera_profile(&self) -> Cow<'static, str> {
		self.msg("save_camera_profile")
	}

	fn restore_camera_profile(&self) -> Cow<'static, str> {
		self.msg("restore_camera_profile")
	}

	fn save_wardrobe_profile(&self) -> Cow<'static, str> {
		self.msg("save_wardrobe_profile")
	}

	fn wardrobe(&self) -> Cow<'static, str> {
		self.msg("wardrobe")
	}

	fn base_wardrobe(&self) -> Cow<'static, str> {
		self.msg("base_wardrobe")
	}

	fn unanimator(&self) -> Cow<'static, str> {
		self.msg("unanimator")
	}

	fn unphysics(&self) -> Cow<'static, str> {
		self.msg("unphysics")
	}

	fn unphysics_summary(&self, enabled: u32, total: u32) -> String {
		self.format_msg(
			"unphysics_summary",
			&[("enabled", enabled.to_string()), ("total", total.to_string())],
		)
	}

	fn dynamics_enabled(&self) -> Cow<'static, str> {
		self.msg("dynamics_enabled")
	}

	fn window(&self) -> Cow<'static, str> {
		self.msg("window")
	}

	fn window_hidden(&self) -> Cow<'static, str> {
		self.msg("window_hidden")
	}

	fn always_on_top(&self) -> Cow<'static, str> {
		self.msg("always_on_top")
	}

	fn input_passthrough(&self) -> Cow<'static, str> {
		self.msg("input_passthrough")
	}

	fn reset_camera(&self) -> Cow<'static, str> {
		self.msg("reset_camera")
	}

	fn open_supervisor(&self) -> Cow<'static, str> {
		self.msg("open_supervisor")
	}

	fn quit_renderer(&self) -> Cow<'static, str> {
		self.msg("quit_renderer")
	}

	fn scene_state(&self, state: &str) -> String {
		match state.trim() {
			"" => self.msg("scene_starting").into_owned(),
			"avatar_scene" => self.msg("scene_avatar_scene").into_owned(),
			"startup" | "loading" | "startup_progress" => self.msg("scene_loading").into_owned(),
			other => other.replace('_', " "),
		}
	}
}

fn renderer_tray_worker(
	opts: AvatarWindowOptions,
	snapshot: RendererRuntimeSnapshot,
	actions: Arc<Mutex<HashMap<String, RendererTrayAction>>>,
	startup: mpsc::Sender<Result<mpsc::Sender<RendererTrayWorkerCommand>, String>>,
) {
	let (tx, rx) = mpsc::channel();
	let (menu, menu_actions) = build_menu(&opts, &snapshot);
	if let Ok(mut actions) = actions.lock() {
		*actions = menu_actions;
	}
	let icon = match TrayIconBuilder::new()
		.with_id(tray_icon_id())
		.with_tooltip(tray_tooltip(&opts, &snapshot))
		.with_icon(load_tray_icon(opts.icon_path.as_deref()).unwrap_or_else(default_tray_icon))
		.with_menu_on_left_click(false)
		.with_menu(Box::new(menu))
		.build()
	{
		Ok(icon) => icon,
		Err(error) => {
			let _ = startup.send(Err(format!("build renderer tray: {error}")));
			return;
		}
	};
	let _ = startup.send(Ok(tx));
	let mut last_menu_key = menu_key(&opts, &snapshot);
	loop {
		pump_windows_messages();
		match rx.recv_timeout(Duration::from_millis(16)) {
			Ok(RendererTrayWorkerCommand::Refresh { opts, snapshot }) => {
				let key = menu_key(&opts, &snapshot);
				let _ = icon.set_tooltip(Some(tray_tooltip(&opts, &snapshot)));
				if key != last_menu_key {
					let (menu, menu_actions) = build_menu(&opts, &snapshot);
					icon.set_menu(Some(Box::new(menu)));
					if let Ok(mut actions) = actions.lock() {
						*actions = menu_actions;
					}
					last_menu_key = key;
				}
			}
			Ok(RendererTrayWorkerCommand::Shutdown) => break,
			Err(mpsc::RecvTimeoutError::Timeout) => {}
			Err(mpsc::RecvTimeoutError::Disconnected) => break,
		}
	}
}

pub(crate) fn install_event_handlers(proxy: EventLoopProxy<RendererControlEvent>) {
	let menu_proxy = proxy.clone();
	MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
		let _ = menu_proxy.send_event(RendererControlEvent::TrayMenu {
			id: event.id().as_ref().to_string(),
		});
	}));
	TrayIconEvent::set_event_handler(Some(move |event| {
		if let TrayIconEvent::DoubleClick {
			button: MouseButton::Left, ..
		}
		| TrayIconEvent::Click {
			button: MouseButton::Left,
			button_state: MouseButtonState::Up,
			..
		} = event
		{
			let _ = proxy.send_event(RendererControlEvent::TrayIconActivate);
		}
	}));
}

#[cfg(windows)]
fn pump_windows_messages() {
	use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};

	unsafe {
		let mut msg = MSG::default();
		while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
			let _ = TranslateMessage(&msg);
			DispatchMessageW(&msg);
		}
	}
}

pub(crate) fn open_supervisor(manifest_path: Option<&Path>) -> Result<(), String> {
	let exe = resolve_supervisor_exe().ok_or_else(|| "un-avatar-supervisor executable was not found".to_string())?;
	let mut command = Command::new(&exe);
	if let Some(manifest_path) = manifest_path {
		command.arg(SUPERVISOR_OPEN_PROFILE_MANIFEST_ARG).arg(manifest_path);
	}
	if let Some(parent) = exe.parent() {
		command.current_dir(parent);
	}
	#[cfg(windows)]
	{
		use std::os::windows::process::CommandExt;
		const CREATE_NO_WINDOW: u32 = 0x0800_0000;
		command.creation_flags(CREATE_NO_WINDOW);
	}
	command
		.spawn()
		.map_err(|error| format!("open supervisor {}: {error}", exe.display()))?;
	Ok(())
}

fn resolve_supervisor_exe() -> Option<PathBuf> {
	if let Ok(path) = env::var("UN_AVATAR_SUPERVISOR_EXE") {
		let path = PathBuf::from(path);
		if path.is_file() {
			return Some(path);
		}
	}
	let current = env::current_exe().ok()?;
	let current_dir = current.parent()?;
	let sibling = current_dir.join("un-avatar-supervisor.exe");
	if sibling.is_file() {
		return Some(sibling);
	}
	let debug_sibling = current_dir
		.parent()
		.and_then(|target_dir| target_dir.parent())
		.map(|repo| repo.join("target").join("debug").join("un-avatar-supervisor.exe"));
	if let Some(path) = debug_sibling.filter(|path| path.is_file()) {
		return Some(path);
	}
	let release_sibling = current_dir
		.parent()
		.and_then(|target_dir| target_dir.parent())
		.map(|repo| repo.join("target").join("release").join("un-avatar-supervisor.exe"));
	release_sibling.filter(|path| path.is_file())
}

fn build_menu(opts: &AvatarWindowOptions, snapshot: &RendererRuntimeSnapshot) -> (Menu, HashMap<String, RendererTrayAction>) {
	let text = TrayText::resolve();
	let menu = Menu::new();
	let mut actions = HashMap::new();
	let profile_enabled = opts.manifest_path.is_some();

	append_header(&menu, opts, snapshot, &text);
	append_separator(&menu);
	append_menu_item(
		&menu,
		&mut actions,
		"preview:show",
		text.show_focus_preview(),
		true,
		RendererTrayAction::ActivatePreview,
	);

	let output = Submenu::with_id("renderer:output", text.output(), true);
	append_menu_item(
		&output,
		&mut actions,
		"output:window",
		check_label(text.window_preview(), !snapshot.spout_enabled && !snapshot.minimized),
		true,
		RendererTrayAction::SetWindowPreview,
	);
	append_menu_item(
		&output,
		&mut actions,
		"output:spout_preview",
		check_label(text.spout_preview(), snapshot.spout_enabled && !snapshot.minimized),
		snapshot.spout_available,
		RendererTrayAction::SetSpoutPreview,
	);
	append_menu_item(
		&output,
		&mut actions,
		"output:spout_only",
		check_label(text.spout_only(), snapshot.spout_enabled && snapshot.minimized),
		snapshot.spout_available,
		RendererTrayAction::SetSpoutOnly,
	);
	append_separator(&output);
	append_disabled(&output, spout_resolution_label(snapshot, &text));
	append_menu_item(
		&output,
		&mut actions,
		"output:spout_720p",
		check_label(
			text.spout_resolution(1280, 720),
			snapshot.spout_width == Some(1280) && snapshot.spout_height == Some(720),
		),
		snapshot.spout_available,
		RendererTrayAction::SetSpoutResolution { width: 1280, height: 720 },
	);
	append_menu_item(
		&output,
		&mut actions,
		"output:spout_1080p",
		check_label(
			text.spout_resolution(1920, 1080),
			snapshot.spout_width == Some(1920) && snapshot.spout_height == Some(1080),
		),
		snapshot.spout_available,
		RendererTrayAction::SetSpoutResolution { width: 1920, height: 1080 },
	);
	append_separator(&output);
	append_menu_item(
		&output,
		&mut actions,
		"output:save_profile",
		text.save_output_profile(),
		profile_enabled,
		RendererTrayAction::SaveOutputToProfile,
	);
	append_submenu(&menu, &output);

	append_wardrobe_menu(&menu, &mut actions, opts, snapshot, &text);
	append_unanimator_actions(&menu, &mut actions, opts, snapshot, &text);

	if snapshot.dynamics_group_count > 0 {
		let dynamics = Submenu::with_id("renderer:dynamics", text.unphysics(), true);
		let summary = text.unphysics_summary(snapshot.dynamics_enabled_group_count, snapshot.dynamics_group_count);
		let enabled = snapshot.dynamics_enabled_group_count > 0;
		append_disabled(&dynamics, summary);
		append_separator(&dynamics);
		append_menu_item(
			&dynamics,
			&mut actions,
			"dynamics:toggle",
			check_label(text.dynamics_enabled(), enabled),
			true,
			RendererTrayAction::SetCurrentWardrobeDynamics(!enabled),
		);
		append_submenu(&menu, &dynamics);
	}

	let window = Submenu::with_id("renderer:window", text.window(), true);
	append_menu_item(
		&window,
		&mut actions,
		"window:always_on_top",
		check_label(text.always_on_top(), snapshot.always_on_top),
		true,
		RendererTrayAction::SetAlwaysOnTop(!snapshot.always_on_top),
	);
	append_menu_item(
		&window,
		&mut actions,
		"window:input_passthrough",
		check_label(text.input_passthrough(), snapshot.input_passthrough),
		snapshot.transparent_window,
		RendererTrayAction::SetInputPassthrough(!snapshot.input_passthrough),
	);
	append_separator(&window);
	append_menu_item(
		&window,
		&mut actions,
		"window:save_profile",
		text.save_window_profile(),
		profile_enabled && snapshot.window_position.is_some(),
		RendererTrayAction::SaveWindowToProfile,
	);
	append_menu_item(
		&window,
		&mut actions,
		"window:restore_profile",
		text.restore_window_profile(),
		profile_enabled,
		RendererTrayAction::RestoreWindowFromProfile,
	);
	append_submenu(&menu, &window);

	append_separator(&menu);
	let camera = Submenu::with_id("renderer:camera", text.camera(), true);
	append_menu_item(
		&camera,
		&mut actions,
		"camera:reset",
		text.reset_camera(),
		true,
		RendererTrayAction::ResetCamera,
	);
	append_menu_item(
		&camera,
		&mut actions,
		"camera:restore_profile",
		text.restore_camera_profile(),
		profile_enabled,
		RendererTrayAction::RestoreCameraFromProfile,
	);
	append_menu_item(
		&camera,
		&mut actions,
		"camera:save_profile",
		text.save_camera_profile(),
		profile_enabled && snapshot.camera.is_some(),
		RendererTrayAction::SaveCameraToProfile,
	);
	append_submenu(&menu, &camera);
	append_menu_item(
		&menu,
		&mut actions,
		"supervisor:open",
		text.open_supervisor(),
		true,
		RendererTrayAction::OpenSupervisor,
	);
	append_separator(&menu);
	append_menu_item(&menu, &mut actions, "quit", text.quit_renderer(), true, RendererTrayAction::Quit);

	(menu, actions)
}

fn append_unanimator_actions(
	menu: &Menu,
	actions: &mut HashMap<String, RendererTrayAction>,
	opts: &AvatarWindowOptions,
	snapshot: &RendererRuntimeSnapshot,
	text: &TrayText,
) {
	let entries = dedupe_menu_action_candidates(
		snapshot,
		snapshot
			.menu_action_candidates
			.iter()
			.filter(|candidate| animator_menu_candidate_visible(candidate)),
	);
	let fallback_entries = if entries.is_empty() {
		dedupe_fallback_runtime_actions(
			snapshot
				.runtime_actions
				.iter()
				.filter(|action| animator_fallback_action_visible(action)),
		)
	} else {
		Vec::new()
	};
	if entries.is_empty() && fallback_entries.is_empty() {
		return;
	}
	let unanimator_menu = Submenu::with_id(UNANIMATOR_TRAY_MENU_ID, text.unanimator(), true);
	if entries.is_empty() {
		for (index, action) in fallback_entries.into_iter().enumerate() {
			let active = fallback_action_active(snapshot, action);
			let label = fallback_action_label(action);
			let action_id = action.action_id.clone();
			let action = if let (Some(name), Some(value)) = (&action.parameter_name, action.parameter_value) {
				let raw = fallback_action_last_label(action);
				let polarity = animator_toggle_polarity(&raw);
				RendererTrayAction::SetParameter {
					name: name.clone(),
					value: if active {
						animator_inactive_parameter_value(value, polarity)
					} else {
						value
					},
				}
			} else {
				RendererTrayAction::ActivateAction(action.action_id.clone())
			};
			append_menu_item(
				&unanimator_menu,
				actions,
				format!("{UNANIMATOR_TRAY_ACTION_ID_PREFIX}:{index}"),
				check_label(animator_label_with_shortcut(truncate_label(&label, 56), opts, &action_id), active),
				true,
				action,
			);
		}
	} else {
		for (index, candidate) in entries.into_iter().enumerate() {
			let active = menu_candidate_active(snapshot, candidate);
			let raw = menu_candidate_last_label(candidate);
			let polarity = animator_toggle_polarity(&raw);
			append_menu_item(
				&unanimator_menu,
				actions,
				format!("{UNANIMATOR_TRAY_ACTION_ID_PREFIX}:{index}"),
				check_label(
					animator_label_with_shortcut(truncate_label(&menu_action_label(candidate), 56), opts, &candidate.action_id),
					active,
				),
				true,
				RendererTrayAction::SetParameter {
					name: candidate.parameter_name.clone(),
					value: if active {
						animator_inactive_parameter_value(candidate.parameter_value, polarity)
					} else {
						candidate.parameter_value
					},
				},
			);
		}
	}
	append_submenu(menu, &unanimator_menu);
}

fn animator_menu_candidate_visible(candidate: &gpu::RuntimeMenuActionCandidateStatus) -> bool {
	if !candidate.available || !candidate.wardrobe_set_ids.is_empty() || candidate.effect_count == 0 {
		return false;
	}
	if candidate.match_kind != "metadata" {
		return true;
	}
	if candidate.control_type.as_deref() == Some("Button") {
		return false;
	}
	if candidate.menu_path.len() > 2 {
		return false;
	}
	let label = candidate.menu_label.as_deref().unwrap_or("");
	!label.contains('<')
		&& !label.contains("VRCFT")
		&& candidate
			.menu_path
			.iter()
			.all(|segment| segment != "Face_Tracking" && !segment.contains("VRCFT") && !segment.contains('<'))
}

fn animator_fallback_action_visible(action: &gpu::RuntimeActionStatus) -> bool {
	action.available && action.wardrobe_set_id.is_none() && action.effect_count > 0
}

fn dedupe_menu_action_candidates<'a>(
	snapshot: &RendererRuntimeSnapshot,
	candidates: impl Iterator<Item = &'a gpu::RuntimeMenuActionCandidateStatus>,
) -> Vec<&'a gpu::RuntimeMenuActionCandidateStatus> {
	let mut entries: Vec<&gpu::RuntimeMenuActionCandidateStatus> = Vec::new();
	let mut keys: HashMap<String, usize> = HashMap::new();
	for candidate in candidates {
		let key = menu_candidate_group_key(candidate);
		if let Some(index) = keys.get(&key).copied() {
			if menu_candidate_preferred(snapshot, entries[index], candidate) {
				entries[index] = candidate;
			}
		} else {
			keys.insert(key, entries.len());
			entries.push(candidate);
		}
	}
	entries
}

fn dedupe_fallback_runtime_actions<'a>(actions: impl Iterator<Item = &'a gpu::RuntimeActionStatus>) -> Vec<&'a gpu::RuntimeActionStatus> {
	let mut entries: Vec<&gpu::RuntimeActionStatus> = Vec::new();
	let mut keys: HashMap<String, usize> = HashMap::new();
	for action in actions {
		let key = fallback_action_group_key(action);
		if let Some(index) = keys.get(&key).copied() {
			if fallback_action_preferred(entries[index], action) {
				entries[index] = action;
			}
		} else {
			keys.insert(key, entries.len());
			entries.push(action);
		}
	}
	entries
}

fn menu_candidate_group_key(candidate: &gpu::RuntimeMenuActionCandidateStatus) -> String {
	let label = animator_normalized_toggle_label(&menu_candidate_last_label(candidate)).0;
	format!("{}:{}", candidate.parameter_name, label.to_ascii_lowercase())
}

fn fallback_action_group_key(action: &gpu::RuntimeActionStatus) -> String {
	let label = animator_normalized_toggle_label(&fallback_action_last_label(action)).0;
	format!(
		"{}:{}",
		action
			.parameter_name
			.as_deref()
			.or(action.supervisor_command.as_deref())
			.unwrap_or(&action.action_id),
		label.to_ascii_lowercase()
	)
}

fn menu_candidate_preferred(
	snapshot: &RendererRuntimeSnapshot,
	current: &gpu::RuntimeMenuActionCandidateStatus,
	next: &gpu::RuntimeMenuActionCandidateStatus,
) -> bool {
	let current_active = menu_candidate_active(snapshot, current);
	let next_active = menu_candidate_active(snapshot, next);
	if next_active && !current_active {
		return true;
	}
	let current_polarity = animator_toggle_polarity(&menu_candidate_last_label(current));
	let next_polarity = animator_toggle_polarity(&menu_candidate_last_label(next));
	next_polarity == Some(AnimatorTogglePolarity::Off) && current_polarity != Some(AnimatorTogglePolarity::Off)
}

fn fallback_action_preferred(current: &gpu::RuntimeActionStatus, next: &gpu::RuntimeActionStatus) -> bool {
	let current_active = current.current_condition_state.as_deref() == Some("active");
	let next_active = next.current_condition_state.as_deref() == Some("active");
	if next_active && !current_active {
		return true;
	}
	let current_polarity = animator_toggle_polarity(&fallback_action_last_label(current));
	let next_polarity = animator_toggle_polarity(&fallback_action_last_label(next));
	next_polarity == Some(AnimatorTogglePolarity::Off) && current_polarity != Some(AnimatorTogglePolarity::Off)
}

fn runtime_action_active(snapshot: &RendererRuntimeSnapshot, action_id: &str) -> bool {
	if snapshot.active_profile_animator_actions.iter().any(|active| active == action_id) {
		return true;
	}
	snapshot
		.runtime_actions
		.iter()
		.find(|action| action.action_id == action_id)
		.and_then(|action| action.current_condition_state.as_deref())
		== Some("active")
}

fn fallback_action_active(snapshot: &RendererRuntimeSnapshot, action: &gpu::RuntimeActionStatus) -> bool {
	runtime_action_active(snapshot, &action.action_id)
}

fn menu_candidate_active(snapshot: &RendererRuntimeSnapshot, candidate: &gpu::RuntimeMenuActionCandidateStatus) -> bool {
	if candidate.match_kind == "metadata" {
		return snapshot
			.runtime_parameter_values
			.get(&candidate.parameter_name)
			.is_some_and(|value| (*value - candidate.parameter_value).abs() <= un_avatar_core::UNA_RUNTIME_ACTION_PARAMETER_EPSILON);
	}
	runtime_action_active(snapshot, &candidate.action_id)
}

fn append_wardrobe_menu(
	menu: &Menu,
	actions: &mut HashMap<String, RendererTrayAction>,
	opts: &AvatarWindowOptions,
	snapshot: &RendererRuntimeSnapshot,
	text: &TrayText,
) {
	let mut entries: Vec<(String, String, RendererTrayAction)> = Vec::new();
	for candidate in &snapshot.menu_wardrobe_candidates {
		entries.push((
			menu_wardrobe_label(candidate),
			candidate.wardrobe_set_id.clone(),
			RendererTrayAction::ActivateAction(candidate.action_id.clone()),
		));
	}
	if entries.is_empty() {
		for action in &snapshot.wardrobe_actions {
			entries.push((
				action.label.clone(),
				action.set_id.clone(),
				RendererTrayAction::ActivateAction(action.action_id.clone()),
			));
		}
	}
	if entries.iter().all(|(_, set_id, _)| !set_id.trim().is_empty()) {
		entries.insert(
			0,
			(
				text.base_wardrobe().to_string(),
				String::new(),
				RendererTrayAction::SetWardrobe(String::new()),
			),
		);
	}
	if entries.is_empty() {
		return;
	}
	let wardrobe = Submenu::with_id("renderer:wardrobe", text.wardrobe(), true);
	let active_set = snapshot.active_wardrobe_set.as_deref().unwrap_or("").trim();
	let base_set = snapshot.base_wardrobe_set.as_deref().unwrap_or("").trim();
	for (index, (label, set_id, action)) in entries.into_iter().enumerate() {
		let set_id = set_id.trim().to_string();
		let active = wardrobe_set_active(active_set, base_set, &set_id);
		let label = wardrobe_label_with_shortcut(truncate_label(&label, 56), opts, &set_id);
		append_menu_item(
			&wardrobe,
			actions,
			format!("wardrobe:{index}"),
			check_label(label, active),
			true,
			action,
		);
	}
	append_separator(&wardrobe);
	append_menu_item(
		&wardrobe,
		actions,
		"wardrobe:save_profile",
		text.save_wardrobe_profile(),
		opts.manifest_path.is_some(),
		RendererTrayAction::SaveWardrobeToProfile,
	);
	append_submenu(menu, &wardrobe);
}

fn wardrobe_label_with_shortcut(label: String, opts: &AvatarWindowOptions, set_id: &str) -> String {
	let Some(shortcut) = opts
		.wardrobe_bindings
		.iter()
		.filter(|binding| binding.kind == crate::WardrobeBindingKind::Keyboard)
		.find(|shortcut| shortcut.set_id.trim() == set_id)
		.map(|shortcut| shortcut.binding.trim())
		.filter(|shortcut| !shortcut.is_empty())
	else {
		return label;
	};
	format!("{label} ({shortcut})")
}

fn animator_label_with_shortcut(label: String, opts: &AvatarWindowOptions, action_id: &str) -> String {
	let Some(shortcut) = opts
		.animator_bindings
		.iter()
		.filter(|binding| binding.kind == crate::WardrobeBindingKind::Keyboard)
		.find(|binding| binding.action_id.trim() == action_id)
		.map(|binding| binding.binding.trim())
		.filter(|binding| !binding.is_empty())
	else {
		return label;
	};
	format!("{label} ({shortcut})")
}

fn wardrobe_set_active(active_set: &str, base_set: &str, set_id: &str) -> bool {
	set_id == active_set || (set_id.is_empty() && !base_set.is_empty() && active_set == base_set)
}

fn menu_action_label(candidate: &gpu::RuntimeMenuActionCandidateStatus) -> String {
	let path = if !candidate.menu_path.is_empty() {
		candidate.menu_path.clone()
	} else {
		match (candidate.menu_label.as_deref(), candidate.action_label.as_str()) {
			(Some(menu_label), action_label) if !action_label.is_empty() && menu_label != action_label => {
				vec![menu_label.to_string(), action_label.to_string()]
			}
			(Some(menu_label), _) if !menu_label.is_empty() => vec![menu_label.to_string()],
			(_, action_label) if !action_label.is_empty() => vec![action_label.to_string()],
			_ => vec![format!("{} = {}", candidate.parameter_name, candidate.parameter_value)],
		}
	};
	normalize_animator_path_label(path)
}

fn menu_candidate_last_label(candidate: &gpu::RuntimeMenuActionCandidateStatus) -> String {
	candidate
		.menu_path
		.last()
		.cloned()
		.or_else(|| candidate.menu_label.clone())
		.filter(|label| !label.is_empty())
		.unwrap_or_else(|| {
			if candidate.action_label.is_empty() {
				candidate.action_id.clone()
			} else {
				candidate.action_label.clone()
			}
		})
}

fn fallback_action_label(action: &gpu::RuntimeActionStatus) -> String {
	let path = action
		.expression_menu_path
		.as_deref()
		.filter(|path| !path.trim().is_empty())
		.map(|path| path.split('/').map(|segment| segment.trim().to_string()).collect::<Vec<_>>())
		.unwrap_or_else(|| vec![fallback_action_last_label(action)]);
	normalize_animator_path_label(path)
}

fn fallback_action_last_label(action: &gpu::RuntimeActionStatus) -> String {
	action
		.expression_menu_path
		.as_deref()
		.and_then(|path| path.split('/').next_back())
		.map(str::trim)
		.filter(|label| !label.is_empty())
		.unwrap_or_else(|| {
			if action.label.is_empty() {
				action.action_id.as_str()
			} else {
				action.label.as_str()
			}
		})
		.to_string()
}

fn normalize_animator_path_label(mut path: Vec<String>) -> String {
	if let Some(last) = path.pop() {
		let (label, _) = animator_normalized_toggle_label(&last);
		path.push(label);
	}
	path.join(" / ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnimatorTogglePolarity {
	On,
	Off,
}

fn animator_toggle_polarity(label: &str) -> Option<AnimatorTogglePolarity> {
	animator_normalized_toggle_label(label).1
}

fn animator_normalized_toggle_label(label: &str) -> (String, Option<AnimatorTogglePolarity>) {
	let trimmed = label.trim();
	let upper = trimmed.to_ascii_uppercase();
	for suffix in ["OFF", "ON"] {
		if !upper.ends_with(suffix) {
			continue;
		}
		let base = trimmed[..trimmed.len() - suffix.len()]
			.trim_end_matches(|ch: char| ch.is_whitespace() || ch == '_' || ch == ':' || ch == '/' || ch == '-')
			.trim();
		if base.is_empty() {
			continue;
		}
		let polarity = if suffix == "OFF" {
			AnimatorTogglePolarity::Off
		} else {
			AnimatorTogglePolarity::On
		};
		return (base.to_string(), Some(polarity));
	}
	(trimmed.to_string(), None)
}

fn animator_inactive_parameter_value(value: f32, polarity: Option<AnimatorTogglePolarity>) -> f32 {
	let _ = polarity;
	if value.abs() <= 0.005 {
		1.0
	} else {
		0.0
	}
}

fn append_header(menu: &Menu, opts: &AvatarWindowOptions, snapshot: &RendererRuntimeSnapshot, text: &TrayText) {
	append_disabled(menu, format!("{}  pid {}", truncate_label(&opts.title, 48), std::process::id()));
	let state = text.scene_state(&snapshot.scene_state);
	let fps = snapshot.fps.map_or("--".to_string(), |fps| format!("{fps:.0} fps"));
	append_disabled(menu, format!("{state}  {fps}"));
}

trait AppendTrayMenuItem {
	fn append_tray_item(&self, item: &dyn IsMenuItem);
}

impl AppendTrayMenuItem for Menu {
	fn append_tray_item(&self, item: &dyn IsMenuItem) {
		let _ = self.append(item);
	}
}

impl AppendTrayMenuItem for Submenu {
	fn append_tray_item(&self, item: &dyn IsMenuItem) {
		let _ = self.append(item);
	}
}

fn append_menu_item<M: AppendTrayMenuItem>(
	menu: &M,
	actions: &mut HashMap<String, RendererTrayAction>,
	id_suffix: impl AsRef<str>,
	text: impl AsRef<str>,
	enabled: bool,
	action: RendererTrayAction,
) {
	let id = format!("renderer:{}", id_suffix.as_ref());
	actions.insert(id.clone(), action);
	let item = MenuItem::with_id(id, text.as_ref(), enabled, None);
	menu.append_tray_item(&item);
}

fn append_disabled<M: AppendTrayMenuItem>(menu: &M, text: impl AsRef<str>) {
	let item = MenuItem::new(text.as_ref(), false, None);
	menu.append_tray_item(&item);
}

fn append_separator<M: AppendTrayMenuItem>(menu: &M) {
	let separator = PredefinedMenuItem::separator();
	menu.append_tray_item(&separator);
}

fn append_submenu(menu: &Menu, submenu: &Submenu) {
	let _ = menu.append(submenu);
}

fn check_label(label: impl AsRef<str>, checked: bool) -> String {
	if checked {
		format!("[x] {}", label.as_ref())
	} else {
		format!("[ ] {}", label.as_ref())
	}
}

fn spout_resolution_label(snapshot: &RendererRuntimeSnapshot, text: &TrayText) -> String {
	match (snapshot.spout_width, snapshot.spout_height) {
		(Some(width), Some(height)) => text.spout_output_size(width, height),
		_ => text.spout_output_default().to_string(),
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrayOutputProfileState {
	pub(crate) spout_enabled: bool,
	pub(crate) spout_name: Option<String>,
	pub(crate) spout_width: Option<u32>,
	pub(crate) spout_height: Option<u32>,
	pub(crate) minimized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrayWindowProfileState {
	pub(crate) position: Option<[i32; 2]>,
	pub(crate) inner_size: Option<[u32; 2]>,
}

pub(crate) struct TrayCameraProfileState {
	pub(crate) target: Option<[f32; 3]>,
	pub(crate) longitude_deg: Option<f32>,
	pub(crate) latitude_deg: Option<f32>,
	pub(crate) radius: Option<f32>,
	pub(crate) diagonal_fov_deg: Option<f32>,
}

pub(crate) fn output_profile_state_from_snapshot(snapshot: &RendererRuntimeSnapshot) -> TrayOutputProfileState {
	TrayOutputProfileState {
		spout_enabled: snapshot.spout_enabled,
		spout_name: snapshot.spout_name.clone(),
		spout_width: snapshot.spout_width.or(snapshot.spout_sender_width),
		spout_height: snapshot.spout_height.or(snapshot.spout_sender_height),
		minimized: snapshot.minimized,
	}
}

pub(crate) fn window_profile_state_from_snapshot(snapshot: &RendererRuntimeSnapshot) -> Result<TrayWindowProfileState, String> {
	let position = snapshot
		.window_position
		.ok_or_else(|| "renderer has not reported window position yet".to_string())?;
	Ok(TrayWindowProfileState {
		position: Some(position),
		inner_size: snapshot.window_inner_size,
	})
}

pub(crate) fn save_output_state_to_profile(manifest_path: &Path, state: &TrayOutputProfileState) -> Result<(), String> {
	let mut manifest = read_profile_manifest(manifest_path)?;
	{
		let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
		let output_table = table
			.entry("output".to_string())
			.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
			.as_table_mut()
			.ok_or_else(|| "manifest [output] must be a table".to_string())?;
		let spout2_table = output_table
			.entry("spout2".to_string())
			.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
			.as_table_mut()
			.ok_or_else(|| "manifest [output.spout2] must be a table".to_string())?;
		spout2_table.insert("enabled".to_string(), toml::Value::Boolean(state.spout_enabled));
		if let Some(name) = state.spout_name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
			spout2_table.insert("name".to_string(), toml::Value::String(name.to_string()));
		}
		if let Some(width) = state.spout_width {
			spout2_table.insert("width".to_string(), toml::Value::Integer(i64::from(width)));
		}
		if let Some(height) = state.spout_height {
			spout2_table.insert("height".to_string(), toml::Value::Integer(i64::from(height)));
		}
		let window_table = table
			.entry("window".to_string())
			.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
			.as_table_mut()
			.ok_or_else(|| "manifest [window] must be a table".to_string())?;
		window_table.insert("minimized".to_string(), toml::Value::Boolean(state.minimized));
	}
	write_profile_manifest(manifest_path, &manifest)
}

pub(crate) fn save_window_state_to_profile(manifest_path: &Path, state: &TrayWindowProfileState) -> Result<(), String> {
	let mut manifest = read_profile_manifest(manifest_path)?;
	let table = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	let window_table = table
		.entry("window".to_string())
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.ok_or_else(|| "manifest [window] must be a table".to_string())?;
	if let Some([x, y]) = state.position {
		window_table.insert("x".to_string(), toml::Value::Integer(i64::from(x)));
		window_table.insert("y".to_string(), toml::Value::Integer(i64::from(y)));
	}
	if let Some([width, height]) = state.inner_size {
		window_table.insert("width".to_string(), toml::Value::Integer(i64::from(width)));
		window_table.insert("height".to_string(), toml::Value::Integer(i64::from(height)));
	}
	write_profile_manifest(manifest_path, &manifest)
}

pub(crate) fn save_camera_state_to_profile(manifest_path: &Path, state: &gpu::CameraStateSnapshot) -> Result<(), String> {
	let mut manifest = read_profile_manifest(manifest_path)?;
	let root = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	let camera_table = root
		.entry("camera".to_string())
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.ok_or_else(|| "manifest [camera] must be a table".to_string())?;
	camera_table.insert(
		"target".to_string(),
		toml::Value::Array(state.target.iter().map(|value| toml::Value::Float(f64::from(*value))).collect()),
	);
	camera_table.insert("longitude_deg".to_string(), toml::Value::Float(f64::from(state.longitude_deg)));
	camera_table.insert("latitude_deg".to_string(), toml::Value::Float(f64::from(state.latitude_deg)));
	camera_table.insert("radius".to_string(), toml::Value::Float(f64::from(state.radius)));
	camera_table.insert(
		"diagonal_fov_deg".to_string(),
		toml::Value::Float(f64::from(state.diagonal_fov_deg)),
	);
	write_profile_manifest(manifest_path, &manifest)
}

pub(crate) fn save_wardrobe_state_to_profile(manifest_path: &Path, active_set: Option<&str>, base_set: Option<&str>) -> Result<(), String> {
	let mut manifest = read_profile_manifest(manifest_path)?;
	let root = manifest.as_table_mut().ok_or_else(|| "manifest root must be a table".to_string())?;
	let active = active_set.map(str::trim).filter(|set_id| !set_id.is_empty());
	let base = base_set.map(str::trim).filter(|set_id| !set_id.is_empty());
	if active.is_none() || active == base {
		root.remove("wardrobe_set");
	} else if let Some(active) = active {
		root.insert("wardrobe_set".to_string(), toml::Value::String(active.to_string()));
	}
	write_profile_manifest(manifest_path, &manifest)
}

pub(crate) fn read_window_state_from_profile(manifest_path: &Path) -> Result<TrayWindowProfileState, String> {
	let manifest = read_profile_manifest(manifest_path)?;
	let root = manifest
		.as_table()
		.ok_or_else(|| format!("manifest {} root must be a table", manifest_path.display()))?;
	let window_table = root
		.get("window")
		.and_then(toml::Value::as_table)
		.ok_or_else(|| format!("manifest {} has no [window]", manifest_path.display()))?;
	let x = read_profile_i32(window_table, "x");
	let y = read_profile_i32(window_table, "y");
	let width = read_profile_u32(window_table, "width");
	let height = read_profile_u32(window_table, "height");
	if x.is_none() && y.is_none() && width.is_none() && height.is_none() {
		return Err(format!("manifest {} has no window x/y/width/height", manifest_path.display()));
	}
	Ok(TrayWindowProfileState {
		position: match (x, y) {
			(Some(x), Some(y)) => Some([x, y]),
			_ => None,
		},
		inner_size: match (width, height) {
			(Some(width), Some(height)) => Some([width, height]),
			_ => None,
		},
	})
}

pub(crate) fn read_camera_state_from_profile(manifest_path: &Path) -> Result<TrayCameraProfileState, String> {
	let manifest = read_profile_manifest(manifest_path)?;
	let root = manifest
		.as_table()
		.ok_or_else(|| format!("manifest {} root must be a table", manifest_path.display()))?;
	let camera_table = root
		.get("camera")
		.and_then(toml::Value::as_table)
		.ok_or_else(|| format!("manifest {} has no [camera]", manifest_path.display()))?;
	let state = TrayCameraProfileState {
		target: read_profile_f32_array3(camera_table, "target"),
		longitude_deg: read_profile_f32(camera_table, "longitude_deg"),
		latitude_deg: read_profile_f32(camera_table, "latitude_deg"),
		radius: read_profile_f32(camera_table, "radius"),
		diagonal_fov_deg: read_profile_f32(camera_table, "diagonal_fov_deg"),
	};
	if state.target.is_none()
		&& state.longitude_deg.is_none()
		&& state.latitude_deg.is_none()
		&& state.radius.is_none()
		&& state.diagonal_fov_deg.is_none()
	{
		return Err(format!("manifest {} has no camera state", manifest_path.display()));
	}
	Ok(state)
}

fn read_profile_manifest(manifest_path: &Path) -> Result<toml::Value, String> {
	let text = fs::read_to_string(manifest_path).map_err(|error| format!("read {}: {error}", manifest_path.display()))?;
	let table = toml::from_str::<toml::Table>(&text).map_err(|error| format!("parse {}: {error}", manifest_path.display()))?;
	Ok(toml::Value::Table(table))
}

fn write_profile_manifest(manifest_path: &Path, manifest: &toml::Value) -> Result<(), String> {
	let text = toml::to_string_pretty(manifest).map_err(|error| format!("serialize manifest: {error}"))?;
	fs::write(manifest_path, text).map_err(|error| format!("write {}: {error}", manifest_path.display()))
}

fn read_profile_u32(table: &toml::map::Map<String, toml::Value>, key: &str) -> Option<u32> {
	table
		.get(key)
		.and_then(toml::Value::as_integer)
		.and_then(|value| u32::try_from(value).ok())
}

fn read_profile_i32(table: &toml::map::Map<String, toml::Value>, key: &str) -> Option<i32> {
	table
		.get(key)
		.and_then(toml::Value::as_integer)
		.and_then(|value| i32::try_from(value).ok())
}

fn read_profile_f32(table: &toml::map::Map<String, toml::Value>, key: &str) -> Option<f32> {
	table.get(key).and_then(|value| match value {
		toml::Value::Float(value) => Some(*value as f32),
		toml::Value::Integer(value) => Some(*value as f32),
		_ => None,
	})
}

fn read_profile_f32_array3(table: &toml::map::Map<String, toml::Value>, key: &str) -> Option<[f32; 3]> {
	let values = table.get(key)?.as_array()?;
	let [x, y, z] = values.as_slice() else {
		return None;
	};
	Some([toml_value_f32(x)?, toml_value_f32(y)?, toml_value_f32(z)?])
}

fn toml_value_f32(value: &toml::Value) -> Option<f32> {
	match value {
		toml::Value::Float(value) => Some(*value as f32),
		toml::Value::Integer(value) => Some(*value as f32),
		_ => None,
	}
}

fn menu_wardrobe_label(candidate: &gpu::RuntimeMenuWardrobeCandidateStatus) -> String {
	if !candidate.menu_path.is_empty() {
		candidate.menu_path.join(" / ")
	} else if let Some(label) = candidate.menu_label.as_deref() {
		label.to_string()
	} else {
		candidate.wardrobe_set_id.clone()
	}
}

fn truncate_label(label: &str, max_chars: usize) -> String {
	let mut out = String::new();
	for (index, ch) in label.chars().enumerate() {
		if index >= max_chars {
			out.push_str("...");
			return out;
		}
		out.push(ch);
	}
	out
}

fn tray_tooltip(opts: &AvatarWindowOptions, snapshot: &RendererRuntimeSnapshot) -> String {
	let text = TrayText::resolve();
	format!(
		"UN Avatar Renderer - {} - pid {} - {}",
		opts.title,
		std::process::id(),
		tray_output_mode_label(snapshot, &text)
	)
}

fn tray_output_mode_label(snapshot: &RendererRuntimeSnapshot, text: &TrayText) -> Cow<'static, str> {
	if snapshot.spout_enabled && snapshot.minimized {
		text.spout_only()
	} else if snapshot.spout_enabled {
		text.spout_preview()
	} else if snapshot.minimized {
		text.window_hidden()
	} else {
		text.window_preview()
	}
}

fn tray_icon_id() -> String {
	format!("{TRAY_ICON_ID_PREFIX}-{}", std::process::id())
}

fn menu_key(opts: &AvatarWindowOptions, snapshot: &RendererRuntimeSnapshot) -> String {
	format!(
		"{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
		snapshot.scene_state,
		snapshot.spout_available,
		snapshot.spout_enabled,
		snapshot.minimized,
		snapshot.transparent_window,
		snapshot.always_on_top,
		snapshot.input_passthrough,
		snapshot.spout_width.map_or(0, |value| value),
		snapshot.spout_height.map_or(0, |value| value),
		snapshot.dynamics_group_count,
		snapshot.dynamics_enabled_group_count,
		snapshot.active_wardrobe_set.as_deref().unwrap_or(""),
		snapshot.base_wardrobe_set.as_deref().unwrap_or(""),
		menu_action_signature(snapshot),
		wardrobe_menu_signature(snapshot),
		snapshot.active_profile_animator_actions.join(","),
		wardrobe_shortcut_signature(opts)
	)
}

fn wardrobe_shortcut_signature(opts: &AvatarWindowOptions) -> String {
	let mut signature = format!("bindings:{}:{}", opts.wardrobe_bindings.len(), opts.animator_bindings.len());
	for shortcut in &opts.wardrobe_bindings {
		signature.push('|');
		signature.push_str(&signature_field(shortcut.set_id.trim()));
		signature.push(':');
		signature.push_str(&signature_field(&format!("{:?}", shortcut.kind)));
		signature.push(':');
		signature.push_str(&signature_field(shortcut.binding.trim()));
		signature.push(':');
		signature.push_str(&signature_field(shortcut.device.as_deref().unwrap_or("").trim()));
		signature.push(':');
		signature.push_str(&signature_field(
			&shortcut.channel.map(|value| value.to_string()).unwrap_or_default(),
		));
		signature.push(':');
		signature.push_str(&signature_field(&shortcut.note.map(|value| value.to_string()).unwrap_or_default()));
	}
	for binding in &opts.animator_bindings {
		signature.push('|');
		signature.push_str(&signature_field(binding.action_id.trim()));
		signature.push(':');
		signature.push_str(&signature_field(&format!("{:?}", binding.kind)));
		signature.push(':');
		signature.push_str(&signature_field(binding.binding.trim()));
		signature.push(':');
		signature.push_str(&signature_field(binding.device.as_deref().unwrap_or("").trim()));
		signature.push(':');
		signature.push_str(&signature_field(
			&binding.channel.map(|value| value.to_string()).unwrap_or_default(),
		));
		signature.push(':');
		signature.push_str(&signature_field(&binding.note.map(|value| value.to_string()).unwrap_or_default()));
	}
	signature
}

fn menu_action_signature(snapshot: &RendererRuntimeSnapshot) -> String {
	let mut signature = format!("actions:{}", snapshot.menu_action_candidates.len());
	let visible_menu_candidate_count = snapshot
		.menu_action_candidates
		.iter()
		.filter(|candidate| animator_menu_candidate_visible(candidate))
		.count();
	for candidate in &snapshot.menu_action_candidates {
		signature.push('|');
		signature.push_str(&signature_field(&candidate.action_id));
		signature.push(':');
		signature.push_str(&signature_field(&candidate.menu_key));
		signature.push(':');
		signature.push_str(&signature_field(&candidate.menu_path.join("/")));
		signature.push(':');
		signature.push_str(if candidate.menu_path_truncated { "1" } else { "0" });
		signature.push(':');
		signature.push_str(&signature_field(candidate.menu_label.as_deref().unwrap_or("")));
		signature.push(':');
		signature.push_str(if candidate.available { "1" } else { "0" });
		signature.push(':');
		signature.push_str(&signature_field(&candidate.action_label));
		signature.push(':');
		signature.push_str(&signature_field(&candidate.parameter_name));
		signature.push(':');
		signature.push_str(&signature_field(&candidate.parameter_value.to_string()));
		signature.push(':');
		signature.push_str(if candidate.wardrobe_set_ids.is_empty() { "0" } else { "1" });
		signature.push(':');
		signature.push_str(if menu_candidate_active(snapshot, candidate) {
			"active"
		} else {
			"inactive"
		});
	}
	if visible_menu_candidate_count == 0 {
		let fallback_actions = snapshot
			.runtime_actions
			.iter()
			.filter(|action| animator_fallback_action_visible(action))
			.collect::<Vec<_>>();
		signature.push_str(&format!("|fallback:{}", fallback_actions.len()));
		for action in fallback_actions {
			signature.push('|');
			signature.push_str(&signature_field(&action.action_id));
			signature.push(':');
			signature.push_str(if action.available { "1" } else { "0" });
			signature.push(':');
			signature.push_str(&signature_field(action.expression_menu_path.as_deref().unwrap_or("")));
			signature.push(':');
			signature.push_str(&signature_field(action.parameter_name.as_deref().unwrap_or("")));
			signature.push(':');
			signature.push_str(&signature_field(
				&action.parameter_value.map(|value| value.to_string()).unwrap_or_default(),
			));
			signature.push(':');
			signature.push_str(&signature_field(action.current_condition_state.as_deref().unwrap_or("")));
		}
	}
	signature
}

fn wardrobe_menu_signature(snapshot: &RendererRuntimeSnapshot) -> String {
	if !snapshot.menu_wardrobe_candidates.is_empty() {
		let mut signature = format!("menu:{}", snapshot.menu_wardrobe_candidates.len());
		for candidate in &snapshot.menu_wardrobe_candidates {
			signature.push('|');
			signature.push_str(&signature_field(&candidate.action_id));
			signature.push(':');
			signature.push_str(&signature_field(&candidate.wardrobe_set_id));
			signature.push(':');
			signature.push_str(&signature_field(candidate.menu_label.as_deref().unwrap_or("")));
			signature.push(':');
			signature.push_str(if candidate.menu_path_truncated { "1" } else { "0" });
			signature.push(':');
			signature.push_str(&signature_field(&candidate.menu_path.join("/")));
		}
		return signature;
	}
	let mut signature = format!("actions:{}", snapshot.wardrobe_actions.len());
	for action in &snapshot.wardrobe_actions {
		signature.push('|');
		signature.push_str(&signature_field(&action.action_id));
		signature.push(':');
		signature.push_str(&signature_field(&action.set_id));
		signature.push(':');
		signature.push_str(&signature_field(&action.label));
		signature.push(':');
		signature.push_str(&signature_field(action.expression_menu_path.as_deref().unwrap_or("")));
	}
	signature
}

fn signature_field(value: &str) -> String {
	format!("{}#{}", value.len(), value)
}

fn load_tray_icon(path: Option<&Path>) -> Option<TrayIconImage> {
	let path = path?;
	let image = image::open(path).ok()?.into_rgba8();
	let (width, height) = image.dimensions();
	TrayIconImage::from_rgba(image.into_raw(), width, height).ok()
}

fn default_tray_icon() -> TrayIconImage {
	let size = 32u32;
	let mut rgba = vec![0u8; (size * size * 4) as usize];
	for y in 0..size {
		for x in 0..size {
			let idx = ((y * size + x) * 4) as usize;
			let in_border = !(4..28).contains(&x) || !(4..28).contains(&y);
			let in_u = (8..12).contains(&x) && (8..24).contains(&y)
				|| (20..24).contains(&x) && (8..24).contains(&y)
				|| (12..20).contains(&x) && (20..24).contains(&y);
			if in_border {
				rgba[idx..idx + 4].copy_from_slice(&[34, 42, 54, 255]);
			} else if in_u {
				rgba[idx..idx + 4].copy_from_slice(&[86, 187, 255, 255]);
			} else {
				rgba[idx..idx + 4].copy_from_slice(&[18, 24, 32, 255]);
			}
		}
	}
	TrayIconImage::from_rgba(rgba, size, size).expect("generated tray icon dimensions are valid")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::initial_runtime_snapshot;

	fn snapshot() -> RendererRuntimeSnapshot {
		initial_runtime_snapshot(&AvatarWindowOptions::default())
	}

	fn test_menu_key(snapshot: &RendererRuntimeSnapshot) -> String {
		menu_key(&AvatarWindowOptions::default(), snapshot)
	}

	#[test]
	fn menu_key_tracks_runtime_operation_state() {
		let mut before = snapshot();
		before.scene_state = "ready".to_string();
		before.spout_available = true;
		before.spout_enabled = false;
		before.minimized = false;
		before.dynamics_group_count = 3;
		before.dynamics_enabled_group_count = 1;
		before.active_wardrobe_set = Some("base".to_string());

		let mut after = before.clone();
		after.spout_enabled = true;
		after.minimized = true;
		after.spout_width = Some(1920);
		after.spout_height = Some(1080);
		after.dynamics_enabled_group_count = 3;
		after.active_wardrobe_set = Some("field_drape".to_string());

		assert_ne!(test_menu_key(&before), test_menu_key(&after));
	}

	#[test]
	fn menu_key_tracks_base_wardrobe_set_resolution() {
		let mut before = snapshot();
		before.active_wardrobe_set = Some("base".to_string());
		before.base_wardrobe_set = None;

		let mut after = before.clone();
		after.base_wardrobe_set = Some("base".to_string());

		assert_ne!(test_menu_key(&before), test_menu_key(&after));
	}

	#[test]
	fn menu_key_tracks_transparent_window_for_input_passthrough_availability() {
		let mut before = snapshot();
		before.transparent_window = false;
		before.input_passthrough = false;

		let mut after = before.clone();
		after.transparent_window = true;

		assert_ne!(test_menu_key(&before), test_menu_key(&after));
	}

	#[test]
	fn menu_key_tracks_wardrobe_source_changes_with_same_count() {
		let mut actions_only = snapshot();
		actions_only.wardrobe_actions = vec![gpu::RuntimeWardrobeActionStatus {
			action_id: "action:field_drape".to_string(),
			label: "Field Drape".to_string(),
			set_id: "field_drape".to_string(),
			..Default::default()
		}];

		let mut menu_candidates = snapshot();
		menu_candidates.menu_wardrobe_candidates = vec![gpu::RuntimeMenuWardrobeCandidateStatus {
			action_id: "menu:field_drape".to_string(),
			wardrobe_set_id: "field_drape".to_string(),
			menu_path: vec!["Wardrobe".to_string(), "Field Drape".to_string()],
			..Default::default()
		}];

		assert_ne!(test_menu_key(&actions_only), test_menu_key(&menu_candidates));
	}

	#[test]
	fn menu_key_tracks_all_wardrobe_candidate_labels() {
		let mut before = snapshot();
		before.menu_wardrobe_candidates = vec![
			gpu::RuntimeMenuWardrobeCandidateStatus {
				action_id: "menu:base".to_string(),
				wardrobe_set_id: "".to_string(),
				menu_path: vec!["Wardrobe".to_string(), "Base".to_string()],
				..Default::default()
			},
			gpu::RuntimeMenuWardrobeCandidateStatus {
				action_id: "menu:field_drape".to_string(),
				wardrobe_set_id: "field_drape".to_string(),
				menu_path: vec!["Wardrobe".to_string(), "Field Drape".to_string()],
				..Default::default()
			},
		];

		let mut after = before.clone();
		after.menu_wardrobe_candidates[1].menu_path = vec!["Wardrobe".to_string(), "Field drape".to_string()];

		assert_ne!(test_menu_key(&before), test_menu_key(&after));
	}

	#[test]
	fn menu_key_tracks_unanimator_action_labels() {
		let mut before = snapshot();
		before.menu_action_candidates = vec![gpu::RuntimeMenuActionCandidateStatus {
			action_id: "action:smile".to_string(),
			action_label: "Smile".to_string(),
			menu_key: "expressions/smile".to_string(),
			menu_label: Some("Smile".to_string()),
			parameter_name: "Smile".to_string(),
			parameter_value: 1.0,
			available: true,
			..Default::default()
		}];

		let mut after = before.clone();
		after.menu_action_candidates[0].menu_label = Some("Big Smile".to_string());

		assert_ne!(test_menu_key(&before), test_menu_key(&after));
	}

	#[test]
	fn menu_key_tracks_unanimator_parameter_dispatch_values() {
		let mut before = snapshot();
		before.menu_action_candidates = vec![gpu::RuntimeMenuActionCandidateStatus {
			action_id: "action:smile".to_string(),
			action_label: "Smile".to_string(),
			menu_key: "expressions/smile".to_string(),
			menu_label: Some("Smile".to_string()),
			parameter_name: "Smile".to_string(),
			parameter_value: 1.0,
			available: true,
			..Default::default()
		}];

		let mut after = before.clone();
		after.menu_action_candidates[0].parameter_value = 2.0;

		assert_ne!(test_menu_key(&before), test_menu_key(&after));
	}

	#[test]
	fn menu_key_falls_back_when_menu_candidates_are_not_unanimator_visible() {
		let mut before = snapshot();
		before.menu_action_candidates = vec![gpu::RuntimeMenuActionCandidateStatus {
			action_id: "wardrobe:field_drape".to_string(),
			action_label: "Field Drape".to_string(),
			menu_key: "wardrobe/field_drape".to_string(),
			parameter_name: "Wardrobe".to_string(),
			parameter_value: 1.0,
			wardrobe_set_ids: vec!["field_drape".to_string()],
			available: true,
			..Default::default()
		}];
		before.runtime_actions = vec![gpu::RuntimeActionStatus {
			action_id: "action:smile".to_string(),
			label: "Smile".to_string(),
			effect_count: 1,
			expression_menu_path: Some("Expressions/Smile".to_string()),
			parameter_name: Some("Smile".to_string()),
			parameter_value: Some(1.0),
			available: true,
			..Default::default()
		}];

		let mut after = before.clone();
		after.runtime_actions[0].expression_menu_path = Some("Expressions/Big Smile".to_string());

		assert_ne!(test_menu_key(&before), test_menu_key(&after));
	}

	#[test]
	fn menu_key_ignores_wardrobe_runtime_actions_in_unanimator_fallback() {
		let mut before = snapshot();
		before.runtime_actions = vec![gpu::RuntimeActionStatus {
			action_id: "wardrobe:field_drape".to_string(),
			label: "Field Drape".to_string(),
			expression_menu_path: Some("Wardrobe/Field Drape".to_string()),
			wardrobe_set_id: Some("field_drape".to_string()),
			..Default::default()
		}];

		let mut after = before.clone();
		after.runtime_actions[0].expression_menu_path = Some("Wardrobe/Field Drape Updated".to_string());

		assert_eq!(test_menu_key(&before), test_menu_key(&after));
	}

	#[test]
	fn menu_key_tracks_all_fallback_wardrobe_action_labels() {
		let mut before = snapshot();
		before.wardrobe_actions = vec![
			gpu::RuntimeWardrobeActionStatus {
				action_id: "action:base".to_string(),
				label: "Base".to_string(),
				set_id: "".to_string(),
				..Default::default()
			},
			gpu::RuntimeWardrobeActionStatus {
				action_id: "action:noble13".to_string(),
				label: "Noble 13".to_string(),
				set_id: "noble13".to_string(),
				..Default::default()
			},
		];

		let mut after = before.clone();
		after.wardrobe_actions[1].label = "Noble13".to_string();

		assert_ne!(test_menu_key(&before), test_menu_key(&after));
	}

	#[test]
	fn menu_key_tracks_wardrobe_shortcut_changes() {
		let snapshot = snapshot();
		let mut before = AvatarWindowOptions::default();
		before.wardrobe_bindings = vec![crate::WardrobeBindingOptions {
			set_id: "field_drape".to_string(),
			kind: crate::WardrobeBindingKind::Keyboard,
			binding: "F12".to_string(),
			device: None,
			channel: None,
			note: None,
		}];
		let mut after = before.clone();
		after.wardrobe_bindings[0].binding = "Ctrl+Alt+1".to_string();

		assert_ne!(menu_key(&before, &snapshot), menu_key(&after, &snapshot));
	}

	#[test]
	fn wardrobe_label_includes_key_binding_when_configured() {
		let mut opts = AvatarWindowOptions::default();
		opts.wardrobe_bindings = vec![crate::WardrobeBindingOptions {
			set_id: "field_drape".to_string(),
			kind: crate::WardrobeBindingKind::Keyboard,
			binding: "F12".to_string(),
			device: None,
			channel: None,
			note: None,
		}];

		assert_eq!(
			wardrobe_label_with_shortcut("Field Drape".to_string(), &opts, "field_drape"),
			"Field Drape (F12)"
		);
		assert_eq!(wardrobe_label_with_shortcut("Base".to_string(), &opts, ""), "Base");
	}

	#[test]
	fn wardrobe_label_prefers_menu_path_then_label_then_set_id() {
		let from_path = gpu::RuntimeMenuWardrobeCandidateStatus {
			menu_path: vec!["Clothes".to_string(), "Field Drape".to_string()],
			menu_label: Some("Ignored label".to_string()),
			wardrobe_set_id: "field_drape".to_string(),
			..Default::default()
		};
		assert_eq!(menu_wardrobe_label(&from_path), "Clothes / Field Drape");

		let from_label = gpu::RuntimeMenuWardrobeCandidateStatus {
			menu_label: Some("Noble 13".to_string()),
			wardrobe_set_id: "noble13".to_string(),
			..Default::default()
		};
		assert_eq!(menu_wardrobe_label(&from_label), "Noble 13");

		let from_set_id = gpu::RuntimeMenuWardrobeCandidateStatus {
			wardrobe_set_id: "base".to_string(),
			..Default::default()
		};
		assert_eq!(menu_wardrobe_label(&from_set_id), "base");
	}

	#[test]
	fn synthetic_base_entry_is_active_when_active_set_matches_resolved_base_set() {
		assert!(wardrobe_set_active("base", "base", ""));
		assert!(wardrobe_set_active("", "base", ""));
		assert!(wardrobe_set_active("field_drape", "base", "field_drape"));
		assert!(!wardrobe_set_active("field_drape", "base", ""));
	}

	#[test]
	fn wardrobe_menu_exposes_all_candidates_without_fixed_cap() {
		let opts = AvatarWindowOptions::default();
		let mut status = snapshot();
		status.menu_wardrobe_candidates = (0..32)
			.map(|index| gpu::RuntimeMenuWardrobeCandidateStatus {
				action_id: format!("wardrobe:{index}"),
				wardrobe_set_id: format!("set_{index}"),
				menu_path: vec!["Wardrobe".to_string(), format!("Set {index}")],
				..Default::default()
			})
			.collect();

		let (_menu, actions) = build_menu(&opts, &status);
		let direct_wardrobe_actions = actions
			.values()
			.filter(|action| matches!(action, RendererTrayAction::SetWardrobe(_)))
			.count();
		let activate_actions = actions
			.values()
			.filter(|action| matches!(action, RendererTrayAction::ActivateAction(_)))
			.count();
		assert_eq!(direct_wardrobe_actions, 1);
		assert_eq!(activate_actions, 32);
		assert!(matches!(
			actions.get("renderer:wardrobe:0"),
			Some(RendererTrayAction::SetWardrobe(set_id)) if set_id.is_empty()
		));
		assert!(matches!(
			actions.get("renderer:wardrobe:1"),
			Some(RendererTrayAction::ActivateAction(action_id)) if action_id == "wardrobe:0"
		));
	}

	#[test]
	fn unanimator_excludes_wardrobe_set_candidates() {
		let opts = AvatarWindowOptions::default();
		let mut status = snapshot();
		status.menu_action_candidates = vec![
			gpu::RuntimeMenuActionCandidateStatus {
				action_id: "action:smile".to_string(),
				action_label: "Smile".to_string(),
				menu_key: "expressions/smile".to_string(),
				menu_label: Some("Expressions".to_string()),
				parameter_name: "Smile".to_string(),
				parameter_value: 1.0,
				available: true,
				effect_count: 1,
				match_kind: "condition".to_string(),
				..Default::default()
			},
			gpu::RuntimeMenuActionCandidateStatus {
				action_id: "action:field_drape".to_string(),
				action_label: "Field Drape".to_string(),
				menu_key: "wardrobe/field".to_string(),
				menu_label: Some("Field Drape".to_string()),
				parameter_name: "Wardrobe".to_string(),
				parameter_value: 2.0,
				wardrobe_set_ids: vec!["field_drape".to_string()],
				available: true,
				..Default::default()
			},
		];

		let (_menu, actions) = build_menu(&opts, &status);

		assert!(matches!(
			actions.get("renderer:unanimator:0"),
			Some(RendererTrayAction::SetParameter { name, value }) if name == "Smile" && (*value - 1.0).abs() < f32::EPSILON
		));
		assert!(!actions.contains_key("renderer:unanimator:1"));
	}

	#[test]
	fn unanimator_excludes_unavailable_menu_candidates_without_fallback() {
		let opts = AvatarWindowOptions::default();
		let mut status = snapshot();
		status.menu_action_candidates = vec![gpu::RuntimeMenuActionCandidateStatus {
			action_id: "action:field_drape_hat_off".to_string(),
			action_label: "Hat OFF".to_string(),
			menu_key: "field_drape/hat_off".to_string(),
			menu_label: Some("Hat OFF".to_string()),
			parameter_name: "HatOff".to_string(),
			parameter_value: 1.0,
			available: false,
			..Default::default()
		}];
		status.runtime_actions = vec![gpu::RuntimeActionStatus {
			action_id: "action:field_drape_hat_off".to_string(),
			label: "Hat OFF".to_string(),
			expression_menu_path: Some("Field Drape/Hat OFF".to_string()),
			parameter_name: Some("HatOff".to_string()),
			parameter_value: Some(1.0),
			..Default::default()
		}];

		let (_menu, actions) = build_menu(&opts, &status);

		assert!(!actions.contains_key("renderer:unanimator:0"));
	}

	#[test]
	fn unanimator_falls_back_to_expression_menu_runtime_actions() {
		let opts = AvatarWindowOptions::default();
		let mut status = snapshot();
		status.runtime_actions = vec![gpu::RuntimeActionStatus {
			action_id: "action:hat".to_string(),
			label: "Hat".to_string(),
			effect_count: 1,
			expression_menu_path: Some("Wardrobe/Hat".to_string()),
			available: true,
			..Default::default()
		}];

		let (_menu, actions) = build_menu(&opts, &status);

		assert!(matches!(
			actions.get("renderer:unanimator:0"),
			Some(RendererTrayAction::ActivateAction(action_id)) if action_id == "action:hat"
		));
	}

	#[test]
	fn unanimator_profile_animator_action_shows_binding_and_active_check() {
		let mut opts = AvatarWindowOptions::default();
		opts.animator_bindings = vec![crate::AnimatorActionBindingOptions {
			action_id: "expression:angry".to_string(),
			kind: crate::WardrobeBindingKind::Keyboard,
			binding: "F8".to_string(),
			device: None,
			channel: None,
			note: None,
		}];
		let mut status = snapshot();
		status.active_profile_animator_actions = vec!["expression:angry".to_string()];
		status.runtime_actions = vec![gpu::RuntimeActionStatus {
			action_id: "expression:angry".to_string(),
			label: "Expression / Angry".to_string(),
			effect_count: 1,
			expression_menu_path: Some("Expressions/Angry".to_string()),
			available: true,
			..Default::default()
		}];

		let (_menu, actions) = build_menu(&opts, &status);

		assert!(matches!(
			actions.get("renderer:unanimator:0"),
			Some(RendererTrayAction::ActivateAction(action_id)) if action_id == "expression:angry"
		));
		assert!(menu_key(&opts, &status).contains("expression:angry"));
	}

	#[test]
	fn unanimator_fallback_excludes_wardrobe_set_actions() {
		let opts = AvatarWindowOptions::default();
		let mut status = snapshot();
		status.runtime_actions = vec![
			gpu::RuntimeActionStatus {
				action_id: "action:smile".to_string(),
				label: "Smile".to_string(),
				effect_count: 1,
				expression_menu_path: Some("Expressions/Smile".to_string()),
				available: true,
				..Default::default()
			},
			gpu::RuntimeActionStatus {
				action_id: "action:field_drape".to_string(),
				label: "Field Drape".to_string(),
				effect_count: 1,
				expression_menu_path: Some("Wardrobe/Field Drape".to_string()),
				wardrobe_set_id: Some("field_drape".to_string()),
				available: true,
				..Default::default()
			},
		];

		let (_menu, actions) = build_menu(&opts, &status);

		assert!(matches!(
			actions.get("renderer:unanimator:0"),
			Some(RendererTrayAction::ActivateAction(action_id)) if action_id == "action:smile"
		));
		assert!(!actions.contains_key("renderer:unanimator:1"));
	}

	#[test]
	fn unanimator_parameter_action_toggles_runtime_parameter() {
		let opts = AvatarWindowOptions::default();
		let mut status = snapshot();
		status.runtime_actions = vec![gpu::RuntimeActionStatus {
			action_id: "action:hat_off".to_string(),
			label: "Hat OFF".to_string(),
			effect_count: 1,
			expression_menu_path: Some("Wardrobe/Hat OFF".to_string()),
			parameter_name: Some("HatOff".to_string()),
			parameter_value: Some(1.0),
			current_condition_state: Some("inactive".to_string()),
			available: true,
			..Default::default()
		}];

		let (_menu, actions) = build_menu(&opts, &status);
		assert!(matches!(
			actions.get("renderer:unanimator:0"),
			Some(RendererTrayAction::SetParameter { name, value }) if name == "HatOff" && (*value - 1.0).abs() < f32::EPSILON
		));

		status.runtime_actions[0].current_condition_state = Some("active".to_string());
		let (_menu, actions) = build_menu(&opts, &status);
		assert!(matches!(
			actions.get("renderer:unanimator:0"),
			Some(RendererTrayAction::SetParameter { name, value }) if name == "HatOff" && value.abs() < f32::EPSILON
		));
	}

	#[test]
	fn unanimator_metadata_on_off_pair_is_single_toggle_action() {
		let opts = AvatarWindowOptions::default();
		let mut status = snapshot();
		status.menu_action_candidates = vec![
			gpu::RuntimeMenuActionCandidateStatus {
				action_id: "action:hat_on".to_string(),
				action_label: "Hat ON".to_string(),
				menu_key: "hat/on".to_string(),
				menu_path: vec!["Object".to_string(), "Hat ON".to_string()],
				control_type: Some("Toggle".to_string()),
				parameter_name: "Hat".to_string(),
				parameter_value: 1.0,
				match_kind: "metadata".to_string(),
				available: true,
				effect_count: 1,
				..Default::default()
			},
			gpu::RuntimeMenuActionCandidateStatus {
				action_id: "action:hat_off".to_string(),
				action_label: "Hat OFF".to_string(),
				menu_key: "hat/off".to_string(),
				menu_path: vec!["Object".to_string(), "Hat OFF".to_string()],
				control_type: Some("Toggle".to_string()),
				parameter_name: "Hat".to_string(),
				parameter_value: 0.0,
				match_kind: "metadata".to_string(),
				available: true,
				effect_count: 1,
				..Default::default()
			},
		];

		let (_menu, actions) = build_menu(&opts, &status);

		assert!(matches!(
			actions.get("renderer:unanimator:0"),
			Some(RendererTrayAction::SetParameter { name, value }) if name == "Hat" && value.abs() < f32::EPSILON
		));
		assert!(!actions.contains_key("renderer:unanimator:1"));
		assert_eq!(menu_action_label(&status.menu_action_candidates[1]), "Object / Hat");

		status.runtime_parameter_values.insert("Hat".to_string(), 0.0);
		let (_menu, actions) = build_menu(&opts, &status);
		assert!(matches!(
			actions.get("renderer:unanimator:0"),
			Some(RendererTrayAction::SetParameter { name, value }) if name == "Hat" && (*value - 1.0).abs() < f32::EPSILON
		));
	}

	#[test]
	fn unanimator_label_prefers_resolved_menu_path() {
		let candidate = gpu::RuntimeMenuActionCandidateStatus {
			action_id: "action:smile".to_string(),
			action_label: "Smile Action".to_string(),
			menu_key: "expressions/smile".to_string(),
			menu_path: vec!["Expressions".to_string(), "Smile".to_string()],
			menu_label: Some("Smile Menu".to_string()),
			parameter_name: "Smile".to_string(),
			parameter_value: 1.0,
			available: true,
			..Default::default()
		};

		assert_eq!(menu_action_label(&candidate), "Expressions / Smile");
	}

	#[test]
	fn tray_menu_exposes_core_runtime_actions() {
		let opts = AvatarWindowOptions::default();
		let mut status = snapshot();
		status.spout_available = true;
		status.dynamics_group_count = 4;

		let (_menu, actions) = build_menu(&opts, &status);

		assert!(matches!(
			actions.get("renderer:preview:show"),
			Some(RendererTrayAction::ActivatePreview)
		));
		assert!(!actions.contains_key("renderer:preview:hide"));
		assert!(matches!(
			actions.get("renderer:output:window"),
			Some(RendererTrayAction::SetWindowPreview)
		));
		assert!(matches!(
			actions.get("renderer:output:spout_preview"),
			Some(RendererTrayAction::SetSpoutPreview)
		));
		assert!(matches!(
			actions.get("renderer:output:spout_only"),
			Some(RendererTrayAction::SetSpoutOnly)
		));
		assert!(matches!(
			actions.get("renderer:output:spout_720p"),
			Some(RendererTrayAction::SetSpoutResolution { width: 1280, height: 720 })
		));
		assert!(matches!(
			actions.get("renderer:window:always_on_top"),
			Some(RendererTrayAction::SetAlwaysOnTop(true))
		));
		assert!(matches!(
			actions.get("renderer:dynamics:toggle"),
			Some(RendererTrayAction::SetCurrentWardrobeDynamics(true))
		));
		assert!(matches!(
			actions.get("renderer:camera:reset"),
			Some(RendererTrayAction::ResetCamera)
		));
		assert!(matches!(
			actions.get("renderer:supervisor:open"),
			Some(RendererTrayAction::OpenSupervisor)
		));
		assert!(matches!(actions.get("renderer:quit"), Some(RendererTrayAction::Quit)));
	}

	#[test]
	fn tray_tooltip_identifies_output_mode() {
		let text = TrayText::en();
		let mut status = snapshot();
		status.spout_enabled = false;
		status.minimized = false;
		assert_eq!(tray_output_mode_label(&status, &text), "Window Preview");

		status.minimized = true;
		assert_eq!(tray_output_mode_label(&status, &text), "Window Hidden");

		status.spout_enabled = true;
		status.minimized = false;
		assert_eq!(tray_output_mode_label(&status, &text), "Spout2 + Preview");

		status.minimized = true;
		assert_eq!(tray_output_mode_label(&status, &text), "Spout2 Only (minimized)");
	}

	#[test]
	fn spout_only_keeps_resolution_as_a_separate_action() {
		let opts = AvatarWindowOptions::default();
		let mut status = snapshot();
		status.spout_available = true;
		status.spout_width = Some(1920);
		status.spout_height = Some(1080);

		let (_menu, actions) = build_menu(&opts, &status);

		assert!(matches!(
			actions.get("renderer:output:spout_only"),
			Some(RendererTrayAction::SetSpoutOnly)
		));
		assert!(matches!(
			actions.get("renderer:output:spout_1080p"),
			Some(RendererTrayAction::SetSpoutResolution { width: 1920, height: 1080 })
		));
	}

	#[test]
	fn spout_resolution_label_does_not_imply_preview_sync() {
		let text = TrayText::en();
		let mut status = snapshot();
		status.spout_width = None;
		status.spout_height = None;
		assert_eq!(spout_resolution_label(&status, &text), "Spout2 output: renderer default");

		status.spout_width = Some(1280);
		status.spout_height = Some(720);
		assert_eq!(spout_resolution_label(&status, &text), "Spout2 output: 1280 x 720");
	}

	#[test]
	fn tray_text_localizes_primary_runtime_labels() {
		let text = TrayText::ja();
		assert_eq!(text.show_focus_preview(), "プレビューを表示 / 前面へ");
		assert_eq!(text.output(), "出力: Spout2");
		assert_eq!(text.camera(), "カメラ");
		assert_eq!(text.wardrobe(), "ワードローブ");
		assert_eq!(text.unanimator(), "UNAnimator");
		assert_eq!(text.quit_renderer(), "この Renderer を終了");
	}

	#[test]
	fn tray_icon_id_is_renderer_process_scoped() {
		let id = tray_icon_id();
		assert!(id.starts_with(TRAY_ICON_ID_PREFIX));
		assert!(id.ends_with(&std::process::id().to_string()));
	}
}
