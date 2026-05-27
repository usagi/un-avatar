//! glTF 2.0 インポート（静的メッシュ + スキニング。Morph・スパースアクセサは読み飛ばし／レポート記録）。
//!
//! 設計正本: `docs/development-plan.md` Commit 1.3〜1.4

#![forbid(unsafe_code)]

use glam::{Mat4, Quat, Vec3};
use un_avatar_core::{
	Approximation, ReportStatus, UnaAlphaMode, UnaDocument, UnaImageRgba, UnaMaterialPbr, UnaMeshBuffers, UnaMorphTargetDeltas,
	UnaSceneNode, UnaSceneSnapshot, UnaShadingModel, UnaSkin,
};
use un_avatar_io::{
	AvatarImporter, Capability, FormatCapabilities, FormatDescriptor, FormatDirection, FormatId, ImportContext, ImportError, ImportInput,
	ImportOptions, ImportProbe, ImportProbeResult, ImportReport, ImportResult, IoRegistry, PluginStability,
};

/// glTF スキン 1 本あたりの joint 上限（レンダラのボーンパレット上限と揃える）。
const MAX_SKIN_JOINTS: usize = 512;

fn transform_cols(transform: gltf::scene::Transform) -> [f32; 16] {
	match transform {
		gltf::scene::Transform::Matrix { matrix } => Mat4::from_cols_array_2d(&matrix).to_cols_array(),
		gltf::scene::Transform::Decomposed {
			translation,
			rotation,
			scale,
		} => Mat4::from_scale_rotation_translation(
			Vec3::from(scale),
			Quat::from_xyzw(rotation[0], rotation[1], rotation[2], rotation[3]),
			Vec3::from(translation),
		)
		.to_cols_array(),
	}
}

fn decode_image(bytes: &[u8]) -> Result<UnaImageRgba, String> {
	let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
	let rgba = img.to_rgba8();
	let (w, h) = rgba.dimensions();
	Ok(UnaImageRgba {
		width: w,
		height: h,
		rgba: rgba.into_raw(),
	})
}

fn from_gltf_image(d: gltf::image::Data) -> Result<UnaImageRgba, String> {
	match d.format {
		gltf::image::Format::R8G8B8A8 => Ok(UnaImageRgba {
			width: d.width,
			height: d.height,
			rgba: d.pixels,
		}),
		gltf::image::Format::R8G8B8 => {
			let mut rgba = Vec::with_capacity(d.pixels.len() / 3 * 4);
			for chunk in d.pixels.chunks_exact(3) {
				rgba.extend_from_slice(chunk);
				rgba.push(255);
			}
			Ok(UnaImageRgba {
				width: d.width,
				height: d.height,
				rgba,
			})
		}
		_ => decode_image(&d.pixels).map_err(|e| format!("画像形式 {:?} のフォールバックデコードに失敗: {e}", d.format)),
	}
}

fn collect_images(images_data: Vec<gltf::image::Data>) -> Result<Vec<UnaImageRgba>, String> {
	let mut out = Vec::with_capacity(images_data.len());
	for d in images_data {
		out.push(from_gltf_image(d)?);
	}
	Ok(out)
}

fn ibm_cols_to_una(m: [[f32; 4]; 4]) -> [f32; 16] {
	Mat4::from_cols_array_2d(&m).to_cols_array()
}

