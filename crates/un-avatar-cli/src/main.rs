//! UN Avatar CLI（bootstrap）。`crate-io-plugin-plan.md` Phase 2.2 の最小版。
//!
//! サブコマンド例: `formats list`, `formats probe`, `convert`, `validate`, `inspect`, `vmc listen`。

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use clap::{Parser, Subcommand};
use serde::Serialize;
use un_avatar_core::{
	morph_weights_for_primitive, UnaAlphaMode, UnaDynamicsSourceKind, UnaHumanoidRuntimeBasis, UnaImagePixelFormat, UnaMaterialPbr,
	UnaRuntimeSourceKind, UnaSceneSnapshot, UnaShadingModel,
};
use un_avatar_io::{
	path_has_format_extension, AvatarExporter, AvatarImporter, ExportCapability, ExportContext, ExportOptions, ExportOutput, ExportReport,
	FormatDescriptor, FormatId, ImportContext, ImportInput, ImportOptions, ImportProbe, ImportReport, IoRegistry, UnaDocument,
};
use un_avatar_io_gltf::{apply_unavatar_wardrobe_set, register_gltf_importer, WardrobeApplyReport};
use un_avatar_io_una::{io_registry_with_una, read_una_any, UnaFileV0};
use un_avatar_io_vrm::register_vrm_importer;
use un_avatar_plugin_host::{register_stdio_exporters_from_plugin_root, register_stdio_importers_from_plugin_root};

#[derive(Serialize)]
struct ConvertJsonReport {
	import_format_id: String,
	export_format_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	import_provider_plugin_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	export_provider_plugin_id: Option<String>,
	import_report: ImportReport,
	export_report: ExportReport,
}

#[derive(Serialize)]
struct FormatsListJson {
	importers: Vec<FormatDescriptor>,
	exporters: Vec<FormatDescriptor>,
}

#[derive(Serialize)]
struct ValidateReport {
	valid: bool,
	path: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	error: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	format_id: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	provider_plugin_id: Option<String>,
}

#[derive(Serialize)]
struct InspectReport {
	path: String,
	una: UnaFileV0,
}

#[derive(Serialize)]
struct DiagnoseReport {
	path: String,
	import_format_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	import_provider_plugin_id: Option<String>,
	timings: DiagnoseTimingSummary,
	import_report: ImportReport,
	runtime: DiagnoseRuntimeSummary,
	scene: DiagnoseSceneSummary,
	#[serde(skip_serializing_if = "Option::is_none")]
	humanoid: Option<DiagnoseHumanoidSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	expressions: Option<DiagnoseExpressionSummary>,
	dynamics: DiagnoseDynamicsSummary,
	#[serde(skip_serializing_if = "Option::is_none")]
	vrm: Option<DiagnoseVrmSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	unavatar: Option<DiagnoseUnavatarSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	wardrobe_probes: Vec<DiagnoseWardrobeProbeSummary>,
	warnings: Vec<String>,
}

#[derive(Serialize)]
struct DiagnoseTimingSummary {
	import_ms: u128,
	wardrobe_apply_ms: u128,
	wardrobe_probe_ms: u128,
	report_build_ms: u128,
}

#[derive(Serialize)]
struct DiagnoseRuntimeSummary {
	source_kind: UnaRuntimeSourceKind,
	humanoid_basis: UnaHumanoidRuntimeBasis,
}

#[derive(Serialize)]
struct DiagnoseSceneSummary {
	has_scene: bool,
	mesh_count: usize,
	primitive_count: usize,
	morph_target_count: usize,
	node_count: usize,
	hidden_node_count: usize,
	skin_count: usize,
	image_count: usize,
	image_source_count: usize,
	image_source_bytes: u64,
	image_source_mime_counts: BTreeMap<String, usize>,
	image_source_color_space_counts: BTreeMap<String, usize>,
	image_source_texture_type_counts: BTreeMap<String, usize>,
	image_source_texture_shape_counts: BTreeMap<String, usize>,
	image_source_layout_counts: BTreeMap<String, usize>,
	image_pixel_format_counts: BTreeMap<String, usize>,
	non_rgba8_image_count: usize,
	largest_image_sources: Vec<DiagnoseImageSourceSummary>,
	material_count: usize,
	liltoon_feature_counts: BTreeMap<String, usize>,
	node_constraint_count: usize,
	shading_counts: BTreeMap<String, usize>,
	alpha_counts: BTreeMap<String, usize>,
	visible_shading_counts: BTreeMap<String, usize>,
	visible_alpha_counts: BTreeMap<String, usize>,
	visible_material_indices: Vec<usize>,
	eye_like_material_indices: Vec<usize>,
	skins: Vec<DiagnoseSkinSummary>,
	materials: Vec<DiagnoseMaterialSummary>,
	visible_mesh_nodes: Vec<DiagnoseVisibleMeshNodeSummary>,
}

#[derive(Serialize)]
struct DiagnoseSkinSummary {
	index: usize,
	joint_count: usize,
	inverse_bind_count: usize,
	effective_joint_count: usize,
	over_renderer_bone_limit: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	skeleton_node: Option<usize>,
	used_by_node_count: usize,
	primitive_joint_attribute_count: usize,
	primitive_weight_attribute_count: usize,
	mismatched_joint_weight_attribute_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_joint_index: Option<u16>,
	out_of_range_joint_attribute_count: usize,
}

#[derive(Serialize)]
struct DiagnoseVisibleMeshNodeSummary {
	node: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_node_id: Option<String>,
	mesh: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	skin: Option<usize>,
	materials: Vec<DiagnoseVisibleMaterialSummary>,
}

#[derive(Serialize)]
struct DiagnoseVisibleMaterialSummary {
	primitive: usize,
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_shader: Option<String>,
	shading: UnaShadingModel,
	alpha_mode: UnaAlphaMode,
	alpha_cutoff: f32,
	transparent_with_z_write: bool,
	draw_skipped_fully_transparent: bool,
	morph_target_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	nonzero_morph_weights: Vec<DiagnoseMorphWeightSummary>,
}

#[derive(Serialize)]
struct DiagnoseMorphWeightSummary {
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	weight: f32,
	position_delta_abs_sum: f32,
	normal_delta_abs_sum: f32,
}

#[derive(Serialize)]
struct DiagnoseImageSourceSummary {
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	mime_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	uri: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_pixel_format: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	channels: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	color_space: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	texture_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	texture_shape: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_layout: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	unity_generate_cubemap: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	srgb: Option<bool>,
	byte_length: u64,
	pixel_format: UnaImagePixelFormat,
	width: u32,
	height: u32,
}

#[derive(Serialize)]
struct DiagnoseMaterialSummary {
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_shader: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	material_family: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	render_queue: Option<i32>,
	source_float_param_count: usize,
	source_color_param_count: usize,
	#[serde(skip_serializing_if = "BTreeMap::is_empty")]
	source_render_float_params: BTreeMap<String, f32>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	liltoon_features: Vec<String>,
	shading: UnaShadingModel,
	alpha_mode: UnaAlphaMode,
	alpha_cutoff: f32,
	double_sided: bool,
	cull_mode: un_avatar_core::UnaCullMode,
	base_color_factor: [f32; 4],
	#[serde(skip_serializing_if = "Option::is_none")]
	base_color_texture_index: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	base_color_texture_alpha: Option<DiagnoseTextureAlphaSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	normal_texture_index: Option<usize>,
	normal_texture_scale: f32,
	eye_like_name: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	mtoon: Option<DiagnoseMToonSummary>,
}

#[derive(Serialize)]
struct DiagnoseTextureAlphaSummary {
	image: usize,
	width: u32,
	height: u32,
	pixel_format: UnaImagePixelFormat,
	has_alpha_channel: bool,
	min_alpha: u8,
	max_alpha: u8,
	transparent_pixels: usize,
	translucent_pixels: usize,
	opaque_pixels: usize,
	coverage: f32,
}

#[derive(Serialize)]
struct DiagnoseMToonSummary {
	transparent_with_z_write: bool,
	shade_color_factor: [f32; 3],
	shade_multiply_texture_index: Option<usize>,
	shading_shift_factor: f32,
	shading_shift_texture_index: Option<usize>,
	shading_toony_factor: f32,
	gi_equalization_factor: f32,
	matcap_factor: [f32; 3],
	matcap_texture_index: Option<usize>,
	parametric_rim_color_factor: [f32; 3],
	rim_multiply_texture_index: Option<usize>,
	reflection_cube_texture_index: Option<usize>,
	outline_width_mode: un_avatar_core::UnaMtoonOutlineWidthMode,
	outline_width_factor: f32,
	outline_width_multiply_texture_index: Option<usize>,
	outline_color_factor: [f32; 3],
	emissive_factor: [f32; 3],
	emissive_texture_index: Option<usize>,
}

#[derive(Serialize)]
struct DiagnoseHumanoidSummary {
	bone_count: usize,
	keys: Vec<String>,
	left_eye_node: Option<usize>,
	right_eye_node: Option<usize>,
}

#[derive(Serialize)]
struct DiagnoseExpressionSummary {
	preset_count: usize,
	presets: Vec<DiagnoseExpressionPresetSummary>,
	#[serde(skip_serializing_if = "Option::is_none")]
	apply_probe: Option<DiagnoseExpressionApplyProbe>,
}

#[derive(Serialize)]
struct DiagnoseExpressionPresetSummary {
	name: String,
	bind_count: usize,
}

#[derive(Serialize)]
struct DiagnoseExpressionApplyProbe {
	weights: BTreeMap<String, f32>,
	active_morph_slots: Vec<DiagnoseExpressionMorphSlot>,
}

#[derive(Serialize)]
struct DiagnoseExpressionMorphSlot {
	mesh: usize,
	primitive: usize,
	active_count: usize,
	max_weight: f32,
}

#[derive(Serialize)]
struct DiagnoseVrmSummary {
	spec_version: String,
	mtoon_materials_v0: usize,
	mtoon_material_indices_v1: Vec<usize>,
	spring_group_count: usize,
}

#[derive(Serialize)]
struct DiagnoseDynamicsSummary {
	group_count: usize,
	vrm_spring_bone_group_count: usize,
	vrc_physbone_group_count: usize,
	unknown_group_count: usize,
	collider_count: usize,
	vrm_spring_bone_collider_count: usize,
	vrc_physbone_collider_count: usize,
	unknown_collider_count: usize,
	source_limit_count: usize,
	source_collision_disabled_count: usize,
	source_inside_bounds_collider_count: usize,
	source_grabbing_enabled_count: usize,
	source_posing_enabled_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	groups: Vec<DiagnoseDynamicsGroupSummary>,
}

#[derive(Default)]
struct DynamicsSourceFeatureCounts {
	limit_count: usize,
	collision_disabled_count: usize,
	inside_bounds_collider_count: usize,
	grabbing_enabled_count: usize,
	posing_enabled_count: usize,
}

#[derive(Serialize)]
struct DiagnoseDynamicsGroupSummary {
	index: usize,
	source_kind: UnaDynamicsSourceKind,
	#[serde(skip_serializing_if = "String::is_empty")]
	comment: String,
	#[serde(skip_serializing_if = "String::is_empty")]
	category: String,
	bone_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	root_node: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	root_path: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tip_node: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	tip_path: Option<String>,
	stiffness: f32,
	drag_force: f32,
	gravity_power: f32,
	gravity_dir: [f32; 3],
	#[serde(skip_serializing_if = "Option::is_none")]
	limit_type: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_angle_x: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_angle_z: Option<f32>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_stretch: Option<f32>,
	hit_radius: f32,
}

#[derive(Serialize)]
struct DiagnoseUnavatarSummary {
	spec_version: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	generator: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	manifest_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source_type: Option<String>,
	extension_node_count: usize,
	variant_count: usize,
	dynamics_entry_count: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	base_set: Option<String>,
	wardrobe_set_count: usize,
	wardrobe_set_ids: Vec<String>,
	wardrobe_sets: Vec<DiagnoseUnavatarWardrobeSetSummary>,
	base_operation_count: usize,
	base_operation_counts: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct DiagnoseUnavatarWardrobeSetSummary {
	id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	display_name: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	source: Option<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	asset_groups: Vec<String>,
	operation_count: usize,
	operation_counts: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct DiagnoseWardrobeProbeSummary {
	set_id: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	display_name: Option<String>,
	probe_ms: u128,
	#[serde(skip_serializing_if = "Option::is_none")]
	visibility_applied: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	visibility_missing: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	blendshape_applied: Option<usize>,
	#[serde(skip_serializing_if = "Option::is_none")]
	blendshape_missing: Option<usize>,
	visible_mesh_node_count: usize,
	visible_mesh_paths: Vec<String>,
	nonzero_morph_weight_count: usize,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	nonzero_morph_weights: Vec<DiagnoseWardrobeProbeMorphSummary>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	missing_visibility_paths: Vec<String>,
	#[serde(skip_serializing_if = "Vec::is_empty")]
	missing_blendshapes: Vec<String>,
}

#[derive(Serialize)]
struct DiagnoseWardrobeProbeMorphSummary {
	mesh: usize,
	primitive: usize,
	index: usize,
	#[serde(skip_serializing_if = "Option::is_none")]
	name: Option<String>,
	weight: f32,
}

#[derive(Serialize)]
struct ImporterProbeRow {
	format_id: String,
	confidence: u8,
	#[serde(skip_serializing_if = "Option::is_none")]
	provider_plugin_id: Option<String>,
}

#[derive(Serialize)]
struct ExporterProbeRow {
	format_id: String,
	confidence: u8,
	#[serde(skip_serializing_if = "Option::is_none")]
	provider_plugin_id: Option<String>,
}

#[derive(Serialize)]
struct FormatsProbeJson {
	path: String,
	importers: Vec<ImporterProbeRow>,
	/// `best_importer_for` が選ぶ形式（同点時はレジストリ順の先勝ち）
	best_importer: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	best_importer_provider_plugin_id: Option<String>,
	exporters: Vec<ExporterProbeRow>,
	/// [`IoRegistry::best_exporter_for`] が選ぶ形式（空の [`UnaDocument`]・既定 [`ExportOptions`] を仮定）
	best_exporter: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	best_exporter_provider_plugin_id: Option<String>,
}

#[derive(Parser)]
#[command(
	name = "un-avatar",
	version,
	about = "UN Avatar CLI（bootstrap）",
	long_about = "UN Avatar CLI（bootstrap）\n\n\
	              環境変数 UN_AVATAR_PLUGIN_PATH に、bundle または複数 bundle の親ディレクトリを PATH 形式で指定できる（Windows は `;`、それ以外は `:`）。\
	              `--plugin-dir` と併用したときはマージし、同一パスは 1 回だけ登録する。\
	              親配下の探索の深さ上限は UN_AVATAR_PLUGIN_DISCOVERY_MAX_DEPTH（既定 8）。\
	              stdio 子の cwd: 既定は bundle 根（manifest 親）。UN_AVATAR_PLUGIN_CHILD_CWD=host（大小無視）のときだけホストと同じ cwd を使う。\
	              プラグイン RPC の stdout 読取: 共通 **`UN_AVATAR_PLUGIN_RPC_TIMEOUT_SECS`**（既定 120 秒）、または **`UN_AVATAR_PLUGIN_RPC_HANDSHAKE_TIMEOUT_SECS`** / **`UN_AVATAR_PLUGIN_RPC_IMPORT_TIMEOUT_SECS`** で `initialize` と `import` を指定。**`export`** は **`UN_AVATAR_PLUGIN_RPC_EXPORT_TIMEOUT_SECS`** または（未設定時）**当該子の import と同じ上限**。セッション全体の壁時計は **`UN_AVATAR_PLUGIN_RPC_SESSION_WALL_SECS`**（未設定・0・無効は無制限）。"
)]
struct Cli {
	/// 外部 stdio プラグインの bundle ディレクトリ（`/path/to/my-plugin` に `manifest.toml`）、または **複数 bundle の親**（`/path/to/plugins` 直下が `plugin-a/` …）。`register_stdio_*_from_plugin_root`（importer と exporter）に渡す。複数指定可
	#[arg(long = "plugin-dir", value_name = "DIR", global = true, action = clap::ArgAction::Append)]
	plugin_dir: Vec<PathBuf>,
	#[command(subcommand)]
	command: Commands,
}

