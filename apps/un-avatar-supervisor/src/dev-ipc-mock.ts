import { mockIPC } from "@tauri-apps/api/mocks";
import enLocaleToml from "../src-tauri/locales/en-US.toml?raw";
import jaLocaleToml from "../src-tauri/locales/ja-JP.toml?raw";
import { BONE_COLLIDER_RADIUS_FIELD_PREFIX } from "./lib/boneColliderFields";
import { hasTauriRuntime } from "./lib/environment";
import { defaultTextureCompressionAdvanced } from "./lib/qualityOptions";
import { DYNAMICS_BONE_COLLIDER_FIELD_PREFIX, defaultDynamicsCategoryOverrides } from "./lib/dynamicsPresets";

type LocaleBundle = {
	locale: string;
	messages: Record<string, string>;
};

const localeBundles: Record<string, LocaleBundle> = {
	"ja-JP": { locale: "ja-JP", messages: parseFlatTomlMessages(jaLocaleToml) },
	"en-US": { locale: "en-US", messages: parseFlatTomlMessages(enLocaleToml) },
};

const mockUnavatarPreviewDataUrl =
	"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII=";

let appSettings = {
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
	pinned_taskbar_profile_ids: [],
	console_window_x: null,
	console_window_y: null,
	console_window_width: null,
	console_window_height: null,
	locale: "ja-JP",
};

