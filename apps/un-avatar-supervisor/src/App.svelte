<script lang="ts">
	import DeletedProfiles from "./lib/DeletedProfiles.svelte";
	import ReviewProfileStop from "./lib/ReviewProfileStop.svelte";
	import ProfileOrderRecovery, {type OrderRecoveryEntry} from "./lib/ProfileOrderRecovery.svelte";
	import { convertFileSrc, invoke as nativeInvoke } from "@tauri-apps/api/core";
	import { orderedProfileInvoke } from "./lib/orderedProfileInvoke";
	const invoke = orderedProfileInvoke(nativeInvoke);
	import { listen } from "@tauri-apps/api/event";
	import { onMount, tick, untrack } from "svelte";
	import AvatarFeatureMap from "./lib/AvatarFeatureMap.svelte";
	import PreviewAvatarFeatureIndex from "./lib/PreviewAvatarFeatureIndex.svelte";
	import "./lib/featureNavigation.css";
	const isolatedDesignPreview = import.meta.env.VITE_UN_DESIGN_PREVIEW === '1' && document.documentElement.dataset.unIsolatedPreview === '1';
	const previewQuery = new URL(location.href).searchParams;
	// Adopted A. Comparison builds retain the previous layout unless explicitly selected.
	const previewOperationsA = import.meta.env.VITE_UN_DESIGN_PREVIEW !== '1' || (isolatedDesignPreview && previewQuery.get('supervisorLayout') === 'A');
	// Adopted B layout. Historical A remains available only in the comparison build.
	let previewEditWorkspace = $state(import.meta.env.VITE_UN_DESIGN_PREVIEW !== '1' || previewQuery.get('workspace') !== 'existing');
	$effect(() => {
		if (import.meta.env.VITE_UN_DESIGN_PREVIEW !== '1') return;
		const update = () => { previewEditWorkspace = document.documentElement.dataset.seriesWorkspace === 'focus'; };
		window.addEventListener('un-series-workspace', update);
		return () => window.removeEventListener('un-series-workspace', update);
	});
	let previewFeatureOutline = $state(import.meta.env.VITE_UN_DESIGN_PREVIEW !== '1' || (previewQuery.get('navigation') !== 'existing' && previewQuery.get('design') !== 'baseline'));
	$effect(() => {
		if (import.meta.env.VITE_UN_DESIGN_PREVIEW !== '1') return;
		const update = (event: Event) => { previewFeatureOutline = (event as CustomEvent).detail === 'outline'; };
		window.addEventListener('un-series-navigation', update);
		return () => window.removeEventListener('un-series-navigation', update);
	});
	$effect(() => {
		if (!isolatedDesignPreview) return;
		const open = async (event: Event) => {
			await refreshAll();
			const id = String((event as CustomEvent).detail ?? "");
			if (avatarSettings.some(setting => setting.id === id)) selectProfileForEditing(id);
			setActiveTab("settings", "isolated-workspace-example");
		};
		window.addEventListener("un-series-profile-example", open);
		return () => window.removeEventListener("un-series-profile-example", open);
	});
	import type { AvatarFeature } from "./lib/avatarFeatureCatalog";
	import { _, locale } from "svelte-i18n";
	import { setUiLocale } from "@usagi.network/un-i18n-svelte";
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
	import type {
		RendererCameraSnapshot,
		RendererInstance,
		RendererPaneTab,
		RendererRuntimeStatus,
		RendererState,
	} from "./lib/rendererTypes";
	import type { DynamicsGroupOverrideSeed } from "./lib/rendererPaneActions";
	import { fallbackVrmMetadata, looksLikeVrmPath, type VrmMetadataDialogState, type VrmMetadataInfo } from "./lib/vrmMetadata";
	import {
		looksLikeUnavatarPath,
		type UnavatarMetadataDialogState,
		type UnavatarMetadataInfo,
		type UnavatarProfileIconCrop,
	} from "./lib/unavatarMetadata";
	import { loadAppSettings, saveAppSettings, type AppSettings, type ThemeMode } from "./lib/appSettings";
	import type { AppNotification, NativeNotificationStatus } from "./lib/appTypes";
	import { cameraOrbitPresetAngles, type CameraOrbitPreset } from "./lib/cameraPresets";
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
	import { localizedSettingSummary } from "./lib/profileStageSummary";
	import { diagonalFovFromLensMm } from "./lib/profileDiagrams";
	import { countErrorNotifications, countRendererStates, rendererStateLabel as localizedRendererStateLabel } from "./lib/runtimeState";
	import {
		defaultRendererLogExpanded,
		filteredRendererLogLines as filterRendererLogLines,
		rendererLogText as rendererLogTextFromLines,
		type RendererLogFilter,
	} from "./lib/rendererLogs";
	import { traceAsync, traceFrontendEvent } from "./lib/frontendTrace";
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
		DYNAMICS_ENABLED_FIELD,
		DYNAMICS_MATCH_OVERRIDE_FIELD,
		DYNAMICS_OVERRIDE_FIELD_PREFIX,
		defaultDynamicsCategoryOverrides,
		type DynamicsGroupOverrideSetting,
		type DynamicsMatchOverrideSetting,
	} from "./lib/dynamicsPresets";
	import {
		loadColorDisplayMode,
		loadLaunchTargetId,
		saveColorDisplayMode,
		saveLaunchTargetId,
		type ColorDisplayMode,
	} from "./lib/storageState";
	import { animatorItemCount } from "./lib/rendererAnimator";
	import { spoutSenderLabel, spoutTimingLabel, textureCacheLabel, texturePolicyLabel, textureSummaryLabel } from "./lib/runtimeLabels";
	import {
		Activity,
		AlertTriangle,
		Database,
		FileCog,
		FolderOpen,
		Monitor,
		RefreshCw,
		Settings,
		TerminalSquare,
		Trash2,
	} from "lucide-svelte";
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
			scopeKey: "profiles.editor.runtime",
		},
	];

	type VrmMetadataModalState = VrmMetadataDialogState & {
		settingId: string;
		rendererToRestart: RendererInstance | null;
		expectedRevision?: string;
	};

	type UnavatarMetadataModalState = UnavatarMetadataDialogState & {
		settingId: string;
		rendererToRestart: RendererInstance | null;
		expectedRevision?: string;
	};

	const appSettingsStorageKey = "un-avatar-supervisor.app-settings";
	const legacyThemeModeStorageKey = "un-avatar-supervisor.theme-mode";

	type PrewarmSceneCacheResult = {
		profile_name: string;
		elapsed_secs: number;
		detail: string | null;
	};
	type SceneCachePrewarmProgress = {
		setting_id: string;
		phase: string;
		current: number;
		total: number;
		detail: string;
		elapsed_secs: number;
	};
	type RendererCacheGroupStatus = {
		label: string;
		path: string;
		bytes: number;
		files: number;
	};
	type RendererCacheStatus = {
		texture: RendererCacheGroupStatus;
		pipeline: RendererCacheGroupStatus;
		total_bytes: number;
		total_files: number;
	};
	const launchTargetStorageKey = "un-avatar-supervisor.launch-target-id";
	const defaultIconSrc = DEFAULT_PROFILE_ICON_SRC;

	let appVersion = $state("");
	let activeTab = $state<"renderers" | "settings" | "logs" | "app" | "map">("renderers");
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
	let rendererCacheStatus = $state<RendererCacheStatus | null>(null);
	let rendererCacheBusy = $state(false);
	let sceneCachePrewarmProgress = $state<SceneCachePrewarmProgress | null>(null);
	let avatarSettings = $state<AvatarSetting[]>([]);
	let profileIconRevision = $state<Record<string, number>>({});
	let wardrobeOptions = $state<UnavatarWardrobeOptions | null>(null);
	let wardrobeOptionsKey = $state("");
	let wardrobeOptionsRequestId = 0;
	let selectedRendererId = $state<number | null>(null);
	/// Selected Renderer 詳細パネルのサブタブ。Overview = 状態 + コントロール、
	/// Expressions = 表情プレビュー、Diagnostics = stderr など。タブ分割で縦長解消。
	let rendererPaneTab = $state<RendererPaneTab>("overview");
	let showStoppedRenderers = $state(false);
	let selectedSettingId = $state<string | null>(null);
	let loadedProfileDetailKey = $state("");
	let draggedSettingId = $state<string | null>(null);
	let profileHint = $state("");
	let settingsHint = $state("");
	let orderDraft = $state<{reviewed: AvatarSetting[]; ids: string[]; committed?: boolean; error?: string} | null>(null);
	let orderSaving = $state(false);
	let settingPointerDrag = $state<{
		id: string;
		reviewed: AvatarSetting[];
		desiredIds: string[];
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
	let featureQuery = $state("");
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
		enabled: false,
		imageDataUrl: null,
		zoom: 1,
		offsetX: 0,
		offsetY: 0,
	});
	let osTheme = $state<"light" | "dark">(detectOsTheme());
	let appSettings = $state<AppSettings>(loadAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey));
	let colorDisplayMode = $state<ColorDisplayMode>(loadColorDisplayMode(COLOR_DISPLAY_MODE_KEY));
	let backendAppSettingsReady = $state(!hasTauriRuntime());
	let backendAppSettingsLoading = false;
	let appSettingsEarlyEdits: Partial<AppSettings> = {};
	let appSettingsBeforeEarlyEdits: AppSettings | null = null;
	let appSettingsPending = $state(0);
	let appSettingsSaveError = $state("");
	let appSettingsSaveSequence = 0;
	let appSettingsSaveTail: Promise<void> = Promise.resolve();
	/// サーバー側 `i18n_available_locales` で得られたサポート言語の BCP-47 タグ一覧。
	let availableLocales = $state<string[]>(["ja-JP", "en-US"]);
	let deleteHoldTargetId = $state<string | null>(null);
	let deleteHoldProgress = $state(0);
	let deleteHoldTimer: number | null = null;
	let deleteHoldStartedAt = 0;
	let cacheDeleteHoldProgress = $state(0);
	let cacheDeleteHoldTimer: number | null = null;
	let cacheDeleteHoldStartedAt = 0;
	let runtimeRefreshBusy = false;
	let runtimeRefreshPending = false;
	let runtimeRefreshTraceSeq = 0;
	let startupAutoLaunchAttempted = false;
	let startupAutoLaunchReady = $state(false);

	const deleteHoldDurationMs = 1200;
	const runtimeRefreshIntervalMs = 250;
	const wardrobeTransitionRuntimeRefreshIntervalMs = 1000;
	const idleRuntimeRefreshIntervalMs = 2000;
	const profileSettingUpdateCoalesceMs = 80;
	const motionLookAtFields = ["motion.look_at.enabled", "motion.look_at.clamp_deg"] as const;
	const motionReceiverFields = [
		"motion.vmc_udp.enabled",
		"motion.vmc_udp.address",
		"motion.unmotion_zenoh.enabled",
		"motion.unmotion_zenoh.key",
		"motion.unmotion_zenoh.connect",
		"motion.unmotion_zenoh.subscriptions",
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
	const wardrobeTransitionTelemetryActive = $derived(
		Object.values(runtimeStatuses).some((status) => status.wardrobe_asset_upload?.transition_progress?.active === true)
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
	let runtimeCommandFailures = $state<{ rendererId: number; name: string; command: string; error: string }[]>([]);
	const issueCount = $derived(rendererStatusCounts.issues + notificationErrorCount + runtimeCommandFailures.length);
	const resolvedTheme = $derived(appSettings.theme_mode === "system" ? osTheme : appSettings.theme_mode);
	const rendererCacheTotalLabel = $derived(
		rendererCacheStatus
			? `${formatCacheBytes(rendererCacheStatus.total_bytes)} / ${$_("app.cache_files", {
					values: { count: rendererCacheStatus.total_files },
				})}`
			: "--"
	);
	$effect(() => {
		reconcileSelectedRendererId(renderers);
	});

	$effect(() => {
		if (rendererPaneTab !== "animator" || !selectedRuntimeStatus) return;
		if (
			animatorItemCount(
				selectedRuntimeStatus.expression_presets,
				selectedRuntimeStatus.menu_action_candidates,
				selectedRuntimeStatus.runtime_actions
			) === 0
		) {
			rendererPaneTab = "overview";
		}
	});

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

	function formatCacheBytes(bytes: number | null | undefined): string {
		if (bytes == null) return "--";
		if (bytes < 1024) return `${bytes} B`;
		const units = ["KiB", "MiB", "GiB", "TiB"];
		let value = bytes / 1024;
		let unitIndex = 0;
		while (value >= 1024 && unitIndex < units.length - 1) {
			value /= 1024;
			unitIndex += 1;
		}
		const decimals = value >= 100 ? 0 : value >= 10 ? 1 : 2;
		return `${value.toFixed(decimals)} ${units[unitIndex]}`;
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
			wardrobe_billboard_anchor: "neck",
			wardrobe_billboard_y_offset_mm: 0,
			wardrobe_shortcuts: [],
			wardrobe_bindings: [],
			animator_actions: [],
			animator_bindings: [],
			vmc_address: "0.0.0.0:39539",
			vmc_port: 39539,
			motion_vmc_enabled: true,
			motion_unmotion_enabled: false,
			unmotion_zenoh_key: null,
			unmotion_zenoh_connect: null,
			unmotion_zenoh_subscriptions: [{ id: "un-motion/frame", lan_enabled: false, host: null, port: 47447 }],
			audio_link_source: "none",
			audio_link_input_device_id: null,
			audio_link_input_device_name_hint: null,
			look_at_enabled: false,
			look_at_clamp_deg: 30,
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
			camera_diagonal_fov_deg: 35.0,
			spout_enabled: true,
			spout_name: "UN Avatar Spout",
			spout_width: 1280,
			spout_height: 720,
			target_fps: 60,
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
			debug_zero_morphs: false,
			debug_vertex_pick_diagnostics: false,
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
			wardrobe_billboard_anchor: "neck",
			wardrobe_billboard_y_offset_mm: 0,
			wardrobe_shortcuts: [],
			wardrobe_bindings: [],
			animator_actions: [],
			animator_bindings: [],
			vmc_address: "0.0.0.0:39539",
			vmc_port: null,
			motion_vmc_enabled: false,
			motion_unmotion_enabled: false,
			unmotion_zenoh_key: null,
			unmotion_zenoh_connect: null,
			unmotion_zenoh_subscriptions: [{ id: "un-motion/frame", lan_enabled: false, host: null, port: 47447 }],
			audio_link_source: "none",
			audio_link_input_device_id: null,
			audio_link_input_device_name_hint: null,
			look_at_enabled: false,
			look_at_clamp_deg: 30,
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
			target_fps: 60,
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
			debug_zero_morphs: false,
			debug_vertex_pick_diagnostics: false,
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
			scene_cache_fingerprint: "preview-debug-v1",
			scene_cache_prewarmed_fingerprint: "preview-debug-v1",
			scene_cache_prewarmed_at: "20260517T000001Z",
		},
	];

	async function openFeature(feature: AvatarFeature): Promise<void> {
		setActiveTab(feature.destination, "feature-map");
		await tick();
		if (feature.section) {
			scrollProfileSection(feature.section);
			const section = document.querySelector<HTMLElement>(`[data-profile-section="${feature.section}"]`);
			section?.focus({ preventScroll: true });
		}
	}
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

	let editorNavigationGeneration = 0;
	function selectProfileForEditing(id: string | null): void {
		editorNavigationGeneration++;
		selectedSettingId = id;
	}

	function setActiveTab(tab: typeof activeTab, source: string): void {
		editorNavigationGeneration++;
		traceFrontendEvent("activeTab:set", { from: activeTab, to: tab, source });
		activeTab = tab;
	}

	async function syncAppSettingsToBackend(settings: AppSettings): Promise<void> {
		if (!hasTauriRuntime()) {
			try { saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, settings); appSettingsSaveError = ""; }
			catch (error) { appSettingsSaveError = String(error); }
			return;
		}
		if (!backendAppSettingsReady) { await loadBackendAppSettings(); return; }
		const sequence = ++appSettingsSaveSequence;
		const snapshot: AppSettings = JSON.parse(JSON.stringify(settings));
		appSettingsPending++;
		const save = appSettingsSaveTail.then(async () => {
			try {
				await invoke("sync_app_settings", { settings: snapshot });
				if (sequence === appSettingsSaveSequence) {
					try { saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, snapshot); }
					catch (error) { throw new Error(`Settings file saved; local display cache failed: ${String(error)}`); }
					appSettingsSaveError = "";
				}
			} catch (error) {
				if (sequence === appSettingsSaveSequence) appSettingsSaveError = String(error);
			} finally { appSettingsPending--; }
		});
		appSettingsSaveTail = save;
		await save;
	}

	async function discardAppSettingsDraft(): Promise<void> {
		if (appSettingsPending > 0) return;
		if (!backendAppSettingsReady) {
			if (appSettingsBeforeEarlyEdits) appSettings = appSettingsBeforeEarlyEdits;
			appSettingsEarlyEdits = {};
			appSettingsBeforeEarlyEdits = null;
			setUiLocale(appSettings.locale || "ja-JP");
			return;
		}
		const sequence = ++appSettingsSaveSequence;
		appSettingsPending++;
		try {
			const saved = await invoke<AppSettings>("get_app_settings");
			if (!saved) throw new Error("Settings read returned no values");
			const effectiveLocale = saved.locale || await invoke<string>("i18n_resolve_default_locale");
			if (sequence === appSettingsSaveSequence) {
				saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, saved);
				appSettings = saved;
				appSettingsEarlyEdits = {};
				backendAppSettingsReady = true;
				appSettingsSaveError = "";
				setUiLocale(effectiveLocale);
			}
		} catch (error) {
			if (sequence === appSettingsSaveSequence) appSettingsSaveError = String(error);
		} finally { appSettingsPending--; }
	}

	function replaceAvatarSetting(setting: AvatarSetting, selectSetting = true): void {
		const next = avatarSettings.filter((item) => item.id !== setting.id && item.manifest_path !== setting.manifest_path);
		next.push(setting);
		next.sort(compareAvatarSettings);
		avatarSettings = next;
		if (selectSetting) {
			selectedSettingId = setting.id;
		}
		launchTargetId = pickInitialLaunchTargetId(launchTargetId, selectedSettingId, next);
	}

	async function loadSelectedProfileDetails(setting: AvatarSetting): Promise<void> {
		if (!hasTauriRuntime()) return;
		const key = `${setting.id}\n${setting.manifest_path}`;
		if (loadedProfileDetailKey === key) return;
		loadedProfileDetailKey = key;
		try {
			const detailed = await invoke<AvatarSetting>("get_avatar_setting_details", { settingId: setting.id });
			if (selectedSetting?.id === setting.id && selectedSetting.manifest_path === setting.manifest_path) {
				replaceAvatarSetting(detailed, false);
			}
		} catch (error) {
			if (loadedProfileDetailKey === key) loadedProfileDetailKey = "";
			message = String(error);
		}
	}

	function bumpProfileIconRevision(path: string | null): void {
		if (!path) return;
		profileIconRevision = {
			...profileIconRevision,
			[path]: (profileIconRevision[path] ?? 0) + 1,
		};
	}

	/// Remember editor selection in order without changing the launch target.
	let lastSelectionSaveQueue: Promise<void> = Promise.resolve();
	async function persistLastSelectedSettingId(id: string | null): Promise<void> {
		if (!hasTauriRuntime()) return;
		if (!backendAppSettingsReady) return;
		const save = lastSelectionSaveQueue.then(async () => {
			if (id === appSettings.last_selected_setting_id) return;
			try {
				await invoke("set_last_selected_setting_id", { value: id });
				appSettings = { ...appSettings, last_selected_setting_id: id };
				saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, appSettings);
			} catch (error) {
				message = `set_last_selected_setting_id failed: ${String(error)}`;
			}
		});
		lastSelectionSaveQueue = save;
		await save;
	}

	function setAppSetting<K extends keyof AppSettings>(key: K, value: AppSettings[K]): void {
		if (!backendAppSettingsReady && !appSettingsBeforeEarlyEdits) appSettingsBeforeEarlyEdits = { ...appSettings };
		appSettings = { ...appSettings, [key]: value };
		if (!backendAppSettingsReady) appSettingsEarlyEdits = { ...appSettingsEarlyEdits, [key]: value };
		void syncAppSettingsToBackend(appSettings);
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
		if (!hasTauriRuntime() || backendAppSettingsLoading) return;
		backendAppSettingsLoading = true;
		appSettingsPending++;
		try {
			const saved = await invoke<AppSettings>("get_app_settings");
			if (!saved) throw new Error("Settings read returned no values");
			const resolvedLocale = saved.locale || (await invoke<string>("i18n_resolve_default_locale").catch(() => "ja-JP"));
			const hasEarlyEdits = Object.keys(appSettingsEarlyEdits).length > 0;
			appSettings = { ...saved, ...appSettingsEarlyEdits };
			appSettingsEarlyEdits = {};
			appSettingsBeforeEarlyEdits = null;
			backendAppSettingsReady = true;
			appSettingsSaveError = "";
			setUiLocale(appSettings.locale || resolvedLocale);
			if (hasEarlyEdits) void syncAppSettingsToBackend(appSettings);
			else saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, appSettings);
		} catch (error) {
			appSettingsSaveError = String(error);
		} finally {
			backendAppSettingsLoading = false;
			appSettingsPending--;
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

	async function readUnavatarMetadataForPath(
		path: string,
		setting: AvatarSetting | null = selectedSetting
	): Promise<UnavatarMetadataInfo | null> {
		if (!setting) return null;
		return await invoke<UnavatarMetadataInfo | null>("read_unavatar_metadata", {
			path,
			manifestPath: setting.manifest_path,
			wardrobeSet: setting.wardrobe_set,
		});
	}

	function firstUnavatarPreviewDataUrl(metadata: UnavatarMetadataInfo): string | null {
		return metadata.preview_images[0]?.data_url ?? metadata.preview_sets[0]?.preview_images[0]?.data_url ?? null;
	}

	function rendererRuntimeStatus(renderer: RendererInstance): RendererRuntimeStatus | null {
		return runtimeStatuses[renderer.id] ?? null;
	}

	function iconSrc(path: string | null): string {
		const profile = path ? avatarSettings.find(setting => setting.icon_path === path) : null;
        const original = profileIconSrc(path, hasTauriRuntime(), convertFileSrc);
        const src = hasTauriRuntime() && profile && (original.startsWith("http://asset.localhost/") || original.startsWith("asset://"))
            ? `http://un-avatar-profile-icon.localhost/${encodeURIComponent(profile.id)}?revision=${encodeURIComponent(profile.revision ?? path ?? "")}`
            : original;
		const revision = path ? (profileIconRevision[path] ?? 0) : 0;
		if (!revision || src.startsWith("data:image/") || src === DEFAULT_PROFILE_ICON_SRC) return src;
		return `${src}${src.includes("?") ? "&" : "?"}v=${revision}`;
	}

	let profileBatchSequence = 0;
	let profileBatchDrafts = $state<{id:number; settingId:string; name:string; updates:ProfilePresetUpdate[]; state:"saving"|"failed"; error:string}[]>([]);
	async function applyProfileUpdates(updates: readonly ProfilePresetUpdate[]): Promise<void> {
		if (updates.length === 0) return;
		if (!selectedSetting || !hasTauriRuntime() || updates.some(([field]) => field === "avatar_path")) {
			for (const [field, value] of updates) {
				await updateSettingValue(field, value);
			}
			return;
		}
		const previousSetting = selectedSetting;
		const snapshot: ProfilePresetUpdate[] = JSON.parse(JSON.stringify(updates));
		const draftId = ++profileBatchSequence;
		profileBatchDrafts = [...profileBatchDrafts, {id:draftId,settingId:previousSetting.id,name:previousSetting.name,updates:snapshot,state:"saving",error:""}];
		let saved = false;
		const updatePayload: Array<{ field: string; value: ProfileSettingValue }> = [];
		const restartTargets: Array<{
			field: string;
			renderer: RendererInstance | null;
		}> = [];
		for (const [field, value] of snapshot) {
			updatePayload.push({ field, value });
			restartTargets.push({
				field,
				renderer: restartableRendererForField(field),
			});
		}
		try {
			const setting = await invoke<AvatarSetting>("update_avatar_setting_values", {
				settingId: previousSetting.id,
				...(previousSetting.revision ? { expectedRevision: previousSetting.revision } : {}),
				updates: updatePayload,
			});
			saved = true;
			profileBatchDrafts = profileBatchDrafts.filter(draft => draft.id !== draftId);
			message = $_("profiles.messages.updated_setting");
			replaceAvatarSetting(setting, false);
			await applyRuntimeProfileUpdates(snapshot, setting, previousSetting);
			for (const { field, renderer } of restartTargets) {
				queueRendererRestart(renderer, field);
			}
		} catch (error) {
			if (!saved) profileBatchDrafts = profileBatchDrafts.map(draft => draft.id === draftId ? {...draft,state:"failed",error:String(error)} : draft);
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

	async function captureRuntimeCameraToProfile(renderer: RendererInstance | null): Promise<void> {
		const camera = renderer ? rendererRuntimeStatus(renderer)?.camera : null;
		if (!camera) {
			message = $_("profiles.messages.runtime_camera_unavailable");
			return;
		}
		await applyProfileUpdates([
			["camera.target_x", camera.target[0]],
			["camera.target_y", camera.target[1]],
			["camera.target_z", camera.target[2]],
			["camera.longitude_deg", camera.longitude_deg],
			["camera.latitude_deg", camera.latitude_deg],
			["camera.radius", camera.radius],
			["camera.diagonal_fov_deg", camera.diagonal_fov_deg],
		]);
		message = $_("profiles.messages.runtime_camera_captured");
	}

	function runtimeCameraForSetting(setting: AvatarSetting | null): RendererCameraSnapshot | null {
		const renderer = rendererForSetting(setting);
		return renderer ? (rendererRuntimeStatus(renderer)?.camera ?? null) : null;
	}

	function captureRuntimeCameraForSetting(setting: AvatarSetting | null): Promise<void> {
		return captureRuntimeCameraToProfile(rendererForSetting(setting));
	}

	function runtimeWindowAvailableForSetting(setting: AvatarSetting | null): boolean {
		const renderer = rendererForSetting(setting);
		const status = renderer ? rendererRuntimeStatus(renderer) : null;
		return Boolean(status?.window_position || status?.window_inner_size);
	}

	async function captureRuntimeWindowToProfile(renderer: RendererInstance | null): Promise<void> {
		const status = renderer ? rendererRuntimeStatus(renderer) : null;
		const updates: ProfilePresetUpdate[] = [];
		if (status?.window_inner_size) {
			updates.push(["window.width", status.window_inner_size[0]]);
			updates.push(["window.height", status.window_inner_size[1]]);
		}
		if (status?.window_position) {
			updates.push(["window.x", status.window_position[0]]);
			updates.push(["window.y", status.window_position[1]]);
		}
		if (updates.length === 0) {
			message = $_("profiles.messages.runtime_window_unavailable");
			return;
		}
		await applyProfileUpdates(updates);
		message = $_("profiles.messages.runtime_window_captured");
	}

	function captureRuntimeWindowForSetting(setting: AvatarSetting | null): Promise<void> {
		return captureRuntimeWindowToProfile(rendererForSetting(setting));
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
		setActiveTab("logs", "jumpToRendererLog");
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
			message = $_("logs.messages.clipboard_write_failed", { values: { error: String(error) } });
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
			message = $_("logs.messages.clipboard_write_failed", { values: { error: String(error) } });
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
				message = $_("logs.messages.saved", { values: { name: basename(path) } });
			} catch (error) {
				message = String(error);
			}
			return;
		}
		browserDownloadText(text, `un-avatar-supervisor-logs-${filenameTimestamp()}.txt`);
	}

	async function revealSupervisorLogsDir(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = $_("logs.messages.open_folder_requires_tauri");
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

	function rendererStateLabel(state: RendererState): string {
		return localizedRendererStateLabel(state, $_);
	}

	function selectedRendererById(id: number | null): RendererInstance | null {
		return id == null ? null : (rendererTableById.get(id) ?? null);
	}

	function reconcileSelectedRendererId(instances: RendererInstance[] = renderers): void {
		const selectable = showStoppedRenderers
			? instances
			: instances.filter((renderer) => renderer.state !== "Exited" && renderer.state !== "Crashed");
		if (selectedRendererId != null && selectable.some((renderer) => renderer.id === selectedRendererId)) return;
		const nextId = selectable[0]?.id ?? null;
		if (selectedRendererId !== nextId) selectedRendererId = nextId;
		if (nextId == null && rendererPaneTab !== "overview") rendererPaneTab = "overview";
	}

	function avatarSettingBySelectedId(id: string | null): AvatarSetting | null {
		return id == null ? null : (avatarSettingById.get(id) ?? null);
	}

	function avatarSettingByIdOrManifestPath(idOrPath: string | null | undefined): AvatarSetting | null {
		if (!idOrPath) return null;
		return (
			avatarSettingById.get(idOrPath) ?? avatarSettings.find((setting) => sameNormalizedPath(setting.manifest_path, idOrPath)) ?? null
		);
	}

	function isLiveRenderer(renderer: RendererInstance): boolean {
		return renderer.pid != null && renderer.state !== "Exited" && renderer.state !== "Crashed";
	}

	function renderersForSetting(setting: AvatarSetting | null): RendererInstance[] {
		if (!setting) return [];
		return renderers.filter((renderer) => isLiveRenderer(renderer) && (renderer.profile_id != null
            ? renderer.profile_id === setting.id
            : sameNormalizedPath(renderer.manifest_path, setting.manifest_path)));
	}

	let reviewRendererTargets = $state<Record<string, number>>({});
	function reviewRendererForSetting(setting: AvatarSetting): RendererInstance | null {
		const live = renderersForSetting(setting);
		const selected = reviewRendererTargets[setting.id];
		if (selected !== undefined) return live.find((renderer) => renderer.id === selected) ?? null;
		return live.length === 1 ? live[0] : null;
	}

	function rendererForSetting(setting: AvatarSetting | null): RendererInstance | null {
		return renderersForSetting(setting)[0] ?? null;
	}

	function settingForRenderer(renderer: RendererInstance | null): AvatarSetting | null {
		if (!renderer) return null;
        if (renderer.profile_id != null) return avatarSettingById.get(renderer.profile_id) ?? null;
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
		const fieldLabel = profileFieldLabel(field, $_);
		pendingRendererRestart = { renderer, fieldLabel };
		message = $_("profiles.live.restart_after", { values: { field: fieldLabel, name: renderer.name } });
	}

	function queueTransparentEnableRestart(renderer: RendererInstance | null): void {
		if (!renderer) return;
		pendingRendererRestart = {
			renderer,
			fieldLabel: $_("profiles.fields.transparent_background"),
		};
		message = $_("profiles.live.transparent_restart", { values: { name: renderer.name } });
	}

	async function restartPendingRenderer(): Promise<void> {
		const pending = pendingRendererRestart;
		if (!pending || busy) return;
		const restarted = await restartRenderer(pending.renderer);
		if (restarted && pendingRendererRestart === pending) pendingRendererRestart = null;
	}

	let refreshAllRequest = 0;
	async function refreshAll(propagateError = false): Promise<void> {
		const request = ++refreshAllRequest;
		const settingsAtRequest = avatarSettings;
		traceFrontendEvent("refreshAll:enter", { activeTab });
		if (!hasTauriRuntime()) {
			renderers = [];
			runtimeStatuses = {};
			notifications = [];
			nativeNotificationStatus = null;
			rendererCacheStatus = null;
			avatarSettings = browserPreviewSettings;
			selectedSettingId = pickInitialSelectedSettingId(
				selectedSettingId,
				appSettings.last_selected_setting_id,
				browserPreviewSettings
			);
			launchTargetId = pickInitialLaunchTargetId(launchTargetId, selectedSettingId, browserPreviewSettings);
			message = $_("app.tauri_commands_disabled");
			return;
		}
		try {
			const instances = await traceAsync("invoke:list_renderers", () => invoke<RendererInstance[]>("list_renderers"), {
				source: "refreshAll",
			});
			const [settings, appNotifications, nativeNotifications] = await Promise.all([
				traceAsync("invoke:list_avatar_settings", () => invoke<AvatarSetting[]>("list_avatar_settings"), { source: "refreshAll" }),
				traceAsync("invoke:list_app_notifications", () => invoke<AppNotification[]>("list_app_notifications"), {
					source: "refreshAll",
				}),
				traceAsync(
					"invoke:get_native_notification_status",
					() => invoke<NativeNotificationStatus>("get_native_notification_status"),
					{
						source: "refreshAll",
					}
				),
			]);
			if (request !== refreshAllRequest) return;
			renderers = instances;
			if (avatarSettings === settingsAtRequest && pendingProfileSettingUpdates.size === 0) {
				loadedProfileDetailKey = "";
				avatarSettings = settings;
			}
			await refreshRendererRuntimeStatuses(instances);
			if (request !== refreshAllRequest) return;
			void refreshRendererCacheStatus();
			notifications = appNotifications;
			nativeNotificationStatus = nativeNotifications;
			reconcileSelectedRendererId(instances);
			selectedSettingId = pickInitialSelectedSettingId(selectedSettingId, appSettings.last_selected_setting_id, avatarSettings);
			launchTargetId = pickInitialLaunchTargetId(launchTargetId, selectedSettingId, avatarSettings);
			traceFrontendEvent("refreshAll:ok", { renderers: instances.length, settings: settings.length });
		} catch (error) {
			if (request !== refreshAllRequest) return;
			if (propagateError === true) throw error;
			message = String(error);
			traceFrontendEvent("refreshAll:error", { error: String(error) });
		}
	}

	async function refreshRendererCacheStatus(): Promise<void> {
		if (!hasTauriRuntime()) {
			rendererCacheStatus = null;
			return;
		}
		try {
			rendererCacheStatus = await invoke<RendererCacheStatus>("get_renderer_cache_status");
		} catch (error) {
			message = String(error);
		}
	}

	async function clearRendererCache(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = $_("app.cache_clear_requires_tauri");
			return;
		}
		if (visibleRenderers.length > 0) {
			message = $_("app.cache_clear_blocked_running");
			return;
		}
		const status = rendererCacheStatus;
		if (!status || status.total_files === 0) return;
		rendererCacheBusy = true;
		try {
			const next = await invoke<RendererCacheStatus>("clear_renderer_cache");
			rendererCacheStatus = next;
			avatarSettings = await invoke<AvatarSetting[]>("list_avatar_settings");
			message = $_("app.cache_cleared");
		} catch (error) {
			message = String(error);
		} finally {
			rendererCacheBusy = false;
		}
	}

	function startCacheDeleteHold(): void {
		if (busy || rendererCacheBusy || visibleRenderers.length > 0 || !rendererCacheStatus || rendererCacheStatus.total_files === 0)
			return;
		cancelCacheDeleteHold();
		cacheDeleteHoldProgress = 0;
		cacheDeleteHoldStartedAt = performance.now();
		const tick = () => {
			cacheDeleteHoldProgress = Math.min(1, (performance.now() - cacheDeleteHoldStartedAt) / deleteHoldDurationMs);
			if (cacheDeleteHoldProgress >= 1) {
				cancelCacheDeleteHold();
				void clearRendererCache();
				return;
			}
			cacheDeleteHoldTimer = window.requestAnimationFrame(tick);
		};
		cacheDeleteHoldTimer = window.requestAnimationFrame(tick);
	}

	function cancelCacheDeleteHold(): void {
		if (cacheDeleteHoldTimer != null) {
			window.cancelAnimationFrame(cacheDeleteHoldTimer);
			cacheDeleteHoldTimer = null;
		}
		cacheDeleteHoldProgress = 0;
	}

	async function refreshRendererRuntimeStatuses(instances: RendererInstance[] = renderers): Promise<void> {
		if (!hasTauriRuntime()) return;
		traceFrontendEvent("runtimeStatuses:enter", { count: instances.length });
		if (instances.length === 0) {
			runtimeStatuses = {};
			runtimeStatusSamples.clear();
			rendererActivationSeq.clear();
			traceFrontendEvent("runtimeStatuses:empty");
			return;
		}
		const runtimeStatusStarted = performance.now();
		const statuses = await Promise.all(
			instances.map((renderer) =>
				invoke<RendererRuntimeStatus>("get_renderer_runtime_status", {
					id: renderer.id,
				})
			)
		);
		const runtimeStatusElapsedMs = Math.round(performance.now() - runtimeStatusStarted);
		if (runtimeStatusElapsedMs >= 150) {
			traceFrontendEvent("invoke:get_renderer_runtime_status_batch:ok", {
				count: instances.length,
				elapsedMs: runtimeStatusElapsedMs,
			});
		}
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
		traceFrontendEvent("runtimeStatuses:ok", { count: statuses.length });
	}

	async function refreshRendererRuntimeView(): Promise<void> {
		if (!hasTauriRuntime()) return;
		if (runtimeRefreshBusy) {
			runtimeRefreshPending = true;
			traceFrontendEvent("runtimeView:pending", { activeTab });
			return;
		}
		runtimeRefreshBusy = true;
		try {
			runtimeRefreshPending = false;
			const traceThisRefresh = ++runtimeRefreshTraceSeq % 20 === 1;
			if (traceThisRefresh) traceFrontendEvent("runtimeView:enter", { activeTab });
			const instances = await traceAsync("invoke:list_renderers", () => invoke<RendererInstance[]>("list_renderers"), {
				source: "runtimeView",
			});
			renderers = instances;
			await refreshRendererRuntimeStatuses(instances);
			notifications = await traceAsync("invoke:list_app_notifications", () => invoke<AppNotification[]>("list_app_notifications"), {
				source: "runtimeView",
			});
			reconcileSelectedRendererId(instances);
			if (traceThisRefresh) traceFrontendEvent("runtimeView:ok", { renderers: instances.length });
		} catch (error) {
			message = String(error);
			traceFrontendEvent("runtimeView:error", { error: String(error) });
		} finally {
			runtimeRefreshBusy = false;
			if (runtimeRefreshPending) {
				void refreshRendererRuntimeView();
			}
		}
	}

	const runtimeCommandGenerations = new Map<string, number>();
	async function applyRendererCommand(
		targetRenderers: RendererInstance[],
		command: string,
		payload: (renderer: RendererInstance) => Record<string, unknown> | Promise<Record<string, unknown>>,
		logLabel = command
	): Promise<number> {
		let applied = 0;
		await Promise.all(
			targetRenderers.map(async (renderer) => {
				const key = `${renderer.id}\n${command}`;
				const generation = (runtimeCommandGenerations.get(key) ?? 0) + 1;
				runtimeCommandGenerations.set(key, generation);
				let failure: string | null = null;
				try {
					await invoke(command, { id: renderer.id, ...(await payload(renderer)) });
					applied += 1;
				} catch (error) {
					console.warn(logLabel, error);
					failure = String(error);
				} finally {
					if (runtimeCommandGenerations.get(key) === generation) {
						runtimeCommandFailures = runtimeCommandFailures.filter((item) => item.rendererId !== renderer.id || item.command !== command);
						if (failure !== null) runtimeCommandFailures = [...runtimeCommandFailures, { rendererId: renderer.id, name: renderer.name, command, error: failure }];
					}
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
			message = $_("logs.messages.notifications_cleared");
		} catch (error) {
			message = String(error);
		}
	}

	async function sendTestNativeNotification(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = $_("logs.messages.native_notifications_require_tauri");
			return;
		}
		busy = true;
		try {
			await invoke("send_test_native_notification");
			nativeNotificationStatus = await invoke<NativeNotificationStatus>("get_native_notification_status");
			message = $_("logs.messages.native_notification_test_sent");
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function launchSetting(id: string | null, allowGroupTarget = true): Promise<void> {
		if (busy || (!id && (!allowGroupTarget || !launchGroupName))) return;
		if (!hasTauriRuntime()) { message = $_("profiles.messages.launch_requires_tauri"); return; }
		const group = allowGroupTarget ? launchGroupName : null;
		const targets = group ? launchGroupSettings.map(setting => ({ id: setting.id, name: setting.name })) : [{ id, name: id ?? "" }];
		const originSelection = selectedRendererId;
		const originNavigation = editorNavigationGeneration;
		const originTab = activeTab;
		const opened: RendererInstance[] = [];
		let targetName = "";
		let outcome = "";
		let failed = false;
		busy = true;
		try {
			if (!targets.length) throw new Error($_("profiles.messages.empty_launch_group", { values: { group } }));
			for (const target of targets) {
				targetName = target.name || target.id || "";
				opened.push(await invoke<RendererInstance>("launch_renderer", { settingId: target.id }));
			}
			outcome = group ? $_("profiles.messages.opened_renderers", { values: { count: opened.length } }) : $_("profiles.messages.opened_renderer", { values: { name: opened[0].name } });
		} catch (error) {
			failed = true;
			outcome = $_("profiles.messages.launch_target_failed", { values: { name: targetName || group || "", error: String(error) } });
			if (opened.length) outcome += "\n" + $_("profiles.messages.partial_launch", { values: { count: opened.length, ids: opened.map(item => `#${item.id}`).join(", ") } });
		} finally {
			const lastInstance = opened[opened.length - 1];
			const retainedSelection = selectedRendererId === originSelection;
			if (lastInstance && retainedSelection) selectedRendererId = lastInstance.id;
			try { await refreshAll(true); }
			catch (error) { outcome += "\n" + $_("profiles.messages.launch_refresh_failed", { values: { error: String(error) } }); }
			finally {
				message = outcome;
				busy = false;
			}
			if (!failed && lastInstance && retainedSelection && selectedRendererId === lastInstance.id && editorNavigationGeneration === originNavigation && activeTab === originTab && appSettings.jump_to_renderers_on_quick_run) {
				setActiveTab("renderers", group ? "launchSetting:group" : "launchSetting:single");
			}
		}
	}

	async function prewarmSceneCache(settingId: string): Promise<void> {
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.cache_warmup_requires_tauri");
			return;
		}
		busy = true;
		sceneCachePrewarmProgress = {
			setting_id: settingId,
			phase: "starting",
			current: 0,
			total: 0,
			detail: $_("profiles.messages.cache_warming"),
			elapsed_secs: 0,
		};
		try {
			message = $_("profiles.messages.cache_warming");
			const result = await invoke<PrewarmSceneCacheResult>("prewarm_renderer_scene_cache", { settingId });
			message = result.detail
				? $_("profiles.messages.cache_warmed_detail", {
						values: { name: result.profile_name, seconds: formatFixed(result.elapsed_secs, 1), detail: result.detail },
					})
				: $_("profiles.messages.cache_warmed", {
						values: { name: result.profile_name, seconds: formatFixed(result.elapsed_secs, 1) },
					});
			avatarSettings = await invoke<AvatarSetting[]>("list_avatar_settings");
			notifications = await invoke<AppNotification[]>("list_app_notifications");
			await refreshRendererCacheStatus();
		} catch (error) {
			message = String(error);
			notifications = await invoke<AppNotification[]>("list_app_notifications");
		} finally {
			sceneCachePrewarmProgress = null;
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

	async function setTaskbarProfilePinned(settingId: string, pinned: boolean): Promise<void> {
		if (!hasTauriRuntime()) {
			const ids = appSettings.pinned_taskbar_profile_ids.filter((id) => id !== settingId);
			appSettings = {
				...appSettings,
				pinned_taskbar_profile_ids: pinned ? [...ids, settingId] : ids,
			};
			saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, appSettings);
			return;
		}
		busy = true;
		try {
			appSettings = await invoke<AppSettings>("set_taskbar_profile_pinned", { settingId, pinned });
			saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, appSettings);
			message = pinned ? $_("profiles.messages.taskbar_profile_pinned") : $_("profiles.messages.taskbar_profile_unpinned");
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function clearTaskbarProfilePins(): Promise<void> {
		if (!hasTauriRuntime()) {
			appSettings = { ...appSettings, pinned_taskbar_profile_ids: [] };
			saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, appSettings);
			return;
		}
		busy = true;
		try {
			appSettings = await invoke<AppSettings>("clear_taskbar_profile_pins");
			saveAppSettings(appSettingsStorageKey, legacyThemeModeStorageKey, appSettings);
			message = $_("settings.messages.taskbar_profile_pins_cleared");
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	$effect(() => {
		if (startupAutoLaunchReady && backendAppSettingsReady && !busy) {
			untrack(() => { void maybeAutoLaunchSelectedOnStartup(); });
		}
	});

	async function maybeAutoLaunchSelectedOnStartup(): Promise<void> {
		if (busy) return;
		if (!backendAppSettingsReady || startupAutoLaunchAttempted) return;
		startupAutoLaunchAttempted = true;
		if (!appSettings.auto_launch_selected_on_startup) return;
		if (!isValidLaunchTarget(launchTargetId, avatarSettings)) return;
		if (visibleRenderers.length > 0) return;
		setActiveTab("renderers", "autoLaunch");
		await launchSetting(launchTargetSetting?.id ?? null);
	}

	async function duplicateSetting(id: string | null): Promise<void> {
		if (!id || busy) return;
		if ([...pendingProfileSettingUpdates.values()].some((pending) => pending.settingId === id)
            || profileBatchDrafts.some((draft) => draft.settingId === id)) {
			message = $_("profiles.messages.duplicate_pending_edits");
			return;
		}
		const originSelection = selectedSettingId;
		const originTab = activeTab;
		const originNavigation = editorNavigationGeneration;
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.duplicate_requires_tauri");
			return;
		}
		busy = true;
		try {
			const setting = await invoke<AvatarSetting>("duplicate_avatar_setting", {
				settingId: id,
			});
			message = $_("profiles.messages.duplicated", { values: { name: setting.name } });
			avatarSettings = [...avatarSettings, setting].sort(compareAvatarSettings);
			if (editorNavigationGeneration === originNavigation && selectedSettingId === originSelection && activeTab === originTab) {
				selectedSettingId = setting.id;
				setActiveTab("settings", "duplicateSetting");
			}
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	let creatingProfile = $state(false);

	async function newSetting(): Promise<void> {
		if (busy) return;
		const originSelection = selectedSettingId;
		const originTab = activeTab;
		const originNavigation = editorNavigationGeneration;
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.new_requires_tauri");
			return;
		}
		busy = true;
		creatingProfile = true;
		try {
			const setting = await invoke<AvatarSetting>("new_avatar_setting");
			message = $_("profiles.messages.created", { values: { name: setting.name } });
			avatarSettings = [...avatarSettings, setting].sort(compareAvatarSettings);
			if (editorNavigationGeneration === originNavigation && selectedSettingId === originSelection && activeTab === originTab) {
				selectedSettingId = setting.id;
				setActiveTab("settings", "newSetting");
			}
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
			creatingProfile = false;
		}
	}

	async function deleteSetting(id: string | null, expectedRevision = id ? avatarSettingBySelectedId(id)?.revision : undefined): Promise<void> {
		if (!id || busy) return;
        if ([...pendingProfileSettingUpdates.values()].some((pending) => pending.settingId === id)
            || profileBatchDrafts.some((draft) => draft.settingId === id)) {
            message = $_("profiles.archive.pending_edits");
            return;
        }
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.delete_requires_tauri");
			return;
		}
		const setting = avatarSettingBySelectedId(id);
		if (!setting) return;
		busy = true;
		try {
			await invoke("delete_avatar_setting", { settingId: id, ...(expectedRevision ? { expectedRevision } : {}) });
			message = $_("profiles.messages.deleted", { values: { name: setting.name } });
			// Reflect the acknowledged deletion before selection effects or a failed
			// telemetry refresh can prevent refreshAll from replacing the list.
			avatarSettings = avatarSettings.filter((entry) => entry.id !== id);
			if (selectedSettingId === id) selectedSettingId = null;
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function restoreArchivedSetting(archiveId: string): Promise<void> {
		if (busy) throw new Error("Profile operation already in progress");
		busy = true;
		try {
			await invoke("restore_avatar_setting", { archiveId });
			try { avatarSettings = await invoke<AvatarSetting[]>("list_avatar_settings"); }
			catch (error) { throw new Error(`${$_("profiles.archive.refresh_failed")}: ${String(error)}`); }
		} finally { busy = false; }
	}

	function startDeleteHold(id: string | null): void {
		if (!id || busy) return;
		cancelDeleteHold();
		const expectedRevision = avatarSettingBySelectedId(id)?.revision;
		deleteHoldTargetId = id;
		deleteHoldProgress = 0;
		deleteHoldStartedAt = performance.now();
		const tick = () => {
			if (deleteHoldTargetId !== id) return;
			deleteHoldProgress = Math.min(1, (performance.now() - deleteHoldStartedAt) / deleteHoldDurationMs);
			if (deleteHoldProgress >= 1) {
				cancelDeleteHold();
				void deleteSetting(id, expectedRevision);
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
			message = $_("profiles.messages.reveal_requires_tauri");
			return;
		}
		try {
			await invoke("reveal_path", { path });
			message = $_("profiles.messages.revealed", { values: { name: basename(path) } });
		} catch (error) {
			message = String(error);
		}
	}

	async function revealProfilesDir(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.open_folder_requires_tauri");
			return;
		}
		try {
			await invoke("reveal_profiles_dir");
			message = $_("profiles.messages.opened_profiles_folder");
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
		if(orderSaving||orderDraft?.committed)return;
		const draft=orderDraft??{reviewed:avatarSettings.map(setting=>({...setting})),ids:avatarSettings.map(setting=>setting.id)};
		orderDraft=draft;orderSaving=true;
		const reviewed = draft.reviewed;
		try {
			const saved = await invoke<AvatarSetting[]>("reorder_avatar_settings", {
				settingIds: [...draft.ids],
				expectedRevisions: reviewed.every(setting=>typeof setting.revision==='string') ? Object.fromEntries(reviewed.map(setting=>[setting.id,setting.revision])) : undefined,
			});
			for (const setting of saved) {
				const previous=reviewed.find(item=>item.id===setting.id);
				if(previous&&avatarSettings.find(item=>item.id===setting.id)?.revision===previous.revision) replaceAvatarSetting(setting,false);
			}
			message = $_("profiles.messages.reordered");
			orderDraft=null;
		} catch (error) {
			if(String(error).startsWith('profile_order_committed_refresh_failed:')) {
				orderDraft={...draft,committed:true};
				message=$_('profiles.order_draft.committed');
			} else message = String(error);
		} finally {orderSaving=false;}
	}
	async function discardSettingsOrder(): Promise<void> {
		if(orderSaving)return;
		orderSaving=true;
		try{
			const beforeRead=avatarSettings;
			const versions=avatarSettings.map(setting=>({id:setting.id,revision:setting.revision}));
			if(pendingProfileSettingUpdates.size)throw Error($_("profiles.order_draft.changed_during_read"));
			const saved=await invoke<AvatarSetting[]>("list_avatar_settings");
			if(avatarSettings!==beforeRead||pendingProfileSettingUpdates.size||versions.length!==avatarSettings.length||versions.some((version,index)=>version.id!==avatarSettings[index]?.id||version.revision!==avatarSettings[index]?.revision))throw Error($_("profiles.order_draft.changed_during_read"));
			avatarSettings=saved;orderDraft=null;
		}
		catch(error){message=String(error);if(orderDraft)orderDraft={...orderDraft,error:String(error)};}
		finally{orderSaving=false;}
	}
	async function restoreProfileOrder(root:string,entry:OrderRecoveryEntry):Promise<void>{
		if(busy||orderSaving||orderDraft||pendingProfileSettingUpdates.size)throw Error($_("profiles.order_recovery.pending"));
		busy=true;
		try{avatarSettings=await invoke<AvatarSetting[]>("restore_profile_order",{expectedRoot:root,name:entry.name,revision:entry.revision});}
		catch(error){
			if(String(error).startsWith('profile_order_committed_refresh_failed:'))orderDraft={reviewed:avatarSettings.map(setting=>({...setting})),ids:avatarSettings.map(setting=>setting.id),committed:true};
			throw error;
		}
		finally{busy=false;}
	}

	function beginSettingPointerDrag(event: PointerEvent, settingId: string): void {
		if (busy || orderSaving || orderDraft || event.button !== 0) return;
		const card = (event.currentTarget as HTMLElement).closest<HTMLElement>("[data-profile-id]");
		const rect = card?.getBoundingClientRect();
		if (!rect) return;
		settingPointerDrag = {
			id: settingId,
			reviewed: avatarSettings.map(setting=>({...setting})),
			desiredIds: avatarSettings.map(setting=>setting.id),
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
		if(settingPointerDrag){const order=settingPointerDrag.reviewed.map(s=>s.id);avatarSettings=[...avatarSettings].sort((a,b)=>(order.indexOf(a.id)<0?order.length:order.indexOf(a.id))-(order.indexOf(b.id)<0?order.length:order.indexOf(b.id)));}
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
		if(settingPointerDrag)settingPointerDrag={...settingPointerDrag,desiredIds:avatarSettings.map(setting=>setting.id)};
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
		orderDraft={reviewed:drag.reviewed,ids:drag.desiredIds};
		event.preventDefault();
		event.stopPropagation();
		void saveSettingsOrder();
	}

	async function browseSettingPath(field: "avatar_path" | "icon_path", kind: "avatar" | "icon"): Promise<void> {
		const targetSetting = selectedSetting ? { ...selectedSetting } : null;
		if (!targetSetting) return;
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.file_picker_requires_tauri");
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
			scheduleCoalescedProfileSettingUpdate(targetSetting, field, path);
			await flushCoalescedProfileSettingUpdate(profileSettingUpdateKey(targetSetting.id, field));
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
		const expectedRevision = targetSetting.revision;
		if (!path.trim()) {
			await saveAvatarPath(path, rendererToRestart, targetSetting.id, expectedRevision);
			return;
		}
		if (looksLikeUnavatarPath(path)) {
			const metadata = await readUnavatarMetadataForPath(path, targetSetting);
			if (!metadata) {
				message = $_("unavatar_rights.messages.no_metadata");
				return;
			}
			const firstPreview = firstUnavatarPreviewDataUrl(metadata);
			unavatarProfileIconCrop = {
				enabled: Boolean(firstPreview),
				imageDataUrl: firstPreview,
				zoom: 1,
				offsetX: 0,
				offsetY: 0,
			};
			unavatarMetadataModal = {
				metadata,
				pendingPath: path,
				settingId: targetSetting.id,
				expectedRevision,
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
				expectedRevision,
				rendererToRestart,
			};
			useThumbnailForProfileIconOnAccept = Boolean(metadata.thumbnail_data_url);
			message = $_("vrm_metadata.messages.review_before_use");
			return;
		}
		await saveAvatarPath(path, rendererToRestart, targetSetting.id, expectedRevision);
	}

	async function saveAvatarPath(
		path: string,
		rendererToRestart: RendererInstance | null,
		settingId: string | null = selectedSetting?.id ?? null,
		expectedRevision?: string
	): Promise<AvatarSetting | null> {
		if (!settingId) return null;
		const setting = await invoke<AvatarSetting>("update_avatar_setting_value", {
			settingId,
			...(expectedRevision ? { expectedRevision } : {}),
			field: "avatar_path",
			value: path,
		});
		message = $_("profiles.messages.updated_avatar_path");
		replaceAvatarSetting(setting, false);
		queueRendererRestart(rendererToRestart, "avatar_path");
		return setting;
	}

	async function acceptVrmMetadataAndUse(): Promise<void> {
		const modal = vrmMetadataModal;
		if (!modal?.pendingPath) return;
		const useThumbnail = modal.avatarPathSaved || useThumbnailForProfileIconOnAccept;
		modal.saveError = undefined;
		busy = true;
		let recoveryModal = modal;
		try {
			if (!modal.avatarPathSaved) {
				const savedSetting = await saveAvatarPath(modal.pendingPath, modal.rendererToRestart, modal.settingId, modal.expectedRevision);
				recoveryModal = {
					...modal, avatarPathSaved: true,
					settingId: savedSetting?.id ?? modal.settingId,
					expectedRevision: savedSetting?.revision,
				};
				if (vrmMetadataModal === modal) {
					vrmMetadataModal = recoveryModal;
					recoveryModal = vrmMetadataModal;
				}
			}
			if (useThumbnail && modal.metadata.thumbnail_data_url) {
				await applySelectedAvatarThumbnail({
					rendererToRestart: modal.rendererToRestart,
					settingId: recoveryModal.settingId,
					avatarPath: modal.pendingPath,
					expectedRevision: recoveryModal.expectedRevision,
					manageBusy: false,
				});
			} else {
				message = $_("vrm_metadata.messages.updated_after_confirmation");
			}
			if (vrmMetadataModal === recoveryModal) vrmMetadataModal = null;
		} catch (error) {
			if (vrmMetadataModal === recoveryModal) recoveryModal.saveError = String(error);
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function acceptUnavatarMetadataAndUse(): Promise<void> {
		const modal = unavatarMetadataModal;
		if (!modal?.pendingPath) return;
		const crop = { ...unavatarProfileIconCrop };
		modal.saveError = undefined;
		busy = true;
		let recoveryModal = modal;
		try {
			const savedSetting = await saveAvatarPath(modal.pendingPath, modal.rendererToRestart, modal.settingId, modal.expectedRevision);
			recoveryModal = {
				...modal, pendingPath: null, iconSelectionOnly: true, avatarPathSaved: true,
				settingId: savedSetting?.id ?? modal.settingId,
				expectedRevision: savedSetting?.revision,
			};
			// Keep the same dialog mounted so its selected preview and crop survive.
			if (unavatarMetadataModal === modal) {
				unavatarMetadataModal = recoveryModal;
				recoveryModal = unavatarMetadataModal;
			}
			let savedProfileIcon = false;
			if (crop.enabled && crop.imageDataUrl) {
				await saveUnavatarProfileIconCrop(recoveryModal.settingId, modal.rendererToRestart, recoveryModal.expectedRevision, crop);
				savedProfileIcon = true;
			}
			if (unavatarMetadataModal === recoveryModal) unavatarMetadataModal = null;
			message = savedProfileIcon
				? $_("profiles.messages.updated_unavatar_sample_icon")
				: $_("vrm_metadata.messages.updated_after_confirmation");
		} catch (error) {
			if (unavatarMetadataModal === recoveryModal) recoveryModal.saveError = String(error);
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
			expectedRevision?: string;
		} = {}
	): Promise<void> {
		const settingId = options.settingId ?? selectedSetting?.manifest_path ?? selectedSetting?.id ?? null;
		const expectedRevision = options.expectedRevision ?? avatarSettingByIdOrManifestPath(settingId)?.revision;
		const avatarPath = options.avatarPath ?? selectedSetting?.avatar_path ?? null;
		// Explicit null retains the captured absence of a renderer after profile navigation.
		const rendererToRestart = options.rendererToRestart === undefined
			? restartableRendererForField("icon_path")
			: options.rendererToRestart;
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
		if (looksLikeUnavatarPath(avatarPath)) {
			await openUnavatarProfileIconPicker(avatarPath, settingId, rendererToRestart, expectedRevision);
			return;
		}
		if (manageBusy) busy = true;
		try {
			message = $_("profiles.messages.saving_thumbnail_icon");
			const setting = await invoke<AvatarSetting>("save_avatar_thumbnail_icon", {
				settingId,
				...(expectedRevision ? { expectedRevision } : {}),
				avatarPath,
			});
			replaceAvatarSetting(setting, false);
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

	async function openUnavatarProfileIconPicker(
		avatarPath: string,
		settingId: string,
		rendererToRestart: RendererInstance | null,
		expectedRevision?: string
	): Promise<void> {
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.thumbnail_requires_tauri");
			return;
		}
		busy = true;
		try {
			const targetSetting = avatarSettingByIdOrManifestPath(settingId) ?? selectedSetting;
			const metadata = await readUnavatarMetadataForPath(avatarPath, targetSetting);
			const firstPreview = metadata ? firstUnavatarPreviewDataUrl(metadata) : null;
			if (!metadata || !firstPreview) {
				message = $_("profiles.messages.no_unavatar_preview_icon");
				return;
			}
			unavatarProfileIconCrop = {
				enabled: true,
				imageDataUrl: firstPreview,
				zoom: 1,
				offsetX: 0,
				offsetY: 0,
			};
			unavatarMetadataModal = {
				metadata,
				pendingPath: null,
				settingId,
				rendererToRestart,
				iconSelectionOnly: true,
				expectedRevision,
			};
			message = $_("profiles.messages.choose_unavatar_sample_icon");
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function saveUnavatarProfileIconCrop(
		settingId: string | null | undefined,
		rendererToRestart: RendererInstance | null,
		expectedRevision?: string,
		crop: UnavatarProfileIconCrop = unavatarProfileIconCrop
	): Promise<AvatarSetting | null> {
		if (!settingId || !crop.imageDataUrl) return null;
		const setting = await invoke<AvatarSetting>("save_profile_icon_from_data_url", {
			settingId,
			...(expectedRevision ? { expectedRevision } : {}),
			imageDataUrl: crop.imageDataUrl,
			crop: {
				zoom: Number(crop.zoom) || 1,
				offset_x: Number(crop.offsetX) || 0,
				offset_y: Number(crop.offsetY) || 0,
			},
		});
		replaceAvatarSetting(setting, false);
		bumpProfileIconRevision(setting.icon_path);
		queueRendererRestart(rendererToRestart, "icon_path");
		return setting;
	}

	async function saveUnavatarProfileIconFromModal(): Promise<void> {
		const modal = unavatarMetadataModal;
		if (!modal || !unavatarProfileIconCrop.enabled || !unavatarProfileIconCrop.imageDataUrl) return;
		modal.saveError = undefined;
		busy = true;
		try {
			await saveUnavatarProfileIconCrop(modal.settingId, modal.rendererToRestart, modal.expectedRevision);
			if (unavatarMetadataModal === modal) unavatarMetadataModal = null;
			message = $_("profiles.messages.updated_unavatar_sample_icon");
		} catch (error) {
			if (unavatarMetadataModal === modal) modal.saveError = String(error);
			message = String(error);
		} finally {
			busy = false;
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
			message = $_("unavatar_rights.messages.avatar_selection_canceled");
		}
	}

	async function reviewSelectedAvatarMetadata(): Promise<void> {
		const targetSetting = selectedSetting ? { ...selectedSetting } : null;
		if (!targetSetting?.avatar_path) return;
		if (!hasTauriRuntime()) {
			message = $_("vrm_metadata.messages.tauri_required");
			return;
		}
		busy = true;
		try {
			if (looksLikeUnavatarPath(targetSetting.avatar_path)) {
				const metadata = await readUnavatarMetadataForPath(targetSetting.avatar_path, targetSetting);
				if (!metadata) {
					message = $_("unavatar_rights.messages.no_metadata");
					return;
				}
				unavatarProfileIconCrop = {
					enabled: false,
					imageDataUrl: firstUnavatarPreviewDataUrl(metadata),
					zoom: 1,
					offsetX: 0,
					offsetY: 0,
				};
				unavatarMetadataModal = {
					metadata,
					pendingPath: null,
					settingId: targetSetting.id,
				expectedRevision: targetSetting.revision,
					rendererToRestart: null,
				};
				return;
			}
			const metadata = await readVrmMetadataForPath(targetSetting.avatar_path, targetSetting);
			if (!metadata) {
				message = $_("vrm_metadata.messages.no_metadata");
				return;
			}
			vrmMetadataModal = {
				metadata,
				pendingPath: null,
				settingId: targetSetting.id,
				expectedRevision: targetSetting.revision,
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
		const applied = await applyRendererCommand([renderer], "set_renderer_window", () => rendererWindowPayload(setting));
		if (!applied) return;
		message = $_("renderers.messages.updated_window");
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
		await setAppliedMessage(applied, $_("renderers.messages.updated_background_color"), (count) =>
			$_("renderers.messages.updated_background_color_count", { values: { count } })
		);
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
		}
		if (fieldSetIncludesAny(fields, motionLookAtFields)) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_look_at", () => rendererLookAtPayload(setting));
			await setAppliedMessage(applied, $_("renderers.messages.updated_look_at"), (count) =>
				$_("renderers.messages.updated_look_at_count", { values: { count } })
			);
		}
		if (fieldSetIncludesAny(fields, motionReceiverFields)) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_motion_receivers", () =>
				rendererMotionReceiversPayload(setting)
			);
			await setAppliedMessage(applied, $_("renderers.messages.updated_motion_receivers"), (count) =>
				$_("renderers.messages.updated_motion_receivers_count", { values: { count } })
			);
		}
	}

	async function applyRuntimePhysicsUpdate(
		fields: readonly string[],
		setting: AvatarSetting,
		renderersToApply: RendererInstance[]
	): Promise<void> {
		if (
			fieldSetIncludes(fields, DYNAMICS_ENABLED_FIELD) ||
			fields.some(field => field.startsWith(DYNAMICS_OVERRIDE_FIELD_PREFIX) && canApplyWithoutRestart(field)) ||
			fieldSetStartsWith(fields, DYNAMICS_BONE_COLLIDER_FIELD_PREFIX)
		) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_dynamics", () => ({ setting }));
			await setAppliedMessage(applied, $_("renderers.messages.updated_physics"), (count) =>
				$_("renderers.messages.updated_physics_count", { values: { count } })
			);
		}
		if (fieldSetIncludes(fields, "debug.show_bone_colliders")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_show_bone_colliders", () => ({
				enabled: setting.show_bone_colliders,
			}));
			await setAppliedMessage(applied, $_("renderers.messages.updated_collider_display"), (count) =>
				$_("renderers.messages.updated_collider_display_count", { values: { count } })
			);
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
			await setAppliedMessage(applied, $_("renderers.messages.updated_avatar_effects"), (count) =>
				$_("renderers.messages.updated_avatar_effects_count", { values: { count } })
			);
		}
		if (fieldSetStartsWith(fields, "effects.avatar.contact_shadow.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_contact_shadow", () =>
				rendererContactShadowPayload(setting)
			);
			await setAppliedMessage(applied, $_("renderers.messages.updated_avatar_effects"), (count) =>
				$_("renderers.messages.updated_avatar_effects_count", { values: { count } })
			);
		}
	}

	async function applyRuntimePostEffectsUpdate(
		fields: readonly string[],
		setting: AvatarSetting,
		renderersToApply: RendererInstance[]
	): Promise<void> {
		if (fieldSetStartsWith(fields, "effects.post.ssao.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_ssao", () => rendererSsaoPayload(setting));
			await setAppliedMessage(applied, $_("renderers.messages.updated_avatar_effects"), (count) =>
				$_("renderers.messages.updated_avatar_effects_count", { values: { count } })
			);
		}
		if (fieldSetStartsWith(fields, "effects.post.bloom.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_bloom", () => rendererBloomPayload(setting));
			await setAppliedMessage(applied, $_("renderers.messages.updated_bloom"), (count) =>
				$_("renderers.messages.updated_bloom_count", { values: { count } })
			);
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
			await setAppliedMessage(applied, $_("renderers.messages.updated_color_adjustment"), (count) =>
				$_("renderers.messages.updated_color_adjustment_count", { values: { count } })
			);
		}
		if (fieldSetStartsWith(fields, "environment.lighting.")) {
			const applied = await applyRendererCommand(renderersToApply, "set_renderer_lighting", () => rendererLightingPayload(setting));
			await setAppliedMessage(applied, $_("renderers.messages.updated_lighting"), (count) =>
				$_("renderers.messages.updated_lighting_count", { values: { count } })
			);
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
		const failures: string[] = [];
		await Promise.all(
			renderersToApply.map(async (renderer) => {
				const failed = (command: string) => {
					const failure = runtimeCommandFailures.find((item) => item.rendererId === renderer.id && item.command === command);
					failures.push(`#${renderer.id}: ${failure?.error ?? command}`);
				};
				const locked = await applyRendererCommand([renderer], "set_renderer_camera_lock", () => ({ locked: setting.camera_locked }));
				if (!locked) { failed("set_renderer_camera_lock"); return; }
				if (!onlyCameraLock) {
					const positioned = await applyRendererCommand([renderer], "set_renderer_camera_state", () => ({
						target: setting.camera_target,
						longitudeDeg: setting.camera_longitude_deg,
						latitudeDeg: setting.camera_latitude_deg,
						radius: setting.camera_radius,
						diagonalFovDeg: setting.camera_diagonal_fov_deg,
						transition: { duration_ms: 320, easing: "ease_out_cubic", mode: "replace" },
					}));
					if (!positioned) { failed("set_renderer_camera_state"); return; }
				}
				applied += 1;
			})
		);
		await setAppliedMessage(applied, $_("renderers.messages.updated_camera"), (count) =>
			$_("renderers.messages.updated_camera_count", { values: { count } })
		);
		if (failures.length) {
			message = $_("profiles.runtime_recovery.camera_apply_partial", {
				values: { profile: `${setting.name} · ${setting.id}`, applied, failures: failures.join(" / ") },
			});
		}
	}

	const profileRuntimeApplyTails = new Map<string, Promise<void>>();

	async function retrySavedCameraForRenderer(rendererId: number): Promise<void> {
		const renderer = selectedRendererById(rendererId);
		const profile = settingForRenderer(renderer);
		if (!renderer || !profile || !isLiveRenderer(renderer)) return;
		const profileId = profile.id;
		const rendererPid = renderer.pid;
		busy = true;
		try {
			// Finish pending edits before the ordered read. Read BEFORE joining the
			// runtime queue: a later saved edit must still run after this snapshot.
			for (const [key, pending] of pendingProfileSettingUpdates) {
				if (pending.settingId === profileId) await flushCoalescedProfileSettingUpdate(key);
			}
			const saved = await invoke<AvatarSetting>("get_avatar_setting_details", { settingId: profileId });
			const preceding = profileRuntimeApplyTails.get(profileId) ?? Promise.resolve();
			const applying = preceding.catch(() => undefined).then(async () => {
				const current = selectedRendererById(rendererId);
				if (!current || current.pid !== rendererPid || !isLiveRenderer(current) || settingForRenderer(current)?.id !== profileId) return;
				await applyRuntimeCameraUpdate(["camera."], saved, [current]);
			});
			profileRuntimeApplyTails.set(profileId, applying);
			try { await applying; }
			finally { if (profileRuntimeApplyTails.get(profileId) === applying) profileRuntimeApplyTails.delete(profileId); }
		} catch (error) { message = String(error); }
		finally { busy = false; }
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

		// Each saved snapshot may emit several asynchronous renderer commands.
		// Finish that sequence before applying the next snapshot of this profile.
		// Capture renderer identities above, before waiting; never follow selection.
		const preceding = profileRuntimeApplyTails.get(setting.id) ?? Promise.resolve();
		const applying = preceding.catch(() => undefined).then(() => withDeferredRuntimeRefresh(async () => {
			await applyRuntimeWindowUpdate(setting, rendererToApply, enablingTransparentOnOpaqueRenderer);
			await applyRuntimeBackgroundColor(fields, setting, renderersToApply);
			await applyRuntimeMotionUpdate(fields, setting, renderersToApply);
			await applyRuntimePhysicsUpdate(fields, setting, renderersToApply);
			await applyRuntimeAvatarEffectsUpdate(fields, setting, renderersToApply);
			await applyRuntimePostEffectsUpdate(fields, setting, renderersToApply);
			await applyRuntimeEnvironmentUpdate(fields, setting, renderersToApply);
			await applyRuntimeCameraUpdate(fields, setting, renderersToApply);
		}));
		profileRuntimeApplyTails.set(setting.id, applying);
		try {
			await applying;
		} finally {
			if (profileRuntimeApplyTails.get(setting.id) === applying) profileRuntimeApplyTails.delete(setting.id);
		}
	}

	async function applyRuntimeProfileUpdate(
		field: string,
		value: ProfileSettingValue,
		setting: AvatarSetting,
		previousSetting: AvatarSetting
	): Promise<void> {
		await applyRuntimeProfileUpdates([[field, value]], setting, previousSetting);
	}

	type PendingProfileSettingUpdate = {
		settingId: string;
		expectedRevision?: string;
		field: string;
		value: ProfileSettingValue;
		timer: number | null;
		inFlight: boolean;
		generation: number;
	};

	const pendingProfileSettingUpdates = new Map<string, PendingProfileSettingUpdate>();
	let failedProfileSettingUpdates = $state<{ key: string; settingId: string; field: string; value: ProfileSettingValue; error: string }[]>([]);
	// Native close review will dispatch this same event before applying its quit
	// policy. Keep queued/coalesced edits and failed drafts protected even after
	// the user navigates to another profile or the Renderer page.
	function guardProfileDraftUnload(event: BeforeUnloadEvent): void {
		const hasSettingsDraft = Object.keys(appSettingsEarlyEdits).length > 0 || (backendAppSettingsReady && Boolean(appSettingsSaveError));
		const unresolved = busy || orderSaving || !!orderDraft || profileBatchDrafts.length > 0
			|| appSettingsPending > 0 || hasSettingsDraft
			|| pendingProfileSettingUpdates.size > 0 || failedProfileSettingUpdates.length > 0
			|| profileRuntimeApplyTails.size > 0
			|| !!vrmMetadataModal?.pendingPath || !!unavatarMetadataModal?.pendingPath;
		if (!unresolved) return;
		event.preventDefault();
		event.returnValue = "";
	}
	onMount(() => {

  const blocked = () => { message = ($locale ?? "ja-JP").startsWith("ja") ? "保存中または未保存の変更があります。保存状態を確認してから終了してください。Rendererは停止していません。" : "Changes are pending or unsaved. Review them before closing. Renderers have not been stopped."; };
  const failed = () => { message = ($locale ?? "ja-JP").startsWith("ja") ? "終了処理を確認できませんでした。状態を確認してから再操作してください。" : "Close could not be confirmed. Review the current state before retrying."; };
  window.addEventListener("beforeunload", guardProfileDraftUnload);
  document.documentElement.dataset.unCloseGuardReady = "true";
  window.addEventListener("un-avatar-close-blocked", blocked);
  window.addEventListener("un-avatar-close-failed", failed);
  return () => {
   delete document.documentElement.dataset.unCloseGuardReady;
  window.removeEventListener("beforeunload", guardProfileDraftUnload);
   window.removeEventListener("un-avatar-close-blocked", blocked);
   window.removeEventListener("un-avatar-close-failed", failed);
  };
	});

	// Keep the editable rule array separate from saved/runtime state, per profile.
	let profileMatchDrafts = $state<Record<string, AvatarSetting["dynamics_match_overrides"]>>({});
	let profileColliderDrafts = $state<Record<string, AvatarSetting["dynamics_collider_augment_overrides"]>>({});
	function physicsSettingForEditor(setting: AvatarSetting): AvatarSetting {
		const draft = profileMatchDrafts[setting.id];
		const colliders = profileColliderDrafts[setting.id];
		return { ...setting, ...(draft ? { dynamics_match_overrides: draft } : {}), ...(colliders ? { dynamics_collider_augment_overrides: colliders } : {}) };
	}

	function profileSettingUpdateKey(settingId: string, field: string): string {
		return `${settingId}\n${field}`;
	}

	function isCoalescibleProfileSettingValue(value: ProfileSettingValue): boolean {
		return typeof value === "number" || (Array.isArray(value) && value.every((item) => typeof item === "number"));
	}

	function scheduleCoalescedProfileSettingUpdate(setting: AvatarSetting, field: string, value: ProfileSettingValue): void {
		const key = profileSettingUpdateKey(setting.id, field);
		failedProfileSettingUpdates = failedProfileSettingUpdates.filter((item) => item.key !== key);
		const pending = pendingProfileSettingUpdates.get(key) ?? {
			settingId: setting.id,
			field,
			value,
			timer: null,
			inFlight: false,
			generation: 0,
		};
		// Remove Svelte state proxies before retaining nested settings. Existing
		// action/rule objects may be proxies even inside a newly created array.
		const capturedValue = structuredClone($state.snapshot(value)) as ProfileSettingValue;
		pending.value = capturedValue;
		if (field === DYNAMICS_MATCH_OVERRIDE_FIELD) {
			profileMatchDrafts[setting.id] = structuredClone(capturedValue) as AvatarSetting["dynamics_match_overrides"];
		}
		if (field === "physics.dynamics.solver.collider_augment_overrides") {
			profileColliderDrafts[setting.id] = structuredClone(capturedValue) as AvatarSetting["dynamics_collider_augment_overrides"];
		}
		pending.expectedRevision = setting.revision;
		pending.generation += 1;
		if (pending.timer !== null) {
			window.clearTimeout(pending.timer);
		}
		pending.timer = window.setTimeout(() => {
			pending.timer = null;
			void flushCoalescedProfileSettingUpdate(key);
		}, profileSettingUpdateCoalesceMs);
		pendingProfileSettingUpdates.set(key, pending);
	}

	async function flushCoalescedProfileSettingUpdate(key: string): Promise<void> {
		const pending = pendingProfileSettingUpdates.get(key);
		if (!pending || pending.inFlight) return;
		if (pending.timer !== null) {
			window.clearTimeout(pending.timer);
			pending.timer = null;
		}
		const previousSetting =
			avatarSettingBySelectedId(pending.settingId) ?? avatarSettings.find((setting) => setting.id === pending.settingId) ?? null;
		if (!previousSetting) {
			pendingProfileSettingUpdates.delete(key);
			return;
		}
		const value = pending.value;
		const generation = pending.generation;
		const rendererToRestart = canApplyWithoutRestart(pending.field)
			? null
			: isLaunchTimeRendererField(pending.field)
				? rendererForSetting(previousSetting)
				: null;
		pending.inFlight = true;
		let saved = false;
		failedProfileSettingUpdates = failedProfileSettingUpdates.filter((item) => item.key !== key);
		try {
			const setting = await invoke<AvatarSetting>("update_avatar_setting_value", {
				settingId: pending.settingId,
				...(pending.expectedRevision ? { expectedRevision: pending.expectedRevision } : {}),
				field: pending.field,
				value,
			});
			saved = true;
			message = $_("profiles.messages.updated_setting");
			replaceAvatarSetting(setting, selectedSettingId === pending.settingId);
			await applyRuntimeProfileUpdate(pending.field, value, setting, previousSetting);
			queueRendererRestart(rendererToRestart, pending.field);
		} catch (error) {
			message = String(error);
			if (!saved && pending.generation === generation) {
				failedProfileSettingUpdates = [...failedProfileSettingUpdates, { key, settingId: pending.settingId, field: pending.field, value, error: String(error) }];
			}
		} finally {
			pending.inFlight = false;
			if (pending.generation === generation) {
				if (saved) {
					pendingProfileSettingUpdates.delete(key);
					if (pending.field === DYNAMICS_MATCH_OVERRIDE_FIELD) delete profileMatchDrafts[pending.settingId];
					if (pending.field === "physics.dynamics.solver.collider_augment_overrides") delete profileColliderDrafts[pending.settingId];
				}
			} else if (pending.timer === null) {
				pending.timer = window.setTimeout(() => {
					pending.timer = null;
					void flushCoalescedProfileSettingUpdate(key);
				}, profileSettingUpdateCoalesceMs);
			}
		}
	}

	async function reviewProfileConflict(settingId: string): Promise<void> {
		selectProfileForEditing(settingId);
		setActiveTab("settings", "profileConflict:review");
		const generation = editorNavigationGeneration;
		try {
			const setting = await invoke<AvatarSetting>("get_avatar_setting_details", { settingId });
			if (generation === editorNavigationGeneration && selectedSettingId === settingId) replaceAvatarSetting(setting, false);
		} catch (error) { message = String(error); }
	}

	function profileUpdaterFor(target: AvatarSetting) {
		return (field: string, value: ProfileSettingValue) => updateSettingValue(field, value, target);
	}
	async function updateSettingValue(field: string, value: ProfileSettingValue, targetSetting: AvatarSetting | null = selectedSetting): Promise<void> {
		if (!targetSetting) return;
		if (!hasTauriRuntime()) {
			message = $_("profiles.messages.setting_changes_require_tauri");
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

		if (isCoalescibleProfileSettingValue(value)) {
			scheduleCoalescedProfileSettingUpdate(targetSetting, field, value);
			return;
		}

		scheduleCoalescedProfileSettingUpdate(targetSetting, field, value);
		await flushCoalescedProfileSettingUpdate(profileSettingUpdateKey(targetSetting.id, field));
	}

	async function refreshWardrobeOptionsForSetting(setting: AvatarSetting | null): Promise<void> {
		const key = setting?.avatar_path ? `${setting.manifest_path}\n${setting.avatar_path}` : "";
		const requestId = ++wardrobeOptionsRequestId;
		if (wardrobeOptionsKey !== key) wardrobeOptions = null;
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
			if (wardrobeOptionsKey === key && wardrobeOptionsRequestId === requestId) {
				wardrobeOptions = options;
			}
		} catch (error) {
			if (wardrobeOptionsKey === key && wardrobeOptionsRequestId === requestId) {
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
			message = $_("renderers.messages.activate_requires_tauri");
			return;
		}
		try {
			await invoke("activate_renderer_window", { id: renderer.id });
			message = $_("renderers.messages.activated", { values: { name: renderer.name } });
		} catch (error) {
			message = String(error);
		}
	}

	async function restartRenderer(renderer: RendererInstance | null): Promise<boolean> {
		if (!renderer) return false;
		const profileTarget = renderer.profile_id ?? renderer.manifest_path;
		if (!profileTarget) return false;
		if (!hasTauriRuntime()) {
			message = $_("renderers.messages.restart_requires_tauri");
			return false;
		}
		const originSelection = selectedRendererId;
		busy = true;
		try {
			// Resolve before stopping: a renamed profile keeps its identity, and a
			// missing profile must not tear down the still-running renderer.
			const setting = await invoke<AvatarSetting>("get_avatar_setting_details", { settingId: profileTarget });
			await invoke("stop_renderer", { id: renderer.id });
			const instance = await invoke<RendererInstance>("launch_renderer", {
				settingId: setting.id,
			});
			if (selectedRendererId === originSelection) selectedRendererId = instance.id;
			message = $_("renderers.messages.restarted", { values: { name: instance.name } });
			await refreshAll();
			return true;
		} catch (error) {
			message = String(error);
			return false;
		} finally {
			busy = false;
		}
	}

	async function resetRendererCamera(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) {
			message = $_("renderers.messages.reset_view_requires_tauri");
			return;
		}
		busy = true;
		try {
			await invoke("reset_renderer_camera", { id: renderer.id });
			message = $_("renderers.messages.reset_view", { values: { name: renderer.name } });
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
			message = $_("renderers.messages.screenshot_requires_tauri");
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
			message = $_("renderers.messages.screenshot_saved");
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
			message = $_("renderers.messages.open_folder_requires_tauri");
			return;
		}
		try {
			await invoke("reveal_path", { path: folder });
			message = $_("renderers.messages.opened_screenshots_folder");
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
			message = $_("renderers.messages.cleared_expression_overrides", { values: { name: renderer.name } });
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
			await refreshRendererRuntimeView();
			message = $_("renderers.messages.activated_wardrobe", { values: { name: renderer.name, wardrobe: wardrobeSetId } });
		} catch (error) {
			message = String(error);
		}
	}

	async function activateRendererRuntimeAction(renderer: RendererInstance | null, actionId: string, label: string): Promise<void> {
		if (!renderer || !hasTauriRuntime()) return;
		try {
			await invoke("activate_renderer_runtime_action", {
				id: renderer.id,
				actionId,
			});
			await refreshRendererRuntimeView();
			message = $_("renderers.messages.activated_animator_action", { values: { name: renderer.name, label } });
		} catch (error) {
			message = String(error);
		}
	}

	async function setRendererRuntimeParameter(
		renderer: RendererInstance | null,
		name: string,
		value: number,
		label: string
	): Promise<void> {
		if (!renderer || !hasTauriRuntime()) return;
		try {
			await invoke("set_renderer_runtime_parameter", {
				id: renderer.id,
				name,
				value,
			});
			await refreshRendererRuntimeView();
			message = $_("renderers.messages.activated_animator_action", { values: { name: renderer.name, label } });
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
			message = $_("renderers.messages.runtime_color_requires_tauri");
			return;
		}
		busy = true;
		try {
			await invoke("set_renderer_clear_color", { id: renderer.id, ...color });
			message = $_("renderers.messages.background_set", { values: { name: renderer.name, label } });
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
			message = $_("renderers.messages.runtime_output_requires_tauri");
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
					? $_("renderers.messages.spout_output_not_activated", { values: { name: renderer.name } })
					: $_("renderers.messages.spout_backend_missing");
			} else {
				message = enabled
					? $_("renderers.messages.spout_output_set", { values: { label, name: renderer.name } })
					: $_("renderers.messages.spout_output_disabled", { values: { label, name: renderer.name } });
			}
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function saveRendererSpoutProfile(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		busy = true;
		try {
			await invoke("save_renderer_spout_profile", { id: renderer.id });
			message = $_("renderers.messages.spout_profile_saved", { values: { name: renderer.name } });
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function restoreRendererOutputFromProfile(renderer: RendererInstance | null): Promise<void> {
		if (!renderer) return;
		if (!hasTauriRuntime()) return;
		try {
			await invoke("restore_renderer_output_from_profile", { id: renderer.id });
			message = $_("renderers.messages.output_restored_from_profile", { values: { name: renderer.name } });
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
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
			message = $_("renderers.messages.runtime_window_requires_tauri");
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
			message = $_("renderers.messages.window_set", { values: { name: renderer.name, label } });
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

	async function addRendererDynamicsMatchOverride(renderer: RendererInstance | null, seed: DynamicsMatchOverrideSetting): Promise<void> {
		if (!renderer?.manifest_path) return;
		if (!hasTauriRuntime()) return;
		const previousSetting = settingForRenderer(renderer);
		if (!previousSetting) return;
		const source_id_contains = (seed.source_id_contains ?? []).map((value) => value.trim()).filter(Boolean);
		if (source_id_contains.length === 0 && !seed.source_id?.trim()) return;
		const existing = previousSetting.dynamics_match_overrides ?? [];
		const normalizedContains = source_id_contains[0]?.toLowerCase() ?? "";
		const normalizedSourceId = seed.source_id?.trim().toLowerCase() ?? "";
		if (
			(normalizedContains &&
				existing.some((override) =>
					(override.source_id_contains ?? []).some((value) => value.trim().toLowerCase() === normalizedContains)
				)) ||
			(normalizedSourceId && existing.some((override) => override.source_id?.trim().toLowerCase() === normalizedSourceId))
		) {
			selectedSettingId = previousSetting.id;
			setActiveTab("settings", "rendererDetails:existingDynamicsMatchOverride");
			message = $_("profiles.messages.updated_setting");
			return;
		}
		const clampedRestResponse = seed.rest_response == null ? undefined : Math.max(0, Math.min(1, seed.rest_response));
		const clampedShapePreservation = seed.shape_preservation == null ? undefined : Math.max(0, Math.min(1, seed.shape_preservation));
		const clampedDampingHalfLife =
			seed.damping_half_life_ms == null ? undefined : Math.max(1, Math.min(10000, seed.damping_half_life_ms));
		const clampedBounceScale = seed.bounce_scale == null ? undefined : Math.max(0, Math.min(4, seed.bounce_scale));
		const clampedMotionCoupling = seed.motion_coupling == null ? undefined : Math.max(0, Math.min(1, seed.motion_coupling));
		const clampedStretchRangeScale = seed.stretch_range_scale == null ? undefined : Math.max(0, Math.min(4, seed.stretch_range_scale));
		const clampedStretchMotion = seed.stretch_motion == null ? undefined : Math.max(0, Math.min(1, seed.stretch_motion));
		const clampedXpbdCompliance = seed.xpbd_compliance == null ? undefined : Math.max(0, Math.min(10, seed.xpbd_compliance));
		const solver = seed.solver === "xpbd" ? "xpbd" : "verlet";
		const override: DynamicsMatchOverrideSetting = {
			name: seed.name?.trim() || source_id_contains[0] || seed.source_id?.trim(),
			...(seed.source_id?.trim() ? { source_id: seed.source_id.trim() } : {}),
			...(source_id_contains.length > 0 ? { source_id_contains } : {}),
			solver,
			...(clampedDampingHalfLife == null ? {} : { damping_half_life_ms: clampedDampingHalfLife }),
			...(clampedRestResponse == null ? {} : { rest_response: clampedRestResponse }),
			...(clampedShapePreservation == null ? {} : { shape_preservation: clampedShapePreservation }),
			...(clampedBounceScale == null ? {} : { bounce_scale: clampedBounceScale }),
			...(clampedMotionCoupling == null ? {} : { motion_coupling: clampedMotionCoupling }),
			...(clampedStretchRangeScale == null ? {} : { stretch_range_scale: clampedStretchRangeScale }),
			...(clampedStretchMotion == null ? {} : { stretch_motion: clampedStretchMotion }),
			...(solver === "xpbd" && clampedXpbdCompliance != null ? { xpbd_compliance: clampedXpbdCompliance } : {}),
		};
		const value: ProfileSettingValue = [...existing, override];
		const originNavigation = editorNavigationGeneration;
		const originSelection = selectedSettingId;
		const originTab = activeTab;
		try {
			const setting = await invoke<AvatarSetting>("update_avatar_setting_value", {
				settingId: previousSetting.id,
				...(previousSetting.revision ? { expectedRevision: previousSetting.revision } : {}),
				field: DYNAMICS_MATCH_OVERRIDE_FIELD,
				value,
			});
			message = $_("profiles.messages.updated_setting");
			replaceAvatarSetting(setting, false);
			await applyRuntimeProfileUpdate(DYNAMICS_MATCH_OVERRIDE_FIELD, value, setting, previousSetting);
			if (editorNavigationGeneration === originNavigation && selectedSettingId === originSelection && activeTab === originTab) {
				selectProfileForEditing(setting.id);
				setActiveTab("settings", "rendererDetails:addDynamicsMatchOverride");
			}
		} catch (error) {
			message = String(error);
		}
	}

	async function addRendererDynamicsGroupOverride(
		renderer: RendererInstance | null,
		sourceId: string,
		seed: DynamicsGroupOverrideSeed | null
	): Promise<void> {
		if (!renderer?.manifest_path) return;
		if (!hasTauriRuntime()) return;
		const previousSetting = settingForRenderer(renderer);
		if (!previousSetting) return;
		const source_id = sourceId.trim();
		if (!source_id) return;
		const existing = previousSetting.dynamics_group_overrides ?? [];
		const normalizedSource = source_id.toLowerCase();
		if (existing.some((override) => override.source_id.trim().toLowerCase() === normalizedSource)) {
			selectedSettingId = previousSetting.id;
			setActiveTab("settings", "rendererDetails:existingDynamicsGroupOverride");
			message = $_("profiles.messages.updated_setting");
			return;
		}
		const clampedRestResponse = seed?.rest_response == null ? undefined : Math.max(0, Math.min(1, seed.rest_response));
		const clampedShapePreservation = seed?.shape_preservation == null ? undefined : Math.max(0, Math.min(1, seed.shape_preservation));
		const clampedDampingHalfLife =
			seed?.damping_half_life_ms == null ? undefined : Math.max(1, Math.min(10000, seed.damping_half_life_ms));
		const clampedBounceScale = seed?.bounce_scale == null ? undefined : Math.max(0, Math.min(4, seed.bounce_scale));
		const clampedMotionCoupling = seed?.motion_coupling == null ? undefined : Math.max(0, Math.min(1, seed.motion_coupling));
		const clampedXpbdCompliance = seed?.xpbd_compliance == null ? undefined : Math.max(0, Math.min(10, seed.xpbd_compliance));
		const solver = seed?.solver === "xpbd" ? "xpbd" : "verlet";
		const override: DynamicsGroupOverrideSetting = {
			source_id,
			solver,
			...(clampedDampingHalfLife == null ? {} : { damping_half_life_ms: clampedDampingHalfLife }),
			...(clampedRestResponse == null ? {} : { rest_response: clampedRestResponse }),
			...(clampedShapePreservation == null ? {} : { shape_preservation: clampedShapePreservation }),
			...(clampedBounceScale == null ? {} : { bounce_scale: clampedBounceScale }),
			...(clampedMotionCoupling == null ? {} : { motion_coupling: clampedMotionCoupling }),
			...(solver === "xpbd" && clampedXpbdCompliance != null ? { xpbd_compliance: clampedXpbdCompliance } : {}),
		};
		const value: ProfileSettingValue = [...existing, override];
		const originNavigation = editorNavigationGeneration;
		const originSelection = selectedSettingId;
		const originTab = activeTab;
		try {
			const setting = await invoke<AvatarSetting>("update_avatar_setting_value", {
				settingId: previousSetting.id,
				...(previousSetting.revision ? { expectedRevision: previousSetting.revision } : {}),
				field: "physics.dynamics.solver.group_overrides",
				value,
			});
			message = $_("profiles.messages.updated_setting");
			replaceAvatarSetting(setting, false);
			await applyRuntimeProfileUpdate("physics.dynamics.solver.group_overrides", value, setting, previousSetting);
			if (editorNavigationGeneration === originNavigation && selectedSettingId === originSelection && activeTab === originTab) {
				selectProfileForEditing(setting.id);
				setActiveTab("settings", "rendererDetails:addDynamicsGroupOverride");
			}
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
			message = $_("renderers.messages.camera_orbit_set", {
				values: { kind: $_(`profiles.editor.options.camera_orbit_${kind}`), name: renderer.name },
			});
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
			message = $_("renderers.messages.camera_saved_to_profile", { values: { name: renderer.name } });
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
			message = $_("renderers.messages.camera_restored_from_profile", { values: { name: renderer.name } });
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
			message = $_("renderers.messages.window_saved_to_profile", { values: { name: renderer.name } });
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
			message = $_("renderers.messages.window_restored_from_profile", { values: { name: renderer.name } });
			await refreshRendererRuntimeView();
		} catch (error) {
			message = String(error);
		}
	}

	async function stopRenderer(id: number | null): Promise<void> {
		if (id == null) return;
		if (!hasTauriRuntime()) {
			message = $_("renderers.messages.stop_requires_tauri");
			return;
		}
		busy = true;
		try {
			await invoke("stop_renderer", { id });
			message = $_("renderers.messages.stop_requested");
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	async function stopAll(): Promise<void> {
		if (!hasTauriRuntime()) {
			message = $_("renderers.messages.stop_all_requires_tauri");
			return;
		}
		busy = true;
		try {
			await invoke("stop_all_renderers");
			message = $_("renderers.messages.all_stopped");
			await refreshAll();
		} catch (error) {
			message = String(error);
		} finally {
			busy = false;
		}
	}

	$effect(() => {
		traceFrontendEvent("activeTab:committed", { activeTab });
	});

	onMount(() => {
		let cancelled = false;
		void (async () => {
			await loadBackendAppSettings();
			await refreshAll();
			if (!cancelled) startupAutoLaunchReady = true;
		})();
		return () => {
			cancelled = true;
		};
	});

	$effect(() => {
		let cancelled = false;
		let timer: number | null = null;
		const scheduleRuntimeRefresh = () => {
			const interval =
				visibleRenderers.length === 0
					? idleRuntimeRefreshIntervalMs
					: wardrobeTransitionTelemetryActive
						? wardrobeTransitionRuntimeRefreshIntervalMs
						: runtimeRefreshIntervalMs;
			timer = window.setTimeout(() => {
				if (activeTab === "renderers" || activeTab === "settings") {
					refreshRendererRuntimeView();
				}
				if (!cancelled) scheduleRuntimeRefresh();
			}, interval);
		};
		scheduleRuntimeRefresh();
		return () => {
			cancelled = true;
			if (timer != null) window.clearTimeout(timer);
		};
	});

	$effect(() => {
		if (!hasTauriRuntime()) return;
		let cancelled = false;
		let unlisten: (() => void) | null = null;
		void listen<string>("profile-open-requested", async (event) => {
			const profileId = String(event.payload ?? "").trim();
			if (!profileId) return;
			await refreshAll();
			if (!avatarSettings.some((setting) => setting.id === profileId)) return;
			selectProfileForEditing(profileId);
			launchTargetId = pickInitialLaunchTargetId(launchTargetId, selectedSettingId, avatarSettings);
			setActiveTab("settings", "profile-open-requested");
		})
			.then((handler) => {
				if (cancelled) {
					handler();
				} else {
					unlisten = handler;
				}
			})
			.catch((error) => {
				message = String(error);
			});
		return () => {
			cancelled = true;
			unlisten?.();
		};
	});

	$effect(() => {
		if (!hasTauriRuntime()) return;
		let cancelled = false;
		let unlisten: (() => void) | null = null;
		void listen<SceneCachePrewarmProgress>("scene-cache-prewarm-progress", (event) => {
			if (!cancelled) sceneCachePrewarmProgress = event.payload;
		})
			.then((handler) => {
				if (cancelled) handler();
				else unlisten = handler;
			})
			.catch((error) => {
				message = String(error);
			});
		return () => {
			cancelled = true;
			unlisten?.();
		};
	});

	$effect(() => {
		document.documentElement.dataset.theme = resolvedTheme;
		document.documentElement.dataset.themeMode = appSettings.theme_mode;
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
		cancelCacheDeleteHold();
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

	$effect(() => {
		const setting = selectedSetting;
		// Read only the selected profile reactively. Resetting the request cache on failure
		// must not schedule the same failed request again in a microtask loop.
		if (setting) untrack(() => void loadSelectedProfileDetails(setting));
	});

	/// selectedSettingId が変わるたびに Tauri 側へ保存し、次回起動時に復元できるようにする。
	/// 初期化フェーズ (backendAppSettingsReady 前) や記録済み値と一致する場合はスキップ。
	$effect(() => {
		const id = selectedSettingId;
		if (!backendAppSettingsReady) return;
		untrack(() => { void persistLastSelectedSettingId(id); });
	});

	$effect(() => {
		saveLaunchTargetId(launchTargetStorageKey, launchTargetId);
	});
</script>

<svelte:head>
	<title>UN Avatar Supervisor</title>
</svelte:head>

<main class="shell" class:preview-feature-outline={previewFeatureOutline}>
	<header class="topbar">
		<div class="brand">
			<img src="/un-avatar-artwork-supervisor.png" alt="" />
			<div>
				<h1>{$_("app.name")}</h1>
				<p>{$_("app.subtitle")}</p>
			</div>
		</div>
		<div class="status-strip" aria-label={$_("app.status_aria")}>
			{#if isolatedDesignPreview}
				<strong role="status">{$locale?.startsWith('ja') ? '比較用 · 模擬データ（実機起動・設定保存なし）' : 'Preview · Simulated data (no native launch or file save)'}</strong>
			{/if}
			<span><Activity size={14} />{$_("app.running_count", { values: { count: runningCount } })}</span>
			<span class:warn={issueCount > 0}><AlertTriangle size={14} />{$_("app.issue_count", { values: { count: issueCount } })}</span>
			<span class="status-message" role="status">{message}</span>
			{#if appSettingsPending > 0 || appSettingsSaveError}
				<button onclick={() => setActiveTab("app", "settingsSave:review")}>{($locale ?? "ja-JP").startsWith("ja") ? (!backendAppSettingsReady ? "アプリ設定の読込を確認" : appSettingsPending > 0 ? "アプリ設定を保存中" : "アプリ設定の保存を確認") : (!backendAppSettingsReady ? "Review settings loading" : appSettingsPending > 0 ? "Saving settings" : "Review settings save")}</button>
			{/if}
			{#if screenshotNoticePath}
				<button class="screenshot-notice" title={screenshotNoticePath} onclick={() => void revealScreenshotFolder()}>
					<FolderOpen size={14} />{basename(screenshotNoticePath)}
				</button>
			{/if}
			{#if pendingRendererRestart && !(activeTab === "settings" && selectedSetting && settingForRenderer(pendingRendererRestart.renderer)?.id === selectedSetting.id)}
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
				title={appSettings.theme_mode === "system"
					? $_("theme.defaulting_to_os", { values: { os: osTheme } })
					: $_("theme.override_saved")}
				onChange={setThemeMode}
			/>
		</div>
	</header>

	<div class="workspace">
		<aside class="side-rail" aria-label={$_("sidebar.aria")}>
			{#if previewOperationsA}
				<button class:active={activeTab === 'settings'} onclick={() => setActiveTab('settings', 'operations-proposal:navigation')}><FileCog size={17} />{$_("supervisor_operations.profiles_runtime")}</button>
				<button class:active={activeTab === 'renderers'} onclick={() => setActiveTab('renderers', 'operations-proposal:navigation')}><Monitor size={17} />{$_("supervisor_operations.all_instances")}</button>
				<button class:active={activeTab === 'map'} onclick={() => setActiveTab('map', 'operations-proposal:navigation')}>{$_('sidebar.feature_map')}</button>
				<button class:active={activeTab === 'logs'} onclick={() => setActiveTab('logs', 'operations-proposal:navigation')}><TerminalSquare size={17} />{$_('sidebar.logs')}</button>
				<button class:active={activeTab === 'app'} onclick={() => setActiveTab('app', 'operations-proposal:navigation')}><Settings size={17} />{$_('sidebar.settings')}</button>
			{:else}
			<button class:active={activeTab === "map"} onclick={() => setActiveTab("map", "side-rail")}>{$_("sidebar.feature_map")}</button>
			<button class:active={activeTab === "renderers"} onclick={() => setActiveTab("renderers", "side-rail")}
				><Monitor size={17} />{$_("sidebar.renderers")}</button
			>
			<button class:active={activeTab === "settings"} onclick={() => setActiveTab("settings", "side-rail")}
				><FileCog size={17} />{$_("sidebar.profiles")}</button
			>
			<button data-preview-duplicate="logs" class:active={activeTab === "logs"} onclick={() => setActiveTab("logs", "side-rail")}
				><TerminalSquare size={17} />{$_("sidebar.logs")}</button
			>
			<button data-preview-duplicate="app" class:active={activeTab === "app"} onclick={() => setActiveTab("app", "side-rail")}
				><Settings size={17} />{$_("sidebar.settings")}</button
			>
			{/if}
			{#if previewFeatureOutline}
				<PreviewAvatarFeatureIndex language={$locale ?? 'ja-JP'} targetName={selectedSetting ? `${selectedSetting.name} · ${selectedSetting.id}` : ''} activeSection={activeTab === 'settings' ? activeProfileSection : ''} activeDestination={activeTab} sectionItems={profileSectionNavItems} onOpen={openFeature} />
			{/if}
			<div class="rail-footer">
				<div class="cache-panel" aria-label={$_("app.cache_panel_aria")}>
					<div class="cache-panel-head">
						<span><Database size={14} />{$_("app.cache_title")}</span>
						<div class="cache-panel-actions">
							<button
								class="icon-button"
								title={$_("app.cache_refresh")}
								disabled={rendererCacheBusy}
								onclick={() => void refreshRendererCacheStatus()}><RefreshCw size={14} /></button
							>
							<button
								class="icon-button danger-icon hold-delete"
								title={visibleRenderers.length > 0 ? $_("app.cache_clear_blocked_running") : $_("app.cache_clear")}
								disabled={busy ||
									rendererCacheBusy ||
									visibleRenderers.length > 0 ||
									!rendererCacheStatus ||
									rendererCacheStatus.total_files === 0}
								style={`--hold-progress: ${cacheDeleteHoldProgress}`}
								onpointerdown={startCacheDeleteHold}
								onpointerup={cancelCacheDeleteHold}
								onpointercancel={cancelCacheDeleteHold}
								onpointerleave={cancelCacheDeleteHold}
								onkeydown={(event) => {
									if (event.key === " " || event.key === "Enter") {
										event.preventDefault();
										startCacheDeleteHold();
									}
								}}
								onkeyup={cancelCacheDeleteHold}><span class="hold-fill"></span><Trash2 size={14} /></button
							>
						</div>
					</div>
					<div class="cache-total">{rendererCacheTotalLabel}</div>
					<div class="cache-groups">
						<div title={rendererCacheStatus?.texture.path ?? ""}>
							<span>{$_("app.cache_texture")}</span>
							<strong>{formatCacheBytes(rendererCacheStatus?.texture.bytes)}</strong>
						</div>
						<div title={rendererCacheStatus?.pipeline.path ?? ""}>
							<span>{$_("app.cache_pipeline")}</span>
							<strong>{formatCacheBytes(rendererCacheStatus?.pipeline.bytes)}</strong>
						</div>
					</div>
				</div>
				<button class="danger" disabled={busy || visibleRenderers.length === 0} onclick={stopAll}
					><AlertTriangle size={16} />{$_("app.stop_all")}</button
				>
			</div>
		</aside>

		{#if activeTab === "map"}
			<AvatarFeatureMap
				profiles={avatarSettings}
				bind:selectedProfileId={() => selectedSettingId, selectProfileForEditing}
				bind:query={featureQuery}
				onOpen={(feature) => void openFeature(feature)}
			/>
		{:else if activeTab === "renderers"}
			<section class="view renderers-view">
				<RenderersToolbar
					{busy}
					{launchTargetSetting}
					{launchGroupName}
					{launchTargetId}
					{launchMenuOpen}
					{showStoppedRenderers}
					{avatarSettings}
					{profileGroups}
					{message}
					{iconSrc}
					groupCount={(group) => avatarSettingsByGroup.get(group)?.length ?? 0}
					onLaunch={() => launchSetting(launchTargetSetting?.id ?? null)}
					onRefresh={refreshAll}
					onToggleLaunchMenu={() => (launchMenuOpen = !launchMenuOpen)}
					onShowStoppedRenderersChange={(checked) => (showStoppedRenderers = checked)}
					onSelectGroup={(group) => {
						launchTargetId = `group:${group}`;
						launchMenuOpen = false;
					}}
					onSelectSetting={(settingId) => {
						launchTargetId = settingId;
						selectProfileForEditing(settingId);
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
							selectProfileForEditing(profileId);
							setActiveTab("settings", "rendererStage:viewProfile");
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
						onRestartRenderer={async (rendererId) => {
							const renderer = selectedRendererById(rendererId);
							if (renderer) await restartRenderer(renderer);
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
						renderers={rendererTableRenderers.map((renderer) => ({ ...renderer, name: settingForRenderer(renderer)?.name ?? renderer.name }))}
						{selectedRendererId}
						{showStoppedRenderers}
						emptySummary={launchGroupName
							? $_("renderers.ready.group_count", {
									values: { count: launchGroupSettings.length },
								})
							: launchTargetSetting
								? localizedSettingSummary(launchTargetSetting, $_)
								: $_("renderers.details.none_selected")}
						statusForRenderer={(rendererId) => runtimeStatuses[rendererId] ?? null}
						iconSrcForRenderer={(rendererId) => iconSrc(settingForRenderer(rendererById.get(rendererId) ?? null)?.icon_path ?? null)}
						onSelectRenderer={(rendererId) => (selectedRendererId = rendererId)}
						onOpenRendererLog={(rendererId) => {
							const renderer = selectedRendererById(rendererId);
							if (renderer) jumpToRendererLog(renderer);
						}}
					/>

					<RendererDetailsPanel
						renderer={selectedRenderer}
						runtimeStatus={selectedRuntimeStatus}
						{rendererPaneTab}
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
						onSaveSpoutProfile={() => {
							if (!selectedRenderer) return;
							return saveRendererSpoutProfile(selectedRenderer);
						}}
						onRestoreOutput={() => {
							if (!selectedRenderer) return;
							return restoreRendererOutputFromProfile(selectedRenderer);
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
						onSetRuntimeParameter={(rendererId, name, value, label) => {
							const renderer = selectedRendererById(rendererId);
							return setRendererRuntimeParameter(renderer, name, value, label);
						}}
						onActivateRuntimeAction={(rendererId, actionId, label) => {
							const renderer = selectedRendererById(rendererId);
							return activateRendererRuntimeAction(renderer, actionId, label);
						}}
						onActivateWardrobeMenuCandidate={(rendererId, actionId, wardrobeSetId) => {
							const renderer = selectedRendererById(rendererId);
							return activateWardrobeMenuCandidate(renderer, actionId, wardrobeSetId);
						}}
						onSetDynamicsEnabled={(rendererId, sourceId, enabled) => {
							const renderer = selectedRendererById(rendererId);
							return setRendererDynamicsEnabled(renderer, sourceId, enabled);
						}}
						onAddDynamicsMatchOverride={(rendererId, seed) => {
							const renderer = selectedRendererById(rendererId);
							return addRendererDynamicsMatchOverride(renderer, seed);
						}}
						onAddDynamicsGroupOverride={(rendererId, sourceId, seed) => {
							const renderer = selectedRendererById(rendererId);
							return addRendererDynamicsGroupOverride(renderer, sourceId, seed);
						}}
						onOpenProfile={() => {
							if (!launchTargetSetting) return;
							selectProfileForEditing(launchGroupSettings[0]?.id ?? launchTargetSetting.id);
							setActiveTab("settings", "rendererDetails:openProfile");
						}}
						onRevealProfilesDir={() => revealProfilesDir()}
					/>
				</div>
			</section>
		{:else if activeTab === "settings"}
			<section
				class="view settings-view"
                class:profile-edit-focus={previewEditWorkspace}
				aria-label={$_("profiles.aria")}
				onpointerover={updateProfileHintFromEvent}
				onfocusin={updateProfileHintFromEvent}
				onpointerleave={clearProfileHint}
			>
				<ProfilesToolbar
					subduedCreation={previewOperationsA}
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
                <div class="profile-history-tools">
				<DeletedProfiles {busy} existingIds={avatarSettings.map((setting) => setting.id)} onRestore={restoreArchivedSetting} />
				<ProfileOrderRecovery busy={busy||orderSaving||!!orderDraft} onRestore={restoreProfileOrder}/>
                </div>
				{#if profileBatchDrafts.length}
					<section class="panel profile-save-recovery" aria-label={($locale ?? "ja-JP").startsWith("ja") ? "一括変更の保存確認" : "Batch save review"}>
						{#each profileBatchDrafts as draft (draft.id)}
							<details><summary>{draft.name} · {draft.settingId} · {draft.state === "saving" ? (($locale ?? "ja-JP").startsWith("ja") ? "保存中" : "Saving") : (($locale ?? "ja-JP").startsWith("ja") ? "保存未確認" : "Save unconfirmed")}</summary>
								<p>{draft.error}</p>
								{#if draft.state === "failed"}
									<p>{($locale ?? "ja-JP").startsWith("ja") ? "要求した一括変更を保持しています。元の対象の保存内容と比較し、必要な変更を新しい編集として行ってください。確認だけでは上書きしません。" : "The requested batch is retained. Compare the original profile's saved values and make the changes you still need as a new edit. Review does not overwrite values."}</p>
									<button disabled={busy} onclick={() => void reviewProfileConflict(draft.settingId)}>{$_("profiles.save_recovery.review_latest")}</button>
									<button disabled={busy} onclick={() => profileBatchDrafts = profileBatchDrafts.filter(item=>item.id!==draft.id)}>{($locale ?? "ja-JP").startsWith("ja") ? "保持した一括変更を破棄" : "Discard retained batch"}</button>
								{/if}
								<details><summary>{($locale ?? "ja-JP").startsWith("ja") ? `保持した ${draft.updates.length} 項目の詳細` : `${draft.updates.length} retained fields`}</summary><pre>{JSON.stringify(draft.updates,null,2)}</pre></details>
							</details>
						{/each}
					</section>
				{/if}
				{#if failedProfileSettingUpdates.length}
					<section class="panel profile-save-recovery" aria-label={$_("profiles.save_recovery.title")}>
						<h3>{$_("profiles.save_recovery.title")}</h3>
						<p>{$_("profiles.save_recovery.explanation")}</p>
						{#each failedProfileSettingUpdates as failure (failure.key)}
							<div>
								<strong>{avatarSettings.find((setting) => setting.id === failure.settingId)?.name ?? failure.settingId} · {failure.settingId}</strong>
								<p>{failure.field}: {JSON.stringify(failure.value)}</p>

								{#if failure.error.includes("profile_conflict")}
									<p>{$_("profiles.save_recovery.conflict_explanation")}</p>
									<button disabled={busy} onclick={() => void reviewProfileConflict(failure.settingId)}>{$_("profiles.save_recovery.review_latest")}</button>
								{:else}
									<button disabled={busy} onclick={() => void flushCoalescedProfileSettingUpdate(failure.key)}>{$_("profiles.save_recovery.retry")}</button>
								{/if}
								<details><summary>{($locale ?? "ja-JP").startsWith("ja") ? "診断情報" : "Diagnostic details"}</summary><pre>{failure.error}</pre></details>
							</div>
						{/each}
					</section>
				{/if}
				{#if runtimeCommandFailures.length}
					<section class="panel profile-save-recovery" aria-label={$_("profiles.runtime_recovery.title")}>
						<h3>{$_("profiles.runtime_recovery.title")}</h3>
						<p>{$_("profiles.runtime_recovery.explanation")}</p>
						{#each runtimeCommandFailures as failure (`${failure.rendererId}:${failure.command}`)}
							<p role="alert">{failure.name} · #{failure.rendererId} — {failure.command}: {failure.error}</p>
                                {#if import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" && ["set_renderer_camera_lock", "set_renderer_camera_state"].includes(failure.command)}
                                    <button disabled={busy} onclick={() => void retrySavedCameraForRenderer(failure.rendererId)}>{$_("profiles.runtime_recovery.retry_camera", { values: { id: failure.rendererId } })}</button>
                                {/if}
						{/each}
					</section>
				{/if}
				{#if selectedSetting}
					{@const liveRenderer = previewFeatureOutline ? reviewRendererForSetting(selectedSetting) : rendererForSetting(selectedSetting)}
					{@const liveRendererCount = runningCountForSetting(selectedSetting)}
					{@const profilePendingRestart =
						pendingRendererRestart &&
						settingForRenderer(pendingRendererRestart.renderer)?.id === selectedSetting.id
							? pendingRendererRestart
							: null}
                    {#snippet existingProfileStage()}
					<ProfileStage
                        compactTools={previewEditWorkspace}
						setting={selectedSetting}
						iconUrl={iconSrc(selectedSetting.icon_path)}
						{liveRenderer}
						{liveRendererCount}
                        targetOverview={previewFeatureOutline}
                        reviewRenderers={renderersForSetting(selectedSetting)}
                        onSelectReviewRenderer={(rendererId) => { if (selectedSetting) reviewRendererTargets[selectedSetting.id] = rendererId; }}
						pendingRestart={profilePendingRestart}
						activeSection={activeProfileSection}
						cacheProgress={sceneCachePrewarmProgress?.setting_id === selectedSetting.id ? sceneCachePrewarmProgress : null}
						{busy}
						onRestartPending={() => restartPendingRenderer()}
						onViewRenderer={(rendererId) => {
							selectedRendererId = rendererId;
							setActiveTab("renderers", "profileStage:viewRenderer");
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
						taskbarPinned={appSettings.pinned_taskbar_profile_ids.includes(selectedSetting.id)}
						onSetTaskbarPinned={(settingId, pinned) => setTaskbarProfilePinned(settingId, pinned)}
						onScrollSection={(section) => scrollProfileSection(section)}
					/>
                    {/snippet}
                    {#if previewOperationsA}
                        <details class="panel operations-proposal-tools"><summary>{$_("supervisor_operations.avatar_tools", {values:{name:selectedSetting.name}})}</summary>
                            {@render existingProfileStage()}
                        </details>
                    {:else}
                        {@render existingProfileStage()}
                    {/if}
				{/if}
				{#if orderDraft}<section role="status" class="panel profile-save-recovery">
					<strong>{$_(orderDraft.committed ? "profiles.order_draft.committed" : "profiles.order_draft.title")}</strong>
					<p>{orderDraft.ids.map(id=>orderDraft?.reviewed.find(s=>s.id===id)?.name??id).join(' → ')}</p>
					{#if orderDraft.error}<p role="alert">{orderDraft.error}</p>{/if}
					{#if !orderDraft.committed}<p>{$_("profiles.order_draft.help")}</p>
					<button disabled={orderSaving} onclick={()=>void saveSettingsOrder()}>{$_("profiles.order_draft.retry")}</button>{/if}
					<button disabled={orderSaving} onclick={()=>void discardSettingsOrder()}>{$_(orderDraft.committed ? "profiles.order_draft.reload" : "profiles.order_draft.discard")}</button>
				</section>{/if}
				<div class="split settings-split" class:operations-proposal-a={previewOperationsA}>
					<ProfileSettingList
						settings={avatarSettings}
						{selectedSettingId}
						{draggedSettingId}
						{settingPointerDrag}
						{iconSrc}
						runningCountForProfileId={(profileId) => renderersForSetting(avatarSettingBySelectedId(profileId)).length}
						onSelect={(settingId) => {
							if (suppressSettingClick) {
								suppressSettingClick = false;
								return;
							}
							selectProfileForEditing(settingId);
						}}
						onBeginDrag={beginSettingPointerDrag}
					/>
					{#if previewOperationsA && selectedSetting}
						<section class="panel operations-proposal-runtime" aria-label={$_("supervisor_operations.runtime_aria")}>
							<h2>{$_("supervisor_operations.runtime_heading", {values:{name:selectedSetting.name,count:renderersForSetting(selectedSetting).length}})}</h2>
							{#if isolatedDesignPreview}<p>隔離試作 · 操作は模擬個体のみ</p>{/if}
							<button class="primary" disabled={busy} onclick={() => launchSetting(selectedSetting.id, false)}>{$_("supervisor_operations.launch")}</button>
							{#each renderersForSetting(selectedSetting) as renderer (renderer.id)}
								<article>
									<h3>Renderer #{renderer.id}</h3>
									<p>{renderer.state}</p>
									<button disabled={busy} onclick={() => stopRenderer(renderer.id)}>{$_("supervisor_operations.stop_instance", {values:{id:renderer.id}})}</button>
									<button onclick={() => { logsRendererFilter = renderer.id; setActiveTab('logs', 'operations-proposal:logs'); }}>{$_("supervisor_operations.instance_logs")}</button>
								</article>
							{:else}
								<p>{$_("supervisor_operations.none")}</p>
							{/each}
							<details><summary>{$_("supervisor_operations.avatar_scope")}</summary>
								<p>{$_("supervisor_operations.avatar_save")}</p>
								<p>{$_("supervisor_operations.window_target", {values:{target:rendererForSetting(selectedSetting) ? `Renderer #${rendererForSetting(selectedSetting)?.id}` : $_("supervisor_operations.no_instance")}})}</p>
							</details>
							<ReviewProfileStop targets={renderersForSetting(selectedSetting)} profileName={selectedSetting.name} disabled={busy} onStop={async(id) => {await invoke('stop_renderer',{id});await refreshAll();}} />
							</section>
					{/if}
					<section class="panel editor-panel" inert={creatingProfile} aria-busy={creatingProfile}>
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

								{#key `${selectedSetting.id}\n${selectedSetting.avatar_path ?? ""}`}
								<ProfileAvatarSection
									setting={selectedSetting}
									{wardrobeOptions}
									{busy}
									onBrowseAvatar={() => browseSettingPath("avatar_path", "avatar")}
									onReviewMetadata={() => reviewSelectedAvatarMetadata()}
									onUpdateSettingValue={profileUpdaterFor(selectedSetting)}
									onActivate={() => (activeProfileSection = "avatar")}
								/>
								{/key}

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
									onUpdateSettingValues={(updates) => applyProfileUpdates(updates)}
									onApplyLookRecommendation={(look) => applyLookRecommendation(look)}
									onActivate={() => (activeProfileSection = "look")}
								/>

								{#key selectedSetting.id}
								<ProfileMotionSection
									setting={selectedSetting}
									{busy}
									onUpdateSettingValue={profileUpdaterFor(selectedSetting)}
									onActivate={() => (activeProfileSection = "motion")}
								/>
								{/key}

								<ProfilePhysicsSection
									setting={physicsSettingForEditor(selectedSetting)}
									{busy}
									onUpdateSettingValue={profileUpdaterFor(selectedSetting)}
									onActivate={() => (activeProfileSection = "physics")}
								/>

								<ProfileCameraSection
									setting={selectedSetting}
									runtimeCamera={runtimeCameraForSetting(selectedSetting)}
									{busy}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onApplyTargetPreset={(preset) => applyCameraTargetPreset(preset)}
									onApplyOrbitPreset={(preset) => applyCameraOrbitPreset(preset)}
									onApplyLensPreset={(preset) => applyCameraLensPreset(preset)}
									onCaptureRuntimeCamera={() => captureRuntimeCameraForSetting(selectedSetting)}
									onActivate={() => (activeProfileSection = "camera")}
								/>

								<ProfileWindowSection
									setting={selectedSetting}
									runtimeWindowAvailable={runtimeWindowAvailableForSetting(selectedSetting)}
                                    reviewRendererIds={import.meta.env.VITE_UN_DESIGN_PREVIEW === "1" ? renderersForSetting(selectedSetting).map(renderer => renderer.id) : []}
									{busy}
									{colorDisplayMode}
									onColorModeChange={setColorDisplayMode}
									onUpdateSettingValue={(field, value) => updateSettingValue(field, value)}
									onBackgroundColorChange={updateBackgroundColorValue}
									onCaptureRuntimeWindow={() => captureRuntimeWindowForSetting(selectedSetting)}
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
			{#await import("./lib/LogsView.svelte") then module}
				{@const LogsView = module.default}
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
			{/await}
		{:else}
			<div class="app-settings-workspace">
			{#if appSettingsSaveError}
				<section class="panel profile-save-recovery" aria-label={($locale ?? "ja-JP").startsWith("ja") ? "アプリ設定の保存状態" : "Application settings save status"}>
					<p>{($locale ?? "ja-JP").startsWith("ja") ? (backendAppSettingsReady ? "アプリ設定の保存を確認できませんでした。変更した内容を画面に保持しています。" : "アプリ設定をまだ読み込めていません。元の設定は上書きしません。") : (backendAppSettingsReady ? "The settings save could not be confirmed. Your changes are retained on screen." : "Application settings have not loaded. Existing settings will not be overwritten.")}</p>
					<p>{appSettingsSaveError}</p>
					<button disabled={appSettingsPending > 0} onclick={() => void syncAppSettingsToBackend(appSettings)}>{($locale ?? "ja-JP").startsWith("ja") ? (backendAppSettingsReady ? "保持した設定を再保存" : "設定の読込を再試行") : (backendAppSettingsReady ? "Retry retained settings" : "Retry loading settings")}</button>
					<button disabled={appSettingsPending > 0} onclick={() => void discardAppSettingsDraft()}>{($locale ?? "ja-JP").startsWith("ja") ? (backendAppSettingsReady ? "変更を破棄して保存済み設定を読み直す" : "読込前の入力を破棄（ファイルは変更しません）") : (backendAppSettingsReady ? "Discard changes and reload saved settings" : "Discard early edits (file unchanged)")}</button>
				</section>
			{/if}
			{#await import("./lib/AppSettingsView.svelte") then module}
				{@const AppSettingsView = module.default}
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
					onClearTaskbarProfilePins={() => clearTaskbarProfilePins()}
					onOpenExternalLink={(url) => openExternalLink(url)}
				/>
			{/await}
			</div>
		{/if}
	</div>
	{#if vrmMetadataModal}
		{#await import("./lib/VrmMetadataModal.svelte") then module}
			{@const VrmMetadataModal = module.default}
			<VrmMetadataModal
				modal={vrmMetadataModal}
				targetProfileId={vrmMetadataModal.settingId}
				targetProfile={avatarSettingByIdOrManifestPath(vrmMetadataModal.settingId)}
				{busy}
				bind:useThumbnailForProfileIconOnAccept
				onClose={closeVrmMetadataModal}
				onAcceptAndUse={acceptVrmMetadataAndUse}
			/>
		{/await}
	{/if}
	{#if unavatarMetadataModal}
		{#await import("./lib/UnavatarRightsModal.svelte") then module}
			{@const UnavatarRightsModal = module.default}
			<UnavatarRightsModal
				modal={unavatarMetadataModal}
				targetProfileId={unavatarMetadataModal.settingId}
				targetProfile={avatarSettingByIdOrManifestPath(unavatarMetadataModal.settingId)}
				{busy}
				bind:profileIconCrop={unavatarProfileIconCrop}
				onClose={closeUnavatarMetadataModal}
				onAcceptAndUse={acceptUnavatarMetadataAndUse}
				onSaveProfileIcon={saveUnavatarProfileIconFromModal}
			/>
		{/await}
	{/if}
</main>
