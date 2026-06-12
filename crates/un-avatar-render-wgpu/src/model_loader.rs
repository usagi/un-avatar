use std::{
	io::{Read, Seek, SeekFrom},
	path::Path,
	sync::Arc,
	time::Instant,
};

use un_avatar_core::{ImportReport, ReportSeverity, UnaDocument};
use un_avatar_io::{AvatarImporter, ImportContext, ImportInput, ImportOptions};
use un_avatar_io_gltf::{apply_unavatar_wardrobe_set, GltfImporter, WardrobeApplyReport};
use un_avatar_io_vrm::{gltf_root_json_from_bytes, has_vrm_extension, import_vrm_bytes, import_vrm_bytes_profiled};

const GLB_MAGIC: u32 = 0x46546C67;
const GLB_VERSION_2: u32 = 2;
const JSON_CHUNK_TYPE: u32 = 0x4E4F534A;

fn read_le_u32(bytes: &[u8; 4]) -> u32 {
	u32::from_le_bytes(*bytes)
}

fn glb_root_json_from_path(path: &Path) -> Option<serde_json::Value> {
	let mut file = std::fs::File::open(path).ok()?;
	let mut header = [0u8; 12];
	file.read_exact(&mut header).ok()?;
	if read_le_u32(header[0..4].try_into().ok()?) != GLB_MAGIC || read_le_u32(header[4..8].try_into().ok()?) != GLB_VERSION_2 {
		return None;
	}
	loop {
		let mut chunk_header = [0u8; 8];
		if file.read_exact(&mut chunk_header).is_err() {
			return None;
		}
		let length = read_le_u32(chunk_header[0..4].try_into().ok()?) as usize;
		let chunk_type = read_le_u32(chunk_header[4..8].try_into().ok()?);
		if chunk_type == JSON_CHUNK_TYPE {
			let mut json = vec![0u8; length];
			file.read_exact(&mut json).ok()?;
			let end = json.iter().position(|byte| *byte == 0).unwrap_or(json.len());
			return serde_json::from_slice(&json[..end]).ok();
		}
		file.seek(SeekFrom::Current(length as i64)).ok()?;
	}
}

pub(crate) fn normalize_wardrobe_set_id(wardrobe_set: Option<&str>) -> Option<&str> {
	wardrobe_set.map(str::trim).filter(|set_id| !set_id.is_empty())
}

pub(crate) fn require_wardrobe_set_id(wardrobe_set: &str) -> Result<&str, String> {
	normalize_wardrobe_set_id(Some(wardrobe_set)).ok_or_else(|| "wardrobe set id required".to_string())
}

fn wardrobe_apply_report_summary(set_id: &str, report: &WardrobeApplyReport) -> String {
	format!(
		"un-avatar-renderer: .unavatar wardrobe set `{set_id}` applied: visibility_applied={} visibility_missing={} blendshape_applied={} blendshape_missing={} dynamics_applied={} dynamics_missing={} material_applied={} material_missing={} material_slot_applied={} material_slot_missing={} active_asset_groups={:?} scoped_active_groups={} scoped_missing_groups={:?} scoped_resident=mesh:{} material:{} image:{} dynamics:{}",
		report.visibility_applied,
		report.visibility_missing,
		report.blendshape_applied,
		report.blendshape_missing,
		report.dynamics_applied,
		report.dynamics_missing,
		report.material_applied,
		report.material_missing,
		report.material_slot_applied,
		report.material_slot_missing,
		report.active_asset_groups,
		report.scoped_active_asset_group_count,
		report.scoped_missing_active_asset_groups,
		report.scoped_resident_mesh_primitive_count,
		report.scoped_resident_material_count,
		report.scoped_resident_image_count,
		report.scoped_resident_dynamics_count
	)
}

fn log_wardrobe_apply_report(set_id: &str, report: &WardrobeApplyReport) {
	eprintln!("{}", wardrobe_apply_report_summary(set_id, report));
	if !report.missing_visibility_paths.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing visibility paths: {:?}",
			report.missing_visibility_paths
		);
	}
	if !report.missing_blendshapes.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing blendshapes: {:?}",
			report.missing_blendshapes
		);
	}
	if !report.missing_dynamics_ids.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing dynamics ids: {:?}",
			report.missing_dynamics_ids
		);
	}
	if !report.missing_materials.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing material targets: {:?}",
			report.missing_materials
		);
	}
	if !report.missing_material_slots.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing material slot targets: {:?}",
			report.missing_material_slots
		);
	}
}

fn import_report_warning_lines(report: &ImportReport, sample_limit: usize) -> Vec<String> {
	let mut lines = Vec::new();
	for diagnostic in &report.diagnostics {
		if diagnostic.severity == ReportSeverity::Warning {
			lines.push(format!("import warning: {}", diagnostic.text));
		}
	}
	for lost in &report.lost_features {
		let detail = lost.detail.as_deref().unwrap_or("");
		if detail.is_empty() {
			lines.push(format!("import lost feature: {}", lost.feature));
		} else {
			lines.push(format!("import lost feature: {} ({detail})", lost.feature));
		}
	}
	for approximation in &report.approximations {
		let detail = approximation.detail.as_deref().unwrap_or("");
		if detail.is_empty() {
			lines.push(format!("import approximation: {}", approximation.feature));
		} else {
			lines.push(format!("import approximation: {} ({detail})", approximation.feature));
		}
	}
	lines.truncate(sample_limit);
	lines
}