let avatarSettings = [
	{
		id: "main",
		name: "Main Avatar",
		created_at: "20260517T000000Z",
		sort_order: 1000,
		storage: "user",
		manifest_path: "profiles/main.toml",
		avatar_path: "assets/example/main.vrm",
		wardrobe_set: null,
		wardrobe_billboard_anchor: "neck",
		wardrobe_billboard_y_offset_mm: 0,
		wardrobe_shortcuts: [],
		wardrobe_bindings: [],
		animator_actions: [],
		animator_bindings: [],
		vmc_address: "0.0.0.0:39539",
		vmc_port: 39539,
		motion_vmc_enabled: true,
		motion_unmotion_enabled: true,
		unmotion_zenoh_key: "un-motion/frame",
		unmotion_zenoh_connect: null,
		audio_link_source: "none",
		audio_link_input_device_id: null,
		audio_link_input_device_name_hint: null,
		primary_motion_source: "unmotion_zenoh",
		dynamics_enabled: true,
		contact_parameter_emission: false,
		dynamics_physics_configured: false,
		dynamics_simulation_hz: 60,
		dynamics_substeps: 1,
		dynamics_category_overrides: defaultDynamicsCategoryOverrides(),
		dynamics_match_overrides: [],
		dynamics_collider_augment_overrides: [],
		dynamics_group_overrides: [],
		dynamics_mesh_cloth_assist: null,
		apply_vmc_root_translation: false,
		camera_target: null,
		camera_longitude_deg: null,
		camera_latitude_deg: null,
		camera_radius: null,
		camera_diagonal_fov_deg: null,
		spout_enabled: false,
		spout_name: "UN Avatar Spout",
		spout_width: 1280,
		spout_height: 720,
		aa: "fxaa",
		texture_resolution_limit: "auto",
		texture_compression: "balanced",
		mipmap_filter: "mitchell",
		render_backend: "dx12",
		block_compression_encoder: "gpu",
		block_compression_cpu_threads: 4,
		texture_compression_advanced: defaultTextureCompressionAdvanced(),
		processed_texture_cache: true,
		transparent: true,
		input_passthrough: false,
		decorations: false,
		always_on_top: true,
		minimized: false,
		show_axes: false,
		show_bone_colliders: false,
		bone_colliders_enabled: false,
		bone_collider_head: 120,
		bone_collider_neck_chest: 80,
		bone_collider_torso: 140,
		bone_collider_upper_arms: 55,
		bone_collider_lower_arms: 45,
		bone_collider_hands: 50,
		debug_disable_mtoon_outlines: false,
		debug_disable_rim_lighting: false,
		debug_force_shading_shift_zero: false,
		debug_disable_matcap: false,
		debug_disable_emissive: false,
		debug_disable_shade_color: false,
		debug_disable_normal_map: false,
		debug_base_texture_only: false,
		outline_policy: "off",
		outline_type: "silhouette",
		outline_width: null,
		outline_color: null,
		outline_lighting_mix: null,
		outline_roundness: null,
		lighting_environment_enabled: true,
		lighting_environment_color: [1, 1, 1],
		lighting_environment_intensity: 0.35,
		lighting_directional_enabled: true,
		lighting_directional_color: [1, 1, 1],
		lighting_directional_intensity: 1,
		lighting_directional_azimuth_deg: 0,
		lighting_directional_elevation_deg: 33.84,
		lighting_directional_follow_camera_yaw: true,
		lighting_directional_follow_camera_pitch: false,
		color_exposure: 0,
		color_contrast: 1,
		color_saturation: 1,
		color_look: "neutral",
		color_look_intensity: 0,
		color_temperature: 0,
		color_tint: 0,
		bloom_enabled: false,
		bloom_strength: 0.35,
		bloom_threshold: 0.65,
		bloom_radius: 8,
		bloom_quality: "compact",
		ssao_enabled: false,
		ssao_strength: 0.25,
		ssao_radius: 4,
		ssao_bias: 0.0015,
		ssao_range: 0.03,
		contact_shadow_enabled: false,
		contact_shadow_strength: 0.35,
		contact_shadow_radius: 0.55,
		contact_shadow_softness: 1.8,
		contact_shadow_height: 0,
		camera_locked: false,
		window_width: 960,
		window_height: 720,
		window_x: null,
		window_y: null,
		icon_path: null,
		allow_multiple_renderers: false,
		gpu_adapter: "auto",
		notes: "Primary streaming avatar",
		group: "Main",
	},
	{
		id: "debug",
		name: "Debug Avatar",
		created_at: "20260517T000001Z",
		sort_order: 2000,
		storage: "user",
		manifest_path: "profiles/debug.toml",
		avatar_path: "assets/example/debug.vrm",
		wardrobe_set: null,
		wardrobe_billboard_anchor: "neck",
		wardrobe_billboard_y_offset_mm: 0,
		wardrobe_shortcuts: [],
		wardrobe_bindings: [],
		animator_actions: [],
		animator_bindings: [],
		vmc_address: "0.0.0.0:39540",
		vmc_port: 39540,
		motion_vmc_enabled: true,
		motion_unmotion_enabled: false,
		unmotion_zenoh_key: null,
		unmotion_zenoh_connect: null,
		audio_link_source: "none",
		audio_link_input_device_id: null,
		audio_link_input_device_name_hint: null,
		primary_motion_source: "vmc",
		dynamics_enabled: true,
		contact_parameter_emission: false,
		dynamics_physics_configured: false,
		dynamics_simulation_hz: 60,
		dynamics_substeps: 1,
		dynamics_category_overrides: defaultDynamicsCategoryOverrides(),
		dynamics_match_overrides: [],
		dynamics_collider_augment_overrides: [],
		dynamics_group_overrides: [],
		dynamics_mesh_cloth_assist: null,
		apply_vmc_root_translation: false,
		camera_target: null,
		camera_longitude_deg: null,
		camera_latitude_deg: null,
		camera_radius: null,
		camera_diagonal_fov_deg: null,
		spout_enabled: false,
		spout_name: null,
		spout_width: null,
		spout_height: null,
		aa: "off",
		texture_resolution_limit: "2k",
		texture_compression: "balanced",
		mipmap_filter: "mitchell",
		render_backend: "dx12",
		block_compression_encoder: "gpu",
		block_compression_cpu_threads: 4,
		texture_compression_advanced: defaultTextureCompressionAdvanced(),
		processed_texture_cache: false,
		transparent: false,
		input_passthrough: false,
		decorations: true,
		always_on_top: false,
		minimized: false,
		show_axes: true,
		show_bone_colliders: true,
		bone_colliders_enabled: false,
		bone_collider_head: 120,
		bone_collider_neck_chest: 80,
		bone_collider_torso: 140,
		bone_collider_upper_arms: 55,
		bone_collider_lower_arms: 45,
		bone_collider_hands: 50,
		debug_disable_mtoon_outlines: false,
		debug_disable_rim_lighting: false,
		debug_force_shading_shift_zero: false,
		debug_disable_matcap: false,
		debug_disable_emissive: false,
		debug_disable_shade_color: false,
		debug_disable_normal_map: false,
		debug_base_texture_only: false,
		outline_policy: "off",
		outline_type: "silhouette",
		outline_width: null,
		outline_color: null,
		outline_lighting_mix: null,
		outline_roundness: null,
		lighting_environment_enabled: true,
		lighting_environment_color: [1, 1, 1],
		lighting_environment_intensity: 0.35,
		lighting_directional_enabled: true,
		lighting_directional_color: [1, 1, 1],
		lighting_directional_intensity: 1,
		lighting_directional_azimuth_deg: 0,
		lighting_directional_elevation_deg: 33.84,
		lighting_directional_follow_camera_yaw: true,
		lighting_directional_follow_camera_pitch: false,
		color_exposure: 0,
		color_contrast: 1,
		color_saturation: 1,
		color_look: "neutral",
		color_look_intensity: 0,
		color_temperature: 0,
		color_tint: 0,
		bloom_enabled: false,
		bloom_strength: 0.35,
		bloom_threshold: 0.65,
		bloom_radius: 8,
		bloom_quality: "compact",
		ssao_enabled: false,
		ssao_strength: 0.25,
		ssao_radius: 4,
		ssao_bias: 0.0015,
		ssao_range: 0.03,
		contact_shadow_enabled: false,
		contact_shadow_strength: 0.35,
		contact_shadow_radius: 0.55,
		contact_shadow_softness: 1.8,
		contact_shadow_height: 0,
		camera_locked: false,
		window_width: 800,
		window_height: 600,
		window_x: null,
		window_y: null,
		icon_path: null,
		allow_multiple_renderers: false,
		gpu_adapter: "auto",
		notes: "Diagnostics profile",
		group: "Debug",
	},
];

