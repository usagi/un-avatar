import { mockIPC } from "@tauri-apps/api/mocks";
import enLocaleToml from "../src-tauri/locales/en-US.toml?raw";
import jaLocaleToml from "../src-tauri/locales/ja-JP.toml?raw";
import { BONE_COLLIDER_RADIUS_FIELD_PREFIX } from "./lib/boneColliderFields";
import { hasTauriRuntime } from "./lib/environment";
import type { RendererInstance } from "./lib/rendererTypes";
import type { AvatarSetting } from "./lib/profileTypes";
import newAvatarSetting from "./fixtures/new-avatar-setting.json";
import { defaultTextureCompressionAdvanced } from "./lib/qualityOptions";
import { DYNAMICS_BONE_COLLIDER_FIELD_PREFIX, defaultDynamicsCategoryOverrides } from "./lib/dynamicsPresets";

type LocaleBundle = {
	locale: string;
	messages: Record<string, string>;
};

// In-memory lifecycle only. No process, device, or renderer telemetry is created.
let previewRendererSequence = 0;
const previewRenderers: RendererInstance[] = [];

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
	last_selected_setting_id: null as string | null,
	pinned_taskbar_profile_ids: [],
	console_window_x: null,
	console_window_y: null,
	console_window_width: null,
	console_window_height: null,
	locale: "ja-JP",
};