/// `UN_AVATAR_PLUGIN_PATH` の生文字列をパス列に分解する（空要素を除く）。
fn parse_plugin_path_list(raw: &OsStr) -> Vec<PathBuf> {
	let sep = if cfg!(windows) { ';' } else { ':' };
	raw.to_string_lossy()
		.split(sep)
		.map(|s| PathBuf::from(s.trim()))
		.filter(|p| !p.as_os_str().is_empty())
		.collect()
}

fn plugin_dirs_from_env() -> Vec<PathBuf> {
	std::env::var_os("UN_AVATAR_PLUGIN_PATH")
		.map(|raw| parse_plugin_path_list(&raw))
		.unwrap_or_default()
}

fn merge_unique_plugin_dirs(env_entries: Vec<PathBuf>, cli: &[PathBuf]) -> Vec<PathBuf> {
	use std::collections::HashSet;
	let mut seen: HashSet<PathBuf> = HashSet::new();
	let mut out = Vec::new();
	for p in env_entries.into_iter().chain(cli.iter().cloned()) {
		if seen.insert(p.clone()) {
			out.push(p);
		}
	}
	out
}

fn io_registry_for_cli(cli_plugin_dirs: &[PathBuf]) -> Result<IoRegistry, String> {
	let dirs = merge_unique_plugin_dirs(plugin_dirs_from_env(), cli_plugin_dirs);
	let mut reg = io_registry_with_una();
	register_vrm_importer(&mut reg);
	register_gltf_importer(&mut reg);
	for dir in dirs {
		register_stdio_importers_from_plugin_root(&mut reg, dir.as_path())
			.map_err(|e| format!("プラグイン検索パス {}: {e}", dir.display()))?;
		register_stdio_exporters_from_plugin_root(&mut reg, dir.as_path())
			.map_err(|e| format!("プラグイン検索パス {} (exporter): {e}", dir.display()))?;
	}
	Ok(reg)
}

fn cached_binary_import_bytes(path: &Path) -> Option<Arc<[u8]>> {
	let ext = path.extension().and_then(|e| e.to_str())?;
	if !ext.eq_ignore_ascii_case("vrm") && !ext.eq_ignore_ascii_case("glb") && !ext.eq_ignore_ascii_case("unavatar") {
		return None;
	}
	std::fs::read(path).ok().map(Arc::<[u8]>::from)
}

fn import_probe_for_path(path: &Path, bytes: Option<Arc<[u8]>>) -> ImportProbe {
	ImportProbe {
		path_hint: Some(path.to_path_buf()),
		bytes,
	}
}

fn import_input_for_path(path: &Path, format_id: &FormatId, bytes: Option<Arc<[u8]>>) -> ImportInput {
	match (format_id.0.as_str(), bytes) {
		("io.un-avatar.vrm" | "io.un-avatar.gltf", Some(bytes)) => ImportInput::Bytes {
			bytes,
			path_hint: Some(path.to_path_buf()),
		},
		_ => ImportInput::Path(path.to_path_buf()),
	}
}

/// `formats probe` 用の集約 JSON（import / export の両方）。
fn build_formats_probe_json(reg: &IoRegistry, path: &Path) -> FormatsProbeJson {
	let path_str = path.to_string_lossy().to_string();
	let probe = import_probe_for_path(path, cached_binary_import_bytes(path));
	let mut importers = Vec::with_capacity(reg.importers().len());
	importers.extend(reg.importers().iter().map(|i| {
		let desc = i.descriptor();
		let r = i.probe(&probe);
		ImporterProbeRow {
			format_id: desc.id.0.clone(),
			confidence: r.confidence,
			provider_plugin_id: desc.provider_plugin_id.clone(),
		}
	}));
	let (best_importer, best_importer_provider_plugin_id) = if let Some(i) = reg.best_importer_for(&probe) {
		let desc = i.descriptor();
		(Some(desc.id.0), desc.provider_plugin_id)
	} else {
		(None, None)
	};

	let doc = UnaDocument::default();
	let opts = ExportOptions;
	let path_str_lossy = path.as_os_str().to_string_lossy();
	let mut exporters = Vec::with_capacity(reg.exporters().len());
	exporters.extend(reg.exporters().iter().map(|e| {
		let desc = e.descriptor();
		let mut confidence = 0u8;
		if e.can_export(&doc, &opts) == ExportCapability::Supported {
			confidence = 60;
			for ext in &desc.extensions {
				if path_has_format_extension(&path_str_lossy, ext) {
					confidence = 120;
					break;
				}
			}
		}
		ExporterProbeRow {
			format_id: desc.id.0.clone(),
			confidence,
			provider_plugin_id: desc.provider_plugin_id.clone(),
		}
	}));
	let (best_exporter, best_exporter_provider_plugin_id) = if let Some(e) = reg.best_exporter_for(&doc, path) {
		let desc = e.descriptor();
		(Some(desc.id.0), desc.provider_plugin_id)
	} else {
		(None, None)
	};

	FormatsProbeJson {
		path: path_str,
		importers,
		best_importer,
		best_importer_provider_plugin_id,
		exporters,
		best_exporter,
		best_exporter_provider_plugin_id,
	}
}

#[derive(Subcommand)]
enum Commands {
	/// 登録されている入出力形式を列挙する
	Formats {
		#[command(subcommand)]
		command: FormatsCommands,
	},
	/// アバターを別形式へ書き出す（現状は UNA v0 のみ）
	Convert {
		/// 入力ファイル、または `.una.d` ディレクトリ
		input: PathBuf,
		/// 出力 `.una` ファイル、または `.una.d` ディレクトリ
		output: PathBuf,
		/// 使う importer の FormatId（例: io.un-avatar.una）。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// 使う exporter の FormatId。省略時は出力パスから選択
		#[arg(long, value_name = "FORMAT_ID")]
		output_format: Option<String>,
		/// import/export レポートを JSON で書き出す（`-` で stdout）
		#[arg(long, value_name = "PATH")]
		json_report: Option<PathBuf>,
	},
	/// UNA など、Importer 経由で読めるか検証する（終了コード 0/1）
	Validate {
		/// `.una` ファイルまたは `.una.d` ディレクトリ
		path: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// 結果を JSON で stdout に出す（失敗時も出力してから終了コード 1）
		#[arg(long)]
		json: bool,
	},
	/// UNA ファイル／バンドルを読み、スキーマ上の概要を表示する
	Inspect {
		path: PathBuf,
		#[arg(long)]
		json: bool,
	},
	/// Importer 経由でモデルを読み、材質・Humanoid・表情・VRM ヒントを診断する
	Diagnose {
		/// 入力ファイル、または `.una.d` ディレクトリ
		path: PathBuf,
		/// 使う importer の FormatId。省略時はパスから probe
		#[arg(long, value_name = "FORMAT_ID")]
		input_format: Option<String>,
		/// Base 適用後に重ねる `.unavatar` wardrobe set id
		#[arg(long, value_name = "SET_ID")]
		wardrobe_set: Option<String>,
		/// Base と全 wardrobe set の可視メッシュ／blendshape 状態を比較表示する
		#[arg(long)]
		wardrobe_probe_all: bool,
		/// 人間向け出力で、現在の wardrobe 状態から参照される material だけを表示する
		#[arg(long)]
		visible_materials_only: bool,
		/// 人間向け出力で、現在の wardrobe 状態の可視 mesh node / primitive / material 対応を表示する
		#[arg(long)]
		visible_meshes: bool,
		/// 結果を JSON で stdout に出す
		#[arg(long)]
		json: bool,
	},
	/// VMC Protocol（OSC/UDP）— Marionette 受信デバッグ
	Vmc {
		#[command(subcommand)]
		command: VmcCommands,
	},
}

#[derive(Subcommand)]
enum VmcCommands {
	/// UDP で待受け、デコードしたイベント（既定）または `--frame` で UNMotionFrame を JSON 行で出力
	Listen {
		#[arg(long, default_value_t = un_avatar_vmc::DEFAULT_MARIONETTE_PORT)]
		port: u16,
		/// 各パケット受信後に蓄積状態から UNMotionFrame を 1 行 JSON で出す
		#[arg(long)]
		frame: bool,
	},
}

#[derive(Subcommand)]
enum FormatsCommands {
	/// importer / exporter の一覧を表示する
	List {
		/// JSON で stdout に出す（ツール連携用）
		#[arg(long)]
		json: bool,
	},
	/// 各 importer の [`ImportProbe`] 結果と、**出力パス**に対する exporter 候補（空ドキュメントで `can_export`／拡張子一致の目安）を表示する
	Probe {
		path: PathBuf,
		#[arg(long)]
		json: bool,
	},
}

fn main() {
	let cli = Cli::parse_from(normalize_cli_args(std::env::args_os()));
	if let Err(e) = run(cli) {
		eprintln!("{e}");
		std::process::exit(1);
	}
}

fn is_known_command(arg: &OsStr) -> bool {
	matches!(
		arg.to_string_lossy().as_ref(),
		"formats" | "convert" | "validate" | "inspect" | "diagnose" | "vmc" | "help"
	)
}

fn looks_like_input_path(arg: &OsStr) -> bool {
	let s = arg.to_string_lossy();
	if s.is_empty() || s.starts_with('-') || is_known_command(arg) {
		return false;
	}
	let p = Path::new(arg);
	if p.exists() {
		return true;
	}
	let pathish = s.contains('/') || s.contains('\\');
	let import_ext = p
		.extension()
		.and_then(OsStr::to_str)
		.map(|ext| {
			matches!(
				ext.to_ascii_lowercase().as_str(),
				"vrm" | "glb" | "gltf" | "unavatar" | "una" | "exampleavatar"
			)
		})
		.unwrap_or(false);
	pathish || import_ext
}

fn normalize_cli_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
	let mut out: Vec<OsString> = args.into_iter().collect();
	let mut i = 1;
	while i < out.len() {
		let arg = &out[i];
		if arg == "--" {
			if out.get(i + 1).is_some_and(|next| looks_like_input_path(next)) {
				out.insert(i + 1, OsString::from("diagnose"));
			}
			break;
		}
		let s = arg.to_string_lossy();
		if s == "--plugin-dir" {
			i += 2;
			continue;
		}
		if s.starts_with("--plugin-dir=") || s == "--help" || s == "-h" || s == "--version" || s == "-V" {
			i += 1;
			continue;
		}
		if s.starts_with('-') {
			break;
		}
		if looks_like_input_path(arg) {
			out.insert(i, OsString::from("diagnose"));
		}
		break;
	}
	out
}

fn run(cli: Cli) -> Result<(), String> {
	let plugin_dirs = cli.plugin_dir;
	match cli.command {
		Commands::Formats { command } => {
			run_formats(&plugin_dirs, command)?;
			Ok(())
		}
		Commands::Convert {
			input,
			output,
			input_format,
			output_format,
			json_report,
		} => run_convert(&plugin_dirs, input, output, input_format, output_format, json_report),
		Commands::Validate { path, input_format, json } => run_validate(&plugin_dirs, path, input_format, json),
		Commands::Inspect { path, json } => run_inspect(path, json),
		Commands::Diagnose {
			path,
			input_format,
			wardrobe_set,
			wardrobe_probe_all,
			visible_materials_only,
			visible_meshes,
			json,
		} => run_diagnose(
			&plugin_dirs,
			path,
			input_format,
			wardrobe_set,
			wardrobe_probe_all,
			visible_materials_only,
			visible_meshes,
			json,
		),
		Commands::Vmc { command } => run_vmc(command),
	}
}

fn run_formats(plugin_dirs: &[PathBuf], cmd: FormatsCommands) -> Result<(), String> {
	match cmd {
		FormatsCommands::List { json } => run_formats_list(plugin_dirs, json),
		FormatsCommands::Probe { path, json } => run_formats_probe(plugin_dirs, path, json),
	}
}

fn run_formats_list(plugin_dirs: &[PathBuf], json: bool) -> Result<(), String> {
	let reg = io_registry_for_cli(plugin_dirs)?;
	if json {
		let out = FormatsListJson {
			importers: reg.importer_descriptors(),
			exporters: reg.exporter_descriptors(),
		};
		write_json_stdout(&out)?;
		return Ok(());
	}
	println!("importers:");
	for importer in reg.importers() {
		let d = importer.descriptor();
		let plug = d.provider_plugin_id.as_ref().map(|p| format!(" ({p})")).unwrap_or_default();
		println!("  {} — {} — [{}]{plug}", d.id.0, d.display_name, d.extensions.join(", "));
	}
	println!("exporters:");
	for exporter in reg.exporters() {
		let d = exporter.descriptor();
		let plug = d.provider_plugin_id.as_ref().map(|p| format!(" ({p})")).unwrap_or_default();
		println!("  {} — {} — [{}]{plug}", d.id.0, d.display_name, d.extensions.join(", "));
	}
	Ok(())
}

