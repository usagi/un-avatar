use std::{
	collections::HashMap,
	env,
	path::{Path, PathBuf},
	process::Command,
};

use tray_icon::{
	menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
	Icon as TrayIconImage, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use winit::event_loop::EventLoopProxy;

use crate::{gpu, AvatarWindowOptions, RendererControlEvent, RendererRuntimeSnapshot};

const TRAY_ICON_ID: &str = "un-avatar-renderer-tray";

#[derive(Clone, Debug)]
pub(crate) enum RendererTrayAction {
	ActivatePreview,
	MinimizePreview,
	SetWindowPreview,
	SetSpoutPreview,
	SetSpoutOnly,
	SetSpoutResolution { width: u32, height: u32 },
	SetAlwaysOnTop(bool),
	SetInputPassthrough(bool),
	SetAllDynamics(bool),
	ActivateAction(String),
	OpenSupervisor,
	ResetCamera,
	Quit,
}

pub(crate) struct RendererTray {
	icon: TrayIcon,
	actions: HashMap<String, RendererTrayAction>,
	last_menu_key: String,
}

impl RendererTray {
	pub(crate) fn new(opts: &AvatarWindowOptions, snapshot: &RendererRuntimeSnapshot) -> Result<Self, String> {
		let (menu, actions) = build_menu(opts, snapshot);
		let icon = TrayIconBuilder::new()
			.with_id(TRAY_ICON_ID)
			.with_tooltip(tray_tooltip(opts, snapshot))
			.with_icon(load_tray_icon(opts.icon_path.as_deref()).unwrap_or_else(default_tray_icon))
			.with_menu(Box::new(menu))
			.build()
			.map_err(|error| format!("build renderer tray: {error}"))?;
		Ok(Self {
			icon,
			actions,
			last_menu_key: menu_key(snapshot),
		})
	}

	pub(crate) fn refresh(&mut self, opts: &AvatarWindowOptions, snapshot: &RendererRuntimeSnapshot) {
		let key = menu_key(snapshot);
		let _ = self.icon.set_tooltip(Some(tray_tooltip(opts, snapshot)));
		if key == self.last_menu_key {
			return;
		}
		let (menu, actions) = build_menu(opts, snapshot);
		self.icon.set_menu(Some(Box::new(menu)));
		self.actions = actions;
		self.last_menu_key = key;
	}

	pub(crate) fn action(&self, id: &str) -> Option<RendererTrayAction> {
		self.actions.get(id).cloned()
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

pub(crate) fn open_supervisor() -> Result<(), String> {
	let exe = resolve_supervisor_exe().ok_or_else(|| "un-avatar-supervisor executable was not found".to_string())?;
	let mut command = Command::new(&exe);
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
	let menu = Menu::new();
	let mut actions = HashMap::new();

	append_header(&menu, opts, snapshot);
	append_separator(&menu);
	append_menu_item(
		&menu,
		&mut actions,
		"preview:show",
		"Show / Focus Preview",
		true,
		RendererTrayAction::ActivatePreview,
	);
	append_menu_item(
		&menu,
		&mut actions,
		"preview:hide",
		"Hide Preview",
		true,
		RendererTrayAction::MinimizePreview,
	);

	let output = Submenu::with_id("renderer:output", "Output", true);
	append_menu_item(
		&output,
		&mut actions,
		"output:window",
		check_label("Window Preview", !snapshot.spout_enabled && !snapshot.minimized),
		true,
		RendererTrayAction::SetWindowPreview,
	);
	append_menu_item(
		&output,
		&mut actions,
		"output:spout_preview",
		check_label("Spout2 + Preview", snapshot.spout_enabled && !snapshot.minimized),
		snapshot.spout_available,
		RendererTrayAction::SetSpoutPreview,
	);
	append_menu_item(
		&output,
		&mut actions,
		"output:spout_only",
		check_label("Spout2 Only", snapshot.spout_enabled && snapshot.minimized),
		snapshot.spout_available,
		RendererTrayAction::SetSpoutOnly,
	);
	append_separator(&output);
	append_disabled(&output, spout_resolution_label(snapshot));
	append_menu_item(
		&output,
		&mut actions,
		"output:spout_720p",
		check_label(
			"Spout2 1280 x 720",
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
			"Spout2 1920 x 1080",
			snapshot.spout_width == Some(1920) && snapshot.spout_height == Some(1080),
		),
		snapshot.spout_available,
		RendererTrayAction::SetSpoutResolution { width: 1920, height: 1080 },
	);
	append_submenu(&menu, &output);

	let window = Submenu::with_id("renderer:window", "Window", true);
	append_menu_item(
		&window,
		&mut actions,
		"window:always_on_top",
		check_label("Always on Top", snapshot.always_on_top),
		true,
		RendererTrayAction::SetAlwaysOnTop(!snapshot.always_on_top),
	);
	append_menu_item(
		&window,
		&mut actions,
		"window:input_passthrough",
		check_label("Input Passthrough", snapshot.input_passthrough),
		snapshot.transparent_window,
		RendererTrayAction::SetInputPassthrough(!snapshot.input_passthrough),
	);
	append_submenu(&menu, &window);

	if snapshot.dynamics_group_count > 0 {
		let dynamics = Submenu::with_id("renderer:dynamics", "UNPhysics", true);
		let summary = format!(
			"{} / {} effective groups",
			snapshot.dynamics_enabled_group_count, snapshot.dynamics_group_count
		);
		append_disabled(&dynamics, summary);
		append_separator(&dynamics);
		append_menu_item(
			&dynamics,
			&mut actions,
			"dynamics:on",
			"Enable All",
			true,
			RendererTrayAction::SetAllDynamics(true),
		);
		append_menu_item(
			&dynamics,
			&mut actions,
			"dynamics:off",
			"Disable All",
			true,
			RendererTrayAction::SetAllDynamics(false),
		);
		append_submenu(&menu, &dynamics);
	}

	append_wardrobe_menu(&menu, &mut actions, snapshot);

	append_separator(&menu);
	append_menu_item(
		&menu,
		&mut actions,
		"camera:reset",
		"Reset Camera",
		true,
		RendererTrayAction::ResetCamera,
	);
	append_menu_item(
		&menu,
		&mut actions,
		"supervisor:open",
		"Open Supervisor",
		true,
		RendererTrayAction::OpenSupervisor,
	);
	append_separator(&menu);
	append_menu_item(&menu, &mut actions, "quit", "Quit this Renderer", true, RendererTrayAction::Quit);

	(menu, actions)
}

fn append_wardrobe_menu(menu: &Menu, actions: &mut HashMap<String, RendererTrayAction>, snapshot: &RendererRuntimeSnapshot) {
	let mut entries: Vec<(String, String, Option<String>)> = Vec::new();
	for candidate in &snapshot.menu_wardrobe_candidates {
		entries.push((
			candidate.action_id.clone(),
			menu_wardrobe_label(candidate),
			Some(candidate.wardrobe_set_id.clone()),
		));
	}
	if entries.is_empty() {
		for action in &snapshot.wardrobe_actions {
			entries.push((action.action_id.clone(), action.label.clone(), Some(action.set_id.clone())));
		}
	}
	if entries.is_empty() {
		return;
	}
	let wardrobe = Submenu::with_id("renderer:wardrobe", "Wardrobe / VRC Menu", true);
	for (index, (action_id, label, set_id)) in entries.into_iter().enumerate() {
		let active = set_id.as_deref() == snapshot.active_wardrobe_set.as_deref();
		append_menu_item(
			&wardrobe,
			actions,
			format!("wardrobe:{index}"),
			check_label(truncate_label(&label, 56), active),
			true,
			RendererTrayAction::ActivateAction(action_id),
		);
	}
	append_submenu(menu, &wardrobe);
}

fn append_header(menu: &Menu, opts: &AvatarWindowOptions, snapshot: &RendererRuntimeSnapshot) {
	append_disabled(menu, format!("{}  pid {}", truncate_label(&opts.title, 48), std::process::id()));
	let state = if snapshot.scene_state.is_empty() {
		"starting".to_string()
	} else {
		snapshot.scene_state.clone()
	};
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

fn spout_resolution_label(snapshot: &RendererRuntimeSnapshot) -> String {
	match (snapshot.spout_width, snapshot.spout_height) {
		(Some(width), Some(height)) => format!("Spout2 output: {width} x {height}"),
		_ => "Spout2 output: follows preview size".to_string(),
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
	format!(
		"UN Avatar Renderer - {} - pid {} - {}",
		opts.title,
		std::process::id(),
		if snapshot.spout_enabled { "Spout2" } else { "Window" }
	)
}

fn menu_key(snapshot: &RendererRuntimeSnapshot) -> String {
	format!(
		"{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
		snapshot.scene_state,
		snapshot.spout_available,
		snapshot.spout_enabled,
		snapshot.minimized,
		snapshot.always_on_top,
		snapshot.input_passthrough,
		snapshot.spout_width.map_or(0, |value| value),
		snapshot.spout_height.map_or(0, |value| value),
		snapshot.dynamics_group_count,
		snapshot.dynamics_enabled_group_count,
		snapshot.active_wardrobe_set.as_deref().unwrap_or(""),
		wardrobe_menu_signature(snapshot)
	)
}

fn wardrobe_menu_signature(snapshot: &RendererRuntimeSnapshot) -> String {
	if !snapshot.menu_wardrobe_candidates.is_empty() {
		let first = snapshot.menu_wardrobe_candidates.first();
		let last = snapshot.menu_wardrobe_candidates.last();
		return format!(
			"menu:{}:{}:{}:{}:{}",
			snapshot.menu_wardrobe_candidates.len(),
			first.map_or("", |candidate| candidate.action_id.as_str()),
			first.map_or("", |candidate| candidate.wardrobe_set_id.as_str()),
			last.map_or("", |candidate| candidate.action_id.as_str()),
			last.map_or("", |candidate| candidate.wardrobe_set_id.as_str())
		);
	}
	let first = snapshot.wardrobe_actions.first();
	let last = snapshot.wardrobe_actions.last();
	format!(
		"actions:{}:{}:{}:{}:{}",
		snapshot.wardrobe_actions.len(),
		first.map_or("", |action| action.action_id.as_str()),
		first.map_or("", |action| action.set_id.as_str()),
		last.map_or("", |action| action.action_id.as_str()),
		last.map_or("", |action| action.set_id.as_str())
	)
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

		assert_ne!(menu_key(&before), menu_key(&after));
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

		assert_ne!(menu_key(&actions_only), menu_key(&menu_candidates));
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
		let wardrobe_actions = actions
			.values()
			.filter(|action| matches!(action, RendererTrayAction::ActivateAction(_)))
			.count();
		assert_eq!(wardrobe_actions, 32);
	}

	#[test]
	fn tray_tooltip_identifies_spout_or_window_mode() {
		let mut opts = AvatarWindowOptions::default();
		opts.title = "mizuki-split".to_string();
		let mut status = snapshot();
		status.spout_enabled = false;
		assert!(tray_tooltip(&opts, &status).contains("Window"));

		status.spout_enabled = true;
		let tooltip = tray_tooltip(&opts, &status);
		assert!(tooltip.contains("mizuki-split"));
		assert!(tooltip.contains("Spout2"));
	}
}