// JSON imports widen tuple and enum types; native_new_profile_matches_preview_fixture
// checks this complete payload against the actual native serializer.
const newAvatarDefaults = newAvatarSetting as unknown as AvatarSetting;
let avatarSettings: AvatarSetting[] = [
	{
		...structuredClone(newAvatarDefaults),
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
		unmotion_zenoh_subscriptions: [{ id: "un-motion/frame", lan_enabled: false, host: null, port: 47447 }],
		audio_link_source: "none",
		audio_link_input_device_id: null,
		audio_link_input_device_name_hint: null,
		primary_motion_source: "unmotion_zenoh",
		dynamics_enabled: true,
		contact_parameter_emission: false,
		dynamics_physics_configured: false,
		dynamics_simulation_hz: 60,
		dynamics_substeps: 1,
		dynamics_surface_constraints_enabled: false,
		dynamics_surface_constraint_topology_max_edge_distance_m: 0.06,
		dynamics_surface_constraint_topology_max_mean_edge_distance_m: 0.03,
		dynamics_surface_constraint_spatial_max_distance_m: 0.012,
		dynamics_surface_constraint_topology_stiffness: 0.35,
		dynamics_surface_constraint_spatial_stiffness: 0.9,
		dynamics_surface_constraint_min_edge_count: 3,
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
		...structuredClone(newAvatarDefaults),
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
		unmotion_zenoh_subscriptions: [{ id: "un-motion/frame", lan_enabled: false, host: null, port: 47447 }],
		audio_link_source: "none",
		audio_link_input_device_id: null,
		audio_link_input_device_name_hint: null,
		primary_motion_source: "vmc",
		dynamics_enabled: true,
		contact_parameter_emission: false,
		dynamics_physics_configured: false,
		dynamics_simulation_hz: 60,
		dynamics_substeps: 1,
		dynamics_surface_constraints_enabled: false,
		dynamics_surface_constraint_topology_max_edge_distance_m: 0.06,
		dynamics_surface_constraint_topology_max_mean_edge_distance_m: 0.03,
		dynamics_surface_constraint_spatial_max_distance_m: 0.012,
		dynamics_surface_constraint_topology_stiffness: 0.35,
		dynamics_surface_constraint_spatial_stiffness: 0.9,
		dynamics_surface_constraint_min_edge_count: 3,
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

function applyPreviewSettingValue(setting: typeof avatarSettings[number], field: string, value: unknown): typeof avatarSettings[number] {
	const advancedPrefix = "render_quality.texture_compression_advanced.";
	if (field.startsWith(advancedPrefix)) {
		const role = field.slice(advancedPrefix.length);
		if (!Object.hasOwn(setting.texture_compression_advanced, role)) throw new Error(`dev IPC mock: unsupported field ${field}`);
		(setting.texture_compression_advanced as Record<string, unknown>)[role] = value;
		return structuredClone(setting);
	}
	if (["camera.target_x", "camera.target_y", "camera.target_z"].includes(field)) {
		const axis = ["camera.target_x", "camera.target_y", "camera.target_z"].indexOf(field);
		const current = (setting as { camera_target: number[] | null }).camera_target;
		const target: [number, number, number] = [current?.[0] ?? 0, current?.[1] ?? 0, current?.[2] ?? 0];
		target[axis] = Number(value);
		(setting as { camera_target: [number, number, number] | null }).camera_target = target;
		return structuredClone(setting);
	}
	const presetField = field.startsWith("render_quality.") ? field.slice("render_quality.".length)
		: field.startsWith("output.spout2.") ? `spout_${field.slice("output.spout2.".length)}`
		: field.startsWith("effects.avatar.outline.") ? `outline_${field.slice("effects.avatar.outline.".length)}`
		: field === "window.minimized" ? "minimized" : null;
	if (presetField && Object.hasOwn(setting, presetField)) {
		(setting as Record<string, unknown>)[presetField] = value;
		return structuredClone(setting);
	}
	let handled = false;
	const identityFields: Record<string, string> = {
		"profile.display_name": "name",
		"profile.group": "group",
		"profile.notes": "notes",
		"profile.allow_multiple_renderers": "allow_multiple_renderers",
		"profile.gpu_adapter": "gpu_adapter",
	};
	const identityField = identityFields[field];
	if (identityField) {
		(setting as Record<string, unknown>)[identityField] = value;
		return structuredClone(setting);
	}
	if (field === "avatar_path") {
		handled = true;
		setting.avatar_path = String(value ?? "");
	}
	if (field === "wardrobe_set") {
		handled = true;
		(setting as { wardrobe_set: string | null }).wardrobe_set = String(value ?? "") || null;
	}
	if (field === "wardrobe.transition.billboard_anchor") {
		handled = true;
		(setting as { wardrobe_billboard_anchor: string }).wardrobe_billboard_anchor = String(value ?? "neck");
	}
	if (field === "wardrobe.transition.billboard_y_offset_mm") {
		handled = true;
		(setting as { wardrobe_billboard_y_offset_mm: number }).wardrobe_billboard_y_offset_mm = Number(value ?? 0);
	}
	if (field === "motion.unmotion_zenoh.enabled") {
		handled = true;
		setting.motion_unmotion_enabled = Boolean(value);
	}
	if (field === "motion.unmotion_zenoh.key") {
		handled = true;
		setting.unmotion_zenoh_key = String(value ?? "") || null;
	}
	if (field === "motion.unmotion_zenoh.connect") {
		handled = true;
		(setting as { unmotion_zenoh_connect: string | null }).unmotion_zenoh_connect = String(value ?? "") || null;
	}
	if (field === "motion.unmotion_zenoh.subscriptions") {
		handled = true;
		setting.unmotion_zenoh_subscriptions = (value as typeof setting.unmotion_zenoh_subscriptions) ?? [];
	}
	if (field === "wardrobe.shortcuts") {
		handled = true;
		(setting as { wardrobe_shortcuts: unknown[] }).wardrobe_shortcuts = (value as unknown[]) ?? [];
	}
	if (field === "wardrobe.bindings") {
		handled = true;
		(setting as { wardrobe_bindings: unknown[] }).wardrobe_bindings = (value as unknown[]) ?? [];
	}
	if (field === "animator.actions") {
		handled = true;
		(setting as unknown as { animator_actions: unknown[] }).animator_actions = (value as unknown[]) ?? [];
	}
	if (field === "animator.bindings") {
		handled = true;
		(setting as unknown as { animator_bindings: unknown[] }).animator_bindings = (value as unknown[]) ?? [];
	}
	if (field === "environment.color.exposure") {
		handled = true;
		setting.color_exposure = Number(value ?? 0);
	}
	if (field === "environment.color.contrast") {
		handled = true;
		setting.color_contrast = Number(value ?? 1);
	}
	if (field === "environment.color.saturation") {
		handled = true;
		setting.color_saturation = Number(value ?? 1);
	}
	if (field === "environment.color.look") {
		handled = true;
		setting.color_look = String(value ?? "neutral");
	}
	if (field === "environment.color.intensity") {
		handled = true;
		setting.color_look_intensity = Number(value ?? 0);
	}
	if (field === "environment.color.temperature") {
		handled = true;
		setting.color_temperature = Number(value ?? 0);
	}
	if (field === "environment.color.tint") {
		handled = true;
		setting.color_tint = Number(value ?? 0);
	}
	if (field === "environment.lighting.environment.enabled") {
		handled = true;
		setting.lighting_environment_enabled = Boolean(value);
	}
	if (field === "environment.lighting.environment.color") {
		handled = true;
		setting.lighting_environment_color = value as [number, number, number];
	}
	if (field === "environment.lighting.environment.intensity") {
		handled = true;
		setting.lighting_environment_intensity = Number(value ?? 0.35);
	}
	if (field === "environment.lighting.directional.enabled") {
		handled = true;
		setting.lighting_directional_enabled = Boolean(value);
	}
	if (field === "environment.lighting.directional.color") {
		handled = true;
		setting.lighting_directional_color = value as [number, number, number];
	}
	if (field === "environment.lighting.directional.intensity") {
		handled = true;
		setting.lighting_directional_intensity = Number(value ?? 1);
	}
	if (field === "environment.lighting.directional.azimuth_deg") {
		handled = true;
		setting.lighting_directional_azimuth_deg = Number(value ?? 0);
	}
	if (field === "environment.lighting.directional.elevation_deg") {
		handled = true;
		setting.lighting_directional_elevation_deg = Number(value ?? 33.84);
	}
	if (field === "environment.lighting.directional.follow_camera_yaw") {
		handled = true;
		setting.lighting_directional_follow_camera_yaw = Boolean(value);
	}
	if (field === "environment.lighting.directional.follow_camera_pitch") {
		handled = true;
		setting.lighting_directional_follow_camera_pitch = Boolean(value);
	}
	if (field === "effects.post.bloom.enabled") {
		handled = true;
		setting.bloom_enabled = Boolean(value);
	}
	if (field === "effects.post.bloom.strength") {
		handled = true;
		setting.bloom_strength = Number(value ?? 0.35);
	}
	if (field === "effects.post.bloom.threshold") {
		handled = true;
		setting.bloom_threshold = Number(value ?? 0.65);
	}
	if (field === "effects.post.bloom.radius") {
		handled = true;
		setting.bloom_radius = Number(value ?? 8);
	}
	if (field === "effects.post.bloom.quality") {
		handled = true;
		setting.bloom_quality = String(value ?? "compact");
	}
	if (field === "effects.post.ssao.enabled") {
		handled = true;
		setting.ssao_enabled = Boolean(value);
	}
	if (field === "effects.post.ssao.strength") {
		handled = true;
		setting.ssao_strength = Number(value ?? 0.25);
	}
	if (field === "effects.post.ssao.radius") {
		handled = true;
		setting.ssao_radius = Number(value ?? 4);
	}
	if (field === "effects.post.ssao.bias") {
		handled = true;
		setting.ssao_bias = Number(value ?? 0.0015);
	}
	if (field === "effects.post.ssao.range") {
		handled = true;
		setting.ssao_range = Number(value ?? 0.03);
	}
	if (field === "effects.avatar.contact_shadow.enabled") {
		handled = true;
		setting.contact_shadow_enabled = Boolean(value);
	}
	if (field === "effects.avatar.contact_shadow.strength") {
		handled = true;
		setting.contact_shadow_strength = Number(value ?? 0.35);
	}
	if (field === "effects.avatar.contact_shadow.radius") {
		handled = true;
		setting.contact_shadow_radius = Number(value ?? 0.55);
	}
	if (field === "effects.avatar.contact_shadow.softness") {
		handled = true;
		setting.contact_shadow_softness = Number(value ?? 1.8);
	}
	if (field === "effects.avatar.contact_shadow.height") {
		handled = true;
		setting.contact_shadow_height = Number(value ?? 0);
	}
	const directField = field
		.replace("debug.", "")
		.replace(BONE_COLLIDER_RADIUS_FIELD_PREFIX, "bone_collider_")
		.replace(DYNAMICS_BONE_COLLIDER_FIELD_PREFIX, "bone_colliders_")
		.replaceAll(".", "_");
	if (Object.hasOwn(setting, directField)) {
		handled = true;
		(setting as any)[directField] = value;
	}
	if (!handled) throw new Error(`dev IPC mock: unsupported field ${field}`);
	return structuredClone(setting);
}

function updatePreviewSettings(id: string, updates: { field: string; value: unknown }[]): typeof avatarSettings[number] {
	if (updates.length === 0) throw new Error("updates must not be empty");
	const current = avatarSettings.find(setting => setting.id === id);
	if (!current) throw new Error("Preview profile not found");
	let next = structuredClone(current);
	for (const update of updates) next = applyPreviewSettingValue(next, update.field, update.value);
	avatarSettings = avatarSettings.map(setting => setting.id === id ? next : setting);
	return structuredClone(next);
}

// In-memory archive contract for the isolated design preview, not disk persistence.
const previewProfileArchives = new Map<string, (typeof avatarSettings)[number]>();
let previewArchiveSequence = 0;
function archivePreviewSetting(id: string): void {
    const setting = avatarSettings.find((entry) => entry.id === id);
    if (!setting) throw new Error(`dev IPC mock: profile not found: ${id}`);
    if (previewRenderers.some(entry => entry.profile_id === id && entry.state !== "Exited" && entry.state !== "Crashed")) {
        throw new Error(`Profile is in use by a Renderer: ${setting.name}`);
    }
    const snapshot = structuredClone(setting);
    previewProfileArchives.set(`preview-archive-${++previewArchiveSequence}`, snapshot);
    avatarSettings = avatarSettings.filter((entry) => entry.id !== id);
}
function listPreviewArchives() {
    return [...previewProfileArchives].map(([archive_id, profile]) => ({ archive_id, profile_id: profile.id, name: profile.name }));
}
function restorePreviewSetting(archiveId: string): void {
    const profile = previewProfileArchives.get(archiveId);
    if (!profile) throw new Error(`dev IPC mock: archive not found: ${archiveId}`);
    if (avatarSettings.some((entry) => entry.id === profile.id || entry.manifest_path === profile.manifest_path)) {
        throw new Error(`dev IPC mock: profile ID or destination already exists: ${profile.id}`);
    }
    avatarSettings = [...avatarSettings, structuredClone(profile)];
}

function withPreviewProfileRevision(cmd: string, payload: unknown, apply: () => unknown): unknown {
    const guarded = ["update_avatar_setting_value", "update_avatar_setting_values", "save_avatar_thumbnail_icon", "save_profile_icon_from_data_url", "delete_avatar_setting"].includes(cmd);
    if (!guarded) return apply();
    const args = (payload ?? {}) as Record<string, unknown>;
    const target = avatarSettings.find(item => item.id === args.settingId || item.manifest_path === args.settingId);
    if (!target) throw new Error(`dev IPC mock: profile not found: ${String(args.settingId)}`);
    if (typeof args.expectedRevision === "string" && args.expectedRevision !== target.revision) {
        throw new Error("profile_conflict: saved profile changed; reload and review before retrying");
    }
    const result = apply();
    if (cmd !== "delete_avatar_setting") {
        const saved = avatarSettings.find(item => item.id === target.id);
        if (saved) {
            saved.revision = `preview:${crypto.randomUUID()}`;
            if (result && typeof result === "object") (result as AvatarSetting).revision = saved.revision;
        }
    }
    return structuredClone(result);
}

const orderRecoveryRecords:{name:string;revision:string;before:AvatarSetting[];after:AvatarSetting[]}[]=[];
const orderRecoveryRoot='isolated-preview/profiles';
export function installDevIpcMock(): void {
	if (hasTauriRuntime()) return;

	mockIPC(
		(cmd, payload) => withPreviewProfileRevision(cmd, payload, () => {
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
				case "get_renderer_cache_status":
					return {
						texture: { path: "preview", bytes: 0, files: 0 },
						pipeline: { path: "preview", bytes: 0, files: 0 },
						total_bytes: 0,
						total_files: 0,
					};
				case "get_app_settings":
					return appSettings;
				case "sync_app_settings":
					appSettings = {
						...appSettings,
						...((args.settings ?? {}) as typeof appSettings),
						last_selected_setting_id: appSettings.last_selected_setting_id,
					};
					return appSettings;
				case "app_version":
					return "dev-preview";
				case "list_renderers":
					return structuredClone(previewRenderers);
				case "launch_renderer": {
					const setting = avatarSettings.find(item => item.id === args.settingId || item.manifest_path === args.settingId);
					if (!setting) throw new Error("Preview profile not found");
					const existing = previewRenderers.find(item => item.profile_id === setting.id && item.state !== "Exited" && item.state !== "Crashed");
					if (!setting.allow_multiple_renderers && existing) return structuredClone(existing);
					const instance: RendererInstance = {
						...structuredClone(setting), id: ++previewRendererSequence,
						// Negative PID identifies a simulated process and cannot identify a Windows process.
						profile_id: setting.id, state: "Running", pid: -previewRendererSequence, uptime_secs: 0,
						primary_motion_source: setting.primary_motion_source === "vmc" ? "vmc" : "unmotion_zenoh",
						last_stderr: null, stderr_tail: [], exit_code: null,
					};
					previewRenderers.push(instance);
					return structuredClone(instance);
				}
				case "stop_renderer":
				case "stop_all_renderers": {
					if (cmd === "stop_renderer" && !previewRenderers.some(item => item.id === Number(args.id))) {
						throw new Error("Preview renderer not found");
					}
					for (const instance of previewRenderers) {
						if (cmd === "stop_all_renderers" || instance.id === Number(args.id)) {
							instance.state = "Exited";
							instance.pid = null;
							instance.exit_code = 0;
						}
					}
					return null;
				}
				case "get_renderer_runtime_status":
					throw new Error("比較用: 実Rendererは起動していないため、描画・受信の計測値はありません");
				case "list_app_notifications":
				case "list_diagnostics_exports":
					return [];
				case "get_avatar_setting_details": {
					const setting = avatarSettings.find((s) => s.id === args.settingId);
					if (!setting) throw new Error("Preview profile not found");
					return structuredClone(setting);
				}
				case "list_avatar_settings":
					return avatarSettings;
				case "new_avatar_setting": {
					let sequence = 1;
					while (avatarSettings.some(item => item.id === `new-avatar-${sequence}`)) sequence++;
					const id = `new-avatar-${sequence}`;
					const setting: AvatarSetting = { ...structuredClone(newAvatarDefaults),
						id, name: sequence === 1 ? "New Avatar" : `New Avatar ${sequence}`,
						manifest_path: `profiles/${id}.toml`,
						sort_order: Math.max(0, ...avatarSettings.map(item => item.sort_order)) + 1 };
					avatarSettings = [...avatarSettings, setting];
					return structuredClone(setting);
				}
				case "duplicate_avatar_setting": {
					const source = avatarSettings.find(item => item.id === args.settingId);
					if (!source) throw new Error("Preview profile not found");
					let sequence = 1;
					while (avatarSettings.some(item => item.id === `${source.id}-copy-${sequence}`)) sequence++;
					const id = `${source.id}-copy-${sequence}`;
					const copy = { ...structuredClone(source), id, name: `${source.name} Copy ${sequence}`, manifest_path: `profiles/${id}.toml` };
					avatarSettings = [...avatarSettings, copy];
					return structuredClone(copy);
				}
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
						avatarSettings.find((item) => item.id === iconSettingId || item.manifest_path === iconSettingId) ??
						avatarSettings[0];
					(iconSetting as { icon_path: string | null }).icon_path =
						"C:/Users/example/AppData/Roaming/UN Avatar/profiles/assets/thumbnails/main-avatar-thumbnail.webp";
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
				case "update_avatar_setting_value":
					return updatePreviewSettings(String(args.settingId ?? ""), [{ field: String(args.field ?? ""), value: args.value }]);
				case "update_avatar_setting_values": {
					if (!Array.isArray(args.updates) || args.updates.some(update => !update || typeof update.field !== "string")) {
						throw new Error("dev IPC mock: invalid profile updates");
					}
					return updatePreviewSettings(String(args.settingId ?? ""), args.updates);
				}
				case "reorder_avatar_settings": {
					const ids = (args.settingIds as string[]) ?? [];
					const expected=args.expectedRevisions as Record<string,string>|undefined;
					if(new Set(ids).size!==ids.length)throw new Error('duplicate profile in reorder');
					if(expected&&(Object.keys(expected).length!==ids.length||ids.some(id=>!expected[id]||avatarSettings.find(s=>s.id===id)?.revision!==expected[id])))throw new Error('profile_conflict: reorder target changed');
					const settingsById = new Map(avatarSettings.map((setting) => [setting.id, setting]));
					const before=structuredClone(avatarSettings);
					const seenIds = new Set<string>();
					const orderedSettings: typeof avatarSettings = [];
					for (const id of ids) {
						const setting = settingsById.get(id);
						if (!setting) continue;
						seenIds.add(id);
						orderedSettings.push({...setting,sort_order:(orderedSettings.length+1)*1000,revision:`preview:${crypto.randomUUID()}`});
					}
					for (const setting of avatarSettings) {
						if (!seenIds.has(setting.id)) orderedSettings.push(setting);
					}
					avatarSettings = orderedSettings;
					orderRecoveryRecords.push({name:`preview-order-${crypto.randomUUID()}.json`,revision:crypto.randomUUID(),before,after:structuredClone(avatarSettings)});
					return avatarSettings;
				}
				case "list_profile_order_recovery":return {root:orderRecoveryRoot,entries:orderRecoveryRecords.map(record=>({name:record.name,revision:record.revision,files:record.before.map(p=>p.manifest_path),state:JSON.stringify(avatarSettings)===JSON.stringify(record.before)?'original':JSON.stringify(avatarSettings)===JSON.stringify(record.after)?'applied':'conflict',error:null}))};
				case "restore_profile_order":{
					const record=orderRecoveryRecords.find(record=>record.name===args.name);
					if(args.expectedRoot!==orderRecoveryRoot||!record||args.revision!==record.revision)throw Error('profile_conflict: recovery record changed');
					if(JSON.stringify(avatarSettings)!==JSON.stringify(record.before)&&JSON.stringify(avatarSettings)!==JSON.stringify(record.after))throw Error('profile_conflict: recovery target edited');
					avatarSettings=structuredClone(record.before);return avatarSettings;
				}
				case "get_native_notification_status":
					return { permission_state: "unsupported" };
				case "delete_avatar_setting":
                    archivePreviewSetting(String(args.settingId ?? ""));
                    return null;
                case "list_deleted_avatar_settings":
                    return listPreviewArchives();
                case "restore_avatar_setting":
                    restorePreviewSetting(String(args.archiveId ?? ""));
                    return null;
				case "clear_app_notifications":
					return null;
				case "send_test_native_notification":
					throw new Error("隔離プレビューではOSへの操作やファイル出力は実行していません。製品版で確認してください。 / Preview: native OS or file operation was not performed.");
				case "log_frontend_error":
					return null;
				case "open_external_url":
					throw new Error("隔離プレビューではOSへの操作やファイル出力は実行していません。製品版で確認してください。 / Preview: native OS or file operation was not performed.");
				case "reveal_path":
					throw new Error("隔離プレビューではOSへの操作やファイル出力は実行していません。製品版で確認してください。 / Preview: native OS or file operation was not performed.");
				case "reveal_supervisor_logs_dir":
					throw new Error("隔離プレビューではOSへの操作やファイル出力は実行していません。製品版で確認してください。 / Preview: native OS or file operation was not performed.");
					return null;
				case "activate_renderer_window":
				case "reset_renderer_camera":
				case "set_renderer_expression_override":
				case "clear_renderer_expression_overrides":
				case "activate_renderer_runtime_action":
				case "set_renderer_runtime_parameter":
				case "set_renderer_clear_color":
				case "set_renderer_camera_lock":
				case "set_renderer_camera_state":
					throw new Error("隔離プレビューでは実Rendererに接続していないため、この操作は実行していません。製品版で対象Rendererを起動して確認してください。 / Preview: this operation requires a real Renderer; it was not performed.");
				case "set_last_selected_setting_id":
					if (args.value != null && typeof args.value !== "string") throw new Error("Invalid selected setting id");
					appSettings = { ...appSettings, last_selected_setting_id: typeof args.value === "string" ? args.value.trim() || null : null };
					return null;
				case "save_renderer_camera_to_profile":
				case "restore_renderer_camera_from_profile":
				case "save_renderer_spout_profile":
				case "restore_renderer_output_from_profile":
				case "save_renderer_window_to_profile":
				case "restore_renderer_window_from_profile":
					throw new Error("隔離プレビューでは実Rendererに接続していないため、この保存・復元は実行していません。製品版で対象Rendererを起動して確認してください。 / Preview: this save or restore requires a real Renderer; no settings were changed.");
				case "set_renderer_apply_vmc_root_translation":
				case "set_renderer_avatar_outline":
				case "set_renderer_bloom":
				case "set_renderer_contact_shadow":
				case "set_renderer_environment_color":
				case "set_renderer_lighting":
				case "set_renderer_look_at":
				case "set_renderer_ssao":
				case "set_renderer_show_axes":
				case "set_renderer_show_bone_colliders":
				case "set_renderer_spout_output":
				case "set_renderer_motion_receivers":
				case "set_renderer_dynamics":
				case "set_renderer_dynamics_enabled":
				case "set_renderer_window":
					throw new Error("隔離プレビューでは実Rendererに接続していないため、この操作は実行していません。製品版で対象Rendererを起動して確認してください。 / Preview: this operation requires a real Renderer; it was not performed.");
				case "reveal_profiles_dir":
					throw new Error("隔離プレビューではOSへの操作やファイル出力は実行していません。製品版で確認してください。 / Preview: native OS or file operation was not performed.");
					return null;
				default:
					throw new Error(`dev IPC mock: unsupported command ${cmd}`);
			}
		}),
		{ shouldMockEvents: true }
	);
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