fn run_formats_probe(plugin_dirs: &[PathBuf], path: PathBuf, json: bool) -> Result<(), String> {
	let reg = io_registry_for_cli(plugin_dirs)?;
	if json {
		let out = build_formats_probe_json(&reg, &path);
		write_json_stdout(&out)?;
		return Ok(());
	}
	let out = build_formats_probe_json(&reg, &path);
	println!("probe: {}", path.display());
	println!("importers:");
	for row in &out.importers {
		let plug = row.provider_plugin_id.as_ref().map(|p| format!("  ({p})")).unwrap_or_default();
		println!("  {}  confidence {}{plug}", row.format_id, row.confidence);
	}
	if let Some(ref id) = out.best_importer {
		let plug = out
			.best_importer_provider_plugin_id
			.as_ref()
			.map(|p| format!(" ({p})"))
			.unwrap_or_default();
		println!("best importer: {id}{plug}");
	} else {
		println!("best importer: (none)");
	}
	println!("exporters:");
	for row in &out.exporters {
		let plug = row.provider_plugin_id.as_ref().map(|p| format!("  ({p})")).unwrap_or_default();
		println!("  {}  confidence {}{plug}", row.format_id, row.confidence);
	}
	if let Some(ref id) = out.best_exporter {
		let plug = out
			.best_exporter_provider_plugin_id
			.as_ref()
			.map(|p| format!(" ({p})"))
			.unwrap_or_default();
		println!("best exporter: {id}{plug}");
	} else {
		println!("best exporter: (none)");
	}
	Ok(())
}

fn write_json_stdout<T: Serialize>(value: &T) -> Result<(), String> {
	let stdout = io::stdout();
	let mut lock = stdout.lock();
	serde_json::to_writer_pretty(&mut lock, value).map_err(|e| e.to_string())?;
	writeln!(lock).map_err(|e| e.to_string())
}

fn write_convert_json_report(path: &Path, bundle: &ConvertJsonReport) -> Result<(), String> {
	if path.as_os_str() == "-" {
		write_json_stdout(bundle)?;
		return Ok(());
	}
	if let Some(parent) = path.parent() {
		if !parent.as_os_str().is_empty() {
			fs::create_dir_all(parent).map_err(|e| e.to_string())?;
		}
	}
	let file = fs::File::create(path).map_err(|e| e.to_string())?;
	let mut writer = BufWriter::new(file);
	serde_json::to_writer_pretty(&mut writer, bundle).map_err(|e| e.to_string())?;
	writeln!(writer).map_err(|e| e.to_string())?;
	Ok(())
}

fn write_validate_stdout(report: &ValidateReport) -> Result<(), String> {
	write_json_stdout(report)
}

fn run_validate(plugin_dirs: &[PathBuf], path: PathBuf, input_format: Option<String>, json: bool) -> Result<(), String> {
	let path_str = path.to_string_lossy().to_string();
	let reg = io_registry_for_cli(plugin_dirs)?;

	let importer: &dyn AvatarImporter = if let Some(ref s) = input_format {
		let id = FormatId::new(s.as_str());
		match reg.importer_by_id(&id) {
			Some(i) => i,
			None => {
				let msg = format!("指定の importer が登録されていません: {s}");
				if json {
					write_validate_stdout(&ValidateReport {
						valid: false,
						path: path_str.clone(),
						error: Some(msg.clone()),
						format_id: None,
						provider_plugin_id: None,
					})?;
				}
				return Err(msg);
			}
		}
	} else {
		let probe = import_probe_for_path(&path, cached_binary_import_bytes(&path));
		match reg.best_importer_for(&probe) {
			Some(i) => i,
			None => {
				let msg =
					"入力に合う importer が見つかりません（`.una` または `manifest.toml` 付き `.una.d` を指定、`--plugin-dir`、または --input-format）"
						.to_string();
				if json {
					write_validate_stdout(&ValidateReport {
						valid: false,
						path: path_str.clone(),
						error: Some(msg.clone()),
						format_id: None,
						provider_plugin_id: None,
					})?;
				}
				return Err(msg);
			}
		}
	};

	let desc = importer.descriptor();
	let format_id = desc.id.0.clone();
	let provider_plugin_id = desc.provider_plugin_id.clone();
	let mut ictx = ImportContext {
		asset_root: path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
	};
	let path_display = path.display().to_string();
	let import_input = import_input_for_path(&path, &desc.id, cached_binary_import_bytes(&path));
	let import_result = importer.import(&mut ictx, import_input, ImportOptions);
	match import_result {
		Ok(_) if json => {
			write_validate_stdout(&ValidateReport {
				valid: true,
				path: path_str,
				error: None,
				format_id: Some(format_id),
				provider_plugin_id: provider_plugin_id.clone(),
			})?;
			Ok(())
		}
		Ok(_) => {
			let plug = provider_plugin_id.as_ref().map(|p| format!(" ({p})")).unwrap_or_default();
			println!("OK  {path_display}  ({format_id}){plug}");
			Ok(())
		}
		Err(e) if json => {
			write_validate_stdout(&ValidateReport {
				valid: false,
				path: path_str,
				error: Some(e.to_string()),
				format_id: Some(format_id),
				provider_plugin_id,
			})?;
			Err(e.to_string())
		}
		Err(e) => Err(e.to_string()),
	}
}

fn run_inspect(path: PathBuf, json: bool) -> Result<(), String> {
	let path_str = path.to_string_lossy().to_string();
	let file = read_una_any(&path).map_err(|e| e.to_string())?;
	if json {
		let out = InspectReport { path: path_str, una: file };
		write_json_stdout(&out)?;
		return Ok(());
	}
	println!("path: {}", path.display());
	println!("format_version: {}", file.format_version);
	println!("scene.empty: {}", file.scene.empty);
	Ok(())
}

fn eye_like_material_name(name: Option<&str>) -> bool {
	let Some(n) = name else {
		return false;
	};
	let l = n.to_ascii_lowercase();
	l.contains("iris")
		|| l.contains("pupil")
		|| l.contains("eyeball")
		|| l.contains("cornea")
		|| l.contains("sight")
		|| l.contains("eyelid")
		|| l.contains("eyelash")
		|| l.contains("eyeline")
		|| l.contains("eyeliner")
		|| l.contains("eyebrow")
		|| l.contains("brow")
		|| l.contains("lash")
		|| l.contains("lid")
		|| l.contains("瞳")
		|| l.contains("虹彩")
		|| l.contains("虹膜")
		|| l.contains("目玉")
		|| l.contains("眼睛")
		|| l.contains("眼球")
		|| l.contains("眼珠")
		|| l.contains("眼白")
		|| l.contains("瞼")
		|| l.contains("まぶた")
		|| l.contains("まつげ")
		|| l.contains("睫")
		|| l.contains("眉")
		|| l.contains("眼睑")
		|| l.contains("眼瞼")
		|| l.contains("眼皮")
		|| l.contains("アイライン")
		|| l.contains("アイラッシュ")
		|| l.contains("eye")
		|| l.contains("highlight")
		|| l.contains("ハイライト")
		|| l.contains("高光")
}

fn bump_count(map: &mut BTreeMap<String, usize>, key: impl Into<String>) {
	*map.entry(key.into()).or_insert(0) += 1;
}

fn pixel_format_has_alpha(format: UnaImagePixelFormat) -> bool {
	matches!(
		format,
		UnaImagePixelFormat::R8G8
			| UnaImagePixelFormat::R8G8B8A8
			| UnaImagePixelFormat::R16G16B16A16
			| UnaImagePixelFormat::R16G16B16A16Float
			| UnaImagePixelFormat::R32G32B32A32Float
	)
}

fn texture_alpha_summary(scene: &UnaSceneSnapshot, image_index: Option<usize>) -> Option<DiagnoseTextureAlphaSummary> {
	let image_index = image_index?;
	let image = scene.images.get(image_index)?;
	let pixels = image.rgba8_compat_pixels();
	let mut min_alpha = u8::MAX;
	let mut max_alpha = u8::MIN;
	let mut transparent_pixels = 0usize;
	let mut translucent_pixels = 0usize;
	let mut opaque_pixels = 0usize;
	for pixel in pixels.chunks_exact(4) {
		let alpha = pixel[3];
		min_alpha = min_alpha.min(alpha);
		max_alpha = max_alpha.max(alpha);
		match alpha {
			0 => transparent_pixels += 1,
			255 => opaque_pixels += 1,
			_ => translucent_pixels += 1,
		}
	}
	let total_pixels = transparent_pixels + translucent_pixels + opaque_pixels;
	let coverage = if total_pixels == 0 {
		0.0
	} else {
		(opaque_pixels as f32 + translucent_pixels as f32) / total_pixels as f32
	};
	Some(DiagnoseTextureAlphaSummary {
		image: image_index,
		width: image.width,
		height: image.height,
		pixel_format: image.pixel_format,
		has_alpha_channel: pixel_format_has_alpha(image.pixel_format),
		min_alpha: if total_pixels == 0 { 0 } else { min_alpha },
		max_alpha: if total_pixels == 0 { 0 } else { max_alpha },
		transparent_pixels,
		translucent_pixels,
		opaque_pixels,
		coverage,
	})
}

fn material_source_shader_is_liltoon(material: &UnaMaterialPbr) -> bool {
	material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("sourceShader"))
		.and_then(|v| v.as_str())
		.is_some_and(|shader| shader.to_ascii_lowercase().contains("liltoon"))
		|| material
			.unavatar_material
			.as_ref()
			.and_then(|m| m.get("family"))
			.and_then(|v| v.as_str())
			.is_some_and(|family| family.eq_ignore_ascii_case("liltoon"))
}

fn material_has_source_params(material: &UnaMaterialPbr) -> bool {
	material_source_param_count(material, "floatParams") > 0 || material_source_param_count(material, "colorParams") > 0
}

fn material_source_param_count(material: &UnaMaterialPbr, key: &str) -> usize {
	material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get(key).or_else(|| m.get(&key.replace("Params", "_params"))))
		.and_then(|v| v.as_object())
		.map_or(0, |params| params.len())
}

fn material_source_float_param(material: &UnaMaterialPbr, name: &str) -> Option<f32> {
	material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("floatParams").or_else(|| m.get("float_params")))
		.and_then(|params| params.get(name))
		.and_then(|value| value.as_f64())
		.map(|value| value as f32)
}

fn material_enabled_keywords(material: &UnaMaterialPbr) -> Vec<String> {
	const KEYWORD_FIELDS: &[&str] = &[
		"enabledKeywords",
		"enabled_keywords",
		"shaderKeywords",
		"shader_keywords",
		"keywords",
	];
	let Some(extras) = material.unavatar_material.as_ref() else {
		return Vec::new();
	};
	let mut out = BTreeSet::new();
	for field in KEYWORD_FIELDS {
		match extras.get(*field) {
			Some(serde_json::Value::Array(values)) => {
				for value in values {
					if let Some(keyword) = value.as_str().filter(|keyword| !keyword.is_empty()) {
						out.insert(keyword.to_string());
					}
				}
			}
			Some(serde_json::Value::Object(values)) => {
				for (keyword, value) in values {
					if value.as_bool().unwrap_or(false) && !keyword.is_empty() {
						out.insert(keyword.to_string());
					}
				}
			}
			_ => {}
		}
	}
	out.into_iter().collect()
}

fn material_keyword_contains(material: &UnaMaterialPbr, needle: &str) -> bool {
	let needle = needle.to_ascii_lowercase();
	material_enabled_keywords(material)
		.iter()
		.any(|keyword| keyword.to_ascii_lowercase().contains(&needle))
}

fn material_source_shader_lower(material: &UnaMaterialPbr) -> String {
	material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("sourceShader"))
		.and_then(|v| v.as_str())
		.unwrap_or("")
		.to_ascii_lowercase()
}

fn material_liltoon_features(material: &UnaMaterialPbr) -> Vec<String> {
	if !material_source_shader_is_liltoon(material) {
		return Vec::new();
	}
	let shader = material_source_shader_lower(material);
	let mut features = BTreeSet::new();
	if shader.contains("lite") {
		features.insert("lite".to_string());
	}
	if shader.contains("cutout") || matches!(material.alpha_mode, UnaAlphaMode::Mask) {
		features.insert("cutout".to_string());
	}
	if shader.contains("transparent") || matches!(material.alpha_mode, UnaAlphaMode::Blend) {
		features.insert("transparent".to_string());
	}
	if shader.contains("twopass") || shader.contains("two_pass") || material_source_float_param(material, "_PreZWrite").is_some() {
		features.insert("twopass".to_string());
	}
	if shader.contains("outline") || material_source_float_param(material, "_UseOutline").is_some_and(|value| value > 0.5) {
		features.insert("outline".to_string());
	}
	if shader.contains("fur")
		|| material_keyword_contains(material, "fur")
		|| material_source_float_param(material, "_UseFur").is_some_and(|value| value > 0.5)
	{
		features.insert("fur".to_string());
	}
	if shader.contains("refraction")
		|| material_keyword_contains(material, "refraction")
		|| material_source_float_param(material, "_UseRefraction").is_some_and(|value| value > 0.5)
	{
		features.insert("refraction".to_string());
	}
	if shader.contains("gem") {
		features.insert("gem".to_string());
	}
	if material_keyword_contains(material, "alphamask")
		|| material_source_float_param(material, "_AlphaMaskMode").is_some_and(|value| value > 0.5)
	{
		features.insert("alpha_mask".to_string());
	}
	if features.is_empty() {
		features.insert("common".to_string());
	}
	features.into_iter().collect()
}

fn material_render_float_params(material: &UnaMaterialPbr) -> BTreeMap<String, f32> {
	const PARAMS: &[&str] = &[
		"_TransparentMode",
		"_AlphaMode",
		"_BlendMode",
		"_Cutoff",
		"_SubpassCutoff",
		"_SrcBlend",
		"_DstBlend",
		"_SrcBlendAlpha",
		"_DstBlendAlpha",
		"_ZWrite",
		"_PreZWrite",
		"_Cull",
		"_PreCull",
		"_ColorMask",
		"_PreColorMask",
		"_AlphaToMask",
		"_PreAlphaToMask",
	];
	PARAMS
		.iter()
		.filter_map(|name| material_source_float_param(material, name).map(|value| ((*name).to_string(), value)))
		.collect()
}