fn build_skins(document: &gltf::Document, buffers: &[gltf::buffer::Data]) -> Result<Vec<UnaSkin>, ImportError> {
	let mut out = Vec::new();
	for skin in document.skins() {
		let joint_nodes: Vec<usize> = skin.joints().map(|n| n.index()).collect();
		if joint_nodes.is_empty() {
			return Err(ImportError::Message(format!("skin {} に joint がありません", skin.index())));
		}
		if joint_nodes.len() > MAX_SKIN_JOINTS {
			return Err(ImportError::Message(format!(
				"skin {} の joint 数 {} が上限 {} を超えています",
				skin.index(),
				joint_nodes.len(),
				MAX_SKIN_JOINTS
			)));
		}

		let reader = skin.reader(|b| buffers.get(b.index()).map(|d| d.as_ref()));
		let inverse_bind_matrices: Vec<[f32; 16]> = if let Some(iter) = reader.read_inverse_bind_matrices() {
			iter.map(ibm_cols_to_una).collect()
		} else {
			vec![Mat4::IDENTITY.to_cols_array(); joint_nodes.len()]
		};

		if inverse_bind_matrices.len() != joint_nodes.len() {
			return Err(ImportError::Message(format!(
				"skin {}: inverseBindMatrices の要素数が joints と一致しません",
				skin.index()
			)));
		}

		out.push(UnaSkin {
			joint_nodes,
			inverse_bind_matrices,
		});
	}
	Ok(out)
}

fn build_materials(document: &gltf::Document) -> Vec<UnaMaterialPbr> {
	document
		.materials()
		.map(|m| {
			let name = m.name().map(|s| s.to_string());
			let double_sided = m.double_sided();
			let pbr = m.pbr_metallic_roughness();
			let factor = pbr.base_color_factor();
			let tex = pbr.base_color_texture().map(|t| t.texture().source().index());
			let normal_texture_index = m.normal_texture().map(|t| t.texture().source().index());
			let normal_texture_scale = m.normal_texture().map(|t| t.scale()).unwrap_or(1.0);
			let occlusion_texture_index = m.occlusion_texture().map(|t| t.texture().source().index());
			let occlusion_texture_strength = m.occlusion_texture().map(|t| t.strength()).unwrap_or(1.0);
			let emissive_factor = m.emissive_factor();
			let emissive_texture_index = m.emissive_texture().map(|t| t.texture().source().index());
			let shading = if m.unlit() {
				UnaShadingModel::Unlit
			} else {
				UnaShadingModel::LitLambert
			};
			let alpha_cutoff = m.alpha_cutoff().unwrap_or(0.5);
			let alpha_mode = match m.alpha_mode() {
				gltf::material::AlphaMode::Opaque => UnaAlphaMode::Opaque,
				gltf::material::AlphaMode::Mask => UnaAlphaMode::Mask,
				gltf::material::AlphaMode::Blend => UnaAlphaMode::Blend,
			};
			UnaMaterialPbr {
				name,
				double_sided,
				base_color_factor: factor,
				base_color_texture_index: tex,
				normal_texture_index,
				normal_texture_scale,
				occlusion_texture_index,
				occlusion_texture_strength,
				emissive_factor,
				emissive_texture_index,
				metallic_factor: pbr.metallic_factor(),
				roughness_factor: pbr.roughness_factor(),
				shading,
				alpha_mode,
				alpha_cutoff,
				mtoon: None,
			}
		})
		.collect()
}

