use std::{path::Path, sync::Arc};

use un_avatar_core::UnaDocument;
use un_avatar_io::{AvatarImporter, ImportContext, ImportInput, ImportOptions};
use un_avatar_io_gltf::{apply_unavatar_wardrobe_set, GltfImporter, WardrobeApplyReport};
use un_avatar_io_vrm::{gltf_root_json_from_bytes, has_vrm_extension, import_vrm_bytes};

pub(crate) fn normalize_wardrobe_set_id(wardrobe_set: Option<&str>) -> Option<&str> {
	wardrobe_set.map(str::trim).filter(|set_id| !set_id.is_empty())
}

fn log_wardrobe_apply_report(set_id: &str, report: &WardrobeApplyReport) {
	eprintln!(
		"un-avatar-renderer: .unavatar wardrobe set `{set_id}` applied: visibility_applied={} visibility_missing={} blendshape_applied={} blendshape_missing={} dynamics_applied={} dynamics_missing={}",
		report.visibility_applied,
		report.visibility_missing,
		report.blendshape_applied,
		report.blendshape_missing,
		report.dynamics_applied,
		report.dynamics_missing
	);
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
}

pub(crate) fn apply_required_wardrobe_set(document: &mut UnaDocument, wardrobe_set: &str) -> Result<WardrobeApplyReport, String> {
	let set_id = normalize_wardrobe_set_id(Some(wardrobe_set)).ok_or_else(|| "wardrobe set id required".to_string())?;
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

pub(crate) fn load_document(path: &Path, wardrobe_set: Option<&str>) -> Result<Arc<UnaDocument>, String> {
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
			let mut document = res.document;
			apply_requested_wardrobe_set(&mut document, wardrobe_set);
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
	}

	#[test]
	fn apply_requested_wardrobe_set_ignores_empty_values() {
		let mut document = UnaDocument::default();
		assert!(apply_requested_wardrobe_set(&mut document, None).is_none());
		assert!(apply_requested_wardrobe_set(&mut document, Some("")).is_none());
	}
}