fn material_summary(index: usize, material: &UnaMaterialPbr, scene: &UnaSceneSnapshot) -> DiagnoseMaterialSummary {
	let source_shader = material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("sourceShader"))
		.and_then(|v| v.as_str())
		.map(str::to_owned);
	let material_family = material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("family"))
		.and_then(|v| v.as_str())
		.map(str::to_owned);
	let render_queue = material
		.unavatar_material
		.as_ref()
		.and_then(|m| m.get("renderQueue"))
		.and_then(|v| v.as_i64())
		.map(|v| v as i32);
	let liltoon_features = material_liltoon_features(material);
	let mtoon = material.mtoon.as_ref().map(|m| DiagnoseMToonSummary {
		transparent_with_z_write: m.transparent_with_z_write,
		shade_color_factor: m.shade_color_factor,
		shade_multiply_texture_index: m.shade_multiply_texture_index,
		shading_shift_factor: m.shading_shift_factor,
		shading_shift_texture_index: m.shading_shift_texture_index,
		shading_toony_factor: m.shading_toony_factor,
		gi_equalization_factor: m.gi_equalization_factor,
		matcap_factor: m.matcap_factor,
		matcap_texture_index: m.matcap_texture_index,
		parametric_rim_color_factor: m.parametric_rim_color_factor,
		rim_multiply_texture_index: m.rim_multiply_texture_index,
		reflection_cube_texture_index: m.reflection_cube_texture_index,
		outline_width_mode: m.outline_width_mode,
		outline_width_factor: m.outline_width_factor,
		outline_width_multiply_texture_index: m.outline_width_multiply_texture_index,
		outline_color_factor: m.outline_color_factor,
		emissive_factor: material.emissive_factor,
		emissive_texture_index: material.emissive_texture_index,
	});
	DiagnoseMaterialSummary {
		index,
		name: material.name.clone(),
		source_shader,
		material_family,
		render_queue,
		source_float_param_count: material_source_param_count(material, "floatParams"),
		source_color_param_count: material_source_param_count(material, "colorParams"),
		source_render_float_params: material_render_float_params(material),
		liltoon_features,
		shading: material.shading,
		alpha_mode: material.alpha_mode,
		alpha_cutoff: material.alpha_cutoff,
		double_sided: material.double_sided,
		cull_mode: material.cull_mode,
		base_color_factor: material.base_color_factor,
		base_color_texture_index: material.base_color_texture_index,
		base_color_texture_alpha: texture_alpha_summary(scene, material.base_color_texture_index),
		normal_texture_index: material.normal_texture_index,
		normal_texture_scale: material.normal_texture_scale,
		eye_like_name: eye_like_material_name(material.name.as_deref()),
		mtoon,
	}
}

fn scene_node_paths_by_index(scene: &un_avatar_core::UnaSceneSnapshot) -> Vec<Option<String>> {
	fn visit(scene: &un_avatar_core::UnaSceneSnapshot, idx: usize, parent: &str, out: &mut [Option<String>]) {
		let Some(node) = scene.nodes.get(idx) else { return };
		let segment = node.name.as_deref().unwrap_or("");
		let path = if parent.is_empty() {
			segment.to_string()
		} else if segment.is_empty() {
			parent.to_string()
		} else {
			format!("{parent}/{segment}")
		};
		if let Some(slot) = out.get_mut(idx) {
			*slot = (!path.is_empty()).then_some(path.clone());
		}
		for &child in &node.children {
			visit(scene, child, &path, out);
		}
	}

	let mut out = vec![None; scene.nodes.len()];
	for &root in &scene.roots {
		visit(scene, root, "", &mut out);
	}
	out
}

fn scene_effective_visibility(scene: &un_avatar_core::UnaSceneSnapshot) -> Vec<bool> {
	fn visit(scene: &un_avatar_core::UnaSceneSnapshot, idx: usize, parent_visible: bool, out: &mut [bool]) {
		let Some(node) = scene.nodes.get(idx) else { return };
		let visible = parent_visible && node.visible;
		if let Some(slot) = out.get_mut(idx) {
			*slot = visible;
		}
		for &child in &node.children {
			visit(scene, child, visible, out);
		}
	}

	let mut out = vec![false; scene.nodes.len()];
	for &root in &scene.roots {
		visit(scene, root, true, &mut out);
	}
	out
}

fn dynamics_group_summaries(doc: &UnaDocument) -> Vec<DiagnoseDynamicsGroupSummary> {
	let (Some(scene), Some(settings)) = (doc.scene.as_ref(), doc.runtime_model().dynamics().spring_bones()) else {
		return Vec::new();
	};
	let node_paths_by_index = scene_node_paths_by_index(scene);
	settings
		.groups
		.iter()
		.enumerate()
		.map(|(index, group)| {
			let root_node = group.bone_node_indices.first().copied();
			let tip_node = group.bone_node_indices.last().copied();
			DiagnoseDynamicsGroupSummary {
				index,
				source_kind: group.source_kind,
				comment: group.comment.clone(),
				category: group.category.clone(),
				bone_count: group.bone_node_indices.len(),
				root_node,
				root_path: root_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				tip_node,
				tip_path: tip_node.and_then(|node| node_paths_by_index.get(node).cloned().flatten()),
				stiffness: group.stiffness,
				drag_force: group.drag_force,
				gravity_power: group.gravity_power,
				gravity_dir: group.gravity_dir,
				limit_type: group
					.limit
					.as_ref()
					.and_then(|limit| (!limit.limit_type.is_empty()).then(|| limit.limit_type.clone())),
				max_angle_x: group.limit.as_ref().map(|limit| limit.max_angle_x),
				max_angle_z: group.limit.as_ref().map(|limit| limit.max_angle_z),
				max_stretch: group.limit.as_ref().map(|limit| limit.max_stretch),
				hit_radius: group.hit_radius,
			}
		})
		.collect()
}

fn dynamics_source_feature_counts(doc: &UnaDocument) -> DynamicsSourceFeatureCounts {
	let Some(unavatar) = doc.unavatar.as_ref() else {
		return DynamicsSourceFeatureCounts::default();
	};
	let Some(dynamics) = unavatar.source.get("dynamics").and_then(|value| value.as_array()) else {
		return DynamicsSourceFeatureCounts::default();
	};
	let mut counts = DynamicsSourceFeatureCounts::default();
	for item in dynamics {
		let source_params = item.get("sourceParams").or_else(|| item.get("source_params"));
		let limit_type = source_params
			.and_then(|params| params.get("limitType").or_else(|| params.get("limit_type")))
			.or_else(|| item.get("limitType").or_else(|| item.get("limit_type")))
			.and_then(|value| value.as_str())
			.unwrap_or("");
		let max_angle_x = source_params
			.and_then(|params| params.get("maxAngleX").or_else(|| params.get("max_angle_x")))
			.or_else(|| item.get("maxAngleX").or_else(|| item.get("max_angle_x")))
			.and_then(json_number_f64)
			.unwrap_or(0.0);
		let max_angle_z = source_params
			.and_then(|params| params.get("maxAngleZ").or_else(|| params.get("max_angle_z")))
			.or_else(|| item.get("maxAngleZ").or_else(|| item.get("max_angle_z")))
			.and_then(json_number_f64)
			.unwrap_or(0.0);
		if !limit_type.is_empty() || max_angle_x.abs() > 0.0 || max_angle_z.abs() > 0.0 {
			counts.limit_count += 1;
		}
		if source_params
			.and_then(|params| params.get("allowCollision").or_else(|| params.get("allow_collision")))
			.and_then(|value| value.as_bool())
			== Some(false)
		{
			counts.collision_disabled_count += 1;
		}
		if source_params
			.and_then(|params| params.get("allowGrabbing").or_else(|| params.get("allow_grabbing")))
			.and_then(|value| value.as_bool())
			== Some(true)
		{
			counts.grabbing_enabled_count += 1;
		}
		if source_params
			.and_then(|params| params.get("allowPosing").or_else(|| params.get("allow_posing")))
			.and_then(|value| value.as_bool())
			== Some(true)
		{
			counts.posing_enabled_count += 1;
		}
		if let Some(colliders) = source_params
			.and_then(|params| params.get("colliders"))
			.or_else(|| item.get("colliders"))
			.and_then(|value| value.as_array())
		{
			counts.inside_bounds_collider_count += colliders
				.iter()
				.filter(|collider| {
					collider
						.get("insideBounds")
						.or_else(|| collider.get("inside_bounds"))
						.and_then(|value| value.as_bool())
						== Some(true)
				})
				.count();
		}
	}
	counts
}

fn json_number_f64(value: &serde_json::Value) -> Option<f64> {
	value.as_f64().or_else(|| value.as_i64().map(|value| value as f64))
}

fn visible_mesh_materials(scene: &un_avatar_core::UnaSceneSnapshot, mesh_index: usize) -> Vec<DiagnoseVisibleMaterialSummary> {
	let Some(primitives) = scene.meshes.get(mesh_index) else {
		return Vec::new();
	};
	primitives
		.iter()
		.enumerate()
		.filter_map(|(primitive_index, primitive)| {
			let material_index = primitive.material_index?;
			let material = scene.materials.get(material_index)?;
			let draw_skipped_fully_transparent = matches!(material.alpha_mode, UnaAlphaMode::Mask | UnaAlphaMode::Blend)
				&& material.base_color_factor[3] <= 0.001
				&& texture_alpha_summary(scene, material.base_color_texture_index).is_some_and(|alpha| alpha.max_alpha == 0);
			let nonzero_morph_weights = primitive
				.default_morph_weights
				.iter()
				.enumerate()
				.filter(|(_, weight)| weight.abs() > 0.000001)
				.map(|(index, &weight)| DiagnoseMorphWeightSummary {
					index,
					name: primitive.morph_target_names.get(index).cloned(),
					weight,
					position_delta_abs_sum: primitive
						.morph_targets
						.get(index)
						.map(|target| target.position_deltas.iter().map(|v| v[0].abs() + v[1].abs() + v[2].abs()).sum())
						.unwrap_or(0.0),
					normal_delta_abs_sum: primitive
						.morph_targets
						.get(index)
						.and_then(|target| target.normal_deltas.as_ref())
						.map(|deltas| deltas.iter().map(|v| v[0].abs() + v[1].abs() + v[2].abs()).sum())
						.unwrap_or(0.0),
				})
				.collect();
			Some(DiagnoseVisibleMaterialSummary {
				primitive: primitive_index,
				index: material_index,
				name: material.name.clone(),
				source_shader: material
					.unavatar_material
					.as_ref()
					.and_then(|m| m.get("sourceShader"))
					.and_then(|v| v.as_str())
					.map(str::to_owned),
				shading: material.shading,
				alpha_mode: material.alpha_mode,
				alpha_cutoff: material.alpha_cutoff,
				transparent_with_z_write: material.mtoon.as_ref().is_some_and(|mtoon| mtoon.transparent_with_z_write),
				draw_skipped_fully_transparent,
				morph_target_count: primitive.morph_targets.len(),
				nonzero_morph_weights,
			})
		})
		.collect()
}

fn skin_summaries(scene: &un_avatar_core::UnaSceneSnapshot) -> Vec<DiagnoseSkinSummary> {
	const RENDERER_MAX_BONES: usize = 512;
	let mut summaries = scene
		.skins
		.iter()
		.enumerate()
		.map(|(index, skin)| {
			let effective_joint_count = skin.joint_nodes.len().min(skin.inverse_bind_matrices.len());
			DiagnoseSkinSummary {
				index,
				joint_count: skin.joint_nodes.len(),
				inverse_bind_count: skin.inverse_bind_matrices.len(),
				effective_joint_count,
				over_renderer_bone_limit: effective_joint_count > RENDERER_MAX_BONES,
				skeleton_node: skin.skeleton_node,
				used_by_node_count: 0,
				primitive_joint_attribute_count: 0,
				primitive_weight_attribute_count: 0,
				mismatched_joint_weight_attribute_count: 0,
				max_joint_index: None,
				out_of_range_joint_attribute_count: 0,
			}
		})
		.collect::<Vec<_>>();
	for node in &scene.nodes {
		let Some(skin_index) = node.skin else { continue };
		let Some(summary) = summaries.get_mut(skin_index) else { continue };
		summary.used_by_node_count += 1;
		let joint_bound = summary.joint_count.min(summary.inverse_bind_count);
		let Some(mesh_index) = node.mesh else { continue };
		let Some(primitives) = scene.meshes.get(mesh_index) else { continue };
		for primitive in primitives {
			if primitive.joints.is_some() != primitive.weights.is_some() {
				summary.mismatched_joint_weight_attribute_count += 1;
			}
			if let Some(joints) = primitive.joints.as_ref() {
				summary.primitive_joint_attribute_count += 1;
				if let Some(max_joint) = joints.iter().flatten().copied().max() {
					summary.max_joint_index = Some(summary.max_joint_index.map_or(max_joint, |current| current.max(max_joint)));
				}
				if joints
					.iter()
					.flatten()
					.any(|joint| (*joint as usize) >= joint_bound || (*joint as usize) >= RENDERER_MAX_BONES)
				{
					summary.out_of_range_joint_attribute_count += 1;
				}
			}
			if primitive.weights.is_some() {
				summary.primitive_weight_attribute_count += 1;
			}
		}
	}
	summaries
}

fn unavatar_wardrobe_sets(doc: &UnaDocument) -> Vec<(String, Option<String>)> {
	doc.unavatar
		.as_ref()
		.and_then(|ext| ext.source.get("wardrobe"))
		.and_then(|wardrobe| wardrobe.get("sets"))
		.and_then(|sets| sets.as_array())
		.map(|sets| {
			sets.iter()
				.filter_map(|set| {
					let id = set.get("id").and_then(|v| v.as_str())?.to_owned();
					let display_name = json_string(set.get("displayName"));
					Some((id, display_name))
				})
				.collect()
		})
		.unwrap_or_default()
}

fn unavatar_base_set_id(doc: &UnaDocument) -> String {
	doc.unavatar
		.as_ref()
		.and_then(|ext| ext.source.get("wardrobe"))
		.and_then(|wardrobe| wardrobe.get("baseSet"))
		.and_then(|v| v.as_str())
		.unwrap_or("base")
		.to_owned()
}

fn wardrobe_probe_for_document(
	set_id: String,
	display_name: Option<String>,
	doc: &UnaDocument,
	apply_report: Option<WardrobeApplyReport>,
	probe_ms: u128,
) -> DiagnoseWardrobeProbeSummary {
	let mut visible_mesh_paths = Vec::new();
	let mut nonzero_morph_weights = Vec::new();
	if let Some(scene) = doc.scene.as_ref() {
		let effective_visibility = scene_effective_visibility(scene);
		let node_paths_by_index = scene_node_paths_by_index(scene);
		for (node_index, node) in scene.nodes.iter().enumerate() {
			if !effective_visibility.get(node_index).copied().unwrap_or(false) {
				continue;
			}
			if node.mesh.is_some() {
				let path = node_paths_by_index
					.get(node_index)
					.cloned()
					.flatten()
					.or_else(|| node.name.clone())
					.unwrap_or_else(|| format!("#{node_index}"));
				visible_mesh_paths.push(path);
			}
		}
		for (mesh_index, primitives) in scene.meshes.iter().enumerate() {
			for (primitive_index, primitive) in primitives.iter().enumerate() {
				for (weight_index, &weight) in primitive.default_morph_weights.iter().enumerate() {
					if weight.abs() > 0.000001 {
						nonzero_morph_weights.push(DiagnoseWardrobeProbeMorphSummary {
							mesh: mesh_index,
							primitive: primitive_index,
							index: weight_index,
							name: primitive.morph_target_names.get(weight_index).cloned(),
							weight,
						});
					}
				}
			}
		}
	}
	let (visibility_applied, visibility_missing, blendshape_applied, blendshape_missing, missing_visibility_paths, missing_blendshapes) =
		if let Some(report) = apply_report {
			(
				Some(report.visibility_applied),
				Some(report.visibility_missing),
				Some(report.blendshape_applied),
				Some(report.blendshape_missing),
				report.missing_visibility_paths,
				report.missing_blendshapes,
			)
		} else {
			(None, None, None, None, Vec::new(), Vec::new())
		};
	DiagnoseWardrobeProbeSummary {
		set_id,
		display_name,
		probe_ms,
		visibility_applied,
		visibility_missing,
		blendshape_applied,
		blendshape_missing,
		visible_mesh_node_count: visible_mesh_paths.len(),
		visible_mesh_paths,
		nonzero_morph_weight_count: nonzero_morph_weights.len(),
		nonzero_morph_weights,
		missing_visibility_paths,
		missing_blendshapes,
	}
}