fn read_primitive(
	prim: gltf::Primitive<'_>,
	buffers: &[gltf::buffer::Data],
	mesh_weights: Option<&[f32]>,
	_report: &mut ImportReport,
) -> Result<Option<UnaMeshBuffers>, ImportError> {
	if prim.mode() != gltf::mesh::Mode::Triangles {
		_report.approximations.push(Approximation {
			feature: "primitive.mode".into(),
			detail: Some(format!("{:?} はスキップ（Triangles のみ）", prim.mode())),
		});
		return Ok(None);
	}

	let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d.as_ref()));
	let Some(iter_pos) = reader.read_positions() else {
		return Err(ImportError::Message("POSITION アクセサがありません".into()));
	};
	let positions: Vec<[f32; 3]> = iter_pos.collect();

	let joints_weights = match (reader.read_joints(0), reader.read_weights(0)) {
		(Some(jr), Some(wr)) => {
			let mut joints: Vec<[u16; 4]> = jr.into_u16().collect();
			let mut weights: Vec<[f32; 4]> = wr.into_f32().collect();
			if joints.len() != positions.len() || weights.len() != positions.len() {
				return Err(ImportError::Message(
					"JOINTS_0 / WEIGHTS_0 の頂点数が POSITION と一致しません".into(),
				));
			}
			for row in &joints {
				for &ji in row {
					if ji as usize >= MAX_SKIN_JOINTS {
						return Err(ImportError::Message(format!(
							"ジョイントインデックス {ji} が上限 {MAX_SKIN_JOINTS} を超えています"
						)));
					}
				}
			}
			for i in 0..weights.len() {
				let s: f32 = weights[i].iter().copied().sum();
				if s < 1e-6 {
					weights[i] = [1.0, 0.0, 0.0, 0.0];
					joints[i] = [0, 0, 0, 0];
				} else if (s - 1.0).abs() > 0.02 {
					let inv = 1.0 / s;
					for w in &mut weights[i] {
						*w *= inv;
					}
				}
			}
			(Some(joints), Some(weights))
		}
		(None, None) => (None, None),
		_ => {
			return Err(ImportError::Message(
				"JOINTS 0 と WEIGHTS 0 の片方だけがある primitive は未対応です".into(),
			));
		}
	};

	let normals = reader.read_normals().map(|it| it.collect());
	let tex_coords_0 = reader.read_tex_coords(0).map(|tc| tc.into_f32().collect());
	let indices = reader.read_indices().map(|idx| idx.into_u32().collect());
	let material_index = prim.material().index();
	let (joints, weights) = joints_weights;

	let mut morph_targets: Vec<UnaMorphTargetDeltas> = Vec::new();
	for (pos_d, norm_d, _tan_d) in reader.read_morph_targets() {
		let position_deltas: Vec<[f32; 3]> = if let Some(iter) = pos_d {
			let v: Vec<[f32; 3]> = iter.collect();
			if v.len() != positions.len() {
				return Err(ImportError::Message(format!(
					"モーフターゲットの POSITION デルタ数 {} がベース頂点数 {} と一致しません",
					v.len(),
					positions.len()
				)));
			}
			v
		} else {
			vec![[0.0, 0.0, 0.0]; positions.len()]
		};
		let normal_deltas = if let Some(iter) = norm_d {
			let v: Vec<[f32; 3]> = iter.collect();
			if v.len() != positions.len() {
				return Err(ImportError::Message(format!(
					"モーフターゲットの NORMAL デルタ数 {} がベース頂点数 {} と一致しません",
					v.len(),
					positions.len()
				)));
			}
			Some(v)
		} else {
			None
		};
		morph_targets.push(UnaMorphTargetDeltas {
			position_deltas,
			normal_deltas,
		});
	}

	let mut default_morph_weights: Vec<f32> = mesh_weights.map(|w| w.to_vec()).unwrap_or_default();
	if morph_targets.is_empty() {
		default_morph_weights.clear();
	} else {
		if default_morph_weights.len() < morph_targets.len() {
			default_morph_weights.resize(morph_targets.len(), 0.0);
		} else if default_morph_weights.len() > morph_targets.len() {
			default_morph_weights.truncate(morph_targets.len());
		}
	}

	Ok(Some(UnaMeshBuffers {
		name: None,
		positions,
		normals,
		tex_coords_0,
		joints,
		weights,
		indices,
		material_index,
		morph_targets,
		default_morph_weights,
	}))
}

