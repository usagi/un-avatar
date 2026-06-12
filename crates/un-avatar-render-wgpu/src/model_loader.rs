use std::{path::Path, sync::Arc};

use un_avatar_core::{ImportReport, ReportSeverity, UnaDocument};
use un_avatar_io::{AvatarImporter, ImportContext, ImportInput, ImportOptions};
use un_avatar_io_gltf::{apply_unavatar_wardrobe_set, GltfImporter, WardrobeApplyReport};
use un_avatar_io_vrm::{gltf_root_json_from_bytes, has_vrm_extension, import_vrm_bytes};

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
	let mut ctx = ImportContext {
		asset_root: path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
		temp_dir: std::env::temp_dir(),
	};
	let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
	let result = if ext.eq_ignore_ascii_case("vrm") || ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("unavatar") {
		let bytes = match std::fs::read(path) {
			Ok(bytes) => Arc::<[u8]>::from(bytes),
			Err(e) => return Err(format!("model read: {e}")),
		};
		let root = if bytes.len() <= 128 * 1024 * 1024 {
			gltf_root_json_from_bytes(bytes.as_ref()).ok()
		} else {
			None
		};
		if ext.eq_ignore_ascii_case("vrm") || root.as_ref().is_some_and(has_vrm_extension) {
			import_vrm_bytes(Some(path), bytes.as_ref(), root)
		} else {
			GltfImporter.import(
				&mut ctx,
				ImportInput::Bytes {
					bytes,
					path_hint: Some(path.to_path_buf()),
				},
				ImportOptions,
			)
		}
	} else {
		GltfImporter.import(&mut ctx, ImportInput::Path(path.to_path_buf()), ImportOptions)
	};
	match result {
		Ok(res) => {
			log_import_report_warnings(&res.report);
			let mut document = res.document;
			apply_requested_wardrobe_set(&mut document, wardrobe_set);
			if contact_parameter_emission {
				document.enable_contact_parameter_emission_runtime_override();
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
