use std::{path::Path, sync::Arc};

use un_avatar_core::UnaDocument;
use un_avatar_io::{AvatarImporter, ImportContext, ImportInput, ImportOptions};
use un_avatar_io_gltf::GltfImporter;
use un_avatar_io_vrm::{gltf_root_json_from_bytes, has_vrm_extension, import_vrm_bytes};

pub(crate) fn load_document(path: &Path) -> Option<Arc<UnaDocument>> {
	let mut ctx = ImportContext {
		asset_root: path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
		temp_dir: std::env::temp_dir(),
	};
	let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
	let result = if ext.eq_ignore_ascii_case("vrm") || ext.eq_ignore_ascii_case("glb") {
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
		Ok(res) => Some(Arc::new(res.document)),
		Err(e) => {
			eprintln!("un-avatar-renderer: model import: {e}");
			None
		}
	}
}