/// glTF [`Document`] から [`UnaSceneSnapshot`] を構築（メッシュ・材質・スキン・ノード階層）。
pub fn scene_snapshot_from_gltf(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	image_data: Vec<gltf::image::Data>,
	report: &mut ImportReport,
) -> Result<UnaSceneSnapshot, ImportError> {
	let mut materials = build_materials(document);
	if materials.is_empty() {
		materials.push(UnaMaterialPbr::default());
	}

	let images = collect_images(image_data).map_err(ImportError::Message)?;

	let skins = build_skins(document, buffers)?;

	let mut meshes: Vec<Vec<UnaMeshBuffers>> = vec![Vec::new(); document.meshes().len()];
	for mesh in document.meshes() {
		let mid = mesh.index();
		let mw = mesh.weights();
		for prim in mesh.primitives() {
			if let Some(buf) = read_primitive(prim, buffers, mw, report)? {
				if mid < meshes.len() {
					meshes[mid].push(buf);
				}
			}
		}
	}

	let mut nodes = Vec::new();
	for node in document.nodes() {
		let children: Vec<usize> = node.children().map(|c| c.index()).collect();
		nodes.push(UnaSceneNode {
			name: node.name().map(|s| s.to_string()),
			transform: transform_cols(node.transform()),
			children,
			mesh: node.mesh().map(|m| m.index()),
			skin: node.skin().map(|s| s.index()),
		});
	}

	let roots: Vec<usize> = document
		.default_scene()
		.or_else(|| document.scenes().next())
		.map(|s| s.nodes().map(|n| n.index()).collect())
		.unwrap_or_default();

	let scene = UnaSceneSnapshot {
		meshes,
		materials,
		images,
		skins,
		nodes,
		roots,
		node_constraints: Vec::new(),
	};
	Ok(scene)
}

/// Built-in glTF Importer（`io.un-avatar.gltf`）。
#[derive(Clone, Copy, Debug, Default)]
pub struct GltfImporter;

impl AvatarImporter for GltfImporter {
	fn descriptor(&self) -> FormatDescriptor {
		FormatDescriptor {
			id: FormatId::new("io.un-avatar.gltf"),
			display_name: "glTF 2.0".to_owned(),
			extensions: vec!["gltf".to_owned(), "glb".to_owned()],
			media_types: vec!["model/gltf+json".to_owned(), "model/gltf-binary".to_owned()],
			direction: FormatDirection::Import,
			capabilities: FormatCapabilities {
				mesh: Capability::ImportOnly,
				skeleton: Capability::ImportOnly,
				skinning: Capability::ImportOnly,
				animation: Capability::Unsupported,
				expression: Capability::ImportOnly,
				material: Capability::ImportOnly,
				physics: Capability::Unsupported,
				cameras: Capability::Unsupported,
				lights: Capability::Unsupported,
				custom_extensions: Capability::Unsupported,
			},
			stability: PluginStability::Experimental,
			provider_plugin_id: None,
		}
	}

	fn probe(&self, input: &ImportProbe) -> ImportProbeResult {
		let Some(p) = input.path_hint.as_ref() else {
			return ImportProbeResult { confidence: 0 };
		};
		let s = p.as_os_str().to_string_lossy().to_lowercase();
		if s.ends_with(".glb") {
			return ImportProbeResult { confidence: 254 };
		}
		if s.ends_with(".gltf") {
			return ImportProbeResult { confidence: 240 };
		}
		ImportProbeResult { confidence: 0 }
	}

	fn import(&self, _ctx: &mut ImportContext, input: ImportInput, _options: ImportOptions) -> Result<ImportResult, ImportError> {
		let (path_hint, document, buffers, image_data) = match input {
			ImportInput::Path(path) => {
				let imported = gltf::import(&path).map_err(|e| ImportError::Message(e.to_string()))?;
				(Some(path), imported.0, imported.1, imported.2)
			}
			ImportInput::Bytes { bytes, path_hint } => {
				let imported = gltf::import_slice(bytes.as_ref()).map_err(|e| ImportError::Message(e.to_string()))?;
				(path_hint, imported.0, imported.1, imported.2)
			}
		};

		let mut report = ImportReport {
			source_format: Some(self.descriptor().id.clone()),
			..Default::default()
		};

		let scene = scene_snapshot_from_gltf(&document, &buffers, image_data, &mut report)?;

		report.status = if report.lost_features.is_empty() && report.approximations.is_empty() {
			ReportStatus::Success
		} else {
			ReportStatus::PartialSuccess
		};
		report.push_info(format!(
			"glTF: {} mesh(es), {} node(s), {} skin(s), {} material(s)",
			document.meshes().len(),
			document.nodes().len(),
			document.skins().len(),
			document.materials().len()
		));
		if let Some(path) = path_hint {
			report.push_info(format!("source: {}", path.display()));
		} else {
			report.push_info("source: in-memory glTF/GLB".to_string());
		}

		Ok(ImportResult {
			document: UnaDocument {
				scene: Some(scene),
				..Default::default()
			},
			report,
		})
	}
}