fn log_import_report_warnings(report: &ImportReport) {
	let total = report
		.diagnostics
		.iter()
		.filter(|diagnostic| diagnostic.severity == ReportSeverity::Warning)
		.count()
		+ report.lost_features.len()
		+ report.approximations.len();
	if total == 0 {
		return;
	}
	eprintln!("un-avatar-renderer: model import reported {total} warning/approximation/lost-feature item(s); showing up to 8");
	for line in import_report_warning_lines(report, 8) {
		eprintln!("un-avatar-renderer: {line}");
	}
}

fn log_import_report_profile(report: &ImportReport) {
	for message in &report.messages {
		if message.starts_with("glTF import profile:")
			|| message.starts_with("glTF scene profile:")
			|| message.starts_with(".unavatar textureAssets:")
		{
			eprintln!("un-avatar-renderer: {message}");
		}
	}
}

pub(crate) fn apply_required_wardrobe_set(document: &mut UnaDocument, wardrobe_set: &str) -> Result<WardrobeApplyReport, String> {
	let set_id = require_wardrobe_set_id(wardrobe_set)?;
	let report =
		apply_unavatar_wardrobe_set(document, set_id).map_err(|e| format!(".unavatar wardrobe set `{set_id}` not applied: {e}"))?;
	log_wardrobe_apply_report(set_id, &report);
	Ok(report)
}

pub(crate) fn apply_requested_wardrobe_set(document: &mut UnaDocument, wardrobe_set: Option<&str>) -> Option<WardrobeApplyReport> {
	let set_id = normalize_wardrobe_set_id(wardrobe_set)?;
	match apply_required_wardrobe_set(document, set_id) {
		Ok(report) => Some(report),
		Err(e) => {
			eprintln!("un-avatar-renderer: {e}");
			None
		}
	}
}

pub(crate) fn load_document(path: &Path, wardrobe_set: Option<&str>, contact_parameter_emission: bool) -> Result<Arc<UnaDocument>, String> {
	load_document_inner(path, wardrobe_set, contact_parameter_emission, false)
}

pub(crate) fn load_document_profiled(
	path: &Path,
	wardrobe_set: Option<&str>,
	contact_parameter_emission: bool,
) -> Result<Arc<UnaDocument>, String> {
	load_document_inner(path, wardrobe_set, contact_parameter_emission, true)
}

fn log_import_profile_step(path: &Path, step: &str, started: Instant) {
	eprintln!(
		"un-avatar-renderer: model import profile path={} step={step} elapsed={:.1}ms",
		path.display(),
		started.elapsed().as_secs_f64() * 1000.0
	);
}

