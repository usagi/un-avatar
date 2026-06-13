<script lang="ts">
	import { convertFileSrc, invoke } from "@tauri-apps/api/core";
	import { _ } from "svelte-i18n";
	import { setUiLocale } from "@usagi.network/un-i18n-svelte";
	import AppSettingsView from "./lib/AppSettingsView.svelte";
	import LogsView from "./lib/LogsView.svelte";
	import ProfileAvatarSection from "./lib/ProfileAvatarSection.svelte";
	import ProfileCameraSection from "./lib/ProfileCameraSection.svelte";
	import ProfileIdentitySection from "./lib/ProfileIdentitySection.svelte";
	import ProfileLightingSection from "./lib/ProfileLightingSection.svelte";
	import ProfileLookSection from "./lib/ProfileLookSection.svelte";
	import ProfileMotionSection from "./lib/ProfileMotionSection.svelte";
	import ProfilePhysicsSection from "./lib/ProfilePhysicsSection.svelte";
	import ProfileOutputSection from "./lib/ProfileOutputSection.svelte";
	import ProfileQualitySection from "./lib/ProfileQualitySection.svelte";
	import ProfileSectionNav from "./lib/ProfileSectionNav.svelte";
	import ProfileSettingList from "./lib/ProfileSettingList.svelte";
	import ProfileStage from "./lib/ProfileStage.svelte";
	import ProfileWindowSection from "./lib/ProfileWindowSection.svelte";
	import ProfilesToolbar from "./lib/ProfilesToolbar.svelte";
	import type {
		AvatarSetting,
		CameraLensPreset,
		CameraTargetPreset,
		LookRecommendation,
		OutputModePreset,
		PreviewWindowPreset,
		ProfileSectionId,
		ProfileSettingValue,
		RenderQualityRecommendation,
		SpoutResolutionPreset,
		UnavatarWardrobeOptions,
	} from "./lib/profileTypes";
	import type { ProfileSectionNavItem } from "./lib/profileStageTypes";
	import RendererDetailsPanel from "./lib/RendererDetailsPanel.svelte";
	import RendererProcessTable from "./lib/RendererProcessTable.svelte";
	import RendererReadyStage from "./lib/RendererReadyStage.svelte";
	import RendererStage from "./lib/RendererStage.svelte";
	import RenderersToolbar from "./lib/RenderersToolbar.svelte";
	import UnavatarRightsModal from "./lib/UnavatarRightsModal.svelte";
	import VrmMetadataModal from "./lib/VrmMetadataModal.svelte";
	import type {
		RendererCameraSnapshot,
		RendererInstance,
		RendererPaneTab,
		RendererRuntimeStatus,
		RendererState,
	} from "./lib/rendererTypes";
	import { fallbackVrmMetadata, looksLikeVrmPath, type VrmMetadataDialogState, type VrmMetadataInfo } from "./lib/vrmMetadata";
	import {
		looksLikeUnavatarPath,
		type UnavatarMetadataDialogState,
		type UnavatarMetadataInfo,
		type UnavatarProfileIconCrop,
	} from "./lib/unavatarMetadata";
	import { loadAppSettings, saveAppSettings, type AppSettings, type ThemeMode } from "./lib/appSettings";
	import type { AppNotification, DiagnosticsExportEntry, NativeNotificationStatus } from "./lib/appTypes";
	import { cameraOrbitPresetAngles, type CameraOrbitPreset } from "./lib/cameraPresets";
	import {
		countTextMatches,
		diagnosticsBundleFindings,
		diagnosticsBundleFromText,
		diagnosticsBundleSummary,
		diagnosticsComparisonDetails,
		diagnosticsComparisonSummary,
		diagnosticsEntrySearchText,
		diagnosticsEntryTime,
		diagnosticsRendererComparisonDetails,
		diagnosticsRendererInsights,
	} from "./lib/diagnostics";
	import {
		aaModeLabel,
		basename,
		dirname,
		filenameTimestamp,
		formatClockTimeFromUnixSecs,
		formatFixed,
		formatUptime,
		textureModeLabel,
	} from "./lib/formatting";
	import { detectOsTheme, hasTauriRuntime } from "./lib/environment";
	import { browserDownloadText } from "./lib/browserDownload";
	import { fieldSetHas, fieldSetIncludes, fieldSetIncludesAny, fieldSetStartsWith } from "./lib/fieldSets";
	import { normalizedPathKey, sameNormalizedPath } from "./lib/paths";
	import { canApplyWithoutRestart, isLaunchTimeRendererField, isRuntimeWindowField, profileFieldLabel } from "./lib/profileFieldRules";
	import {
		CAMERA_TARGET_PRESETS,
		LOOK_RECOMMENDATIONS,
		OUTPUT_MODE_PRESETS,
		PREVIEW_WINDOW_PRESETS,
		RENDER_QUALITY_RECOMMENDATIONS,
		SPOUT_RESOLUTION_PRESETS,
		type ProfilePresetUpdate,
	} from "./lib/profilePresets";
	import { defaultTextureCompressionAdvanced } from "./lib/qualityOptions";
	import {
		compareAvatarSettings,
		isValidLaunchTarget,
		pickInitialLaunchTargetId,
		pickInitialSelectedSettingId,
	} from "./lib/profileSelection";
	import { DEFAULT_PROFILE_ICON_SRC, profileIconSrc } from "./lib/profileIcons";
	import { motionLabel, settingSummary, windowLabel } from "./lib/profileLabels";
	import { diagonalFovFromLensMm } from "./lib/profileDiagrams";
	import { countErrorNotifications, countRendererStates } from "./lib/runtimeState";
	import {
		defaultRendererLogExpanded,
		filteredRendererLogLines as filterRendererLogLines,
		rendererLogText as rendererLogTextFromLines,
		type RendererLogFilter,
	} from "./lib/rendererLogs";
	import {
		rendererAvatarOutlinePayload,
		rendererBloomPayload,
		rendererClearColorPayload,
		rendererContactShadowPayload,
		rendererEnvironmentColorPayload,
		rendererLightingPayload,
		rendererLookAtPayload,
		rendererMotionReceiversPayload,
		rendererSsaoPayload,
		rendererWindowPayload,
	} from "./lib/runtimePayloads";
	import {
		DYNAMICS_BONE_COLLIDER_FIELD_PREFIX,
		DYNAMICS_ENABLE_ALL_ON_LAUNCH_FIELD,
		DYNAMICS_ENABLED_FIELD,
		DYNAMICS_OVERRIDE_FIELD_PREFIX,
		defaultDynamicsCategoryOverrides,
	} from "./lib/dynamicsPresets";
	import {
		loadColorDisplayMode,
		loadLaunchTargetId,
		saveColorDisplayMode,
		saveLaunchTargetId,
		type ColorDisplayMode,
	} from "./lib/storageState";
	import { spoutSenderLabel, spoutTimingLabel, textureCacheLabel, texturePolicyLabel, textureSummaryLabel } from "./lib/runtimeLabels";
	import { Activity, AlertTriangle, Camera, FileCog, FolderOpen, Monitor, Play, Settings, TerminalSquare } from "lucide-svelte";
	import ThemeModeSwitch from "./lib/ThemeModeSwitch.svelte";

	const COLOR_DISPLAY_MODE_KEY = "un-avatar-supervisor.colorDisplayMode";

	const profileSectionNavItems: ProfileSectionNavItem[] = [
		{
			id: "identity",
			labelKey: "profiles.editor.profile_setting_heading",
			scopeKey: null,
		},
		{
			id: "avatar",
			labelKey: "profiles.editor.avatar",
			scopeKey: "profiles.editor.launch_time",
		},
		{
			id: "quality",
			labelKey: "profiles.editor.render_quality",
			scopeKey: "profiles.editor.launch_time",
		},
		{
			id: "lighting",
			labelKey: "profiles.editor.lighting",
			scopeKey: null,
		},
		{
			id: "look",
			labelKey: "profiles.editor.rendering_presentation",
			scopeKey: null,
		},
		{
			id: "motion",
			labelKey: "profiles.editor.motion",
			scopeKey: null,
		},
		{
			id: "physics",
			labelKey: "profiles.editor.physics",
			scopeKey: null,
		},
		{
			id: "camera",
			labelKey: "profiles.editor.camera",
			scopeKey: null,
		},
		{
			id: "window",
			labelKey: "profiles.editor.window",
			scopeKey: null,
		},
		{
			id: "output",
			labelKey: "profiles.editor.output",
			scopeKey: "profiles.editor.launch_time",
		},
	];

	type VrmMetadataModalState = VrmMetadataDialogState & {
		settingId: string;
		rendererToRestart: RendererInstance | null;
	};

	type UnavatarMetadataModalState = UnavatarMetadataDialogState & {
		settingId: string;
		rendererToRestart: RendererInstance | null;
	};

	const appSettingsStorageKey = "un-avatar-supervisor.app-settings";
	const legacyThemeModeStorageKey = "un-avatar-supervisor.theme-mode";
	const launchTargetStorageKey = "un-avatar-supervisor.launch-target-id";
	const defaultIconSrc = DEFAULT_PROFILE_ICON_SRC;

	let appVersion = $state("");
	let activeTab = $state<"renderers" | "settings" | "logs" | "app">("renderers");
	let renderers = $state<RendererInstance[]>([]);
	let runtimeStatuses = $state<Record<number, RendererRuntimeStatus>>({});
	let runtimeStatusSamples = new Map<
		number,
		{
			at: number;
			unmotionRecv: number;
			motionApply: number;
		}
	>();
	let rendererActivationSeq = new Map<number, number>();
	let notifications = $state<AppNotification[]>([]);
	let nativeNotificationStatus = $state<NativeNotificationStatus | null>(null);
	let diagnosticsExports = $state<DiagnosticsExportEntry[]>([]);
	let avatarSettings = $state<AvatarSetting[]>([]);
	let profileIconRevision = $state<Record<string, number>>({});
	let wardrobeOptions = $state<UnavatarWardrobeOptions | null>(null);
	let wardrobeOptionsKey = $state("");
	let selectedRendererId = $state<number | null>(null);
	/// Selected Renderer 詳細パネルのサブタブ。Overview = 状態 + コントロール、
	/// Expressions = 表情プレビュー、Diagnostics = stderr など。タブ分割で縦長解消。
	let rendererPaneTab = $state<RendererPaneTab>("overview");
	let showStoppedRenderers = $state(false);
	let selectedSettingId = $state<string | null>(null);
	let draggedSettingId = $state<string | null>(null);
	let profileHint = $state("");
	let settingsHint = $state("");
	let settingPointerDrag = $state<{
		id: string;
		startX: number;
		startY: number;
		currentX: number;
		currentY: number;
		offsetX: number;
		offsetY: number;
		width: number;
		height: number;
		active: boolean;
	} | null>(null);
	let suppressSettingClick = $state(false);
	const defaultProfileHint = $derived($_("profiles.hints.default"));
	const defaultSettingsHint = $derived($_("settings.hints.default"));
	let launchTargetId = $state<string | null>(loadLaunchTargetId(launchTargetStorageKey));
	let launchMenuOpen = $state(false);
	let activeProfileSection = $state<ProfileSectionId>("identity");
	let profileSectionScrollTarget = $state<ProfileSectionId | null>(null);
	let profileSectionScrollUnlockTimer: number | null = null;
	let busy = $state(false);
	let message = $state("");
	let screenshotNoticePath = $state<string | null>(null);
	let pendingRendererRestart = $state<{
		renderer: RendererInstance;
		fieldLabel: string;
	} | null>(null);
	let vrmMetadataModal = $state<VrmMetadataModalState | null>(null);
	let unavatarMetadataModal = $state<UnavatarMetadataModalState | null>(null);
	let useThumbnailForProfileIconOnAccept = $state(true);
	let unavatarProfileIconCrop = $state<UnavatarProfileIconCrop>({
		enabled: true,
		imageDataUrl: null,
		zoom: 1,
		offsetX: 0,
		offsetY: 0,
	});
	let lastDiagnosticsPath = $state<string | null>(null);
	let lastDiagnosticsArchivePath = $state<string | null>(null);
	let diagnosticsPreviewTitle = $state<string | null>(null);
	let diagnosticsPreviewText = $state<string | null>(null);
	let diagnosticsSearch = $state("");
	let diagnosticsComparePaths = $state<string[]>([]);
	let diagnosticsCompareTexts = $state<Record<string, string>>({});
	let osTheme = $state<"light" | "dark">(detectOsTheme());
	let appSettings = $state<AppSettings>(loadAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey));
	let colorDisplayMode = $state<ColorDisplayMode>(loadColorDisplayMode(COLOR_DISPLAY_MODE_KEY));
	let backendAppSettingsReady = $state(!hasTauriRuntime());
	/// サーバー側 `i18n_available_locales` で得られたサポート言語の BCP-47 タグ一覧。
	let availableLocales = $state<string[]>(["ja-JP", "en-US"]);
	let deleteHoldTargetId = $state<string | null>(null);
	let deleteHoldProgress = $state(0);
	let deleteHoldTimer: number | null = null;
	let deleteHoldStartedAt = 0;
	let runtimeRefreshBusy = false;
	let runtimeRefreshPending = false;
	let startupAutoLaunchAttempted = false;

	const deleteHoldDurationMs = 1200;
	const motionLookAtFields = ["motion.look_at.enabled", "motion.look_at.clamp_deg"] as const;
	const motionReceiverFields = [
		"motion.vmc_udp.enabled",
		"motion.vmc_udp.address",
		"motion.unmotion_zenoh.enabled",
		"motion.unmotion_zenoh.key",
	] as const;

	/// Renderers タブは稼働中プロセスの操作面に絞る。Exited/Crashed は Logs タブに履歴として残す。
	const visibleRenderers = $derived(renderers.filter((renderer) => renderer.state !== "Exited" && renderer.state !== "Crashed"));
	const rendererTableRenderers = $derived(showStoppedRenderers ? renderers : visibleRenderers);
	const rendererById = $derived(
		(() => {
			const byId = new Map<number, RendererInstance>();
			for (const renderer of renderers) {
				byId.set(renderer.id, renderer);
			}
			return byId;
		})()
	);
	const rendererTableById = $derived(
		(() => {
			const byId = new Map<number, RendererInstance>();
			for (const renderer of rendererTableRenderers) {
				byId.set(renderer.id, renderer);
			}
			return byId;
		})()
	);
	const avatarSettingById = $derived(
		(() => {
			const byId = new Map<string, AvatarSetting>();
			for (const setting of avatarSettings) {
				byId.set(setting.id, setting);
			}
			return byId;
		})()
	);
	const avatarSettingByManifestPath = $derived(
		(() => {
			const byPath = new Map<string, AvatarSetting>();
			for (const setting of avatarSettings) {
				const path = normalizedPathKey(setting.manifest_path);
				if (path) byPath.set(path, setting);
			}
			return byPath;
		})()
	);
	const avatarSettingsByGroup = $derived(
		(() => {
			const byGroup = new Map<string, AvatarSetting[]>();
			for (const setting of avatarSettings) {
				const group = setting.group.trim();
				if (!group) continue;
				const existing = byGroup.get(group);
				if (existing) {
					existing.push(setting);
				} else {
					byGroup.set(group, [setting]);
				}
			}
			return byGroup;
		})()
	);
	const liveRenderersByManifestPath = $derived(
		(() => {
			const byPath = new Map<string, RendererInstance[]>();
			for (const renderer of renderers) {
				if (!isLiveRenderer(renderer)) continue;
				const path = normalizedPathKey(renderer.manifest_path);
				if (!path) continue;
				const existing = byPath.get(path);
				if (existing) {
					existing.push(renderer);
				} else {
					byPath.set(path, [renderer]);
				}
			}
			return byPath;
		})()
	);
	/// 選択中のレンダラー詳細。操作対象は `visibleRenderers` から選ぶ。
	const selectedRenderer = $derived(selectedRendererById(selectedRendererId) ?? rendererTableRenderers[0] ?? null);
	const selectedRuntimeStatus = $derived(selectedRenderer ? (runtimeStatuses[selectedRenderer.id] ?? null) : null);
	const selectedSetting = $derived(avatarSettingBySelectedId(selectedSettingId) ?? avatarSettings[0] ?? null);
	const launchProfileId = $derived(launchTargetId?.startsWith("group:") ? null : launchTargetId);
	const launchTargetSetting = $derived(avatarSettingBySelectedId(launchProfileId) ?? selectedSetting);
	const profileGroups = $derived(Array.from(avatarSettingsByGroup.keys()).sort((a, b) => a.localeCompare(b)));
	const launchGroupName = $derived(launchTargetId?.startsWith("group:") ? launchTargetId.slice("group:".length) : "");
	const launchGroupSettings = $derived(launchGroupName ? (avatarSettingsByGroup.get(launchGroupName) ?? []) : []);
	const rendererStatusCounts = $derived(countRendererStates(renderers));
	const notificationErrorCount = $derived(countErrorNotifications(notifications));
	const runningCount = $derived(rendererStatusCounts.running);
	const issueCount = $derived(rendererStatusCounts.issues + notificationErrorCount);
	const resolvedTheme = $derived(appSettings.theme_mode === "system" ? osTheme : appSettings.theme_mode);
	const diagnosticsSearchQuery = $derived(diagnosticsSearch.trim().toLowerCase());
	const filteredDiagnosticsExports = $derived(
		diagnosticsSearchQuery
			? diagnosticsExports.filter((entry) => diagnosticsEntrySearchText(entry).includes(diagnosticsSearchQuery))
			: diagnosticsExports
	);
	const diagnosticsExportByPath = $derived(
		(() => {
			const byPath = new Map<string, DiagnosticsExportEntry>();
			for (const entry of diagnosticsExports) {
				byPath.set(entry.path, entry);
			}
			return byPath;
		})()
	);
	const diagnosticsPreviewMatchCount = $derived(
		diagnosticsPreviewText && diagnosticsSearchQuery ? countTextMatches(diagnosticsPreviewText, diagnosticsSearchQuery) : null
	);
	const diagnosticsPreviewBundle = $derived(diagnosticsPreviewText ? diagnosticsBundleFromText(diagnosticsPreviewText) : null);
	const diagnosticsPreviewSummary = $derived(
		diagnosticsPreviewBundle
			? diagnosticsBundleSummary(diagnosticsPreviewBundle)
			: diagnosticsPreviewText
				? [{ label: "Summary", value: "Invalid diagnostics JSON" }]
				: []
	);
	const diagnosticsPreviewFindings = $derived(diagnosticsPreviewBundle ? diagnosticsBundleFindings(diagnosticsPreviewBundle) : []);
	const diagnosticsPreviewRenderers = $derived(
		diagnosticsPreviewBundle
			? diagnosticsRendererInsights(diagnosticsPreviewBundle, {
					pending: $_("renderers.summary.pending"),
					connected: $_("renderers.summary.connected"),
					disconnected: $_("renderers.summary.disconnected"),
				})
			: []
	);
	const diagnosticsCompareEntries = $derived(
		(() => {
			const entries: DiagnosticsExportEntry[] = [];
			for (const path of diagnosticsComparePaths) {
				const entry = diagnosticsExportByPath.get(path);
				if (entry) entries.push(entry);
			}
			return entries;
		})()
	);
	const diagnosticsCompareBundles = $derived(
		(() => {
			const bundles: Record<string, unknown>[] = [];
			for (const entry of diagnosticsCompareEntries) {
				const bundle = diagnosticsBundleFromText(diagnosticsCompareTexts[entry.path] ?? "");
				if (bundle) bundles.push(bundle);
			}
			return bundles;
		})()
	);
	const diagnosticsCompareSummary = $derived(
		diagnosticsCompareEntries.length === 2
			? diagnosticsComparisonSummary(diagnosticsCompareEntries[0], diagnosticsCompareEntries[1])
			: null
	);
	const diagnosticsCompareDetails = $derived(
		diagnosticsCompareEntries.length === 2 && diagnosticsCompareBundles.length === 2
			? diagnosticsComparisonDetails(diagnosticsCompareBundles[0], diagnosticsCompareBundles[1])
			: []
	);
	const diagnosticsCompareRendererDetails = $derived(
		diagnosticsCompareEntries.length === 2 && diagnosticsCompareBundles.length === 2
			? diagnosticsRendererComparisonDetails(diagnosticsCompareBundles[0], diagnosticsCompareBundles[1], {
					pending: $_("renderers.summary.pending"),
				})
			: []
	);
	function hintFromEventTarget(event: Event): string | undefined {
		const target = event.target instanceof HTMLElement ? event.target : null;
		return target?.closest<HTMLElement>("[data-hint]")?.dataset.hint;
	}

	function updateProfileHintFromEvent(event: Event): void {
		const hint = hintFromEventTarget(event);
		if (hint) profileHint = hint;
	}

	function clearProfileHint(): void {
		profileHint = "";
	}

	function updateSettingsHintFromEvent(event: Event): void {
		const hint = hintFromEventTarget(event);
		if (hint) settingsHint = hint;
	}

	function clearSettingsHint(): void {
		settingsHint = "";
	}

	const browserPreviewSettings: AvatarSetting[] = [
		{
			id: "browser-preview-main",
			name: "Main Avatar",
			created_at: "20260517T000000Z",
			sort_order: 1000,
			storage: "seed",
			manifest_path: "profiles/main.toml",
			avatar_path: null,
			wardrobe_set: null,
			vmc_address: "0.0.0.0:39539",
			vmc_port: 39539,
			motion_vmc_enabled: true,
			motion_unmotion_enabled: false,
			unmotion_zenoh_key: null,
			audio_link_source: "none",
			audio_link_input_device_id: null,
			audio_link_input_device_name_hint: null,
			look_at_enabled: false,
			look_at_clamp_deg: 30,
			primary_motion_source: "vmc",
			dynamics_enabled: true,
			dynamics_enable_all_on_launch: false,
			contact_parameter_emission: false,
			dynamics_physics_configured: false,
			dynamics_simulation_hz: 60,
			dynamics_substeps: 1,
			dynamics_category_overrides: defaultDynamicsCategoryOverrides(),
			apply_vmc_root_translation: false,
			camera_target: null,
			camera_longitude_deg: null,
			camera_latitude_deg: null,
			camera_radius: null,
			camera_diagonal_fov_deg: 35.0,
			spout_enabled: true,
			spout_name: "UN Avatar Spout",
			spout_width: 1280,
			spout_height: 720,
			aa: "off",
			texture_resolution_limit: "off",
			texture_compression: "balanced",
			mipmap_filter: "mitchell",
			render_backend: "dx12",
			block_compression_encoder: "gpu",
			block_compression_cpu_threads: 4,
			texture_compression_advanced: defaultTextureCompressionAdvanced(),
			processed_texture_cache: true,
			skin_tone_matching: false,
			background_color: [0.12, 0.14, 0.18],
			transparent: true,
			input_passthrough: false,
			decorations: false,
			always_on_top: false,
			minimized: false,
			show_axes: false,
			show_bone_colliders: false,
			bone_colliders_enabled: true,
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
			outline_type: "mtoon",
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
			notes: "OBS main streaming setup",
			group: "Main",
			scene_cache_fingerprint: "preview-main-v1",
			scene_cache_prewarmed_fingerprint: null,
			scene_cache_prewarmed_at: null,
		},
		{
			id: "browser-preview-debug",
			name: "Debug View",
			created_at: "20260517T000001Z",
			sort_order: 2000,
			storage: "seed",
			manifest_path: "profiles/debug.toml",
			avatar_path: null,
			wardrobe_set: null,
			vmc_address: "0.0.0.0:39539",
			vmc_port: null,
			motion_vmc_enabled: false,
			motion_unmotion_enabled: false,
			unmotion_zenoh_key: null,
			audio_link_source: "none",
			audio_link_input_device_id: null,
			audio_link_input_device_name_hint: null,
			look_at_enabled: false,
			look_at_clamp_deg: 30,
			primary_motion_source: "vmc",
			dynamics_enabled: true,
			dynamics_enable_all_on_launch: false,
			contact_parameter_emission: false,
			dynamics_physics_configured: false,
			dynamics_simulation_hz: 60,
			dynamics_substeps: 1,
			dynamics_category_overrides: defaultDynamicsCategoryOverrides(),
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
			texture_resolution_limit: "off",
			texture_compression: "balanced",
			mipmap_filter: "mitchell",
			render_backend: "dx12",
			block_compression_encoder: "gpu",
			block_compression_cpu_threads: 4,
			texture_compression_advanced: defaultTextureCompressionAdvanced(),
			processed_texture_cache: true,
			skin_tone_matching: false,
			background_color: [0.12, 0.14, 0.18],
			transparent: false,
			input_passthrough: false,
			decorations: true,
			always_on_top: false,
			minimized: false,
			show_axes: true,
			show_bone_colliders: true,
			bone_colliders_enabled: true,
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
			outline_type: "mtoon",
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
			notes: "Diagnostics profile",
			group: "Debug",
			scene_cache_fingerprint: "preview-debug-v1",
			scene_cache_prewarmed_fingerprint: "preview-debug-v1",
			scene_cache_prewarmed_at: "20260517T000001Z",
		},
	];

	function scrollProfileSection(id: ProfileSectionId): void {
		activeProfileSection = id;
		profileSectionScrollTarget = id;
		if (profileSectionScrollUnlockTimer != null && typeof window !== "undefined") {
			window.clearTimeout(profileSectionScrollUnlockTimer);
		}
		if (typeof document === "undefined") {
			profileSectionScrollTarget = null;
			return;
		}
		const section = document.querySelector<HTMLElement>(`[data-profile-section="${id}"]`);
		section?.scrollIntoView({ block: "start", behavior: "auto" });
		if (typeof window !== "undefined") {
			profileSectionScrollUnlockTimer = window.setTimeout(() => {
				profileSectionScrollTarget = null;
				profileSectionScrollUnlockTimer = null;
			}, 1200);
		}
	}

	function updateActiveProfileSectionFromScroll(event: Event): void {
		const scroller = event.currentTarget as HTMLElement;
		const scrollerTop = scroller.getBoundingClientRect().top;
		if (profileSectionScrollTarget) {
			return;
		}
		const sections = Array.from(scroller.querySelectorAll<HTMLElement>("[data-profile-section]"));
		let current = activeProfileSection;
		for (const section of sections) {
			if (section.getBoundingClientRect().top - scrollerTop > 72) break;
			current = section.dataset.profileSection as ProfileSectionId;
		}
		activeProfileSection = current;
	}

	function setColorDisplayMode(mode: ColorDisplayMode): void {
		colorDisplayMode = mode;
		saveColorDisplayMode(COLOR_DISPLAY_MODE_KEY, mode);
	}

	async function syncAppSettingsToBackend(settings: AppSettings): Promise<void> {
		if (!hasTauriRuntime()) return;
		if (!backendAppSettingsReady) return;
		try {
			await invoke("sync_app_settings", { settings });
		} catch (error) {
			message = String(error);
		}
	}

	function replaceAvatarSetting(setting: AvatarSetting): void {
		const next = avatarSettings.filter((item) => item.id !== setting.id && item.manifest_path !== setting.manifest_path);
		next.push(setting);
		next.sort(compareAvatarSettings);
		avatarSettings = next;
		selectedSettingId = setting.id;
		launchTargetId = pickInitialLaunchTargetId(launchTargetId, selectedSettingId, next);
	}

	function bumpProfileIconRevision(path: string | null): void {
		if (!path) return;
		profileIconRevision = {
			...profileIconRevision,
			[path]: (profileIconRevision[path] ?? 0) + 1,
		};
	}

	/// Renderers / Avatar Settings 画面で選択された ID を Tauri 側へ即座に保存する。
	/// 失敗は UI を阻害しないようサイレントに無視する。
	async function persistLastSelectedSettingId(id: string | null): Promise<void> {
		if (!hasTauriRuntime()) return;
		if (!backendAppSettingsReady) return;
		try {
			await invoke("set_last_selected_setting_id", { value: id });
			appSettings = { ...appSettings, last_selected_setting_id: id };
			saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, appSettings);
		} catch {
			// 永続化失敗は次回起動時に「先頭プロファイル」にフォールバックするだけなので致命的ではない
		}
	}

	function setAppSetting<K extends keyof AppSettings>(key: K, value: AppSettings[K]): void {
		appSettings = { ...appSettings, [key]: value };
		saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, appSettings);
		syncAppSettingsToBackend(appSettings);
	}

	function setThemeMode(mode: ThemeMode): void {
		setAppSetting("theme_mode", mode);
	}

	function setLocaleSetting(locale: string): void {
		setAppSetting("locale", locale);
		if (locale === "") {
			void invoke<string>("i18n_resolve_default_locale").then((tag) => setUiLocale(tag));
			return;
		}
		setUiLocale(locale);
	}

	async function openExternalLink(url: string): Promise<void> {
		if (hasTauriRuntime()) {
			try {
				await invoke("open_external_url", { url });
				return;
			} catch (error) {
				message = String(error);
			}
		}
		try {
			window.open(url, "_blank", "noopener");
		} catch {
			// ignore; user can copy the URL manually
		}
	}

	async function loadBackendAppSettings(): Promise<void> {
		if (!hasTauriRuntime()) return;
		try {
			appSettings = await invoke<AppSettings>("get_app_settings");
			saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, appSettings);
			// backend が解決した locale (永続化済 or 起動時 OS 自動解決済) を svelte-i18n へ反映する。
			// 空文字なら resolve_default_locale を再取得して適用 (起動経路の単一化のため)。
			const effective = appSettings.locale || (await invoke<string>("i18n_resolve_default_locale").catch(() => "ja-JP"));
			setUiLocale(effective);
		} catch (error) {
			message = String(error);
		} finally {
			backendAppSettingsReady = true;
		}
		try {
			appVersion = await invoke<string>("app_version");
		} catch {
			appVersion = "";
		}
		try {
			availableLocales = await invoke<string[]>("i18n_available_locales");
		} catch {
			availableLocales = ["ja-JP", "en-US"];
		}
	}

	async function readVrmMetadataForPath(path: string, setting: AvatarSetting | null = selectedSetting): Promise<VrmMetadataInfo | null> {
		if (!setting) return null;
		try {
			return await invoke<VrmMetadataInfo | null>("read_vrm_metadata", {
				path,
				manifestPath: setting.manifest_path,
			});
		} catch (error) {
			if (looksLikeVrmPath(path)) {
				return fallbackVrmMetadata(path, error, $_);
			}
			throw error;
		}
	}

	async function readUnavatarMetadataForPath(path: string, setting: AvatarSetting | null = selectedSetting): Promise<UnavatarMetadataInfo | null> {
		if (!setting) return null;
		return await invoke<UnavatarMetadataInfo | null>("read_unavatar_metadata", {
			path,
			manifestPath: setting.manifest_path,
			wardrobeSet: setting.wardrobe_set,
		});
	}

	function rendererRuntimeStatus(renderer: RendererInstance): RendererRuntimeStatus | null {
		return runtimeStatuses[renderer.id] ?? null;
	}

	function iconSrc(path: string | null): string {
		const src = profileIconSrc(path, hasTauriRuntime(), convertFileSrc);
		const revision = path ? (profileIconRevision[path] ?? 0) : 0;
		if (!revision || src.startsWith("data:image/") || src === DEFAULT_PROFILE_ICON_SRC) return src;
		return `${src}${src.includes("?") ? "&" : "?"}v=${revision}`;
	}

	async function applyProfileUpdates(updates: readonly ProfilePresetUpdate[]): Promise<void> {
		if (updates.length === 0) return;
		if (!selectedSetting || !hasTauriRuntime() || updates.some(([field]) => field === "avatar_path")) {
			for (const [field, value] of updates) {
				await updateSettingValue(field, value);
			}
			return;
		}
		const previousSetting = selectedSetting;
		const updatePayload: Array<{ field: string; value: ProfileSettingValue }> = [];
		const restartTargets: Array<{
			field: string;
			renderer: RendererInstance | null;
		}> = [];
		for (const [field, value] of updates) {
			updatePayload.push({ field, value });
			restartTargets.push({
				field,
				renderer: restartableRendererForField(field),
			});
		}
		try {
			const setting = await invoke<AvatarSetting>("update_avatar_setting_values", {
				settingId: previousSetting.id,
				updates: updatePayload,
			});
			message = "Updated avatar setting";
			replaceAvatarSetting(setting);
			await applyRuntimeProfileUpdates(updates, setting, previousSetting);
			for (const { field, renderer } of restartTargets) {
				queueRendererRestart(renderer, field);
			}
		} catch (error) {
			message = String(error);
		}
	}

	function applyRenderQualityRecommendation(level: RenderQualityRecommendation): Promise<void> {
		return applyProfileUpdates(RENDER_QUALITY_RECOMMENDATIONS[level]);
	}

	function applyLookRecommendation(look: LookRecommendation): Promise<void> {
		return applyProfileUpdates(LOOK_RECOMMENDATIONS[look]);
	}

	function applyCameraTargetPreset(kind: CameraTargetPreset): Promise<void> {
		return applyProfileUpdates(CAMERA_TARGET_PRESETS[kind]);
	}

	function applyCameraOrbitPreset(kind: CameraOrbitPreset): Promise<void> {
		const preset = cameraOrbitPresetAngles(kind);
		return applyProfileUpdates([
			["camera.longitude_deg", preset.longitude],
			["camera.latitude_deg", preset.latitude],
		]);
	}

	function applyCameraLensPreset(focalLengthMm: CameraLensPreset): Promise<void> {
		return applyProfileUpdates([["camera.diagonal_fov_deg", diagonalFovFromLensMm(focalLengthMm)]]);
	}

	function applySpoutResolutionPreset(kind: SpoutResolutionPreset): Promise<void> {
		return applyProfileUpdates(SPOUT_RESOLUTION_PRESETS[kind]);
	}

	function applyOutputModePreset(kind: OutputModePreset): Promise<void> {
		return applyProfileUpdates(OUTPUT_MODE_PRESETS[kind]);
	}

	function applyPreviewWindowPreset(kind: PreviewWindowPreset): Promise<void> {
		return applyProfileUpdates(PREVIEW_WINDOW_PRESETS[kind]);
	}

	function runningCountForSetting(setting: AvatarSetting): number {
		return renderersForSetting(setting).length;
	}

	let logsTextFilter = $state("");
	let logsRendererFilter = $state<RendererLogFilter>("all");
	let logsAutoscroll = $state(true);
	let logsViewRef: HTMLElement | null = $state(null);

	function filteredRendererLogLines(): string[] {
		return filterRendererLogLines(renderers, logsTextFilter, logsRendererFilter);
	}

	function rendererLogText(): string {
		return rendererLogTextFromLines(filteredRendererLogLines());
	}

	function jumpToRendererLog(renderer: RendererInstance): void {
		selectedRendererId = renderer.id;
		logsRendererFilter = renderer.id;
		logsTextFilter = "";
		rendererLogsLayout = "unified";
		activeTab = "logs";
		queueMicrotask(scrollLogsToBottom);
	}

	function scrollLogsToBottom(): void {
		if (!logsViewRef) return;
		logsViewRef.scrollTop = logsViewRef.scrollHeight;
	}

	let rendererLogsLayout = $state<"per-renderer" | "unified">("per-renderer");
	let rendererLogsCopyFlash = $state(false);
	let rendererLogsCopyFlashTimer: ReturnType<typeof setTimeout> | null = null;
	let rendererLogsExpanded = $state<Record<number, boolean>>({});
	function isRendererLogExpanded(renderer: RendererInstance): boolean {
		const explicit = rendererLogsExpanded[renderer.id];
		if (explicit !== undefined) return explicit;
		return defaultRendererLogExpanded(renderer);
	}
	function toggleRendererLogExpanded(renderer: RendererInstance): void {
		rendererLogsExpanded[renderer.id] = !isRendererLogExpanded(renderer);
	}

	async function copyRendererLog(renderer: RendererInstance): Promise<void> {
		try {
			await navigator.clipboard.writeText(renderer.stderr_tail.join("\n"));
		} catch (error) {
			message = `clipboard write failed: ${String(error)}`;
		}
	}

	async function copyAllRendererLogs(): Promise<void> {
		const text = rendererLogText();
		try {
			await navigator.clipboard.writeText(text);
			rendererLogsCopyFlash = true;
			if (rendererLogsCopyFlashTimer) clearTimeout(rendererLogsCopyFlashTimer);
			rendererLogsCopyFlashTimer = setTimeout(() => {
				rendererLogsCopyFlash = false;
			}, 1500);
		} catch (error) {
			message = `clipboard write failed: ${String(error)}`;
		}
	}

	async function saveAllRendererLogs(): Promise<void> {
		const text = rendererLogText();
		if (hasTauriRuntime()) {
			try {
				const path = await invoke<string>("save_supervisor_logs", {
					content: text,
					filePrefix: "un-avatar-supervisor",
				});
				message = `Saved ${basename(path)}`;
			} catch (error) {
				message = String(error);
			}
			return;
		}
		browserDownloadText(text, `un-avatar-supervisor-logs-${filenameTimestamp()}.txt`);
	}

	async function revealSupervisorLogsDir(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = "Browser preview: open folder requires Tauri";
			return;
		}
		try {
			await invoke("reveal_supervisor_logs_dir");
		} catch (error) {
			message = String(error);
		}
	}

	function notificationTime(notification: AppNotification): string {
		return formatClockTimeFromUnixSecs(notification.created_at_secs);
	}

	async function toggleDiagnosticsCompare(entry: DiagnosticsExportEntry): Promise<void> {
		if (diagnosticsComparePaths.includes(entry.path)) {
			diagnosticsComparePaths = diagnosticsComparePaths.filter((path) => path !== entry.path);
			return;
		}
		if (hasTauriRuntime() && !diagnosticsCompareTexts[entry.path]) {
			try {
				diagnosticsCompareTexts = {
					...diagnosticsCompareTexts,
					[entry.path]: await invoke<string>("read_diagnostics_export", {
						path: entry.path,
					}),
				};
			} catch (error) {
				message = String(error);
				return;
			}
		}
		diagnosticsComparePaths = [...diagnosticsComparePaths.slice(-1), entry.path];
	}

	function isDiagnosticsCompared(entry: DiagnosticsExportEntry): boolean {
		return diagnosticsComparePaths.includes(entry.path);
	}

	function rendererStateLabel(state: RendererState): string {
		if (state === "Running") return $_("profiles.live.running");
		return state;
	}

	function selectedRendererById(id: number | null): RendererInstance | null {
		return id == null ? null : (rendererTableById.get(id) ?? null);
	}

	function avatarSettingBySelectedId(id: string | null): AvatarSetting | null {
		return id == null ? null : (avatarSettingById.get(id) ?? null);
	}

	function isLiveRenderer(renderer: RendererInstance): boolean {
		return renderer.pid != null && renderer.state !== "Exited" && renderer.state !== "Crashed";
	}

	function renderersForSetting(setting: AvatarSetting | null): RendererInstance[] {
		if (!setting) return [];
		return liveRenderersByManifestPath.get(normalizedPathKey(setting.manifest_path)) ?? [];
	}

	function rendererForSetting(setting: AvatarSetting | null): RendererInstance | null {
		return renderersForSetting(setting)[0] ?? null;
	}

	function settingForRenderer(renderer: RendererInstance | null): AvatarSetting | null {
		if (!renderer?.manifest_path) return null;
		return settingForManifestPath(renderer.manifest_path);
	}

	function settingForManifestPath(manifestPath: string | null | undefined): AvatarSetting | null {
		if (!manifestPath) return null;
		return avatarSettingByManifestPath.get(normalizedPathKey(manifestPath)) ?? null;
	}

	function syncSelectionFromRendererActivation(statuses: RendererRuntimeStatus[], instances: RendererInstance[]): void {
		let activated: RendererRuntimeStatus | null = null;
		for (const status of statuses) {
			const previous = rendererActivationSeq.get(status.id) ?? 0;
			rendererActivationSeq.set(status.id, status.window_activation_seq);
			if (
				status.window_focused &&
				status.window_activation_seq > previous &&
				(!activated || status.window_activation_seq > activated.window_activation_seq)
			) {
				activated = status;
			}
		}
		if (!activated) return;
		const renderer = instances.find((item) => item.id === activated.id) ?? null;
		if (!renderer) return;
		selectedRendererId = renderer.id;
		const setting = settingForRenderer(renderer);
		if (setting) selectedSettingId = setting.id;
	}

	function restartableRendererForField(field: string): RendererInstance | null {
		if (canApplyWithoutRestart(field)) return null;
		return isLaunchTimeRendererField(field) ? rendererForSetting(selectedSetting) : null;
	}

	function queueRendererRestart(renderer: RendererInstance | null, field: string): void {
		if (!renderer) return;
		pendingRendererRestart = { renderer, fieldLabel: profileFieldLabel(field) };
		message = `${profileFieldLabel(field)} will apply after ${renderer.name} restarts`;
	}

	function queueTransparentEnableRestart(renderer: RendererInstance | null): void {
		if (!renderer) return;
		pendingRendererRestart = {
			renderer,
			fieldLabel: "Transparent background",
		};
		message = `Transparent background may require restarting ${renderer.name}`;
	}

	async function restartPendingRenderer(): Promise<void> {
		const pending = pendingRendererRestart;
		if (!pending) return;
		pendingRendererRestart = null;
		await restartRenderer(pending.renderer);
	}

	async function refreshAll(): Promise<void> {
		if (!hasTauriRuntime()) {
			renderers = [];
			runtimeStatuses = {};
			notifications = [];
			nativeNotificationStatus = null;
			diagnosticsExports = [];
			diagnosticsPreviewTitle = null;
			diagnosticsPreviewText = null;
			avatarSettings = browserPreviewSettings;
			selectedSettingId = pickInitialSelectedSettingId(
				selectedSettingId,
				appSettings.last_selected_setting_id,
				browserPreviewSettings
			);
			launchTargetId = pickInitialLaunchTargetId(launchTargetId, selectedSettingId, browserPreviewSettings);
			message = "Browser preview: Tauri commands are disabled";
			return;
		}
		try {
			const instances = await invoke<RendererInstance[]>("list_renderers");
			const [settings, appNotifications, nativeNotifications] = await Promise.all([
				invoke<AvatarSetting[]>("list_avatar_settings"),
				invoke<AppNotification[]>("list_app_notifications"),
				invoke<NativeNotificationStatus>("get_native_notification_status"),
			]);
			renderers = instances;
			avatarSettings = settings;
			await refreshRendererRuntimeStatuses(instances);
			notifications = appNotifications;
			nativeNotificationStatus = nativeNotifications;
			selectedRendererId ??= instances[0]?.id ?? null;
			selectedSettingId = pickInitialSelectedSettingId(selectedSettingId, appSettings.last_selected_setting_id, settings);
			launchTargetId = pickInitialLaunchTargetId(launchTargetId, selectedSettingId, settings);
		} catch (error) {
			message = String(error);
		}
	}

	async function refreshRendererRuntimeStatuses(instances: RendererInstance[] = renderers): Promise<void> {
		if (!hasTauriRuntime()) return;
		if (instances.length === 0) {
			runtimeStatuses = {};
			runtimeStatusSamples.clear();
			rendererActivationSeq.clear();
			return;
		}
		const statuses = await Promise.all(
			instances.map((renderer) =>
				invoke<RendererRuntimeStatus>("get_renderer_runtime_status", {
					id: renderer.id,
				})
			)
		);
		const now = performance.now();
		const liveIds = new Set<number>();
		for (const renderer of instances) {
			liveIds.add(renderer.id);
		}
		for (const id of runtimeStatusSamples.keys()) {
			if (!liveIds.has(id)) runtimeStatusSamples.delete(id);
		}
		for (const id of rendererActivationSeq.keys()) {
			if (!liveIds.has(id)) rendererActivationSeq.delete(id);
		}
		for (const status of statuses) {
			const prev = runtimeStatusSamples.get(status.id);
			if (prev) {
				const elapsed = Math.max(0.001, (now - prev.at) / 1000);
				status.unmotion_zenoh_received_fps = Math.max(0, (status.unmotion_zenoh_received_frames - prev.unmotionRecv) / elapsed);
				status.motion_applied_fps = Math.max(0, (status.motion_applied_frames - prev.motionApply) / elapsed);
			}
			runtimeStatusSamples.set(status.id, {
				at: now,
				unmotionRecv: status.unmotion_zenoh_received_frames,
				motionApply: status.motion_applied_frames,
			});
		}
		const nextRuntimeStatuses: Record<number, RendererRuntimeStatus> = {};
		for (const status of statuses) {
			nextRuntimeStatuses[status.id] = status;
		}
		runtimeStatuses = nextRuntimeStatuses;
		syncSelectionFromRendererActivation(statuses, instances);
	}

	async function refreshRendererRuntimeView(): Promise<void> {
		if (!hasTauriRuntime()) return;
		if (runtimeRefreshBusy) {
			runtimeRefreshPending = true;
			return;
		}
		runtimeRefreshBusy = true;
		try {
			runtimeRefreshPending = false;
			const instances = await invoke<RendererInstance[]>("list_renderers");
			renderers = instances;
			await refreshRendererRuntimeStatuses(instances);
			notifications = await invoke<AppNotification[]>("list_app_notifications");
			if (!instances.some((renderer) => renderer.id === selectedRendererId)) {
				selectedRendererId = instances[0]?.id ?? null;
			}
		} catch (error) {
			message = String(error);
		} finally {
			runtimeRefreshBusy = false;
			if (runtimeRefreshPending) {
				void refreshRendererRuntimeView();
			}
		}
	}

	async function applyRendererCommand(
		targetRenderers: RendererInstance[],
		command: string,
		payload: (renderer: RendererInstance) => Record<string, unknown> | Promise<Record<string, unknown>>,
		logLabel = command
	): Promise<number> {
		let applied = 0;
		await Promise.all(
			targetRenderers.map(async (renderer) => {
				try {
					await invoke(command, { id: renderer.id, ...(await payload(renderer)) });
					applied += 1;
				} catch (error) {
					console.warn(logLabel, error);
				}
			})
		);
		return applied;
	}

	async function clearNotifications(): Promise<void> {
		if (!hasTauriRuntime()) {
			notifications = [];
			return;
		}
		try {
			await invoke("clear_app_notifications");
			notifications = [];
			message = "Notifications cleared";
		} catch (error) {
			message = String(error);
		}
	}

	async function sendTestNativeNotification(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = "Browser preview: native notifications require Tauri";
			return;
		}
		busy = true;
		try {
			await invoke("send_test_native_notification");
			nativeNotificationStatus = await invoke<NativeNotificationStatus>("get_native_notification_status");
			message = "Native notification test sent";
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function exportDiagnostics(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = "Browser preview: diagnostics export requires Tauri";
			return;
		}
		try {
			const path = await invoke<string>("export_diagnostics");
			lastDiagnosticsPath = path;
			lastDiagnosticsArchivePath = null;
			diagnosticsExports = await invoke<DiagnosticsExportEntry[]>("list_diagnostics_exports");
			message = `Diagnostics exported to ${basename(path)}`;
		} catch (error) {
			message = String(error);
		}
	}

	async function compressLastDiagnostics(): Promise<void> {
		if (!lastDiagnosticsPath) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: diagnostics compression requires Tauri";
			return;
		}
		try {
			const path = await invoke<string>("compress_diagnostics", {
				path: lastDiagnosticsPath,
			});
			lastDiagnosticsArchivePath = path;
			diagnosticsExports = await invoke<DiagnosticsExportEntry[]>("list_diagnostics_exports");
			message = `Diagnostics archive created: ${basename(path)}`;
		} catch (error) {
			message = String(error);
		}
	}

	async function revealLastDiagnostics(): Promise<void> {
		await revealPath(lastDiagnosticsPath);
	}

	async function revealLastDiagnosticsArchive(): Promise<void> {
		await revealPath(lastDiagnosticsArchivePath);
	}

	async function compressDiagnosticsEntry(entry: DiagnosticsExportEntry): Promise<void> {
		if (!hasTauriRuntime()) {
			message = "Browser preview: diagnostics compression requires Tauri";
			return;
		}
		try {
			const path = await invoke<string>("compress_diagnostics", {
				path: entry.path,
			});
			lastDiagnosticsPath = entry.path;
			lastDiagnosticsArchivePath = path;
			diagnosticsExports = await invoke<DiagnosticsExportEntry[]>("list_diagnostics_exports");
			message = `Diagnostics archive created: ${basename(path)}`;
		} catch (error) {
			message = String(error);
		}
	}

	async function previewDiagnosticsEntry(entry: DiagnosticsExportEntry): Promise<void> {
		if (!hasTauriRuntime()) {
			message = "Browser preview: diagnostics preview requires Tauri";
			return;
		}
		try {
			diagnosticsPreviewText = await invoke<string>("read_diagnostics_export", {
				path: entry.path,
			});
			diagnosticsPreviewTitle = basename(entry.path);
			message = `Previewing ${basename(entry.path)}`;
		} catch (error) {
			message = String(error);
		}
	}

	async function launchSetting(id: string | null, allowGroupTarget = true): Promise<void> {
		if (!id && (!allowGroupTarget || !launchGroupName)) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: launch requires Tauri";
			return;
		}
		busy = true;
		try {
			if (allowGroupTarget && launchGroupName) {
				if (launchGroupSettings.length === 0) {
					throw new Error(`No profiles in group: ${launchGroupName}`);
				}
				let lastInstance: RendererInstance | null = null;
				for (const setting of launchGroupSettings) {
					lastInstance = await invoke<RendererInstance>("launch_renderer", {
						settingId: setting.id,
					});
				}
				if (lastInstance) selectedRendererId = lastInstance.id;
				message = `Opened ${launchGroupSettings.length} renderers`;
				await refreshAll();
				if (appSettings.jump_to_renderers_on_quick_run) {
					activeTab = "renderers";
				}
				return;
			}

			const instance = await invoke<RendererInstance>("launch_renderer", {
				settingId: id,
			});
			selectedRendererId = instance.id;
			message = `Opened ${instance.name}`;
			await refreshAll();
			if (appSettings.jump_to_renderers_on_quick_run) {
				activeTab = "renderers";
			}
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function prewarmSceneCache(settingId: string): Promise<void> {
		if (!hasTauriRuntime()) {
			message = "Browser preview: cache warmup requires Tauri";
			return;
		}
		busy = true;
		try {
			message = "Warming renderer cache...";
			const result = await invoke<string>("prewarm_renderer_scene_cache", { settingId });
			message = result;
			avatarSettings = await invoke<AvatarSetting[]>("list_avatar_settings");
			notifications = await invoke<AppNotification[]>("list_app_notifications");
		} catch (error) {
			message = String(error);
			notifications = await invoke<AppNotification[]>("list_app_notifications");
		} finally {
			busy = false;
		}
	}

	async function createDesktopShortcut(settingId: string): Promise<void> {
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.desktop_shortcut_requires_tauri");
			return;
		}
		busy = true;
		try {
			const path = await invoke<string>("create_renderer_desktop_shortcut", { settingId });
			message = $_("profiles.messages.desktop_shortcut_created", { values: { path } });
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function createTaskbarLauncher(settingId: string): Promise<void> {
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.taskbar_launcher_requires_tauri");
			return;
		}
		busy = true;
		try {
			const path = await invoke<string>("create_taskbar_launcher_shortcuts", { settingId });
			message = $_("profiles.messages.taskbar_launcher_created", { values: { path } });
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function maybeAutoLaunchSelectedOnStartup(): Promise<void> {
		if (startupAutoLaunchAttempted) return;
		startupAutoLaunchAttempted = true;
		if (!appSettings.auto_launch_selected_on_startup) return;
		if (!isValidLaunchTarget(launchTargetId, avatarSettings)) return;
		if (visibleRenderers.length > 0) return;
		activeTab = "renderers";
		await launchSetting(launchTargetSetting?.id ?? null);
	}

	async function duplicateSetting(id: string | null): Promise<void> {
		if (!id) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: duplicate requires Tauri";
			return;
		}
		busy = true;
		try {
			const setting = await invoke<AvatarSetting>("duplicate_avatar_setting", {
				settingId: id,
			});
			message = `Duplicated ${setting.name}`;
			await refreshAll();
			selectedSettingId = setting.id;
			activeTab = "settings";
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function newSetting(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = "Browser preview: new setting requires Tauri";
			return;
		}
		busy = true;
		try {
			const setting = await invoke<AvatarSetting>("new_avatar_setting");
			message = `Created ${setting.name}`;
			await refreshAll();
			selectedSettingId = setting.id;
			activeTab = "settings";
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function deleteSetting(id: string | null): Promise<void> {
		if (!id) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: delete requires Tauri";
			return;
		}
		const setting = avatarSettingBySelectedId(id);
		if (!setting) return;
		busy = true;
		try {
			await invoke("delete_avatar_setting", { settingId: id });
			message = `Deleted ${setting.name}`;
			selectedSettingId = null;
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	function startDeleteHold(id: string | null): void {
		if (!id || busy) return;
		cancelDeleteHold();
		deleteHoldTargetId = id;
		deleteHoldProgress = 0;
		deleteHoldStartedAt = performance.now();
		const tick = () => {
			if (deleteHoldTargetId !== id) return;
			deleteHoldProgress = Math.min(1, (performance.now() - deleteHoldStartedAt) / deleteHoldDurationMs);
			if (deleteHoldProgress >= 1) {
				cancelDeleteHold();
				void deleteSetting(id);
				return;
			}
			deleteHoldTimer = window.requestAnimationFrame(tick);
		};
		deleteHoldTimer = window.requestAnimationFrame(tick);
	}

	function cancelDeleteHold(): void {
		if (deleteHoldTimer != null) {
			window.cancelAnimationFrame(deleteHoldTimer);
			deleteHoldTimer = null;
		}
		deleteHoldTargetId = null;
		deleteHoldProgress = 0;
	}

	async function revealPath(path: string | null): Promise<void> {
		if (!path) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: reveal requires Tauri";
			return;
		}
		try {
			await invoke("reveal_path", { path });
			message = `Revealed ${basename(path)}`;
		} catch (error) {
			message = String(error);
		}
	}

	async function revealProfilesDir(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = "Browser preview: open folder requires Tauri";
			return;
		}
		try {
			await invoke("reveal_profiles_dir");
			message = "Opened profiles folder";
		} catch (error) {
			message = String(error);
		}
	}

	function previewSettingReorder(sourceId: string, insertIndex: number): void {
		const sourceIndex = avatarSettings.findIndex((setting) => setting.id === sourceId);
		if (sourceIndex < 0) return;
		insertIndex = Math.max(0, Math.min(insertIndex, avatarSettings.length));
		if (sourceIndex < insertIndex) insertIndex -= 1;
		if (sourceIndex === insertIndex) return;
		const next = [...avatarSettings];
		const [moved] = next.splice(sourceIndex, 1);
		next.splice(insertIndex, 0, moved);
		avatarSettings = next;
	}

	async function saveSettingsOrder(): Promise<void> {
		try {
			await invoke<AvatarSetting[]>("reorder_avatar_settings", {
				settingIds: avatarSettings.map((setting) => setting.id),
			});
			message = "Reordered profiles";
		} catch (error) {
			message = String(error);
			await refreshAll();
		}
	}

	function beginSettingPointerDrag(event: PointerEvent, settingId: string): void {
		if (busy || event.button !== 0) return;
		const card = (event.currentTarget as HTMLElement).closest<HTMLElement>("[data-profile-id]");
		const rect = card?.getBoundingClientRect();
		if (!rect) return;
		settingPointerDrag = {
			id: settingId,
			startX: event.clientX,
			startY: event.clientY,
			currentX: event.clientX,
			currentY: event.clientY,
			offsetX: event.clientX - rect.left,
			offsetY: event.clientY - rect.top,
			width: rect.width,
			height: rect.height,
			active: false,
		};
		window.addEventListener("pointermove", updateSettingPointerDrag);
		window.addEventListener("pointerup", finishSettingPointerDrag);
		window.addEventListener("pointercancel", cancelSettingPointerDrag);
		document.documentElement.classList.add("profile-dragging");
		event.preventDefault();
		event.stopPropagation();
	}

	function removeSettingDragListeners(): void {
		window.removeEventListener("pointermove", updateSettingPointerDrag);
		window.removeEventListener("pointerup", finishSettingPointerDrag);
		window.removeEventListener("pointercancel", cancelSettingPointerDrag);
		document.documentElement.classList.remove("profile-dragging");
	}

	function cancelSettingPointerDrag(): void {
		removeSettingDragListeners();
		settingPointerDrag = null;
		draggedSettingId = null;
	}

	function updateSettingPointerDrag(event: PointerEvent): void {
		const drag = settingPointerDrag;
		if (!drag) return;
		const distance = Math.abs(event.clientX - drag.startX) + Math.abs(event.clientY - drag.startY);
		const active = drag.active || distance > 5;
		settingPointerDrag = { ...drag, currentX: event.clientX, currentY: event.clientY, active };
		if (active && !drag.active) {
			draggedSettingId = drag.id;
			suppressSettingClick = true;
		}
		if (!active) return;
		event.preventDefault();
		event.stopPropagation();
		scrollSettingListDuringDrag(event.clientY);
		const cards = Array.from(document.querySelectorAll<HTMLElement>("[data-profile-id]"));
		const insertIndex = cards.findIndex((card) => {
			if (card.dataset.profileId === drag.id) return false;
			const rect = card.getBoundingClientRect();
			return event.clientY < rect.top + rect.height / 2;
		});
		previewSettingReorder(drag.id, insertIndex < 0 ? avatarSettings.length : insertIndex);
	}

	function scrollSettingListDuringDrag(clientY: number): void {
		const list = document.querySelector<HTMLElement>(".setting-list");
		if (!list) return;
		const rect = list.getBoundingClientRect();
		const edge = 56;
		if (clientY < rect.top + edge) {
			list.scrollBy({ top: -10, behavior: "auto" });
		} else if (clientY > rect.bottom - edge) {
			list.scrollBy({ top: 10, behavior: "auto" });
		}
	}

	function finishSettingPointerDrag(event: PointerEvent): void {
		const drag = settingPointerDrag;
		removeSettingDragListeners();
		settingPointerDrag = null;
		draggedSettingId = null;
		if (!drag?.active) return;
		event.preventDefault();
		event.stopPropagation();
		void saveSettingsOrder();
	}

	async function browseSettingPath(field: "avatar_path" | "icon_path", kind: "avatar" | "icon"): Promise<void> {
		const targetSetting = selectedSetting;
		if (!targetSetting) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: file picker requires Tauri";
			return;
		}
		const rendererToRestart = restartableRendererForField(field);
		busy = true;
		try {
			const path = await invoke<string | null>("pick_file_path", { kind });
			if (!path) {
				message = $_("profiles.messages.file_selection_canceled");
				return;
			}
			if (field === "avatar_path") {
				await requestAvatarPathUpdate(path, rendererToRestart, targetSetting);
				return;
			}
			const setting = await invoke<AvatarSetting>("update_avatar_setting_path", {
				settingId: targetSetting.id,
				field,
				path,
			});
			message = $_("profiles.messages.updated_icon_path");
			replaceAvatarSetting(setting);
			queueRendererRestart(rendererToRestart, field);
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function requestAvatarPathUpdate(
		path: string,
		rendererToRestart: RendererInstance | null,
		targetSetting: AvatarSetting | null = selectedSetting
	): Promise<void> {
		if (!targetSetting) return;
		if (!path.trim()) {
			await saveAvatarPath(path, rendererToRestart, targetSetting.id);
			return;
		}
		if (looksLikeUnavatarPath(path)) {
			const metadata = await readUnavatarMetadataForPath(path, targetSetting);
			if (!metadata) {
				message = "Selected avatar has no UNAvatar metadata";
				return;
			}
			unavatarProfileIconCrop = {
				enabled: Boolean(metadata.preview_images.length || metadata.preview_sets.some((set) => set.preview_images.length)),
				imageDataUrl: metadata.preview_images[0]?.data_url ?? metadata.preview_sets[0]?.preview_images[0]?.data_url ?? null,
				zoom: 1,
				offsetX: 0,
				offsetY: 0,
			};
			unavatarMetadataModal = {
				metadata,
				pendingPath: path,
				settingId: targetSetting.id,
				rendererToRestart,
			};
			message = $_("unavatar_rights.title");
			return;
		}
		const metadata = await readVrmMetadataForPath(path, targetSetting);
		if (metadata) {
			vrmMetadataModal = {
				metadata,
				pendingPath: path,
				settingId: targetSetting.id,
				rendererToRestart,
			};
			useThumbnailForProfileIconOnAccept = Boolean(metadata.thumbnail_data_url);
			message = $_("vrm_metadata.messages.review_before_use");
			return;
		}
		await saveAvatarPath(path, rendererToRestart, targetSetting.id);
	}

	async function saveAvatarPath(
		path: string,
		rendererToRestart: RendererInstance | null,
		settingId: string | null = selectedSetting?.id ?? null
	): Promise<AvatarSetting | null> {
		if (!settingId) return null;
		const setting = await invoke<AvatarSetting>("update_avatar_setting_value", {
			settingId,
			field: "avatar_path",
			value: path,
		});
		message = $_("profiles.messages.updated_avatar_path");
		replaceAvatarSetting(setting);
		queueRendererRestart(rendererToRestart, "avatar_path");
		return setting;
	}

	async function acceptVrmMetadataAndUse(): Promise<void> {
		const modal = vrmMetadataModal;
		if (!modal?.pendingPath) return;
		vrmMetadataModal = null;
		busy = true;
		try {
			const savedSetting = await saveAvatarPath(modal.pendingPath, modal.rendererToRestart, modal.settingId);
			if (useThumbnailForProfileIconOnAccept && modal.metadata.thumbnail_data_url) {
				await applySelectedAvatarThumbnail({
					rendererToRestart: modal.rendererToRestart,
					settingId: savedSetting?.id ?? savedSetting?.manifest_path ?? modal.settingId,
					avatarPath: modal.pendingPath,
					manageBusy: false,
				});
			} else {
				message = $_("vrm_metadata.messages.updated_after_confirmation");
			}
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function acceptUnavatarMetadataAndUse(): Promise<void> {
		const modal = unavatarMetadataModal;
		if (!modal?.pendingPath) return;
		unavatarMetadataModal = null;
		busy = true;
		try {
			const savedSetting = await saveAvatarPath(modal.pendingPath, modal.rendererToRestart, modal.settingId);
			if (unavatarProfileIconCrop.enabled && unavatarProfileIconCrop.imageDataUrl) {
				const setting = await invoke<AvatarSetting>("save_profile_icon_from_data_url", {
					settingId: savedSetting?.id ?? savedSetting?.manifest_path ?? modal.settingId,
					imageDataUrl: unavatarProfileIconCrop.imageDataUrl,
					crop: {
						zoom: Number(unavatarProfileIconCrop.zoom) || 1,
						offset_x: Number(unavatarProfileIconCrop.offsetX) || 0,
						offset_y: Number(unavatarProfileIconCrop.offsetY) || 0,
					},
				});
				replaceAvatarSetting(setting);
				bumpProfileIconRevision(setting.icon_path);
				queueRendererRestart(modal.rendererToRestart, "icon_path");
			}
			message = $_("vrm_metadata.messages.updated_after_confirmation");
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function applySelectedAvatarThumbnail(
		options: {
			rendererToRestart?: RendererInstance | null;
			settingId?: string | null;
			avatarPath?: string | null;
			manageBusy?: boolean;
		} = {}
	): Promise<void> {
		const settingId = options.settingId ?? selectedSetting?.manifest_path ?? selectedSetting?.id ?? null;
		const avatarPath = options.avatarPath ?? selectedSetting?.avatar_path ?? null;
		const rendererToRestart = options.rendererToRestart ?? restartableRendererForField("icon_path");
		const manageBusy = options.manageBusy ?? true;
		if (!settingId) return;
		if (!avatarPath?.trim()) {
			message = $_("profiles.messages.avatar_path_required_for_thumbnail");
			return;
		}
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.thumbnail_requires_tauri");
			return;
		}
		if (manageBusy) busy = true;
		try {
			message = $_("profiles.messages.saving_thumbnail_icon");
			const setting = await invoke<AvatarSetting>("save_avatar_thumbnail_icon", {
				settingId,
				avatarPath,
			});
			replaceAvatarSetting(setting);
			bumpProfileIconRevision(setting.icon_path);
			queueRendererRestart(rendererToRestart, "icon_path");
			message = $_("profiles.messages.updated_thumbnail_icon");
		} catch (error) {
			message = String(error);
			if (!manageBusy) throw error;
		} finally {
			if (manageBusy) busy = false;
		}
	}

	function closeVrmMetadataModal(): void {
		const wasPending = Boolean(vrmMetadataModal?.pendingPath);
		vrmMetadataModal = null;
		if (wasPending) {
			message = $_("vrm_metadata.messages.avatar_selection_canceled");
		}
	}

	function closeUnavatarMetadataModal(): void {
		const wasPending = Boolean(unavatarMetadataModal?.pendingPath);
		unavatarMetadataModal = null;
		if (wasPending) {
			message = $_("vrm_metadata.messages.avatar_selection_canceled");
		}
	}

	async function reviewSelectedVrmMetadata(): Promise<void> {
		if (!selectedSetting?.avatar_path) return;
		if (!hasTauriRuntime()) {
			message = $_("vrm_metadata.messages.tauri_required");
			return;
		}
		busy = true;
		try {
			if (looksLikeUnavatarPath(selectedSetting.avatar_path)) {
				const metadata = await readUnavatarMetadataForPath(selectedSetting.avatar_path, selectedSetting);
				if (!metadata) {
					message = "Selected avatar has no UNAvatar metadata";
					return;
				}
				unavatarProfileIconCrop = {
					enabled: false,
					imageDataUrl: metadata.preview_images[0]?.data_url ?? metadata.preview_sets[0]?.preview_images[0]?.data_url ?? null,
					zoom: 1,
					offsetX: 0,
					offsetY: 0,
				};
				unavatarMetadataModal = {
					metadata,
					pendingPath: null,
					settingId: selectedSetting.id,
					rendererToRestart: null,
				};
				return;
			}
			const metadata = await readVrmMetadataForPath(selectedSetting.avatar_path, selectedSetting);
			if (!metadata) {
				message = $_("vrm_metadata.messages.no_metadata");
				return;
			}
			vrmMetadataModal = {
				metadata,
				pendingPath: null,
				settingId: selectedSetting.id,
				rendererToRestart: null,
			};
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function setAppliedMessage(applied: number, singleMessage: string, multiMessage: (applied: number) => string): Promise<void> {
		if (applied <= 0) return;
		await refreshRendererRuntimeViewAfterApply();
		message = applied === 1 ? singleMessage : multiMessage(applied);
	}

	let runtimeApplyRefreshDepth = 0;
	let runtimeApplyRefreshPending = false;

	async function withDeferredRuntimeRefresh(callback: () => Promise<void>): Promise<void> {
		runtimeApplyRefreshDepth += 1;
		try {
			await callback();
		} finally {
			runtimeApplyRefreshDepth -= 1;
			if (runtimeApplyRefreshDepth === 0 && runtimeApplyRefreshPending) {
				runtimeApplyRefreshPending = false;
				await refreshRendererRuntimeView();
			}
		}
	}

	async function refreshRendererRuntimeViewAfterApply(): Promise<void> {
		if (runtimeApplyRefreshDepth > 0) {
			runtimeApplyRefreshPending = true;
			return;
		}
		await refreshRendererRuntimeView();
	}

	async function applyRuntimeWindowUpdate(
		setting: AvatarSetting,
		renderer: RendererInstance | null,
		shouldQueueTransparentRestart: boolean
	): Promise<void> {
		if (!renderer) return;
		await invoke("set_renderer_window", {
			id: renderer.id,
			...rendererWindowPayload(setting),
		});
		message = "Updated renderer window";
		await refreshRendererRuntimeViewAfterApply();
		if (shouldQueueTransparentRestart) {
			queueTransparentEnableRestart(renderer);
		}
	}

	async function applyRuntimeBackgroundColor(
		fields: readonly string[],
		setting: AvatarSetting,
		renderersToApply: RendererInstance[]
	): Promise<void> {
		if (!fieldSetIncludes(fields, "window.background_color")) return;
		const applied = await applyRendererCommand(renderersToApply, "set_renderer_clear_color", () => rendererClearColorPayload(setting));
		await setAppliedMessage(applied, "Updated background color", (count) => `Updated background color on ${count} renderers`);
	}

	async function applyRuntimeMotionUpdate(
		fields: readonly string[],
		setting: AvatarSetting,
		renderersToApply: RendererInstance[]
	): Promise<void> {
		if (fieldSetIncludes(fields, "motion.apply_vmc_root_translation")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_apply_vmc_root_translation", () => ({
				enabled: setting.apply_vmc_root_translation,
			}));
			if (applied > 0) await refreshRendererRuntimeViewAfterApply();
			return;
		}
		if (fieldSetIncludesAny(fields, motionLookAtFields)) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_look_at", () => rendererLookAtPayload(setting));
			await setAppliedMessage(applied, "Updated LookAt", (count) => `Updated LookAt on ${count} renderers`);
			return;
		}
		if (fieldSetIncludesAny(fields, motionReceiverFields)) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_motion_receivers", () =>
				rendererMotionReceiversPayload(setting)
			);
			await setAppliedMessage(applied, "Updated motion receivers", (count) => `Updated motion receivers on ${count} renderers`);
		}
	}

	async function applyRuntimePhysicsUpdate(
		fields: readonly string[],
		setting: AvatarSetting,
		renderersToApply: RendererInstance[]
	): Promise<void> {
		if (fieldSetIncludes(fields, DYNAMICS_ENABLE_ALL_ON_LAUNCH_FIELD)) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_all_dynamics_launch_setting", () => ({ setting }));
			await setAppliedMessage(applied, "Updated dynamics override", (count) => `Updated dynamics override on ${count} renderers`);
			return;
		}
		if (
			fieldSetIncludes(fields, DYNAMICS_ENABLED_FIELD) ||
			fieldSetStartsWith(fields, DYNAMICS_OVERRIDE_FIELD_PREFIX) ||
			fieldSetStartsWith(fields, DYNAMICS_BONE_COLLIDER_FIELD_PREFIX)
		) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_dynamics", () => ({ setting }));
			await setAppliedMessage(applied, "Updated motion physics", (count) => `Updated motion physics on ${count} renderers`);
			return;
		}
		if (fieldSetIncludes(fields, "debug.show_bone_colliders")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_show_bone_colliders", () => ({
				enabled: setting.show_bone_colliders,
			}));
			await setAppliedMessage(applied, "Updated collider display", (count) => `Updated collider display on ${count} renderers`);
		}
	}

	async function applyRuntimeAvatarEffectsUpdate(
		fields: readonly string[],
		setting: AvatarSetting,
		renderersToApply: RendererInstance[]
	): Promise<void> {
		if (fieldSetStartsWith(fields, "effects.avatar.outline.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_avatar_outline", () =>
				rendererAvatarOutlinePayload(setting)
			);
			await setAppliedMessage(applied, "Updated avatar effects", (count) => `Updated avatar effects on ${count} renderers`);
			return;
		}
		if (fieldSetStartsWith(fields, "effects.avatar.contact_shadow.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_contact_shadow", () =>
				rendererContactShadowPayload(setting)
			);
			await setAppliedMessage(applied, "Updated avatar effects", (count) => `Updated avatar effects on ${count} renderers`);
		}
	}

	async function applyRuntimePostEffectsUpdate(
		fields: readonly string[],
		setting: AvatarSetting,
		renderersToApply: RendererInstance[]
	): Promise<void> {
		if (fieldSetStartsWith(fields, "effects.post.ssao.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_ssao", () => rendererSsaoPayload(setting));
			await setAppliedMessage(applied, "Updated avatar effects", (count) => `Updated avatar effects on ${count} renderers`);
			return;
		}
		if (fieldSetStartsWith(fields, "effects.post.bloom.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_bloom", () => rendererBloomPayload(setting));
			await setAppliedMessage(applied, "Updated bloom", (count) => `Updated bloom on ${count} renderers`);
		}
	}

	async function applyRuntimeEnvironmentUpdate(
		fields: readonly string[],
		setting: AvatarSetting,
		renderersToApply: RendererInstance[]
	): Promise<void> {
		if (fieldSetStartsWith(fields, "environment.color.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_environment_color", () =>
				rendererEnvironmentColorPayload(setting)
			);
			await setAppliedMessage(applied, "Updated color adjustment", (count) => `Updated color adjustment on ${count} renderers`);
			return;
		}
		if (fieldSetStartsWith(fields, "environment.lighting.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_lighting", () => rendererLightingPayload(setting));
			await setAppliedMessage(applied, "Updated lighting", (count) => `Updated lighting on ${count} renderers`);
		}
	}

	async function applyRuntimeCameraUpdate(
		fields: readonly string[],
		setting: AvatarSetting,
		renderersToApply: RendererInstance[]
	): Promise<void> {
		if (!fieldSetStartsWith(fields, "camera.")) return;
		const onlyCameraLock = fields.every((field) => field === "camera.locked");
		let applied = 0;
		await Promise.all(
			renderersToApply.map(async (renderer) => {
				try {
					await invoke("set_renderer_camera_lock", {
						id: renderer.id,
						locked: setting.camera_locked,
					});
					if (!onlyCameraLock) {
						await invoke("set_renderer_camera_state", {
							id: renderer.id,
							target: setting.camera_target,
							longitudeDeg: setting.camera_longitude_deg,
							latitudeDeg: setting.camera_latitude_deg,
							radius: setting.camera_radius,
							diagonalFovDeg: setting.camera_diagonal_fov_deg,
							transition: {
								duration_ms: 320,
								easing: "ease_out_cubic",
								mode: "replace",
							},
						});
					}
					applied += 1;
				} catch (error) {
					console.warn("set_renderer_camera_state", error);
				}
			})
		);
		await setAppliedMessage(applied, "Updated camera", (count) => `Updated camera on ${count} renderers`);
	}

	async function applyRuntimeProfileUpdates(
		updates: readonly ProfilePresetUpdate[],
		setting: AvatarSetting,
		previousSetting: AvatarSetting
	): Promise<void> {
		const fields: string[] = [];
		let enablingTransparent = false;
		for (const [field, value] of updates) {
			fields.push(field);
			if (field === "window.transparent" && value === true) {
				enablingTransparent = true;
			}
		}
		const rendererToApply = fieldSetHas(fields, isRuntimeWindowField) ? rendererForSetting(previousSetting) : null;
		const enablingTransparentOnOpaqueRenderer = enablingTransparent && rendererToApply !== null && !rendererToApply.transparent;
		const renderersToApply = renderersForSetting(previousSetting);

		await withDeferredRuntimeRefresh(async () => {
			await applyRuntimeWindowUpdate(setting, rendererToApply, enablingTransparentOnOpaqueRenderer);
			await applyRuntimeBackgroundColor(fields, setting, renderersToApply);
			await applyRuntimeMotionUpdate(fields, setting, renderersToApply);
			await applyRuntimePhysicsUpdate(fields, setting, renderersToApply);
			await applyRuntimeAvatarEffectsUpdate(fields, setting, renderersToApply);
			await applyRuntimePostEffectsUpdate(fields, setting, renderersToApply);
			await applyRuntimeEnvironmentUpdate(fields, setting, renderersToApply);
			await applyRuntimeCameraUpdate(fields, setting, renderersToApply);
		});
	}

	async function applyRuntimeProfileUpdate(
		field: string,
		value: ProfileSettingValue,
		setting: AvatarSetting,
		previousSetting: AvatarSetting
	): Promise<void> {
		await applyRuntimeProfileUpdates([[field, value]], setting, previousSetting);
	}

	async function updateSettingValue(field: string, value: ProfileSettingValue): Promise<void> {
		const targetSetting = selectedSetting;
		if (!targetSetting) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: setting changes require Tauri";
			return;
		}
		if (field === "avatar_path" && typeof value === "string") {
			busy = true;
			try {
				await requestAvatarPathUpdate(value, restartableRendererForField(field), targetSetting);
			} catch (error) {
				message = String(error);
			} finally {
				busy = false;
			}
			return;
		}

		const previousSetting = targetSetting;
		const rendererToRestart = restartableRendererForField(field);
		try {
			const setting = await invoke<AvatarSetting>("update_avatar_setting_value", {
				settingId: previousSetting.id,
				field,
				value,
			});
			message = "Updated avatar setting";
			replaceAvatarSetting(setting);
			await applyRuntimeProfileUpdate(field, value, setting, previousSetting);
			queueRendererRestart(rendererToRestart, field);
		} catch (error) {
			message = String(error);
		}
	}

	async function refreshWardrobeOptionsForSetting(setting: AvatarSetting | null): Promise<void> {
		const key = setting?.avatar_path ? `${setting.manifest_path}\n${setting.avatar_path}` : "";
		wardrobeOptionsKey = key;
		if (!key || !setting?.avatar_path || !hasTauriRuntime()) {
			wardrobeOptions = null;
			return;
		}
		try {
			const options = await invoke<UnavatarWardrobeOptions>("read_unavatar_wardrobe_options", {
				path: setting.avatar_path,
				manifestPath: setting.manifest_path,
			});
			if (wardrobeOptionsKey === key) {
				wardrobeOptions = options;
			}
		} catch (error) {
			if (wardrobeOptionsKey === key) {
				wardrobeOptions = {
					available: false,
					base_label: "Base",
					sets: [],
					error: String(error),
				};
			}
		}
	}

	function updateBackgroundColorValue(value: [number, number, number]): void {
		void updateSettingValue("window.background_color", value);
	}

	async function activateRenderer(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: activate requires Tauri";
			return;
		}
		try {
			await invoke("activate_renderer_window", { id: renderer.id });
			message = `Activated ${renderer.name}`;
		} catch (error) {
			message = String(error);
		}
	}

	async function restartRenderer(renderer: RendererInstance | null): Promise<void> {
		if (!renderer?.manifest_path) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: restart requires Tauri";
			return;
		}
		busy = true;
		try {
			await invoke("stop_renderer", { id: renderer.id });
			const instance = await invoke<RendererInstance>("launch_renderer", {
				settingId: renderer.manifest_path,
			});
			selectedRendererId = instance.id;
			message = `Restarted ${instance.name}`;
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function resetRendererCamera(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: reset view requires Tauri";
			return;
		}
		busy = true;
		try {
			await invoke("reset_renderer_camera", { id: renderer.id });
			message = `Reset view for ${renderer.name}`;
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function captureRendererScreenshot(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: screenshot requires Tauri";
			return;
		}
		busy = true;
		screenshotNoticePath = null;
		try {
			const path = await invoke<string>("capture_renderer_screenshot", {
				id: renderer.id,
				path: null,
			});
			screenshotNoticePath = path;
			message = "Screenshot saved";
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function revealScreenshotFolder(): Promise<void> {
		const folder = dirname(screenshotNoticePath);
		if (!folder) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: open folder requires Tauri";
			return;
		}
		try {
			await invoke("reveal_path", { path: folder });
			message = "Opened screenshots folder";
		} catch (error) {
			message = String(error);
		}
	}

	let expressionOverrides = $state<Record<number, Record<string, number>>>({});
	let expressionFilter = $state("");

	function setExpressionOverride(renderer: RendererInstance | null, name: string, weight: number): void {
		if (!renderer) return;
		const clamped = Math.min(1, Math.max(0, Number(weight) || 0));
		expressionOverrides = {
			...expressionOverrides,
			[renderer.id]: {
				...(expressionOverrides[renderer.id] ?? {}),
				[name]: clamped,
			},
		};
		if (!hasTauriRuntime()) return;
		// ドラッグ中の連続発火に追随できるよう fire-and-forget（disabled にはしない）。エラーは message にだけ反映。
		invoke("set_renderer_expression_override", {
			id: renderer.id,
			name,
			weight: clamped,
		}).catch((error: unknown) => {
			message = String(error);
		});
	}

	async function clearExpressionOverrides(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		expressionOverrides = { ...expressionOverrides, [renderer.id]: {} };
		if (!hasTauriRuntime()) return;
		try {
			await invoke("clear_renderer_expression_overrides", {
				id: renderer.id,
			});
			message = `Expression overrides cleared for ${renderer.name}`;
		} catch (error) {
			message = String(error);
		}
	}

	async function activateWardrobeMenuCandidate(
		renderer: RendererInstance | null,
		actionId: string,
		wardrobeSetId: string
	): Promise<void> {
		if (!renderer || !hasTauriRuntime()) return;
		try {
			await invoke("activate_renderer_runtime_action", {
				id: renderer.id,
				actionId,
			});
			message = `Activated wardrobe ${wardrobeSetId} for ${renderer.name}`;
		} catch (error) {
			message = String(error);
		}
	}

	async function setRendererClearColor(
		renderer: RendererInstance | null,
		color: { r: number; g: number; b: number; a: number },
		label: string
	): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: runtime color control requires Tauri";
			return;
		}
		busy = true;
		try {
			await invoke("set_renderer_clear_color", { id: renderer.id, ...color });
			message = `Set ${renderer.name} background to ${label}`;
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function setRendererSpoutOutput(
		renderer: RendererInstance | null,
		enabled: boolean,
		size: { width: number; height: number } | null = null,
		label = enabled ? "on" : "off"
	): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: runtime output control requires Tauri";
			return;
		}
		busy = true;
		try {
			await invoke("set_renderer_spout_output", {
				id: renderer.id,
				enabled,
				width: size?.width ?? null,
				height: size?.height ?? null,
			});
			await refreshRendererRuntimeView();
			const status = runtimeStatuses[renderer.id];
			if (enabled && status?.connected && !status.spout_enabled) {
				message = status.spout_available
					? `Spout output did not activate for ${renderer.name}`
					: "Spout2 SDK backend is not built into this renderer";
			} else {
				message = `${enabled ? "Set" : "Disabled"} Spout output ${label} for ${renderer.name}`;
			}
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function setRendererWindow(
		renderer: RendererInstance | null,
		patch: {
			decorations?: boolean;
			transparent?: boolean;
			inputPassthrough?: boolean;
			alwaysOnTop?: boolean;
			minimized?: boolean;
			width?: number;
			height?: number;
		},
		label: string
	): Promise<void> {
		if (!renderer) return;
		const enablingTransparentOnOpaqueRenderer = patch.transparent === true && !renderer.transparent;
		if (!hasTauriRuntime()) {
			message = "Browser preview: runtime window control requires Tauri";
			return;
		}
		busy = true;
		try {
			await invoke("set_renderer_window", {
				id: renderer.id,
				decorations: patch.decorations ?? null,
				transparent: patch.transparent ?? null,
				inputPassthrough: patch.inputPassthrough ?? null,
				alwaysOnTop: patch.alwaysOnTop ?? null,
				minimized: patch.minimized ?? null,
				width: patch.width ?? null,
				height: patch.height ?? null,
			});
			message = `Set ${renderer.name} window to ${label}`;
			await refreshRendererRuntimeView();
			if (enablingTransparentOnOpaqueRenderer) {
				queueTransparentEnableRestart(renderer);
			}
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function setRendererShowAxes(renderer: RendererInstance | null, enabled: boolean): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		try {
			await invoke("set_renderer_show_axes", { id: renderer.id, enabled });
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		}
	}

	async function setRendererShowBoneColliders(renderer: RendererInstance | null, enabled: boolean): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		try {
			await invoke("set_renderer_show_bone_colliders", {
				id: renderer.id,
				enabled,
			});
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		}
	}

	async function setRendererDynamicsEnabled(renderer: RendererInstance | null, sourceId: string, enabled: boolean): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		try {
			await invoke("set_renderer_dynamics_enabled", {
				id: renderer.id,
				sourceId,
				enabled,
			});
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		}
	}

	async function setRendererAllDynamicsEnabled(renderer: RendererInstance | null, enabled: boolean): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		try {
			await invoke("set_renderer_all_dynamics_enabled", {
				id: renderer.id,
				enabled,
			});
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		}
	}

	async function setRendererCameraLock(renderer: RendererInstance | null, locked: boolean): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		try {
			await invoke("set_renderer_camera_lock", { id: renderer.id, locked });
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		}
	}

	async function setRendererCameraOrbitPreset(renderer: RendererInstance | null, kind: "left" | "front" | "right"): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		const preset = cameraOrbitPresetAngles(kind);
		busy = true;
		try {
			await invoke("set_renderer_camera_state", {
				id: renderer.id,
				target: null,
				longitudeDeg: preset.longitude,
				latitudeDeg: preset.latitude,
				radius: null,
				diagonalFovDeg: null,
				transition: { duration_ms: 320, easing: "ease_out_cubic", mode: "queue" },
			});
			message = `Set camera ${kind} for ${renderer.name}`;
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function saveRendererCameraToProfile(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		busy = true;
		try {
			await invoke("save_renderer_camera_to_profile", { id: renderer.id });
			message = `Saved camera to ${renderer.name} profile`;
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function restoreRendererCameraFromProfile(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		try {
			await invoke("restore_renderer_camera_from_profile", { id: renderer.id });
			message = `Restored camera from ${renderer.name} profile`;
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		}
	}

	async function saveRendererWindowToProfile(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		busy = true;
		try {
			await invoke("save_renderer_window_to_profile", { id: renderer.id });
			message = `Saved window x/y/width/height to ${renderer.name} profile`;
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function restoreRendererWindowFromProfile(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		try {
			await invoke("restore_renderer_window_from_profile", { id: renderer.id });
			message = `Restored window from ${renderer.name} profile`;
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		}
	}

	async function stopRenderer(id: number | null): Promise<void> {
		if (id == null) return;
		if (!hasTauriRuntime()) {
			message = "Browser preview: stop requires Tauri";
			return;
		}
		busy = true;
		try {
			await invoke("stop_renderer", { id });
			message = "Stop requested";
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function stopAll(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = "Browser preview: stop all requires Tauri";
			return;
		}
		busy = true;
		try {
			await invoke("stop_all_renderers");
			message = "All renderers stopped";
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	$effect(() => {
		let cancelled = false;
		void (async () => {
			await loadBackendAppSettings();
			await refreshAll();
			if (!cancelled) {
				await maybeAutoLaunchSelectedOnStartup();
			}
		})();
		const timer = window.setInterval(() => {
			if (activeTab === "renderers" || activeTab === "settings") {
				refreshRendererRuntimeView();
			}
		}, 250);
		return () => {
			cancelled = true;
			window.clearInterval(timer);
		};
	});

	$effect(() => {
		document.documentElement.dataset.theme = resolvedTheme;
		document.documentElement.dataset.themeMode = appSettings.theme_mode;
		syncAppSettingsToBackend(appSettings);
	});

	$effect(() => {
		const query = window.matchMedia("(prefers-color-scheme: dark)");
		const updateOsTheme = () => (osTheme = query.matches ? "dark" : "light");
		updateOsTheme();
		query.addEventListener("change", updateOsTheme);
		return () => query.removeEventListener("change", updateOsTheme);
	});

	$effect(() => () => {
		cancelDeleteHold();
		cancelSettingPointerDrag();
	});

	$effect(() => {
		if (activeTab !== "logs" || !logsAutoscroll) return;
		void renderers;
		void logsTextFilter;
		void logsRendererFilter;
		queueMicrotask(scrollLogsToBottom);
	});

	$effect(() => {
		void refreshWardrobeOptionsForSetting(selectedSetting);
	});

	/// selectedSettingId が変わるたびに Tauri 側へ保存し、次回起動時に復元できるようにする。
	/// 初期化フェーズ (backendAppSettingsReady 前) や記録済み値と一致する場合はスキップ。
	$effect(() => {
		const id = selectedSettingId;
		if (!backendAppSettingsReady) return;
		if (id === appSettings.last_selected_setting_id) return;
		void persistLastSelectedSettingId(id);
	});

	$effect(() => {
		saveLaunchTargetId(launchTargetStorageKey, launchTargetId);
	});
</script>

<svelte:head>
	<title>UN Avatar Supervisor</title>
</svelte:head>

<main class="shell">
	<header class="topbar">
		<div class="brand">
			<img src="/un-avatar-artwork-supervisor.png" alt="" />
			<div>
				<h1>{$_("app.name")}</h1>
				<p>{$_("app.subtitle")}</p>
			</div>
		</div>
		<div class="status-strip" aria-label="Renderer status summary">
			<span><Activity size={14} />{runningCount} running</span>
			<span class:warn={issueCount > 0}><AlertTriangle size={14} />{issueCount} issues</span>
			<span>{message}</span>
			{#if screenshotNoticePath}
				<button class="screenshot-notice" title={screenshotNoticePath} onclick={() => void revealScreenshotFolder()}>
					<FolderOpen size={14} />{basename(screenshotNoticePath)}
				</button>
			{/if}
			{#if pendingRendererRestart && !(activeTab === "settings" && selectedSetting && sameNormalizedPath(pendingRendererRestart.renderer.manifest_path, selectedSetting.manifest_path))}
				<span class="restart-notice">
					{$_("profiles.live.pending_field", {
						values: { field: pendingRendererRestart.fieldLabel },
					})}
					<button onclick={() => void restartPendingRenderer()}>{$_("renderers.toolbar.restart")}</button>
					<button onclick={() => (pendingRendererRestart = null)}>{$_("common.dismiss")}</button>
				</span>
			{/if}
		</div>
		<div class="header-actions">
			<ThemeModeSwitch
				className="theme-switch"
				mode={appSettings.theme_mode}
				title={appSettings.theme_mode === "system" ? `Defaulting to OS ${osTheme}` : "Theme override is saved"}
				ariaLabel="Theme"
				onChange={setThemeMode}
			/>
		</div>
	</header>

	<div class="workspace">
		<aside class="side-rail" aria-label="Primary navigation">
			<button class:active={activeTab === "renderers"} onclick={() => (activeTab = "renderers")}
				><Monitor size={17} />{$_("sidebar.renderers")}</button
			>
			<button class:active={activeTab === "settings"} onclick={() => (activeTab = "settings")}
				><FileCog size={17} />{$_("sidebar.profiles")}</button
			>
			<button class:active={activeTab === "logs"} onclick={() => (activeTab = "logs")}
				><TerminalSquare size={17} />{$_("sidebar.logs")}</button
			>
			<button class:active={activeTab === "app"} onclick={() => (activeTab = "app")}
				><Settings size={17} />{$_("sidebar.settings")}</button
			>
			<div class="rail-footer">
				<button class="danger" disabled={busy || visibleRenderers.length === 0} onclick={stopAll}
					><AlertTriangle size={16} />{$_("app.stop_all")}</button
				>
			</div>
		</aside>

		{#if activeTab === "renderers"}
			<section class="view renderers-view">
				<RenderersToolbar
					{busy}
					{launchTargetSetting}
					{launchGroupName}
					{launchTargetId}
					bind:launchMenuOpen
					bind:showStoppedRenderers
					{avatarSettings}
					{profileGroups}
					{message}
					{iconSrc}
					groupCount={(group) => avatarSettingsByGroup.get(group)?.length ?? 0}
					onLaunch={() => launchSetting(launchTargetSetting?.id ?? null)}
					onRefresh={refreshAll}
					onSelectGroup={(group) => {
						launchTargetId = `group:${group}`;
						launchMenuOpen = false;
					}}
					onSelectSetting={(settingId) => {
						launchTargetId = settingId;
						selectedSettingId = settingId;
						launchMenuOpen = false;
					}}
				/>

				{#if selectedRenderer}
					{@const rendererProfile = settingForRenderer(selectedRenderer)}
					<RendererStage
						renderer={selectedRenderer}
						runtimeStatus={selectedRuntimeStatus}
						profile={rendererProfile}
						iconUrl={iconSrc(rendererProfile?.icon_path ?? null)}
						{busy}
						rendererStateLabel={(state) => rendererStateLabel(state as RendererState)}
						onViewProfile={(profileId) => {
							selectedSettingId = profileId;
							activeTab = "settings";
						}}
						onActivateRenderer={(rendererId) => {
							const renderer = selectedRendererById(rendererId);
							if (renderer) return activateRenderer(renderer);
						}}
						onResetCamera={(rendererId) => {
							const renderer = selectedRendererById(rendererId);
							if (renderer) return resetRendererCamera(renderer);
						}}
						onCaptureScreenshot={(rendererId) => {
							const renderer = selectedRendererById(rendererId);
							if (renderer) return captureRendererScreenshot(renderer);
						}}
						onRestartRenderer={(rendererId) => {
							const renderer = selectedRendererById(rendererId);
							if (renderer) return restartRenderer(renderer);
						}}
						onStopRenderer={(rendererId) => stopRenderer(rendererId)}
					/>
				{:else if launchTargetSetting}
					<RendererReadyStage
						setting={launchTargetSetting}
						iconUrl={iconSrc(launchGroupName ? null : launchTargetSetting.icon_path)}
						{launchGroupName}
						launchGroupCount={launchGroupSettings.length}
						{runningCount}
						{issueCount}
						profileCount={avatarSettings.length}
						profileGroupCount={profileGroups.length}
					/>
				{/if}

				<div class="split">
					<RendererProcessTable
						renderers={rendererTableRenderers}
						{selectedRendererId}
						{showStoppedRenderers}
						emptySummary={launchGroupName
							? $_("renderers.ready.group_count", {
									values: { count: launchGroupSettings.length },
								})
							: launchTargetSetting
								? settingSummary(launchTargetSetting)
								: $_("renderers.details.none_selected")}
						statusForRenderer={(rendererId) => runtimeStatuses[rendererId] ?? null}
						iconSrcForManifest={(manifestPath) => iconSrc(settingForManifestPath(manifestPath)?.icon_path ?? null)}
						onSelectRenderer={(rendererId) => (selectedRendererId = rendererId)}
						onOpenRendererLog={(rendererId) => {
							const renderer = selectedRendererById(rendererId);
							if (renderer) jumpToRendererLog(renderer);
						}}
					/>

					<RendererDetailsPanel
						renderer={selectedRenderer}
						runtimeStatus={selectedRuntimeStatus}
						bind:rendererPaneTab
						{launchGroupName}
						{launchTargetSetting}
						launchGroupCount={launchGroupSettings.length}
						{runningCount}
						{issueCount}
						profileCount={avatarSettings.length}
						profileGroupCount={profileGroups.length}
						{busy}
						{colorDisplayMode}
						{expressionOverrides}
						bind:expressionFilter
						onSelectRendererPaneTab={(tab) => (rendererPaneTab = tab)}
						onSetSpoutOutput={(enabled, size, label) => {
							if (!selectedRenderer) return;
							return setRendererSpoutOutput(selectedRenderer, enabled, size, label);
						}}
						onSetWindow={(patch, label) => {
							if (!selectedRenderer) return;
							return setRendererWindow(selectedRenderer, patch, label);
						}}
						onSaveWindow={() => {
							if (!selectedRenderer) return;
							return saveRendererWindowToProfile(selectedRenderer);
						}}
						onRestoreWindow={() => {
							if (!selectedRenderer) return;
							return restoreRendererWindowFromProfile(selectedRenderer);
						}}
						onSetShowAxes={(enabled) => {
							if (!selectedRenderer) return;
							return setRendererShowAxes(selectedRenderer, enabled);
						}}
						onSetShowBoneColliders={(enabled) => {
							if (!selectedRenderer) return;
							return setRendererShowBoneColliders(selectedRenderer, enabled);
						}}
						onSetCameraLock={(enabled) => {
							if (!selectedRenderer) return;
							return setRendererCameraLock(selectedRenderer, enabled);
						}}
						onSetCameraOrbitPreset={(preset) => {
							if (!selectedRenderer) return;
							return setRendererCameraOrbitPreset(selectedRenderer, preset);
						}}
						onSaveCamera={() => {
							if (!selectedRenderer) return;
							return saveRendererCameraToProfile(selectedRenderer);
						}}
						onRestoreCamera={() => {
							if (!selectedRenderer) return;
							return restoreRendererCameraFromProfile(selectedRenderer);
						}}
						onSetClearColor={([r, g, b]) => {
							if (!selectedRenderer) return;
							return setRendererClearColor(selectedRenderer, { r, g, b, a: selectedRenderer.transparent ? 0 : 1 }, "custom");
						}}
						onColorModeChange={setColorDisplayMode}
						onClearExpressionOverrides={(rendererId) => {
							const renderer = selectedRendererById(rendererId);
							void clearExpressionOverrides(renderer);
						}}
						onSetExpressionOverride={(rendererId, preset, weight) => {
							const renderer = selectedRendererById(rendererId);
							setExpressionOverride(renderer, preset, weight);
						}}
						onActivateWardrobeMenuCandidate={(rendererId, actionId, wardrobeSetId) => {
							const renderer = selectedRendererById(rendererId);
							return activateWardrobeMenuCandidate(renderer, actionId, wardrobeSetId);
						}}
						onSetDynamicsEnabled={(rendererId, sourceId, enabled) => {
							const renderer = selectedRendererById(rendererId);
							return setRendererDynamicsEnabled(renderer, sourceId, enabled);
						}}
						onSetAllDynamicsEnabled={(rendererId, enabled) => {
							const renderer = selectedRendererById(rendererId);
							return setRendererAllDynamicsEnabled(renderer, enabled);
						}}
						onOpenProfile={() => {
							if (!launchTargetSetting) return;
							selectedSettingId = launchGroupSettings[0]?.id ?? launchTargetSetting.id;
							activeTab = "settings";
						}}
						onRevealProfilesDir={() => revealProfilesDir()}
					/>
				</div>
			</section>
		{:else if activeTab === "settings"}
			<section
				class="view settings-view"
				aria-label="Profiles"
				onpointerover={updateProfileHintFromEvent}
				onfocusin={updateProfileHintFromEvent}
				onpointerleave={clearProfileHint}
			>
				<ProfilesToolbar
					{busy}
					selectedSettingId={selectedSetting?.id ?? null}
					{deleteHoldTargetId}
					{deleteHoldProgress}
					onNew={newSetting}
					onDuplicate={duplicateSetting}
					onStartDeleteHold={startDeleteHold}
					onCancelDeleteHold={cancelDeleteHold}
					onOpenFolder={() => revealProfilesDir()}
				/>
				{#if selectedSetting}
					{@const liveRenderer = rendererForSetting(selectedSetting)}
					{@const liveRendererCount = runningCountForSetting(selectedSetting)}
					{@const profilePendingRestart =
						pendingRendererRestart &&
						sameNormalizedPath(pendingRendererRestart.renderer.manifest_path, selectedSetting.manifest_path)
							? pendingRendererRestart
							: null}
					<ProfileStage
						setting={selectedSetting}
						iconUrl={iconSrc(selectedSetting.icon_path)}
						{liveRenderer}
						{liveRendererCount}
						pendingRestart={profilePendingRestart}
						activeSection={activeProfileSection}
						{busy}
						onRestartPending={() => restartPendingRenderer()}
						onViewRenderer={(rendererId) => {
							selectedRendererId = rendererId;
							activeTab = "renderers";
						}}
						onActivateRenderer={(rendererId) => {
							const renderer = selectedRendererById(rendererId);
							if (renderer) return activateRenderer(renderer);
						}}
						onCaptureRendererScreenshot={(rendererId) => {
							const renderer = selectedRendererById(rendererId);
							if (renderer) return captureRendererScreenshot(renderer);
						}}
						onLaunchProfile={(settingId) => launchSetting(settingId, false)}
						onPrewarmSceneCache={(settingId) => prewarmSceneCache(settingId)}
						onCreateDesktopShortcut={(settingId) => createDesktopShortcut(settingId)}
						onCreateTaskbarLauncher={(settingId) => createTaskbarLauncher(settingId)}
						onScrollSection={(section) => scrollProfileSection(section)}
					/>
				{/if}
				<div class="split settings-split">
					<ProfileSettingList
						settings={avatarSettings}
						{selectedSettingId}
						{draggedSettingId}
						{settingPointerDrag}
						{iconSrc}
						runningCountForManifestPath={(manifestPath) =>
							liveRenderersByManifestPath.get(normalizedPathKey(manifestPath))?.length ?? 0}
						onSelect={(settingId) => {
							if (suppressSettingClick) {
								suppressSettingClick = false;
								return;
							}
							selectedSettingId = settingId;
						}}
						onBeginDrag={beginSettingPointerDrag}
					/>
					<section class="panel editor-panel">
						{#if selectedSetting}
							<ProfileSectionNav
								items={profileSectionNavItems}
								activeSectionId={activeProfileSection}
								onSelect={(sectionId) => scrollProfileSection(sectionId as ProfileSectionId)}
							/>
							<div class="setting-editor" onscroll={updateActiveProfileSectionFromScroll}>
								<ProfileIdentitySection
									setting={selectedSetting}
									iconUrl={iconSrc(selectedSetting.icon_path)}
									{busy}
									onBrowseIcon={() => browseSettingPath("icon_path", "icon")}
									onApplyAvatarThumbnail={() => applySelectedAvatarThumbnail()}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onActivate={() => (activeProfileSection = "identity")}
								/>

								<ProfileAvatarSection
									setting={selectedSetting}
									{wardrobeOptions}
									{busy}
									onBrowseAvatar={() => browseSettingPath("avatar_path", "avatar")}
									onReviewMetadata={() => reviewSelectedVrmMetadata()}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onActivate={() => (activeProfileSection = "avatar")}
								/>

								<ProfileQualitySection
									setting={selectedSetting}
									{busy}
									showDeveloperControls={appSettings.show_developer_controls}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onApplyRenderQualityRecommendation={(recommendation) =>
										applyRenderQualityRecommendation(recommendation)}
									onActivate={() => (activeProfileSection = "quality")}
								/>

								<ProfileLightingSection
									setting={selectedSetting}
									{busy}
									{colorDisplayMode}
									onColorModeChange={setColorDisplayMode}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onActivate={() => (activeProfileSection = "lighting")}
								/>

								<ProfileLookSection
									setting={selectedSetting}
									{busy}
									{colorDisplayMode}
									onColorModeChange={setColorDisplayMode}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onApplyLookRecommendation={(look) => applyLookRecommendation(look)}
									onActivate={() => (activeProfileSection = "look")}
								/>

								<ProfileMotionSection
									setting={selectedSetting}
									{busy}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onActivate={() => (activeProfileSection = "motion")}
								/>

								<ProfilePhysicsSection
									setting={selectedSetting}
									{busy}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onActivate={() => (activeProfileSection = "physics")}
								/>

								<ProfileCameraSection
									setting={selectedSetting}
									{busy}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onApplyTargetPreset={(preset) => applyCameraTargetPreset(preset)}
									onApplyOrbitPreset={(preset) => applyCameraOrbitPreset(preset)}
									onApplyLensPreset={(preset) => applyCameraLensPreset(preset)}
									onActivate={() => (activeProfileSection = "camera")}
								/>

								<ProfileWindowSection
									setting={selectedSetting}
									{busy}
									{colorDisplayMode}
									onColorModeChange={setColorDisplayMode}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onBackgroundColorChange={updateBackgroundColorValue}
									onActivate={() => (activeProfileSection = "window")}
								/>

								<ProfileOutputSection
									setting={selectedSetting}
									{busy}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onApplySpoutResolutionPreset={(preset) => applySpoutResolutionPreset(preset)}
									onApplyOutputModePreset={(preset) => applyOutputModePreset(preset)}
									onApplyPreviewWindowPreset={(preset) => applyPreviewWindowPreset(preset)}
									onActivate={() => (activeProfileSection = "output")}
								/>
							</div>
							<div class="profile-hint-bar" aria-live="polite">
								<span>{profileHint || defaultProfileHint}</span>
							</div>
						{:else}
							<h2>{$_("profiles.editor.profile_setting_heading")}</h2>
							<p class="empty">{$_("profiles.editor.select_or_create")}</p>
						{/if}
					</section>
				</div>
			</section>
		{:else if activeTab === "logs"}
			<LogsView
				{renderers}
				bind:rendererLogsLayout
				bind:logsRendererFilter
				bind:logsTextFilter
				bind:logsAutoscroll
				{rendererLogsCopyFlash}
				{rendererLogsExpanded}
				onCopyAllRendererLogs={() => void copyAllRendererLogs()}
				onSaveAllRendererLogs={() => void saveAllRendererLogs()}
				onRevealSupervisorLogsDir={() => void revealSupervisorLogsDir()}
				onCopyRendererLog={(renderer) => {
					const fullRenderer = rendererById.get(renderer.id);
					if (fullRenderer) void copyRendererLog(fullRenderer);
				}}
				onToggleRendererLogExpanded={(renderer) => {
					const fullRenderer = rendererById.get(renderer.id);
					if (fullRenderer) toggleRendererLogExpanded(fullRenderer);
				}}
				onLogsViewRef={(element) => (logsViewRef = element)}
			/>
		{:else}
			<AppSettingsView
				{appSettings}
				{availableLocales}
				{appVersion}
				{nativeNotificationStatus}
				{busy}
				{settingsHint}
				{defaultSettingsHint}
				onSettingsHintEvent={updateSettingsHintFromEvent}
				onClearSettingsHint={clearSettingsHint}
				onSetThemeMode={setThemeMode}
				onSetAppSetting={(key, value) => setAppSetting(key, value)}
				onSetLocale={setLocaleSetting}
				onSendTestNativeNotification={() => sendTestNativeNotification()}
				onOpenExternalLink={(url) => openExternalLink(url)}
			/>
		{/if}
	</div>
	{#if vrmMetadataModal}
		<VrmMetadataModal
			modal={vrmMetadataModal}
			{busy}
			bind:useThumbnailForProfileIconOnAccept
			onClose={closeVrmMetadataModal}
			onAcceptAndUse={acceptVrmMetadataAndUse}
		/>
	{/if}
	{#if unavatarMetadataModal}
		<UnavatarRightsModal
			modal={unavatarMetadataModal}
			{busy}
			bind:profileIconCrop={unavatarProfileIconCrop}
			onClose={closeUnavatarMetadataModal}
			onAcceptAndUse={acceptUnavatarMetadataAndUse}
		/>
	{/if}
</main>