/// glTF importer をレジストリに登録する（UNA の次など任意）。
pub fn register_gltf_importer(registry: &mut IoRegistry) {
	registry.register_importer(Box::new(GltfImporter));
}

#[cfg(test)]
mod tests {
	use super::*;
	use glam::Mat4;
	use std::io::Write;

	fn triangle_bin_bytes() -> Vec<u8> {
		let mut v = Vec::with_capacity(48);
		for f in [-1.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
			v.extend_from_slice(&f.to_le_bytes());
		}
		for i in [0u32, 1u32, 2u32] {
			v.extend_from_slice(&i.to_le_bytes());
		}
		v
	}

	fn skin_one_bone_bin_bytes() -> Vec<u8> {
		let mut v = Vec::with_capacity(172);
		for f in [-1.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
			v.extend_from_slice(&f.to_le_bytes());
		}
		for _ in 0..3 {
			v.extend_from_slice(&[0u8, 0, 0, 0]);
		}
		for _ in 0..3 {
			for f in [1.0_f32, 0.0, 0.0, 0.0] {
				v.extend_from_slice(&f.to_le_bytes());
			}
		}
		for i in [0u32, 1u32, 2u32] {
			v.extend_from_slice(&i.to_le_bytes());
		}
		for x in Mat4::IDENTITY.to_cols_array() {
			v.extend_from_slice(&x.to_le_bytes());
		}
		v
	}

	#[test]
	fn imports_triangle_gltf() {
		let dir = std::env::temp_dir().join(format!("un-avatar-gltf-test-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let gltf_path = dir.join("triangle.gltf");
		let json = include_str!("../tests/fixtures/triangle.gltf");
		std::fs::File::create(&gltf_path).unwrap().write_all(json.as_bytes()).unwrap();
		std::fs::write(dir.join("triangle.bin"), triangle_bin_bytes()).unwrap();

		let imp = GltfImporter;
		let mut ctx = ImportContext::dummy();
		let got = imp.import(&mut ctx, ImportInput::Path(gltf_path), ImportOptions).unwrap();
		assert!(got.document.scene.is_some());
		let sc = got.document.scene.as_ref().unwrap();
		assert!(!sc.meshes[0].is_empty());
		assert_eq!(sc.meshes[0][0].positions.len(), 3);
		assert!(sc.meshes[0][0].joints.is_none() && sc.meshes[0][0].weights.is_none());
		let _ = std::fs::remove_dir_all(&dir);
	}

	#[test]
	fn imports_single_bone_skin_gltf() {
		let dir = std::env::temp_dir().join(format!("un-avatar-gltf-skin-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let gltf_path = dir.join("skin_one_bone.gltf");
		let json = include_str!("../tests/fixtures/skin_one_bone.gltf");
		std::fs::File::create(&gltf_path).unwrap().write_all(json.as_bytes()).unwrap();
		std::fs::write(dir.join("skin_one_bone.bin"), skin_one_bone_bin_bytes()).unwrap();

		let imp = GltfImporter;
		let mut ctx = ImportContext::dummy();
		let got = imp.import(&mut ctx, ImportInput::Path(gltf_path), ImportOptions).unwrap();
		let sc = got.document.scene.as_ref().unwrap();
		assert_eq!(sc.skins.len(), 1);
		assert_eq!(sc.skins[0].joint_nodes, vec![1]);
		assert_eq!(sc.skins[0].inverse_bind_matrices.len(), 1);
		let prim = &sc.meshes[0][0];
		assert!(prim.joints.is_some() && prim.weights.is_some());
		assert_eq!(prim.joints.as_ref().unwrap().len(), 3);
		let _ = std::fs::remove_dir_all(&dir);
	}
}