fn build_wardrobe_probes(base_doc: &UnaDocument) -> Result<Vec<DiagnoseWardrobeProbeSummary>, String> {
	if base_doc.unavatar.is_none() {
		return Ok(Vec::new());
	}
	let base_id = unavatar_base_set_id(base_doc);
	let sets = unavatar_wardrobe_sets(base_doc);
	let mut probes = Vec::new();
	let base_display_name = sets
		.iter()
		.find(|(id, _)| id == &base_id)
		.and_then(|(_, display_name)| display_name.clone());
	let started = Instant::now();
	probes.push(wardrobe_probe_for_document(
		base_id.clone(),
		base_display_name,
		base_doc,
		None,
		started.elapsed().as_millis(),
	));
	for (set_id, display_name) in sets {
		if set_id == base_id {
			continue;
		}
		let started = Instant::now();
		let mut doc = base_doc.clone();
		let apply_report = apply_unavatar_wardrobe_set(&mut doc, &set_id)?;
		probes.push(wardrobe_probe_for_document(
			set_id,
			display_name,
			&doc,
			Some(apply_report),
			started.elapsed().as_millis(),
		));
	}
	Ok(probes)
}

fn json_string(value: Option<&serde_json::Value>) -> Option<String> {
	value.and_then(|v| v.as_str()).map(str::to_owned)
}

fn diagnose_texture_shape_is_cube(shape: Option<&str>) -> bool {
	shape.is_some_and(|shape| shape.eq_ignore_ascii_case("TextureCube") || shape.eq_ignore_ascii_case("Cube"))
}

fn unavatar_summary(ext: &un_avatar_core::UnaUnavatarExtension) -> DiagnoseUnavatarSummary {
	let source = &ext.source;
	let wardrobe = source.get("wardrobe");
	let sets = wardrobe.and_then(|w| w.get("sets")).and_then(|v| v.as_array());
	let base_set = json_string(wardrobe.and_then(|w| w.get("baseSet")));
	let base = sets.and_then(|sets| {
		sets.iter().find(|set| {
			let is_named_base = base_set
				.as_deref()
				.is_some_and(|base_set| set.get("id").and_then(|v| v.as_str()) == Some(base_set));
			let is_default = set.get("default").and_then(|v| v.as_bool()).unwrap_or(false);
			is_named_base || is_default
		})
	});
	let base_operations = base.and_then(|set| set.get("operations")).and_then(|v| v.as_array());
	let mut base_operation_counts = BTreeMap::new();
	if let Some(base_operations) = base_operations {
		for op in base_operations {
			let ty = op
				.get("type")
				.or_else(|| op.get("op"))
				.and_then(|v| v.as_str())
				.unwrap_or("unknown");
			bump_count(&mut base_operation_counts, ty);
		}
	}
	let wardrobe_set_ids = sets
		.map(|sets| {
			sets.iter()
				.filter_map(|set| set.get("id").and_then(|v| v.as_str()).map(str::to_owned))
				.collect()
		})
		.unwrap_or_default();
	let wardrobe_sets = sets
		.map(|sets| {
			sets.iter()
				.map(|set| {
					let operations = set.get("operations").and_then(|v| v.as_array());
					let mut operation_counts = BTreeMap::new();
					if let Some(operations) = operations {
						for op in operations {
							let ty = op
								.get("type")
								.or_else(|| op.get("op"))
								.and_then(|v| v.as_str())
								.unwrap_or("unknown");
							bump_count(&mut operation_counts, ty);
						}
					}
					let asset_groups = set
						.get("assetGroups")
						.and_then(|v| v.as_array())
						.map(|groups| groups.iter().filter_map(|g| g.as_str().map(str::to_owned)).collect())
						.unwrap_or_default();
					DiagnoseUnavatarWardrobeSetSummary {
						id: set.get("id").and_then(|v| v.as_str()).unwrap_or("").to_owned(),
						display_name: json_string(set.get("displayName")),
						source: json_string(set.get("source")),
						asset_groups,
						operation_count: operations.map(Vec::len).unwrap_or(0),
						operation_counts,
					}
				})
				.collect()
		})
		.unwrap_or_default();

	DiagnoseUnavatarSummary {
		spec_version: ext.spec_version.clone(),
		generator: json_string(source.get("generator")),
		manifest_name: json_string(source.get("manifest").and_then(|m| m.get("name"))),
		source_type: json_string(source.get("manifest").and_then(|m| m.get("sourceType"))),
		extension_node_count: source.get("nodes").and_then(|v| v.as_array()).map(Vec::len).unwrap_or(0),
		variant_count: source.get("variants").and_then(|v| v.as_array()).map(Vec::len).unwrap_or(0),
		dynamics_entry_count: source.get("dynamics").and_then(|v| v.as_array()).map(Vec::len).unwrap_or(0),
		base_set,
		wardrobe_set_count: sets.map(Vec::len).unwrap_or(0),
		wardrobe_set_ids,
		wardrobe_sets,
		base_operation_count: base_operations.map(Vec::len).unwrap_or(0),
		base_operation_counts,
	}
}

fn build_diagnose_report(
	path: &Path,
	import_format_id: String,
	provider_plugin_id: Option<String>,
	timings: DiagnoseTimingSummary,
	import_report: ImportReport,
	doc: UnaDocument,
	wardrobe_probes: Vec<DiagnoseWardrobeProbeSummary>,
) -> DiagnoseReport {
	let mut warnings = Vec::new();
	let scene = if let Some(sc) = doc.scene.as_ref() {
		let mut shading_counts = BTreeMap::new();
		let mut alpha_counts = BTreeMap::new();
		let mut eye_like_material_indices = Vec::new();
		let mut materials = Vec::new();
		let mut liltoon_material_count = 0usize;
		let mut liltoon_missing_render_queue = 0usize;
		let mut liltoon_missing_source_params = 0usize;
		let mut liltoon_feature_counts = BTreeMap::new();
		let mut suspicious_liltoon_masks = Vec::new();
		let mut fully_transparent_visible_materials = Vec::new();
		for (i, material) in sc.materials.iter().enumerate() {
			bump_count(&mut shading_counts, format!("{:?}", material.shading));
			bump_count(&mut alpha_counts, format!("{:?}", material.alpha_mode));
			if material_source_shader_is_liltoon(material) {
				liltoon_material_count += 1;
				for feature in material_liltoon_features(material) {
					bump_count(&mut liltoon_feature_counts, feature);
				}
				if material
					.unavatar_material
					.as_ref()
					.and_then(|m| m.get("renderQueue").or_else(|| m.get("render_queue")))
					.is_none()
				{
					liltoon_missing_render_queue += 1;
				}
				if !material_has_source_params(material) {
					liltoon_missing_source_params += 1;
				}
				if material.alpha_mode == UnaAlphaMode::Mask && material.alpha_cutoff > 0.01 {
					suspicious_liltoon_masks.push(i);
				}
			}
			if matches!(material.alpha_mode, UnaAlphaMode::Mask | UnaAlphaMode::Blend)
				&& material.base_color_factor[3] <= 0.001
				&& texture_alpha_summary(sc, material.base_color_texture_index).is_some_and(|alpha| alpha.max_alpha == 0)
			{
				fully_transparent_visible_materials.push(i);
			}
			if eye_like_material_name(material.name.as_deref()) {
				eye_like_material_indices.push(i);
				if material.alpha_mode == UnaAlphaMode::Mask && material.base_color_factor[3] <= 0.001 {
					warnings.push(format!(
						"eye-like material[{i}] is MASK with near-zero material alpha; consider --relax-iris-alpha"
					));
				}
			}
			materials.push(material_summary(i, material, sc));
		}
		if liltoon_material_count > 0 && liltoon_missing_render_queue > 0 {
			warnings.push(format!(
				"lilToon material source payload is missing renderQueue on {liltoon_missing_render_queue}/{liltoon_material_count} materials; re-export with the current Unity exporter to improve alpha/order diagnostics"
			));
		}
		if liltoon_material_count > 0 && liltoon_missing_source_params > 0 {
			warnings.push(format!(
				"lilToon material source payload is missing floatParams/colorParams on {liltoon_missing_source_params}/{liltoon_material_count} materials; re-export with the current Unity exporter before UNToon compatibility tuning"
			));
		}
		if !suspicious_liltoon_masks.is_empty() {
			warnings.push(format!(
				"lilToon materials with MASK alpha and cutoff > 0.01: {:?}; verify these are actual Cutout materials, not ordinary Opaque shaders with _Cutoff",
				suspicious_liltoon_masks
			));
		}
		let high_risk_hits = ["fur", "refraction", "gem", "twopass"]
			.iter()
			.filter_map(|feature| {
				liltoon_feature_counts
					.get(*feature)
					.copied()
					.filter(|count| *count > 0)
					.map(|count| (*feature, count))
			})
			.collect::<Vec<_>>();
		if !high_risk_hits.is_empty() {
			warnings.push(format!(
				"lilToon high-variance shader features present: {:?}; verify these against Unity because they depend on extra passes or screen/environment inputs",
				high_risk_hits
			));
		}
		if !fully_transparent_visible_materials.is_empty() {
			warnings.push(format!(
				"fully transparent alpha materials are present: {:?}; renderer may skip these draws unless used as authoring helpers",
				fully_transparent_visible_materials
			));
		}
		if doc.vrm.is_some() && !sc.materials.is_empty() && !sc.materials.iter().any(|m| m.shading == UnaShadingModel::MToonLike) {
			warnings.push("VRM document has no MToonLike materials after import".to_string());
		}
		let mut image_source_mime_counts = BTreeMap::new();
		let mut image_source_color_space_counts = BTreeMap::new();
		let mut image_source_texture_type_counts = BTreeMap::new();
		let mut image_source_texture_shape_counts = BTreeMap::new();
		let mut image_source_layout_counts = BTreeMap::new();
		let mut image_pixel_format_counts = BTreeMap::new();
		let mut image_source_count = 0usize;
		let mut image_source_bytes = 0u64;
		let mut largest_image_sources = Vec::new();
		for (index, image) in sc.images.iter().enumerate() {
			bump_count(&mut image_pixel_format_counts, format!("{:?}", image.pixel_format));
			if let Some(source) = sc.image_sources.get(index).and_then(Option::as_ref) {
				let is_cube_source = diagnose_texture_shape_is_cube(source.texture_shape.as_deref());
				largest_image_sources.push(DiagnoseImageSourceSummary {
					index,
					name: source.name.clone(),
					mime_type: source.mime_type.clone(),
					uri: source.uri.clone(),
					source_pixel_format: source.source_pixel_format.clone(),
					channels: source.channels.clone(),
					color_space: source.color_space.clone(),
					texture_type: source.texture_type.clone(),
					texture_shape: source.texture_shape.clone(),
					source_layout: is_cube_source.then(|| source.source_layout.clone()).flatten(),
					unity_generate_cubemap: is_cube_source.then(|| source.unity_generate_cubemap.clone()).flatten(),
					srgb: source.srgb,
					byte_length: source.byte_length,
					pixel_format: image.pixel_format,
					width: image.width,
					height: image.height,
				});
			}
		}
		largest_image_sources.sort_by(|a, b| b.byte_length.cmp(&a.byte_length).then_with(|| a.index.cmp(&b.index)));
		largest_image_sources.truncate(12);
		for source in sc.image_sources.iter().flatten() {
			image_source_count += 1;
			image_source_bytes = image_source_bytes.saturating_add(source.byte_length);
			bump_count(
				&mut image_source_mime_counts,
				source.mime_type.as_deref().unwrap_or("unknown").to_string(),
			);
			bump_count(
				&mut image_source_color_space_counts,
				source.color_space.as_deref().unwrap_or("unknown").to_string(),
			);
			bump_count(
				&mut image_source_texture_type_counts,
				source.texture_type.as_deref().unwrap_or("unknown").to_string(),
			);
			bump_count(
				&mut image_source_texture_shape_counts,
				source.texture_shape.as_deref().unwrap_or("unknown").to_string(),
			);
			bump_count(
				&mut image_source_layout_counts,
				source.source_layout.as_deref().unwrap_or("unknown").to_string(),
			);
		}
		let effective_visibility = scene_effective_visibility(sc);
		let node_paths_by_index = scene_node_paths_by_index(sc);
		let mut visible_material_indices = Vec::new();
		let mut visible_shading_counts = BTreeMap::new();
		let mut visible_alpha_counts = BTreeMap::new();
		let visible_mesh_nodes = sc
			.nodes
			.iter()
			.enumerate()
			.filter(|(idx, _)| effective_visibility.get(*idx).copied().unwrap_or(false))
			.filter_map(|(idx, node)| {
				if let Some(mesh) = node.mesh {
					if let Some(primitives) = sc.meshes.get(mesh) {
						for primitive in primitives {
							let Some(material_index) = primitive.material_index else { continue };
							let Some(material) = sc.materials.get(material_index) else {
								continue;
							};
							visible_material_indices.push(material_index);
							bump_count(&mut visible_shading_counts, format!("{:?}", material.shading));
							bump_count(&mut visible_alpha_counts, format!("{:?}", material.alpha_mode));
						}
					}
				}
				node.mesh.map(|mesh| DiagnoseVisibleMeshNodeSummary {
					node: idx,
					name: node.name.clone(),
					path: node_paths_by_index.get(idx).cloned().flatten(),
					source_node_id: node.source_node_id.clone(),
					mesh,
					skin: node.skin,
					materials: visible_mesh_materials(sc, mesh),
				})
			})
			.collect();
		visible_material_indices.sort_unstable();
		visible_material_indices.dedup();
		let skins = skin_summaries(sc);
		let skin_over_limit = skins
			.iter()
			.filter(|skin| skin.over_renderer_bone_limit)
			.map(|skin| skin.index)
			.collect::<Vec<_>>();
		if !skin_over_limit.is_empty() {
			warnings.push(format!(
				"skins exceed renderer bone palette limit: {:?}; affected vertices will be clamped unless the renderer limit or skin split is improved",
				skin_over_limit
			));
		}
		let skin_mismatched_attrs = skins
			.iter()
			.filter(|skin| skin.mismatched_joint_weight_attribute_count > 0)
			.map(|skin| (skin.index, skin.mismatched_joint_weight_attribute_count))
			.collect::<Vec<_>>();
		if !skin_mismatched_attrs.is_empty() {
			warnings.push(format!(
				"skins have primitives with mismatched JOINTS/WEIGHTS attributes: {:?}; verify exporter and source mesh skinning data",
				skin_mismatched_attrs
			));
		}
		let skin_out_of_range = skins
			.iter()
			.filter(|skin| skin.out_of_range_joint_attribute_count > 0)
			.map(|skin| (skin.index, skin.out_of_range_joint_attribute_count))
			.collect::<Vec<_>>();
		if !skin_out_of_range.is_empty() {
			warnings.push(format!(
				"skins have primitives with joint indices outside effective palette: {:?}; renderer clamps these vertices",
				skin_out_of_range
			));
		}
		DiagnoseSceneSummary {
			has_scene: true,
			mesh_count: sc.meshes.len(),
			primitive_count: sc.meshes.iter().map(Vec::len).sum(),
			morph_target_count: sc.meshes.iter().flatten().map(|primitive| primitive.morph_targets.len()).sum(),
			node_count: sc.nodes.len(),
			hidden_node_count: sc.nodes.iter().filter(|node| !node.visible).count(),
			skin_count: sc.skins.len(),
			image_count: sc.images.len(),
			image_source_count,
			image_source_bytes,
			image_source_mime_counts,
			image_source_color_space_counts,
			image_source_texture_type_counts,
			image_source_texture_shape_counts,
			image_source_layout_counts,
			image_pixel_format_counts,
			non_rgba8_image_count: sc
				.images
				.iter()
				.filter(|image| image.pixel_format != UnaImagePixelFormat::R8G8B8A8)
				.count(),
			largest_image_sources,
			material_count: sc.materials.len(),
			liltoon_feature_counts,
			node_constraint_count: sc.node_constraints.len(),
			shading_counts,
			alpha_counts,
			visible_shading_counts,
			visible_alpha_counts,
			visible_material_indices,
			eye_like_material_indices,
			skins,
			materials,
			visible_mesh_nodes,
		}
	} else {
		warnings.push("imported document has no scene".to_string());
		DiagnoseSceneSummary {
			has_scene: false,
			mesh_count: 0,
			primitive_count: 0,
			morph_target_count: 0,
			node_count: 0,
			hidden_node_count: 0,
			skin_count: 0,
			image_count: 0,
			image_source_count: 0,
			image_source_bytes: 0,
			image_source_mime_counts: BTreeMap::new(),
			image_source_color_space_counts: BTreeMap::new(),
			image_source_texture_type_counts: BTreeMap::new(),
			image_source_texture_shape_counts: BTreeMap::new(),
			image_source_layout_counts: BTreeMap::new(),
			image_pixel_format_counts: BTreeMap::new(),
			non_rgba8_image_count: 0,
			largest_image_sources: Vec::new(),
			material_count: 0,
			liltoon_feature_counts: BTreeMap::new(),
			node_constraint_count: 0,
			shading_counts: BTreeMap::new(),
			alpha_counts: BTreeMap::new(),
			visible_shading_counts: BTreeMap::new(),
			visible_alpha_counts: BTreeMap::new(),
			visible_material_indices: Vec::new(),
			eye_like_material_indices: Vec::new(),
			skins: Vec::new(),
			materials: Vec::new(),
			visible_mesh_nodes: Vec::new(),
		}
	};

	let humanoid = doc.humanoid_profile.as_ref().map(|profile| {
		let keys: Vec<String> = profile.bone_node_indices.keys().cloned().collect();
		DiagnoseHumanoidSummary {
			bone_count: profile.bone_node_indices.len(),
			left_eye_node: profile.bone_node_indices.get("lefteye").copied(),
			right_eye_node: profile.bone_node_indices.get("righteye").copied(),
			keys,
		}
	});
	if doc.vrm.is_some() && humanoid.is_none() {
		warnings.push("VRM document has no humanoid profile".to_string());
	}

	let expression_apply_probe = expression_apply_probe(&doc);
	let expressions = doc.expression_catalog.as_ref().map(|catalog| DiagnoseExpressionSummary {
		preset_count: catalog.presets.len(),
		presets: catalog
			.presets
			.iter()
			.map(|preset| DiagnoseExpressionPresetSummary {
				name: preset.name.clone(),
				bind_count: preset.binds.len(),
			})
			.collect(),
		apply_probe: expression_apply_probe,
	});

	let runtime_model = doc.runtime_model();
	let runtime = DiagnoseRuntimeSummary {
		source_kind: runtime_model.source_kind(),
		humanoid_basis: runtime_model.humanoid_basis(),
	};
	let runtime_dynamics = runtime_model.dynamics();
	let dynamics_groups = dynamics_group_summaries(&doc);
	let dynamics_source_features = dynamics_source_feature_counts(&doc);
	let dynamics = DiagnoseDynamicsSummary {
		group_count: runtime_dynamics.group_count(),
		vrm_spring_bone_group_count: runtime_dynamics.source_group_count(UnaDynamicsSourceKind::VrmSpringBone),
		vrc_physbone_group_count: runtime_dynamics.source_group_count(UnaDynamicsSourceKind::VrcPhysBone),
		unknown_group_count: runtime_dynamics.source_group_count(UnaDynamicsSourceKind::Unknown),
		collider_count: runtime_dynamics.collider_count(),
		vrm_spring_bone_collider_count: runtime_dynamics.source_collider_count(UnaDynamicsSourceKind::VrmSpringBone),
		vrc_physbone_collider_count: runtime_dynamics.source_collider_count(UnaDynamicsSourceKind::VrcPhysBone),
		unknown_collider_count: runtime_dynamics.source_collider_count(UnaDynamicsSourceKind::Unknown),
		source_limit_count: dynamics_source_features.limit_count,
		source_collision_disabled_count: dynamics_source_features.collision_disabled_count,
		source_inside_bounds_collider_count: dynamics_source_features.inside_bounds_collider_count,
		source_grabbing_enabled_count: dynamics_source_features.grabbing_enabled_count,
		source_posing_enabled_count: dynamics_source_features.posing_enabled_count,
		groups: dynamics_groups,
	};
	let vrm = doc.vrm.as_ref().map(|vrm| DiagnoseVrmSummary {
		spec_version: vrm.spec_version.clone(),
		mtoon_materials_v0: vrm.mtoon_materials_v0.len(),
		mtoon_material_indices_v1: vrm.mtoon_material_indices_v1.clone(),
		spring_group_count: dynamics.vrm_spring_bone_group_count,
	});
	let unavatar = doc.unavatar.as_ref().map(unavatar_summary);
	if let Some(unavatar) = &unavatar {
		if unavatar.dynamics_entry_count > 0 && dynamics.group_count == 0 {
			warnings.push(format!(
				".unavatar has {} raw dynamics entries but no runtime dynamics groups; check dynamics root node references and importer lowering",
				unavatar.dynamics_entry_count
			));
		}
	}

	DiagnoseReport {
		path: path.to_string_lossy().to_string(),
		import_format_id,
		import_provider_plugin_id: provider_plugin_id,
		timings,
		import_report,
		runtime,
		scene,
		humanoid,
		expressions,
		dynamics,
		vrm,
		unavatar,
		wardrobe_probes,
		warnings,
	}
}