fn load_document_inner(
	path: &Path,
	wardrobe_set: Option<&str>,
	contact_parameter_emission: bool,
	profile: bool,
) -> Result<Arc<UnaDocument>, String> {
	let mut ctx = ImportContext {
		asset_root: path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
		temp_dir: std::env::temp_dir(),
	};
	let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
	let result = if ext.eq_ignore_ascii_case("vrm") {
		let step_started = Instant::now();
		let bytes = match std::fs::read(path) {
			Ok(bytes) => Arc::<[u8]>::from(bytes),
			Err(e) => return Err(format!("model read: {e}")),
		};
		if profile {
			log_import_profile_step(path, "read_bytes", step_started);
		}
		let step_started = Instant::now();
		let root = if bytes.len() <= 128 * 1024 * 1024 {
			gltf_root_json_from_bytes(bytes.as_ref()).ok()
		} else {
			None
		};
		if profile {
			log_import_profile_step(path, "parse_root_json", step_started);
		}
		let step_started = Instant::now();
		let result = if profile {
			import_vrm_bytes_profiled(Some(path), bytes.as_ref(), root)
		} else {
			import_vrm_bytes(Some(path), bytes.as_ref(), root)
		};
		if profile {
			log_import_profile_step(path, "import_vrm_bytes", step_started);
		}
		result
	} else if ext.eq_ignore_ascii_case("glb") {
		let step_started = Instant::now();
		let root = glb_root_json_from_path(path);
		if profile {
			log_import_profile_step(path, "parse_glb_root_json", step_started);
		}
		if root.as_ref().is_some_and(has_vrm_extension) {
			let step_started = Instant::now();
			let bytes = match std::fs::read(path) {
				Ok(bytes) => Arc::<[u8]>::from(bytes),
				Err(e) => return Err(format!("model read: {e}")),
			};
			if profile {
				log_import_profile_step(path, "read_vrm_glb_bytes", step_started);
			}
			let step_started = Instant::now();
			let result = if profile {
				import_vrm_bytes_profiled(Some(path), bytes.as_ref(), root)
			} else {
				import_vrm_bytes(Some(path), bytes.as_ref(), root)
			};
			if profile {
				log_import_profile_step(path, "import_vrm_bytes", step_started);
			}
			result
		} else {
			let step_started = Instant::now();
			let result = GltfImporter.import(&mut ctx, ImportInput::Path(path.to_path_buf()), ImportOptions);
			if profile {
				log_import_profile_step(path, "import_gltf_path", step_started);
			}
			result
		}
	} else if ext.eq_ignore_ascii_case("unavatar") {
		let step_started = Instant::now();
		let result = GltfImporter.import(&mut ctx, ImportInput::Path(path.to_path_buf()), ImportOptions);
		if profile {
			log_import_profile_step(path, "import_gltf_path", step_started);
		}
		result
	} else {
		let step_started = Instant::now();
		let result = GltfImporter.import(&mut ctx, ImportInput::Path(path.to_path_buf()), ImportOptions);
		if profile {
			log_import_profile_step(path, "import_gltf_path", step_started);
		}
		result
	};
	match result {
		Ok(res) => {
			let step_started = Instant::now();
			log_import_report_warnings(&res.report);
			if profile {
				log_import_report_profile(&res.report);
			}
			if profile {
				log_import_profile_step(path, "report_warnings", step_started);
			}
			let step_started = Instant::now();
			let mut document = res.document;
			apply_requested_wardrobe_set(&mut document, wardrobe_set);
			if profile {
				log_import_profile_step(path, "apply_wardrobe_set", step_started);
			}
			let step_started = Instant::now();
			if contact_parameter_emission {
				document.enable_contact_parameter_emission_runtime_override();
			}
			if profile {
				log_import_profile_step(path, "contact_parameter_emission", step_started);
			}
			Ok(Arc::new(document))
		}
		Err(e) => Err(format!("model import: {e}")),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn normalize_wardrobe_set_id_trims_empty_values() {
		assert_eq!(normalize_wardrobe_set_id(None), None);
		assert_eq!(normalize_wardrobe_set_id(Some("")), None);
		assert_eq!(normalize_wardrobe_set_id(Some(" \t ")), None);
		assert_eq!(normalize_wardrobe_set_id(Some(" field_drape ")), Some("field_drape"));
	}

	#[test]
	fn apply_required_wardrobe_set_rejects_empty_values() {
		let mut document = UnaDocument::default();
		let err = apply_required_wardrobe_set(&mut document, " \t ").expect_err("empty wardrobe id should be rejected");
		assert_eq!(err, "wardrobe set id required");
		assert_eq!(require_wardrobe_set_id(" field_drape ").unwrap(), "field_drape");
	}

	#[test]
	fn apply_requested_wardrobe_set_ignores_empty_values() {
		let mut document = UnaDocument::default();
		assert!(apply_requested_wardrobe_set(&mut document, None).is_none());
		assert!(apply_requested_wardrobe_set(&mut document, Some("")).is_none());
	}

	#[test]
	fn import_report_warning_lines_include_approximations_and_lost_features() {
		let mut report = ImportReport::default();
		report
			.diagnostics
			.push(un_avatar_core::ReportMessage::warning("diagnostic warning"));
		report.lost_features.push(un_avatar_core::LostFeature {
			feature: "ModularAvatar.Unsupported".to_string(),
			detail: Some("preserved only".to_string()),
		});
		report.approximations.push(un_avatar_core::Approximation {
			feature: "ModularAvatar.ModularAvatarShapeChanger".to_string(),
			detail: Some("enabled static set/delete payloads only".to_string()),
		});

		let lines = import_report_warning_lines(&report, 8);

		assert_eq!(lines.len(), 3);
		assert_eq!(lines[0], "import warning: diagnostic warning");
		assert!(lines[1].contains("import lost feature: ModularAvatar.Unsupported"));
		assert!(lines[2].contains("import approximation: ModularAvatar.ModularAvatarShapeChanger"));
	}

	#[test]
	fn wardrobe_apply_report_summary_includes_material_counts() {
		let report = WardrobeApplyReport {
			active_asset_groups: vec!["outfit:field".to_string()],
			scoped_active_asset_group_count: 1,
			scoped_missing_active_asset_groups: vec!["outfit:missing".to_string()],
			scoped_resident_mesh_primitive_count: 2,
			scoped_resident_material_count: 3,
			scoped_resident_image_count: 4,
			scoped_resident_dynamics_count: 5,
			visibility_applied: 6,
			visibility_missing: 7,
			blendshape_applied: 8,
			blendshape_missing: 9,
			dynamics_applied: 10,
			dynamics_missing: 11,
			material_applied: 12,
			material_missing: 13,
			material_slot_applied: 14,
			material_slot_missing: 15,
			..Default::default()
		};
		let summary = wardrobe_apply_report_summary("field_drape", &report);
		assert!(summary.contains("material_applied=12 material_missing=13"));
		assert!(summary.contains("material_slot_applied=14 material_slot_missing=15"));
		assert!(summary.contains("scoped_resident=mesh:2 material:3 image:4 dynamics:5"));
		assert!(summary.contains("active_asset_groups=[\"outfit:field\"]"));
		assert!(summary.contains("scoped_missing_groups=[\"outfit:missing\"]"));
	}
}