export function installDevIpcMock(): void {
	if (hasTauriRuntime()) return;

	mockIPC((cmd, payload) => {
		const args = (payload ?? {}) as Record<string, unknown>;
		switch (cmd) {
			case "i18n_available_locales":
				return Object.keys(localeBundles);
			case "i18n_get_svelte_bundle": {
				const tag = String(args.locale ?? "ja-JP");
				return localeBundles[tag] ?? localeBundles["ja-JP"];
			}
			case "i18n_resolve_default_locale":
				return "ja-JP";
			case "get_app_settings":
				return appSettings;
			case "sync_app_settings":
				appSettings = {
					...appSettings,
					...((args.settings ?? {}) as typeof appSettings),
				};
				return appSettings;
			case "app_version":
				return "dev-preview";
			case "list_renderers":
			case "list_app_notifications":
			case "list_diagnostics_exports":
				return [];
			case "list_avatar_settings":
				return avatarSettings;
			case "list_gpu_adapters":
				return [
					{
						value: "gpu:10de:2684:NVIDIA GeForce RTX",
						label: "NVIDIA GeForce RTX (DiscreteGpu, vendor 10de, device 2684)",
						name: "NVIDIA GeForce RTX",
						device_type: "DiscreteGpu",
						vendor: 0x10de,
						device: 0x2684,
					},
					{
						value: "gpu:8086:a780:Intel UHD Graphics",
						label: "Intel UHD Graphics (IntegratedGpu, vendor 8086, device a780)",
						name: "Intel UHD Graphics",
						device_type: "IntegratedGpu",
						vendor: 0x8086,
						device: 0xa780,
					},
				];
			case "read_vrm_metadata":
				return {
					path: String(args.path ?? "assets/example/main.vrm"),
					file_name: "main.vrm",
					vrm_format: "VRM 1.0",
					spec_version: "1.0",
					title: "Main Avatar",
					version: "1.0.0",
					authors: ["UN Avatar Preview"],
					contact_information: "preview@example.invalid",
					references: ["https://example.invalid/avatar"],
					copyright_information: "Copyright 2026 Preview Author",
					third_party_licenses: "Sample metadata for Supervisor preview.",
					license_name: null,
					other_license_url: "https://example.invalid/license",
					other_permission_url: "https://example.invalid/permission",
					thumbnail_data_url:
						"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 512 640'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' x2='1' y1='0' y2='1'%3E%3Cstop stop-color='%23f48a62'/%3E%3Cstop offset='0.52' stop-color='%2361c7d7'/%3E%3Cstop offset='1' stop-color='%2320283a'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='512' height='640' fill='%23151a25'/%3E%3Ccircle cx='256' cy='214' r='116' fill='url(%23g)'/%3E%3Cpath d='M126 540c18-104 76-166 130-166s112 62 130 166' fill='url(%23g)'/%3E%3Cpath d='M197 202c18 24 39 36 63 36s43-12 55-36' fill='none' stroke='%23fff' stroke-width='18' stroke-linecap='round' opacity='.78'/%3E%3C/svg%3E",
					technical_stats: [
						{ label: "File size", value: "18.4 MB" },
						{ label: "Vertices", value: "68,240" },
						{ label: "Triangles", value: "112,816" },
						{ label: "Bones", value: "74" },
						{ label: "Textures", value: "23 · max 4096x4096" },
						{ label: "Texture RAM", value: "512.0 MB RGBA" },
						{ label: "Morph targets", value: "128" },
						{ label: "Expressions", value: "64" },
						{ label: "PerfectSync", value: "supported (52/52)" },
					],
					permissions: [
						{ label: "Allowed user", value: "OnlyAuthor" },
						{ label: "Commercial usage", value: "PersonalNonProfit" },
						{ label: "Credit notation", value: "Required" },
						{ label: "Redistribution", value: "false" },
					],
				};
			case "read_unavatar_metadata": {
				const mockPreview = (view: string) => ({
					view,
					width: 1024,
					height: 1024,
					data_url: mockUnavatarPreviewDataUrl,
				});
				return {
					path: String(args.path ?? "assets/example/main.unavatar"),
					file_name: "main.unavatar",
					name: "Main UNAvatar",
					spec_version: "0.1-preview",
					generator: "UN Avatar Preview",
					source_type: "VRChat Avatar",
					export_mode: "Split Wardrobe",
					created_utc: "2026-06-14T00:00:00Z",
					wardrobe_set_count: 3,
					dynamics_count: 12,
					contact_count: 2,
					modular_avatar_component_count: 18,
					redistribution_allowed: null,
					preview_images: [mockPreview("front"), mockPreview("side")],
					preview_sets: [
						{
							id: "base",
							name: "Base",
							preview_images: [mockPreview("front"), mockPreview("side")],
						},
						{
							id: "noble13",
							name: "Noble 13",
							preview_images: [mockPreview("front"), mockPreview("back")],
						},
						{
							id: "field_drape",
							name: "Field Drape",
							preview_images: [mockPreview("front"), mockPreview("detail")],
						},
					],
				};
			}
			case "save_profile_icon_from_data_url": {
				const iconSettingId = String(args.settingId ?? "");
				const iconSetting =
					avatarSettings.find((item) => item.id === iconSettingId || item.manifest_path === iconSettingId) ?? avatarSettings[0];
				(iconSetting as { icon_path: string | null }).icon_path =
					"C:/Users/the/AppData/Roaming/UN Avatar/profiles/assets/thumbnails/main-avatar-thumbnail.webp";
				return iconSetting;
			}
			case "read_unavatar_wardrobe_options":
				return {
					available: true,
					base_label: "Base",
					error: null,
					sets: [
						{ id: "original", name: "Original" },
						{ id: "noble1", name: "Noble 1" },
						{ id: "noble13", name: "Noble 13" },
					],
				};
			case "read_unavatar_animator_action_page": {
				const candidates = [
					"Expression / Joy",
					"Expression / Angry",
					"Expression / Fun",
					"Cloth / Cloth1",
					"Cloth / Cloth2",
					"Object / hat",
					"Object / shoes",
					"Object / outer",
				].map((label, index) => ({
					id: label.startsWith("Expression")
						? `expression:${label.split(" / ")[1]?.toLowerCase()}`
						: `animator:0:0:${label.split(" / ")[1]?.toLowerCase()}:${index}`,
					label,
					controller: label.startsWith("Expression") ? "VRM Expression" : "Preview FX",
					layer: label.split(" / ")[0] ?? "Action",
					state_path: label.split(" / ")[1] ?? label,
					effect_count: 1,
					condition_count: label.startsWith("Expression") ? 0 : 1,
					selected_mode: "off",
				}));
				return {
					available: true,
					total_count: 62,
					matched_count: candidates.length,
					selected_count: 0,
					offset: 0,
					limit: candidates.length,
					candidates,
					error: null,
				};
			}
			case "save_avatar_thumbnail_icon": {
				const id = String(args.settingId ?? "");
				const setting = avatarSettings.find((item) => item.id === id) ?? avatarSettings[0];
				(setting as { icon_path: string | null }).icon_path =
					"data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 512 640'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' x2='1' y1='0' y2='1'%3E%3Cstop stop-color='%23f48a62'/%3E%3Cstop offset='0.52' stop-color='%2361c7d7'/%3E%3Cstop offset='1' stop-color='%2320283a'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect width='512' height='640' fill='%23151a25'/%3E%3Ccircle cx='256' cy='214' r='116' fill='url(%23g)'/%3E%3Cpath d='M126 540c18-104 76-166 130-166s112 62 130 166' fill='url(%23g)'/%3E%3Cpath d='M197 202c18 24 39 36 63 36s43-12 55-36' fill='none' stroke='%23fff' stroke-width='18' stroke-linecap='round' opacity='.78'/%3E%3C/svg%3E";
				return setting;
			}
			case "update_avatar_setting_value": {
				const id = String(args.settingId ?? "");
				const field = String(args.field ?? "");
				const setting = avatarSettings.find((item) => item.id === id);
				if (setting && field === "avatar_path") {
					setting.avatar_path = String(args.value ?? "");
				}
				if (setting && field === "wardrobe_set") {
					(setting as { wardrobe_set: string | null }).wardrobe_set = String(args.value ?? "") || null;
				}
				if (setting && field === "wardrobe.transition.billboard_anchor") {
					(setting as { wardrobe_billboard_anchor: string }).wardrobe_billboard_anchor = String(args.value ?? "neck");
				}
				if (setting && field === "wardrobe.transition.billboard_y_offset_mm") {
					(setting as { wardrobe_billboard_y_offset_mm: number }).wardrobe_billboard_y_offset_mm = Number(args.value ?? 0);
				}
				if (setting && field === "motion.unmotion_zenoh.enabled") {
					setting.motion_unmotion_enabled = Boolean(args.value);
				}
				if (setting && field === "motion.unmotion_zenoh.key") {
					setting.unmotion_zenoh_key = String(args.value ?? "") || null;
				}
				if (setting && field === "motion.unmotion_zenoh.connect") {
					(setting as { unmotion_zenoh_connect: string | null }).unmotion_zenoh_connect = String(args.value ?? "") || null;
				}
				if (setting && field === "wardrobe.shortcuts") {
					(setting as { wardrobe_shortcuts: unknown[] }).wardrobe_shortcuts = (args.value as unknown[]) ?? [];
				}
				if (setting && field === "wardrobe.bindings") {
					(setting as { wardrobe_bindings: unknown[] }).wardrobe_bindings = (args.value as unknown[]) ?? [];
				}
				if (setting && field === "animator.actions") {
					(setting as unknown as { animator_actions: unknown[] }).animator_actions = (args.value as unknown[]) ?? [];
				}
				if (setting && field === "animator.bindings") {
					(setting as unknown as { animator_bindings: unknown[] }).animator_bindings = (args.value as unknown[]) ?? [];
				}
				if (setting && field === "environment.color.exposure") {
					setting.color_exposure = Number(args.value ?? 0);
				}
				if (setting && field === "environment.color.contrast") {
					setting.color_contrast = Number(args.value ?? 1);
				}
				if (setting && field === "environment.color.saturation") {
					setting.color_saturation = Number(args.value ?? 1);
				}
				if (setting && field === "environment.color.look") {
					setting.color_look = String(args.value ?? "neutral");
				}
				if (setting && field === "environment.color.intensity") {
					setting.color_look_intensity = Number(args.value ?? 0);
				}
				if (setting && field === "environment.color.temperature") {
					setting.color_temperature = Number(args.value ?? 0);
				}
				if (setting && field === "environment.color.tint") {
					setting.color_tint = Number(args.value ?? 0);
				}
				if (setting && field === "environment.lighting.environment.enabled") {
					setting.lighting_environment_enabled = Boolean(args.value);
				}
				if (setting && field === "environment.lighting.environment.color") {
					setting.lighting_environment_color = args.value as [number, number, number];
				}
				if (setting && field === "environment.lighting.environment.intensity") {
					setting.lighting_environment_intensity = Number(args.value ?? 0.35);
				}
				if (setting && field === "environment.lighting.directional.enabled") {
					setting.lighting_directional_enabled = Boolean(args.value);
				}
				if (setting && field === "environment.lighting.directional.color") {
					setting.lighting_directional_color = args.value as [number, number, number];
				}
				if (setting && field === "environment.lighting.directional.intensity") {
					setting.lighting_directional_intensity = Number(args.value ?? 1);
				}
				if (setting && field === "environment.lighting.directional.azimuth_deg") {
					setting.lighting_directional_azimuth_deg = Number(args.value ?? 0);
				}
				if (setting && field === "environment.lighting.directional.elevation_deg") {
					setting.lighting_directional_elevation_deg = Number(args.value ?? 33.84);
				}
				if (setting && field === "environment.lighting.directional.follow_camera_yaw") {
					setting.lighting_directional_follow_camera_yaw = Boolean(args.value);
				}
				if (setting && field === "environment.lighting.directional.follow_camera_pitch") {
					setting.lighting_directional_follow_camera_pitch = Boolean(args.value);
				}
				if (setting && field === "effects.post.bloom.enabled") {
					setting.bloom_enabled = Boolean(args.value);
				}
				if (setting && field === "effects.post.bloom.strength") {
					setting.bloom_strength = Number(args.value ?? 0.35);
				}
				if (setting && field === "effects.post.bloom.threshold") {
					setting.bloom_threshold = Number(args.value ?? 0.65);
				}
				if (setting && field === "effects.post.bloom.radius") {
					setting.bloom_radius = Number(args.value ?? 8);
				}
				if (setting && field === "effects.post.bloom.quality") {
					setting.bloom_quality = String(args.value ?? "compact");
				}
				if (setting && field === "effects.post.ssao.enabled") {
					setting.ssao_enabled = Boolean(args.value);
				}
				if (setting && field === "effects.post.ssao.strength") {
					setting.ssao_strength = Number(args.value ?? 0.25);
				}
				if (setting && field === "effects.post.ssao.radius") {
					setting.ssao_radius = Number(args.value ?? 4);
				}
				if (setting && field === "effects.post.ssao.bias") {
					setting.ssao_bias = Number(args.value ?? 0.0015);
				}
				if (setting && field === "effects.post.ssao.range") {
					setting.ssao_range = Number(args.value ?? 0.03);
				}
				if (setting && field === "effects.avatar.contact_shadow.enabled") {
					setting.contact_shadow_enabled = Boolean(args.value);
				}
				if (setting && field === "effects.avatar.contact_shadow.strength") {
					setting.contact_shadow_strength = Number(args.value ?? 0.35);
				}
				if (setting && field === "effects.avatar.contact_shadow.radius") {
					setting.contact_shadow_radius = Number(args.value ?? 0.55);
				}
				if (setting && field === "effects.avatar.contact_shadow.softness") {
					setting.contact_shadow_softness = Number(args.value ?? 1.8);
				}
				if (setting && field === "effects.avatar.contact_shadow.height") {
					setting.contact_shadow_height = Number(args.value ?? 0);
				}
				const directField = field
					.replace("debug.", "")
					.replace(BONE_COLLIDER_RADIUS_FIELD_PREFIX, "bone_collider_")
					.replace(DYNAMICS_BONE_COLLIDER_FIELD_PREFIX, "bone_colliders_")
					.replaceAll(".", "_");
				if (setting && directField in setting) {
					(setting as any)[directField] = args.value;
				}
				return setting ?? avatarSettings[0];
			}
			case "reorder_avatar_settings": {
				const ids = (args.settingIds as string[]) ?? [];
				const settingsById = new Map(avatarSettings.map((setting) => [setting.id, setting]));
				const seenIds = new Set<string>();
				const orderedSettings: typeof avatarSettings = [];
				for (const id of ids) {
					const setting = settingsById.get(id);
					if (!setting) continue;
					seenIds.add(id);
					orderedSettings.push(setting);
				}
				for (const setting of avatarSettings) {
					if (!seenIds.has(setting.id)) orderedSettings.push(setting);
				}
				avatarSettings = orderedSettings;
				return avatarSettings;
			}
			case "get_native_notification_status":
				return { permission_state: "unsupported" };
			case "delete_avatar_setting": {
				const id = String(args.settingId ?? "");
				avatarSettings = avatarSettings.filter((setting) => setting.id !== id);
				return null;
			}
			case "clear_app_notifications":
			case "send_test_native_notification":
			case "log_frontend_error":
			case "open_external_url":
			case "reveal_path":
			case "reveal_supervisor_logs_dir":
			case "activate_renderer_window":
			case "stop_renderer":
			case "reset_renderer_camera":
			case "set_renderer_expression_override":
			case "clear_renderer_expression_overrides":
			case "activate_renderer_runtime_action":
			case "set_renderer_runtime_parameter":
			case "set_renderer_clear_color":
			case "set_renderer_camera_lock":
			case "set_renderer_camera_state":
				return null;
			case "set_last_selected_setting_id":
			case "save_renderer_camera_to_profile":
			case "restore_renderer_camera_from_profile":
			case "save_renderer_spout_profile":
			case "restore_renderer_output_from_profile":
			case "save_renderer_window_to_profile":
			case "restore_renderer_window_from_profile":
			case "set_renderer_show_axes":
			case "set_renderer_show_bone_colliders":
			case "set_renderer_spout_output":
			case "set_renderer_motion_receivers":
			case "set_renderer_dynamics":
			case "set_renderer_dynamics_enabled":
			case "set_renderer_window":
			case "reveal_profiles_dir":
			case "stop_all_renderers":
				return null;
			default:
				throw new Error(`dev IPC mock: unsupported command ${cmd}`);
		}
	});
}

function parseFlatTomlMessages(text: string): Record<string, string> {
	const messages: Record<string, string> = {};
	let section = "";

	for (const rawLine of text.split(/\r?\n/)) {
		const line = rawLine.trim();
		if (!line || line.startsWith("#") || line.startsWith("_")) continue;
		const sectionMatch = line.match(/^\[([^\]]+)\]$/);
		if (sectionMatch) {
			section = sectionMatch[1] ?? "";
			continue;
		}
		const pairMatch = line.match(/^([A-Za-z0-9_]+)\s*=\s*("(?:\\.|[^"\\])*")\s*$/);
		if (!pairMatch) continue;
		const key = pairMatch[1];
		const value = JSON.parse(pairMatch[2]) as string;
		messages[section ? `${section}.${key}` : key] = value.replaceAll("%{", "{");
	}

	return messages;
}