fn expression_apply_probe(doc: &UnaDocument) -> Option<DiagnoseExpressionApplyProbe> {
	let mut doc = doc.clone();
	doc.expression_catalog.as_ref()?;
	let mut frame = un_motion_frame::UNMotionFrame::new(0);
	frame.face = Some(un_motion_frame::FaceMotion {
		tracking_state: un_motion_frame::TrackingState::Valid,
		confidence: 1.0,
		head: None,
		expressions: vec![
			un_motion_frame::ExpressionSample {
				name: "jawOpen".to_string(),
				value: 0.6,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
			un_motion_frame::ExpressionSample {
				name: "mouthPucker".to_string(),
				value: 0.4,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
			un_motion_frame::ExpressionSample {
				name: "mouthSmileLeft".to_string(),
				value: 0.8,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
			un_motion_frame::ExpressionSample {
				name: "eyeBlinkLeft".to_string(),
				value: 0.7,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
			un_motion_frame::ExpressionSample {
				name: "browDownLeft".to_string(),
				value: 0.5,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			},
		],
	});
	un_avatar_skeleton::apply_un_motion_frame_to_document(&mut doc, &frame, un_avatar_skeleton::ApplyUnMotionFrameOpts::default());
	let weights: BTreeMap<String, f32> = doc
		.expression_weights
		.as_ref()?
		.preset_weights
		.iter()
		.filter_map(|(name, value)| if *value > 0.0001 { Some((name.clone(), *value)) } else { None })
		.collect();
	let mut active_morph_slots = Vec::new();
	if let (Some(scene), Some(catalog), Some(expression_weights)) = (&doc.scene, &doc.expression_catalog, &doc.expression_weights) {
		for (mesh_i, primitives) in scene.meshes.iter().enumerate() {
			for (prim_i, primitive) in primitives.iter().enumerate() {
				let morphs = morph_weights_for_primitive(primitive, Some(catalog), Some(expression_weights), mesh_i, prim_i);
				let active_count = morphs.iter().filter(|value| **value > 0.0001).count();
				if active_count > 0 {
					let max_weight = morphs.iter().copied().fold(0.0f32, f32::max);
					active_morph_slots.push(DiagnoseExpressionMorphSlot {
						mesh: mesh_i,
						primitive: prim_i,
						active_count,
						max_weight,
					});
				}
			}
		}
	}
	Some(DiagnoseExpressionApplyProbe {
		weights,
		active_morph_slots,
	})
}

fn run_diagnose(
	plugin_dirs: &[PathBuf],
	path: PathBuf,
	input_format: Option<String>,
	wardrobe_set: Option<String>,
	wardrobe_probe_all: bool,
	visible_materials_only: bool,
	visible_meshes: bool,
	json: bool,
) -> Result<(), String> {
	let reg = io_registry_for_cli(plugin_dirs)?;
	let importer: &dyn AvatarImporter = if let Some(ref s) = input_format {
		let id = FormatId::new(s.as_str());
		reg.importer_by_id(&id)
			.ok_or_else(|| format!("指定の importer が登録されていません: {s}"))?
	} else {
		let probe = import_probe_for_path(&path, cached_binary_import_bytes(&path));
		reg.best_importer_for(&probe)
			.ok_or_else(|| "入力に合う importer が見つかりません".to_string())?
	};
	let desc = importer.descriptor();
	let mut ictx = ImportContext {
		asset_root: path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
	};
	let import_started = Instant::now();
	let mut imported = importer
		.import(
			&mut ictx,
			import_input_for_path(&path, &desc.id, cached_binary_import_bytes(&path)),
			ImportOptions,
		)
		.map_err(|e| e.to_string())?;
	let import_ms = import_started.elapsed().as_millis();
	let base_document_for_probes = imported.document.clone();
	let mut wardrobe_apply_ms = 0;
	if let Some(set_id) = wardrobe_set.as_deref().filter(|set_id| !set_id.trim().is_empty()) {
		let started = Instant::now();
		let applied = apply_unavatar_wardrobe_set(&mut imported.document, set_id)?;
		wardrobe_apply_ms = started.elapsed().as_millis();
		imported.report.push_info(format!(
			".unavatar wardrobe set `{set_id}`: visibility_applied={}, visibility_missing={}, blendshape_applied={}, blendshape_missing={}",
			applied.visibility_applied, applied.visibility_missing, applied.blendshape_applied, applied.blendshape_missing
		));
	}
	let wardrobe_probe_started = Instant::now();
	let wardrobe_probes = if wardrobe_probe_all {
		build_wardrobe_probes(&base_document_for_probes)?
	} else {
		Vec::new()
	};
	let wardrobe_probe_ms = wardrobe_probe_started.elapsed().as_millis();
	let report_build_started = Instant::now();
	let report = build_diagnose_report(
		&path,
		desc.id.0,
		desc.provider_plugin_id,
		DiagnoseTimingSummary {
			import_ms,
			wardrobe_apply_ms,
			wardrobe_probe_ms,
			report_build_ms: 0,
		},
		imported.report,
		imported.document,
		wardrobe_probes,
	);
	let report_build_ms = report_build_started.elapsed().as_millis();
	let report = DiagnoseReport {
		timings: DiagnoseTimingSummary {
			report_build_ms,
			..report.timings
		},
		..report
	};
	if json {
		write_json_stdout(&report)?;
		return Ok(());
	}
	println!("path: {}", report.path);
	println!("importer: {}", report.import_format_id);
	println!(
		"timings: import={}ms wardrobe_apply={}ms wardrobe_probe={}ms report_build={}ms",
		report.timings.import_ms, report.timings.wardrobe_apply_ms, report.timings.wardrobe_probe_ms, report.timings.report_build_ms
	);
	println!(
		"runtime: source={:?} humanoid_basis={:?}",
		report.runtime.source_kind, report.runtime.humanoid_basis
	);
	if let Some(vrm) = &report.vrm {
		println!(
			"vrm: spec={} mtoon_v0={} mtoon_v1={:?} spring_groups={}",
			vrm.spec_version, vrm.mtoon_materials_v0, vrm.mtoon_material_indices_v1, vrm.spring_group_count
		);
	} else {
		println!("vrm: none");
	}
	println!(
		"dynamics: groups={} vrm_spring={} vrc_physbone={} unknown={} colliders={} collider_vrm_spring={} collider_vrc_physbone={} collider_unknown={} source_limits={} source_collision_disabled={} source_inside_bounds_colliders={} source_grabbing={} source_posing={}",
		report.dynamics.group_count,
		report.dynamics.vrm_spring_bone_group_count,
		report.dynamics.vrc_physbone_group_count,
		report.dynamics.unknown_group_count,
		report.dynamics.collider_count,
		report.dynamics.vrm_spring_bone_collider_count,
		report.dynamics.vrc_physbone_collider_count,
		report.dynamics.unknown_collider_count,
		report.dynamics.source_limit_count,
		report.dynamics.source_collision_disabled_count,
		report.dynamics.source_inside_bounds_collider_count,
		report.dynamics.source_grabbing_enabled_count,
		report.dynamics.source_posing_enabled_count
	);
	for group in report.dynamics.groups.iter().take(16) {
		println!(
			"  dynamics_group[{}]: source={:?} bones={} root={:?} tip={:?} stiffness={} drag={} gravity={} radius={} comment={:?}",
			group.index,
			group.source_kind,
			group.bone_count,
			group.root_path.as_deref().or(group.root_node.map(|_| "#")),
			group.tip_path.as_deref().or(group.tip_node.map(|_| "#")),
			group.stiffness,
			group.drag_force,
			group.gravity_power,
			group.hit_radius,
			group.comment
		);
	}
	if let Some(unavatar) = &report.unavatar {
		println!(
			"unavatar: spec={} generator={:?} name={:?} source={:?} raw_dynamics={}",
			unavatar.spec_version, unavatar.generator, unavatar.manifest_name, unavatar.source_type, unavatar.dynamics_entry_count
		);
		println!(
			"wardrobe: base={:?} sets={} {:?} base_ops={} {:?} extension_nodes={} variants={}",
			unavatar.base_set,
			unavatar.wardrobe_set_count,
			unavatar.wardrobe_set_ids,
			unavatar.base_operation_count,
			unavatar.base_operation_counts,
			unavatar.extension_node_count,
			unavatar.variant_count
		);
		for set in &unavatar.wardrobe_sets {
			println!(
				"wardrobe_set[{}]: name={:?} source={:?} ops={} {:?} groups={:?}",
				set.id, set.display_name, set.source, set.operation_count, set.operation_counts, set.asset_groups
			);
		}
		for probe in &report.wardrobe_probes {
			println!(
				"wardrobe_probe[{}]: name={:?} probe={}ms visible_meshes={} nonzero_morphs={} apply=vis {:?}/{:?} blend {:?}/{:?} missing=vis {} blend {}",
				probe.set_id,
				probe.display_name,
				probe.probe_ms,
				probe.visible_mesh_node_count,
				probe.nonzero_morph_weight_count,
				probe.visibility_applied,
				probe.visibility_missing,
				probe.blendshape_applied,
				probe.blendshape_missing,
				probe.missing_visibility_paths.len(),
				probe.missing_blendshapes.len()
			);
			for path in probe.visible_mesh_paths.iter().take(24) {
				println!("  visible: {path}");
			}
			if probe.visible_mesh_paths.len() > 24 {
				println!("  visible: ... {} more", probe.visible_mesh_paths.len() - 24);
			}
			for morph in probe.nonzero_morph_weights.iter().take(12) {
				println!(
					"  morph: mesh={} primitive={} index={} name={:?} weight={}",
					morph.mesh, morph.primitive, morph.index, morph.name, morph.weight
				);
			}
			if probe.nonzero_morph_weights.len() > 12 {
				println!("  morph: ... {} more", probe.nonzero_morph_weights.len() - 12);
			}
		}
	} else {
		println!("unavatar: none");
	}
	println!(
		"scene: meshes={} primitives={} morph_targets={} nodes={} hidden_nodes={} skins={} images={} materials={}",
		report.scene.mesh_count,
		report.scene.primitive_count,
		report.scene.morph_target_count,
		report.scene.node_count,
		report.scene.hidden_node_count,
		report.scene.skin_count,
		report.scene.image_count,
		report.scene.material_count
	);
	for skin in &report.scene.skins {
		println!(
			"skin[{}]: joints={} inverse_binds={} effective={} over_renderer_limit={} skeleton={:?} used_nodes={} prim_joints={} prim_weights={} mismatched_attrs={} max_joint={:?} out_of_range_prim_joints={}",
			skin.index,
			skin.joint_count,
			skin.inverse_bind_count,
			skin.effective_joint_count,
			skin.over_renderer_bone_limit,
			skin.skeleton_node,
			skin.used_by_node_count,
			skin.primitive_joint_attribute_count,
			skin.primitive_weight_attribute_count,
			skin.mismatched_joint_weight_attribute_count,
			skin.max_joint_index,
			skin.out_of_range_joint_attribute_count
		);
	}
	println!("node_constraints: {}", report.scene.node_constraint_count);
	println!(
		"image_sources: {} / {} images, {} bytes, MIME {:?}",
		report.scene.image_source_count, report.scene.image_count, report.scene.image_source_bytes, report.scene.image_source_mime_counts
	);
	println!(
		"image_source_metadata: color_space {:?}, texture_type {:?}, texture_shape {:?}, source_layout {:?}",
		report.scene.image_source_color_space_counts,
		report.scene.image_source_texture_type_counts,
		report.scene.image_source_texture_shape_counts,
		report.scene.image_source_layout_counts
	);
	println!(
		"image_pixel_formats: {:?}, non_rgba8={}",
		report.scene.image_pixel_format_counts, report.scene.non_rgba8_image_count
	);
	if !report.scene.largest_image_sources.is_empty() {
		println!("largest_image_sources:");
		for source in &report.scene.largest_image_sources {
			println!(
				"  image[{}]: {}x{} {:?} {} bytes mime={:?} source_format={:?} channels={:?} color_space={:?} texture_type={:?} texture_shape={:?} source_layout={:?} unity_generate_cubemap={:?} srgb={:?} name={:?} uri={:?}",
				source.index,
				source.width,
				source.height,
				source.pixel_format,
				source.byte_length,
				source.mime_type,
				source.source_pixel_format,
				source.channels,
				source.color_space,
				source.texture_type,
				source.texture_shape,
				source.source_layout,
				source.unity_generate_cubemap,
				source.srgb,
				source.name,
				source.uri
			);
		}
	}
	println!("shading: {:?}", report.scene.shading_counts);
	println!("alpha: {:?}", report.scene.alpha_counts);
	println!("liltoon_features: {:?}", report.scene.liltoon_feature_counts);
	println!("visible_shading: {:?}", report.scene.visible_shading_counts);
	println!("visible_alpha: {:?}", report.scene.visible_alpha_counts);
	println!("visible_materials: {:?}", report.scene.visible_material_indices);
	let visible_material_indices: BTreeSet<usize> = report.scene.visible_material_indices.iter().copied().collect();
	if visible_materials_only {
		println!("materials: visible only ({} unique indices)", visible_material_indices.len());
	}
	if let Some(h) = &report.humanoid {
		println!(
			"humanoid: bones={} left_eye={:?} right_eye={:?}",
			h.bone_count, h.left_eye_node, h.right_eye_node
		);
	} else {
		println!("humanoid: none");
	}
	if let Some(e) = &report.expressions {
		println!("expressions: presets={}", e.preset_count);
	} else {
		println!("expressions: none");
	}
	if visible_meshes {
		println!("visible_mesh_nodes:");
		for node in &report.scene.visible_mesh_nodes {
			println!(
				"  node[{}]: mesh={} skin={:?} path={:?} name={:?}",
				node.node, node.mesh, node.skin, node.path, node.name
			);
			for material in &node.materials {
				println!(
					"    prim[{}]: material[{}] name={:?} source={:?} shading={:?} alpha={:?} cutoff={} zwrite={} skipped={} morph_targets={} nonzero_morphs={}",
					material.primitive,
					material.index,
					material.name,
					material.source_shader,
					material.shading,
					material.alpha_mode,
					material.alpha_cutoff,
					material.transparent_with_z_write,
					material.draw_skipped_fully_transparent,
					material.morph_target_count,
					material.nonzero_morph_weights.len()
				);
				for morph in material.nonzero_morph_weights.iter().take(8) {
					println!(
						"      morph[{}]: name={:?} weight={} pos_delta_sum={} nrm_delta_sum={}",
						morph.index, morph.name, morph.weight, morph.position_delta_abs_sum, morph.normal_delta_abs_sum
					);
				}
				if material.nonzero_morph_weights.len() > 8 {
					println!("      morph: ... {} more", material.nonzero_morph_weights.len() - 8);
				}
			}
		}
	}
	for material in report
		.scene
		.materials
		.iter()
		.filter(|material| !visible_materials_only || visible_material_indices.contains(&material.index))
	{
		println!(
			"material[{}]: name={:?} source={:?}/{:?} rq={:?} source_params=float:{} color:{} shading={:?} alpha={:?} cutoff={} cull={:?} double_sided={} tex={:?} normal={:?}/{} eye_like={}",
			material.index,
			material.name,
			material.material_family,
			material.source_shader,
			material.render_queue,
			material.source_float_param_count,
			material.source_color_param_count,
			material.shading,
			material.alpha_mode,
			material.alpha_cutoff,
			material.cull_mode,
			material.double_sided,
			material.base_color_texture_index,
			material.normal_texture_index,
			material.normal_texture_scale,
			material.eye_like_name
		);
		if let Some(mtoon) = &material.mtoon {
			println!(
				"  mtoon: zwrite={} shade={:?} shade_tex={:?} shift={} toony={} rim={:?} matcap_tex={:?} reflection_tex={:?} outline={:?}/{} emissive={:?}",
				mtoon.transparent_with_z_write,
				mtoon.shade_color_factor,
				mtoon.shade_multiply_texture_index,
				mtoon.shading_shift_factor,
				mtoon.shading_toony_factor,
				mtoon.parametric_rim_color_factor,
				mtoon.matcap_texture_index,
				mtoon.reflection_cube_texture_index,
				mtoon.outline_width_mode,
				mtoon.outline_width_factor,
				mtoon.emissive_factor
			);
		}
		if !material.source_render_float_params.is_empty() {
			println!("  liltoon_render_state: {:?}", material.source_render_float_params);
		}
	}
	for warning in &report.warnings {
		println!("warning: {warning}");
	}
	Ok(())
}

fn run_vmc(command: VmcCommands) -> Result<(), String> {
	use std::net::SocketAddr;

	match command {
		VmcCommands::Listen { port, frame } => {
			let addr = SocketAddr::from(([0, 0, 0, 0], port));
			let mut marionette = un_avatar_vmc::VmcMarionette::bind(addr).map_err(|e| format!("bind {addr}: {e}"))?;
			eprintln!("un-avatar vmc listen: UDP {addr} (Ctrl+Cで終了)");
			let mut seq = 0u64;
			loop {
				match marionette.recv_and_apply() {
					Ok((_from, _n, events)) => {
						seq = seq.wrapping_add(1);
						if frame {
							let line = serde_json::to_string(&marionette.assemble_frame(seq, un_avatar_vmc::wall_clock_ns()))
								.map_err(|e| e.to_string())?;
							println!("{line}");
						} else {
							for ev in events {
								let line = serde_json::to_string(&ev).map_err(|e| e.to_string())?;
								println!("{line}");
							}
						}
					}
					Err(un_avatar_vmc::RecvApplyError::Io(e)) => return Err(format!("recv: {e}")),
					Err(un_avatar_vmc::RecvApplyError::Decode {
						from,
						nbytes,
						err,
						payload_head_hex,
					}) => {
						eprintln!("un-avatar vmc listen: decode from {from} nbytes={nbytes}: {err}; hex_head={payload_head_hex}");
					}
				}
			}
		}
	}
}

fn run_convert(
	plugin_dirs: &[PathBuf],
	input: PathBuf,
	output: PathBuf,
	input_format: Option<String>,
	output_format: Option<String>,
	json_report: Option<PathBuf>,
) -> Result<(), String> {
	let reg = io_registry_for_cli(plugin_dirs)?;
	let cached_bytes = cached_binary_import_bytes(&input);
	let probe = import_probe_for_path(&input, cached_bytes.clone());
	let importer: &dyn AvatarImporter = if let Some(ref s) = input_format {
		let id = FormatId::new(s.as_str());
		reg.importer_by_id(&id)
			.ok_or_else(|| format!("指定の importer が登録されていません: {s}"))?
	} else {
		reg.best_importer_for(&probe).ok_or_else(|| {
			"入力に合う importer が見つかりません（`.una` または `manifest.toml` 付き `.una.d` を指定、`--plugin-dir`、または --input-format）".to_string()
		})?
	};
	let mut ictx = ImportContext {
		asset_root: input.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
	};
	let import_desc = importer.descriptor();
	let imported = importer
		.import(
			&mut ictx,
			import_input_for_path(&input, &import_desc.id, cached_bytes),
			ImportOptions,
		)
		.map_err(|e| e.to_string())?;
	let exporter: &dyn AvatarExporter = if let Some(ref s) = output_format {
		let id = FormatId::new(s.as_str());
		let exp = reg
			.exporter_by_id(&id)
			.ok_or_else(|| format!("指定の exporter が登録されていません: {s}"))?;
		if exp.can_export(&imported.document, &ExportOptions) != ExportCapability::Supported {
			return Err(format!("exporter {s} はこのドキュメントを書き出せません"));
		}
		exp
	} else {
		reg.best_exporter_for(&imported.document, &output).ok_or_else(|| {
			"出力に使える exporter が見つかりません（`.una` または `.una.d` のパスを指定、または --output-format）".to_string()
		})?
	};
	let export_desc = exporter.descriptor();
	let mut ectx = ExportContext {
		output_root: output.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")),
		temp_dir: std::env::temp_dir(),
	};
	let export_result = exporter
		.export(&mut ectx, &imported.document, ExportOutput::Path(output), ExportOptions)
		.map_err(|e| e.to_string())?;
	if let Some(ref path) = json_report {
		let bundle = ConvertJsonReport {
			import_format_id: import_desc.id.0.clone(),
			export_format_id: export_desc.id.0.clone(),
			import_provider_plugin_id: import_desc.provider_plugin_id.clone(),
			export_provider_plugin_id: export_desc.provider_plugin_id.clone(),
			import_report: imported.report,
			export_report: export_result.report,
		};
		write_convert_json_report(path, &bundle)?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	use std::fs;

	use un_avatar_io_una::write_una_path;

	#[test]
	fn parse_plugin_path_trims_and_skips_empty() {
		let raw = if cfg!(windows) {
			OsStr::new(" a ; ;b ")
		} else {
			OsStr::new(" a : :b ")
		};
		let v = parse_plugin_path_list(raw);
		assert_eq!(v, vec![PathBuf::from("a"), PathBuf::from("b")]);
	}

	#[test]
	fn merge_unique_plugin_dirs_preserves_order_and_dedups() {
		let a = PathBuf::from("/x/a");
		let b = PathBuf::from("/x/b");
		let merged = merge_unique_plugin_dirs(vec![a.clone(), b.clone()], &[a.clone(), PathBuf::from("/x/c")]);
		assert_eq!(merged, vec![a, b, PathBuf::from("/x/c")]);
	}

	#[test]
	fn normalize_cli_args_treats_path_as_diagnose_shorthand() {
		let args = ["un-avatar", "target/tmp/model.vrm", "--json"].map(OsString::from);
		let normalized = normalize_cli_args(args);
		assert_eq!(
			normalized,
			vec![
				OsString::from("un-avatar"),
				OsString::from("diagnose"),
				OsString::from("target/tmp/model.vrm"),
				OsString::from("--json"),
			]
		);
	}

	#[test]
	fn normalize_cli_args_preserves_explicit_commands_and_global_plugin_dir() {
		let args = ["un-avatar", "--plugin-dir", "plugins/sample-io-plugin", "formats", "list"].map(OsString::from);
		let normalized = normalize_cli_args(args.clone());
		assert_eq!(normalized, args);
	}

	#[test]
	fn io_registry_for_cli_empty_is_una_vrm_and_gltf() {
		let reg = io_registry_for_cli(&[]).unwrap();
		assert_eq!(reg.importer_descriptors().len(), 3);
		assert_eq!(reg.exporter_descriptors().len(), 1);
	}

	#[test]
	fn validate_import_pipeline_accepts_default_una_file() {
		let dir = std::env::temp_dir().join(format!("ua-cli-val-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let path = dir.join("t.una");
		write_una_path(&path, &UnaFileV0::default()).unwrap();
		let reg = io_registry_with_una();
		let probe = ImportProbe {
			path_hint: Some(path.clone()),
			bytes: None,
		};
		let imp = reg.best_importer_for(&probe).expect("importer");
		let mut ctx = ImportContext {
			asset_root: dir.clone(),
			temp_dir: std::env::temp_dir(),
		};
		imp.import(&mut ctx, ImportInput::Path(path), ImportOptions).unwrap();
		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn inspect_reads_una_summary_fields() {
		let dir = std::env::temp_dir().join(format!("ua-cli-inspect-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let path = dir.join("x.una");
		write_una_path(&path, &UnaFileV0::default()).unwrap();
		let f = read_una_any(&path).unwrap();
		assert_eq!(f.format_version, un_avatar_io_una::UNA_FORMAT_VERSION_V0);
		assert!(f.scene.empty);
		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn diagnose_report_summarizes_materials_and_vrm_hints() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				materials: vec![
					un_avatar_core::UnaMaterialPbr {
						name: Some("Eye_Iris".into()),
						shading: UnaShadingModel::MToonLike,
						alpha_mode: UnaAlphaMode::Mask,
						..Default::default()
					},
					un_avatar_core::UnaMaterialPbr {
						name: Some("Body".into()),
						shading: UnaShadingModel::MToonLike,
						alpha_mode: UnaAlphaMode::Opaque,
						..Default::default()
					},
				],
				..Default::default()
			}),
			vrm: Some(un_avatar_core::UnaVrmExtension {
				spec_version: "1.0".into(),
				meta: serde_json::Value::Null,
				humanoid_bones: BTreeMap::new(),
				mtoon_materials_v0: Vec::new(),
				mtoon_material_indices_v1: vec![0, 1],
				source: serde_json::Value::Null,
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.vrm"),
			"io.un-avatar.vrm".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		assert_eq!(report.scene.material_count, 2);
		assert_eq!(report.scene.node_constraint_count, 0);
		assert_eq!(report.scene.shading_counts.get("MToonLike"), Some(&2));
		assert!(report.scene.visible_shading_counts.is_empty());
		assert!(report.scene.visible_alpha_counts.is_empty());
		assert!(report.scene.visible_material_indices.is_empty());
		assert_eq!(report.scene.eye_like_material_indices, vec![0]);
		assert_eq!(report.vrm.as_ref().unwrap().mtoon_material_indices_v1, vec![0, 1]);
		assert!(!report.warnings.iter().any(|w| w.contains("eye-like material[0]")));
	}

	#[test]
	fn diagnose_report_warns_about_skinning_palette_risks() {
		fn identity_transform() -> [f32; 16] {
			[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
		}
		fn primitive(joints: Option<Vec<[u16; 4]>>, weights: Option<Vec<[f32; 4]>>) -> un_avatar_core::UnaMeshBuffers {
			un_avatar_core::UnaMeshBuffers {
				name: None,
				positions: vec![[0.0, 0.0, 0.0]],
				normals: None,
				tangents: None,
				tex_coords_0: None,
				tex_coords_1: None,
				tex_coords_2: None,
				tex_coords_3: None,
				colors_0: None,
				joints,
				weights,
				indices: None,
				material_index: None,
				morph_targets: Vec::new(),
				morph_target_names: Vec::new(),
				default_morph_weights: Vec::new(),
			}
		}

		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot {
				nodes: vec![un_avatar_core::UnaSceneNode {
					name: None,
					source_node_id: None,
					visible: true,
					transform: identity_transform(),
					children: Vec::new(),
					mesh: Some(0),
					skin: Some(0),
					probe_anchor_node: None,
					local_bounds: None,
				}],
				meshes: vec![vec![
					primitive(Some(vec![[600, 0, 0, 0]]), Some(vec![[1.0, 0.0, 0.0, 0.0]])),
					primitive(Some(vec![[0, 0, 0, 0]]), None),
				]],
				skins: vec![un_avatar_core::UnaSkin {
					joint_nodes: (0..513).collect(),
					inverse_bind_matrices: vec![[0.0; 16]; 513],
					skeleton_node: Some(0),
				}],
				..Default::default()
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		let skin = &report.scene.skins[0];
		assert_eq!(skin.effective_joint_count, 513);
		assert!(skin.over_renderer_bone_limit);
		assert_eq!(skin.max_joint_index, Some(600));
		assert_eq!(skin.mismatched_joint_weight_attribute_count, 1);
		assert_eq!(skin.out_of_range_joint_attribute_count, 1);
		assert!(report.warnings.iter().any(|w| w.contains("renderer bone palette limit")));
		assert!(report.warnings.iter().any(|w| w.contains("mismatched JOINTS/WEIGHTS")));
		assert!(report.warnings.iter().any(|w| w.contains("outside effective palette")));
	}

	#[test]
	fn diagnose_report_warns_when_unavatar_dynamics_do_not_lower() {
		let doc = UnaDocument {
			scene: Some(un_avatar_core::UnaSceneSnapshot::default()),
			unavatar: Some(un_avatar_core::UnaUnavatarExtension {
				spec_version: "0.1".into(),
				source: serde_json::json!({
					"dynamics": [
						{
							"source": "vrc_physbone",
							"roots": [999],
							"sourceParams": {
								"limitType": "Angle",
								"maxAngleX": 45.0,
								"allowCollision": false,
								"allowGrabbing": true,
								"allowPosing": true,
								"colliders": [
									{"insideBounds": true}
								]
							}
						}
					]
				}),
			}),
			..Default::default()
		};

		let report = build_diagnose_report(
			Path::new("avatar.unavatar"),
			"io.un-avatar.gltf".into(),
			None,
			DiagnoseTimingSummary {
				import_ms: 0,
				wardrobe_apply_ms: 0,
				wardrobe_probe_ms: 0,
				report_build_ms: 0,
			},
			ImportReport::default(),
			doc,
			Vec::new(),
		);

		assert_eq!(report.unavatar.as_ref().unwrap().dynamics_entry_count, 1);
		assert_eq!(report.dynamics.group_count, 0);
		assert_eq!(report.dynamics.source_limit_count, 1);
		assert_eq!(report.dynamics.source_collision_disabled_count, 1);
		assert_eq!(report.dynamics.source_inside_bounds_collider_count, 1);
		assert_eq!(report.dynamics.source_grabbing_enabled_count, 1);
		assert_eq!(report.dynamics.source_posing_enabled_count, 1);
		assert!(report
			.warnings
			.iter()
			.any(|w| w.contains("raw dynamics entries but no runtime dynamics groups")));
	}

	#[test]
	fn validate_import_works_with_explicit_format_on_path_without_una_suffix() {
		let dir = std::env::temp_dir().join(format!("ua-cli-val-fmt-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).unwrap();
		let path = dir.join("blob");
		write_una_path(&path, &UnaFileV0::default()).unwrap();
		let reg = io_registry_with_una();
		let imp = reg.importer_by_id(&FormatId::new("io.un-avatar.una")).expect("una importer");
		let mut ctx = ImportContext {
			asset_root: dir.clone(),
			temp_dir: std::env::temp_dir(),
		};
		imp.import(&mut ctx, ImportInput::Path(path), ImportOptions).unwrap();
		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn best_exporter_matches_una_d_suffix() {
		let reg = io_registry_with_una();
		let doc = un_avatar_io::UnaDocument::default();
		let out = PathBuf::from("avatar.una.d");
		let e = reg.best_exporter_for(&doc, &out).expect("exporter");
		assert_eq!(e.descriptor().id.0, "io.un-avatar.una");
	}

	#[test]
	fn convert_json_report_serializes() {
		let mut import_report = ImportReport::default();
		import_report.push_info("import line");
		let mut export_report = ExportReport::default();
		export_report.push_info("export line");
		let bundle = ConvertJsonReport {
			import_format_id: "io.un-avatar.una".into(),
			export_format_id: "io.un-avatar.una".into(),
			import_provider_plugin_id: None,
			export_provider_plugin_id: None,
			import_report,
			export_report,
		};
		let v = serde_json::to_value(&bundle).unwrap();
		assert!(v.get("import_report").is_some());
		assert!(v.get("export_report").is_some());
		assert_eq!(v["import_format_id"], "io.un-avatar.una");
		assert!(v.get("import_provider_plugin_id").is_none());
		assert!(v.get("export_provider_plugin_id").is_none());
		assert!(v["import_report"]["diagnostics"].is_array());
		assert_eq!(v["import_report"]["diagnostics"][0]["severity"], "info");
		assert!(v["export_report"]["diagnostics"].is_array());
	}

	#[test]
	fn convert_json_report_includes_provider_ids_when_set() {
		let bundle = ConvertJsonReport {
			import_format_id: "io.un-avatar.example.avatar".into(),
			export_format_id: "io.un-avatar.una".into(),
			import_provider_plugin_id: Some("network.usagi.un_avatar.plugin.sample_io".into()),
			export_provider_plugin_id: None,
			import_report: ImportReport::default(),
			export_report: ExportReport::default(),
		};
		let v = serde_json::to_value(&bundle).unwrap();
		assert_eq!(v["import_provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
		assert!(v.get("export_provider_plugin_id").is_none());
	}

	#[test]
	fn validate_report_json_skips_provider_when_none() {
		let r = ValidateReport {
			valid: true,
			path: "p".into(),
			error: None,
			format_id: Some("io.un-avatar.una".into()),
			provider_plugin_id: None,
		};
		let v = serde_json::to_value(&r).unwrap();
		assert!(v.get("provider_plugin_id").is_none());
		assert_eq!(v["format_id"], "io.un-avatar.una");
	}

	#[test]
	fn validate_report_json_includes_provider_when_set() {
		let r = ValidateReport {
			valid: true,
			path: "p".into(),
			error: None,
			format_id: Some("io.un-avatar.example.avatar".into()),
			provider_plugin_id: Some("network.usagi.un_avatar.plugin.sample_io".into()),
		};
		let v = serde_json::to_value(&r).unwrap();
		assert_eq!(v["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
	}

	#[test]
	fn formats_list_json_sets_provider_plugin_id_for_stdio_importer() {
		let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin");
		let reg = io_registry_for_cli(&[plugins]).unwrap();
		let out = FormatsListJson {
			importers: reg.importer_descriptors(),
			exporters: reg.exporter_descriptors(),
		};
		let v = serde_json::to_value(&out).unwrap();
		let imp = v["importers"]
			.as_array()
			.unwrap()
			.iter()
			.find(|x| x["id"] == "io.un-avatar.example.avatar")
			.expect("sample importer row");
		assert_eq!(imp["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
		let exp = v["exporters"]
			.as_array()
			.unwrap()
			.iter()
			.find(|x| x["id"] == "io.un-avatar.example.avatar")
			.expect("sample exporter row");
		assert_eq!(exp["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
		let una = v["importers"]
			.as_array()
			.unwrap()
			.iter()
			.find(|x| x["id"] == "io.un-avatar.una")
			.expect("una");
		assert!(una.get("provider_plugin_id").is_none());
	}

	#[test]
	fn formats_list_json_contains_una() {
		let reg = io_registry_with_una();
		let out = FormatsListJson {
			importers: reg.importer_descriptors(),
			exporters: reg.exporter_descriptors(),
		};
		let v = serde_json::to_value(&out).unwrap();
		assert!(!v["importers"].as_array().unwrap().is_empty());
		assert_eq!(v["importers"][0]["id"], "io.un-avatar.una");
	}

	#[test]
	fn formats_probe_json_has_positive_confidence_for_una_path() {
		let reg = io_registry_with_una();
		let v = serde_json::to_value(super::build_formats_probe_json(&reg, std::path::Path::new("model.una"))).unwrap();
		let arr = v["importers"].as_array().expect("array");
		let row = arr.iter().find(|x| x["format_id"] == "io.un-avatar.una").expect("una row");
		assert!(row["confidence"].as_u64().unwrap() > 0);
		assert!(row.get("provider_plugin_id").is_none());
		assert_eq!(v["best_importer"], "io.un-avatar.una");
		assert!(v.get("best_importer_provider_plugin_id").is_none());

		let ex = v["exporters"].as_array().expect("exporters");
		let erow = ex.iter().find(|x| x["format_id"] == "io.un-avatar.una").expect("una exporter row");
		assert_eq!(erow["confidence"], 120);
		assert!(erow.get("provider_plugin_id").is_none());
		assert_eq!(v["best_exporter"], "io.un-avatar.una");
		assert!(v.get("best_exporter_provider_plugin_id").is_none());
	}

	#[test]
	fn formats_probe_json_includes_provider_for_sample_plugin_path() {
		let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin");
		let reg = io_registry_for_cli(&[plugins]).unwrap();
		let v = serde_json::to_value(super::build_formats_probe_json(&reg, std::path::Path::new("x.exampleavatar"))).unwrap();
		let arr = v["importers"].as_array().expect("array");
		let row = arr
			.iter()
			.find(|x| x["format_id"] == "io.un-avatar.example.avatar")
			.expect("sample row");
		assert!(row["confidence"].as_u64().unwrap() > 0);
		assert_eq!(row["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
		assert_eq!(v["best_importer"], "io.un-avatar.example.avatar");
		assert_eq!(v["best_importer_provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");

		let ex = v["exporters"].as_array().expect("exporters");
		let erow = ex
			.iter()
			.find(|x| x["format_id"] == "io.un-avatar.example.avatar")
			.expect("sample exporter row");
		assert_eq!(erow["confidence"], 120);
		assert_eq!(erow["provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
		assert_eq!(v["best_exporter"], "io.un-avatar.example.avatar");
		assert_eq!(v["best_exporter_provider_plugin_id"], "network.usagi.un_avatar.plugin.sample_io");
	}

	#[test]
	fn io_una_registry_resolves_importer_exporter_by_id() {
		let reg = io_registry_with_una();
		let id = FormatId::new("io.un-avatar.una");
		assert!(reg.importer_by_id(&id).is_some());
		assert!(reg.exporter_by_id(&id).is_some());
	}
}
