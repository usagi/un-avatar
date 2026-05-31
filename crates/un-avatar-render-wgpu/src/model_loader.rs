use std::{path::Path, sync::Arc};

use un_avatar_core::UnaDocument;
use un_avatar_io::{AvatarImporter, ImportContext, ImportInput, ImportOptions};
use un_avatar_io_gltf::{apply_unavatar_wardrobe_set, GltfImporter};
use un_avatar_io_vrm::{gltf_root_json_from_bytes, has_vrm_extension, import_vrm_bytes};

pub(crate) fn load_document(path: &Path, wardrobe_set: Option<&str>) -> Option<Arc<UnaDocument>> {
	let mut ctx = ImportContext {
		asset_root: path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
		temp_dir: std::env::temp_dir(),
	};
	let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
	let result = if ext.eq_ignore_ascii_case("vrm") || ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("unavatar") {
		let bytes = match std::fs::read(path) {
			Ok(bytes) => Arc::<[u8]>::from(bytes),
			Err(e) => {
				eprintln!("un-avatar-renderer: model read: {e}");
				return None;
			}
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
			if let Some(set_id) = wardrobe_set.filter(|set_id| !set_id.trim().is_empty()) {
				match apply_unavatar_wardrobe_set(&mut document, set_id) {
					Ok(report) => {
						eprintln!(
							"un-avatar-renderer: .unavatar wardrobe set `{set_id}` applied: visibility_applied={} visibility_missing={} blendshape_applied={} blendshape_missing={}",
							report.visibility_applied, report.visibility_missing, report.blendshape_applied, report.blendshape_missing
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
					}
					Err(e) => eprintln!("un-avatar-renderer: .unavatar wardrobe set `{set_id}` not applied: {e}"),
				}
			}
			Some(Arc::new(document))
		}
		Err(e) => {
			eprintln!("un-avatar-renderer: model import: {e}");
			None
		}
	}
}
