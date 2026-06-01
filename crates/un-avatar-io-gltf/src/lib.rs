//! glTF 2.0 インポート（静的メッシュ + スキニング。Morph・スパースアクセサは読み飛ばし／レポート記録）。
//!
//! 設計正本: `docs/development-plan.md` Commit 1.3〜1.4

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::io::Cursor;

use exr::prelude::{f16, pixel_vec::PixelVec, read, ReadChannels, ReadLayers};
use glam::{Mat4, Quat, Vec3};
use serde_json::Value;
use un_avatar_core::{
	Approximation, ReportStatus, UnaAlphaMode, UnaDocument, UnaImagePixelFormat, UnaImageRgba, UnaImageSourceMetadata, UnaMaterialPbr,
	UnaMeshBuffers, UnaMorphTargetDeltas, UnaMtoonMaterial, UnaMtoonOutlineWidthMode, UnaSceneNode, UnaSceneSnapshot, UnaShadingModel,
	UnaSkin, UnaUnavatarExtension,
};
use un_avatar_io::{
	AvatarImporter, Capability, FormatCapabilities, FormatDescriptor, FormatDirection, FormatId, ImportContext, ImportError, ImportInput,
	ImportOptions, ImportProbe, ImportProbeResult, ImportReport, ImportResult, IoRegistry, PluginStability,
};
use un_avatar_types::HumanoidProfile;

/// glTF スキン 1 本あたりの joint 上限（レンダラのボーンパレット上限と揃える）。
const MAX_SKIN_JOINTS: usize = 512;
const UN_AVATAR_EXTENSION_NAME: &str = "UN_avatar";
const GLB_MAGIC: u32 = 0x46546C67;
const GLB_VERSION_2: u32 = 2;
const JSON_CHUNK_TYPE: u32 = 0x4E4F534A;
const BIN_CHUNK_TYPE: u32 = 0x004E4942;

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

fn from_gltf_image(d: gltf::image::Data) -> Result<(UnaImageRgba, Option<String>), String> {
	let source_format = format!("{:?}", d.format);
	let pixel_format = match d.format {
		gltf::image::Format::R8 => UnaImagePixelFormat::R8,
		gltf::image::Format::R8G8 => UnaImagePixelFormat::R8G8,
		gltf::image::Format::R8G8B8 => UnaImagePixelFormat::R8G8B8,
		gltf::image::Format::R8G8B8A8 => UnaImagePixelFormat::R8G8B8A8,
		gltf::image::Format::R16 => UnaImagePixelFormat::R16,
		gltf::image::Format::R16G16 => UnaImagePixelFormat::R16G16,
		gltf::image::Format::R16G16B16 => UnaImagePixelFormat::R16G16B16,
		gltf::image::Format::R16G16B16A16 => UnaImagePixelFormat::R16G16B16A16,
		gltf::image::Format::R32G32B32FLOAT => UnaImagePixelFormat::R32G32B32Float,
		gltf::image::Format::R32G32B32A32FLOAT => UnaImagePixelFormat::R32G32B32A32Float,
	};
	let image = UnaImageRgba {
		width: d.width,
		height: d.height,
		pixel_format,
		pixels: d.pixels,
	};
	let approximation =
		(!pixel_format.is_rgba8_upload_native()).then(|| format!("decoded {source_format} texture stored as source pixels"));
	Ok((image, approximation))
}

fn collect_images(images_data: Vec<gltf::image::Data>, report: &mut ImportReport) -> Result<Vec<UnaImageRgba>, String> {
	let mut out = Vec::with_capacity(images_data.len());
	for (index, d) in images_data.into_iter().enumerate() {
		let (image, approximation) = from_gltf_image(d)?;
		if let Some(detail) = approximation {
			report.approximations.push(Approximation {
				feature: format!("image[{index}].pixel_format"),
				detail: Some(detail),
			});
		}
		out.push(image);
	}
	Ok(out)
}

fn collect_image_source_metadata(document: &gltf::Document, buffers: &[gltf::buffer::Data]) -> Vec<Option<UnaImageSourceMetadata>> {
	document
		.images()
		.map(|image| {
			let name = image.name().map(str::to_string);
			match image.source() {
				gltf::image::Source::View { view, mime_type } => {
					let buffer_index = view.buffer().index();
					let buffer = buffers.get(buffer_index)?;
					let start = view.offset();
					let end = start.checked_add(view.length())?;
					let bytes = buffer.0.get(start..end)?;
					Some(UnaImageSourceMetadata {
						name,
						mime_type: Some(mime_type.to_string()),
						uri: None,
						source_pixel_format: None,
						channels: None,
						color_space: None,
						byte_length: bytes.len() as u64,
						source_hash: fnv1a64(bytes),
					})
				}
				gltf::image::Source::Uri { uri, mime_type } => Some(UnaImageSourceMetadata {
					name,
					mime_type: mime_type.map(str::to_string),
					uri: Some(uri.to_string()),
					source_pixel_format: None,
					channels: None,
					color_space: None,
					byte_length: 0,
					source_hash: fnv1a64(uri.as_bytes()),
				}),
			}
		})
		.collect()
}

fn collect_glb_image_source_metadata(root: &Value, bin: &[u8]) -> Vec<Option<UnaImageSourceMetadata>> {
	let Some(images) = root.get("images").and_then(Value::as_array) else {
		return Vec::new();
	};
	let buffer_views = root.get("bufferViews").and_then(Value::as_array);
	images
		.iter()
		.map(|image| {
			let name = image.get("name").and_then(Value::as_str).map(str::to_string);
			let mime_type = image.get("mimeType").and_then(Value::as_str).map(str::to_string);
			if let Some(uri) = image.get("uri").and_then(Value::as_str) {
				return Some(UnaImageSourceMetadata {
					name,
					mime_type,
					uri: Some(uri.to_string()),
					source_pixel_format: None,
					channels: None,
					color_space: None,
					byte_length: 0,
					source_hash: fnv1a64(uri.as_bytes()),
				});
			}
			let view_index = image.get("bufferView").and_then(Value::as_u64)? as usize;
			let view = buffer_views?.get(view_index)?;
			let offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
			let length = view.get("byteLength").and_then(Value::as_u64)? as usize;
			let bytes = bin.get(offset..offset.checked_add(length)?)?;
			Some(UnaImageSourceMetadata {
				name,
				mime_type,
				uri: None,
				source_pixel_format: None,
				channels: None,
				color_space: None,
				byte_length: bytes.len() as u64,
				source_hash: fnv1a64(bytes),
			})
		})
		.collect()
}

fn append_unavatar_texture_assets(
	scene: &mut UnaSceneSnapshot,
	root: &Value,
	bin: &[u8],
	report: &mut ImportReport,
) -> BTreeMap<String, usize> {
	let mut map = BTreeMap::new();
	let Some(assets) = root
		.get("extensions")
		.and_then(Value::as_object)
		.and_then(|extensions| extensions.get(UN_AVATAR_EXTENSION_NAME))
		.and_then(|ext| ext.get("textureAssets"))
		.and_then(Value::as_array)
	else {
		return map;
	};
	for asset in assets {
		let id = asset.get("id").and_then(Value::as_str).unwrap_or("");
		if id.is_empty() {
			continue;
		}
		let mime_type = asset.get("mimeType").and_then(Value::as_str).unwrap_or("");
		let Some(bytes) = texture_asset_bytes(root, bin, asset) else {
			report.lost_features.push(un_avatar_core::LostFeature {
				feature: format!("UN_avatar.textureAssets[{id}]"),
				detail: Some("missing or invalid bufferView".to_string()),
			});
			continue;
		};
		let source_pixel_format = asset.get("sourcePixelFormat").and_then(Value::as_str);
		let channels = asset.get("channels").and_then(Value::as_str);
		let decoded = match decode_unavatar_texture_asset(bytes, mime_type, source_pixel_format, channels) {
			Ok(image) => image,
			Err(error) => {
				report.lost_features.push(un_avatar_core::LostFeature {
					feature: format!("UN_avatar.textureAssets[{id}]"),
					detail: Some(error),
				});
				continue;
			}
		};
		let image_index = scene.images.len();
		scene.images.push(decoded);
		scene.image_sources.push(Some(UnaImageSourceMetadata {
			name: asset.get("name").and_then(Value::as_str).map(str::to_string),
			mime_type: Some(mime_type.to_string()),
			uri: asset.get("assetPath").and_then(Value::as_str).map(str::to_string),
			source_pixel_format: source_pixel_format.map(str::to_string),
			channels: channels.map(str::to_string),
			color_space: asset.get("colorSpace").and_then(Value::as_str).map(str::to_string),
			byte_length: bytes.len() as u64,
			source_hash: fnv1a64(bytes),
		}));
		map.insert(id.to_string(), image_index);
	}
	map
}

fn texture_asset_bytes<'a>(root: &Value, bin: &'a [u8], asset: &Value) -> Option<&'a [u8]> {
	let view_index = asset.get("bufferView").and_then(Value::as_u64)? as usize;
	let view = root.get("bufferViews").and_then(Value::as_array)?.get(view_index)?;
	let offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
	let length = view.get("byteLength").and_then(Value::as_u64)? as usize;
	bin.get(offset..offset.checked_add(length)?)
}

fn decode_unavatar_texture_asset(
	bytes: &[u8],
	mime_type: &str,
	source_pixel_format: Option<&str>,
	channels: Option<&str>,
) -> Result<UnaImageRgba, String> {
	match mime_type {
		"image/exr" => {
			if matches!(source_pixel_format, Some("RGB16F" | "RGBA16F")) {
				return decode_exr_half_texture_asset(bytes, channels);
			}
			let decoded =
				image::load_from_memory_with_format(bytes, image::ImageFormat::OpenExr).map_err(|e| format!("EXR decode: {e}"))?;
			if channels == Some("rgb") {
				let rgb = decoded.to_rgb32f();
				let width = rgb.width();
				let height = rgb.height();
				let pixels = rgb.into_raw().into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
				Ok(UnaImageRgba {
					width,
					height,
					pixel_format: UnaImagePixelFormat::R32G32B32Float,
					pixels,
				})
			} else {
				let rgba = decoded.to_rgba32f();
				let width = rgba.width();
				let height = rgba.height();
				let pixels = rgba.into_raw().into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
				Ok(UnaImageRgba {
					width,
					height,
					pixel_format: UnaImagePixelFormat::R32G32B32A32Float,
					pixels,
				})
			}
		}
		other => Err(format!("unsupported UN_avatar texture asset MIME: {other}")),
	}
}

fn decode_exr_half_texture_asset(bytes: &[u8], channels: Option<&str>) -> Result<UnaImageRgba, String> {
	let image = read()
		.no_deep_data()
		.largest_resolution_level()
		.rgba_channels(PixelVec::<(f16, f16, f16, f16)>::constructor, PixelVec::set_pixel)
		.first_valid_layer()
		.all_attributes()
		.from_buffered(Cursor::new(bytes))
		.map_err(|e| format!("EXR half decode: {e}"))?;
	let width = image.layer_data.size.width() as u32;
	let height = image.layer_data.size.height() as u32;
	let pixels_rgba = image.layer_data.channel_data.pixels.pixels;
	let keep_rgb = channels == Some("rgb");
	let mut pixels = Vec::with_capacity(pixels_rgba.len() * if keep_rgb { 6 } else { 8 });
	for (r, g, b, a) in pixels_rgba {
		pixels.extend_from_slice(&r.to_bits().to_le_bytes());
		pixels.extend_from_slice(&g.to_bits().to_le_bytes());
		pixels.extend_from_slice(&b.to_bits().to_le_bytes());
		if !keep_rgb {
			pixels.extend_from_slice(&a.to_bits().to_le_bytes());
		}
	}
	Ok(UnaImageRgba {
		width,
		height,
		pixel_format: if keep_rgb {
			UnaImagePixelFormat::R16G16B16Float
		} else {
			UnaImagePixelFormat::R16G16B16A16Float
		},
		pixels,
	})
}

fn apply_unavatar_material_texture_asset_refs(scene: &mut UnaSceneSnapshot, root: &Value, asset_map: &BTreeMap<String, usize>) {
	if asset_map.is_empty() {
		return;
	}
	let Some(materials) = root.get("materials").and_then(Value::as_array) else {
		return;
	};
	for (index, material) in materials.iter().enumerate() {
		let Some(scene_material) = scene.materials.get_mut(index) else {
			continue;
		};
		let Some(unavatar_material) = material.get("extras").and_then(|extras| extras.get("UN_avatar_material")) else {
			continue;
		};
		let Some(mtoon) = unavatar_material.get("mtoon") else {
			continue;
		};
		if let Some(image_index) = texture_asset_ref(mtoon, "shadeMultiplyTextureIndexAsset", asset_map) {
			scene_material
				.mtoon
				.get_or_insert_with(Default::default)
				.shade_multiply_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "matcapTextureIndexAsset", asset_map) {
			scene_material.mtoon.get_or_insert_with(Default::default).matcap_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "rimMultiplyTextureIndexAsset", asset_map) {
			scene_material.mtoon.get_or_insert_with(Default::default).rim_multiply_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "reflectionCubeTextureIndexAsset", asset_map) {
			scene_material
				.mtoon
				.get_or_insert_with(Default::default)
				.reflection_cube_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "outlineWidthMultiplyTextureIndexAsset", asset_map) {
			scene_material
				.mtoon
				.get_or_insert_with(Default::default)
				.outline_width_multiply_texture_index = Some(image_index);
		}
	}
}

fn texture_asset_ref(value: &Value, key: &str, asset_map: &BTreeMap<String, usize>) -> Option<usize> {
	let id = value.get(key).and_then(Value::as_str)?;
	asset_map.get(id).copied()
}

fn fnv1a64(bytes: &[u8]) -> u64 {
	let mut hash = 0xcbf29ce484222325u64;
	for &byte in bytes {
		hash ^= byte as u64;
		hash = hash.wrapping_mul(0x100000001b3);
	}
	hash
}

fn gltf_root_json_from_bytes(bytes: &[u8]) -> Result<Value, ImportError> {
	if bytes.starts_with(b"glTF") {
		let glb = gltf::Glb::from_slice(bytes).map_err(|e| ImportError::Message(format!("GLB 解析: {e}")))?;
		serde_json::from_slice(glb.json.as_ref()).map_err(|e| ImportError::Message(format!("GLB JSON: {e}")))
	} else {
		serde_json::from_slice(bytes).map_err(|e| ImportError::Message(format!("glTF JSON: {e}")))
	}
}

fn normalize_webp_glb_for_gltf_import(bytes: &[u8]) -> Result<Cow<'_, [u8]>, ImportError> {
	if !bytes.starts_with(b"glTF") {
		return Ok(Cow::Borrowed(bytes));
	}
	let (mut root, bin) = read_glb_json_and_bin(bytes)?;
	let has_webp = root.get("images").and_then(Value::as_array).is_some_and(|images| {
		images
			.iter()
			.any(|image| image.get("mimeType").and_then(Value::as_str) == Some("image/webp"))
	});
	if !has_webp {
		return Ok(Cow::Borrowed(bytes));
	}

	let mut views = extract_glb_buffer_views(&root, &bin)?;
	let Some(images) = root.get_mut("images").and_then(Value::as_array_mut) else {
		return Ok(Cow::Borrowed(bytes));
	};
	for image in images {
		if image.get("mimeType").and_then(Value::as_str) != Some("image/webp") {
			continue;
		}
		let Some(view_index) = image.get("bufferView").and_then(Value::as_u64).map(|v| v as usize) else {
			continue;
		};
		let Some(view) = views.get_mut(view_index) else {
			continue;
		};
		let decoded =
			image::load_from_memory(&view.bytes).map_err(|e| ImportError::Message(format!("WebP image decode for glTF import: {e}")))?;
		let mut png = Vec::new();
		decoded
			.write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
			.map_err(|e| ImportError::Message(format!("WebP image PNG fallback encode: {e}")))?;
		view.bytes = png;
		if let Some(obj) = image.as_object_mut() {
			obj.insert("mimeType".to_string(), Value::String("image/png".to_string()));
		}
	}
	Ok(Cow::Owned(rebuild_glb(&mut root, &views)?))
}

struct GltfBufferViewBytes {
	bytes: Vec<u8>,
	target: Option<Value>,
}

fn read_glb_json_and_bin(bytes: &[u8]) -> Result<(Value, Vec<u8>), ImportError> {
	if bytes.len() < 12 || read_glb_u32(bytes, 0)? != GLB_MAGIC || read_glb_u32(bytes, 4)? != GLB_VERSION_2 {
		return Err(ImportError::Message("GLB 2.0 expected".to_string()));
	}
	let mut offset = 12usize;
	let mut json = None;
	let mut bin = Vec::new();
	while offset + 8 <= bytes.len() {
		let length = read_glb_u32(bytes, offset)? as usize;
		let chunk_type = read_glb_u32(bytes, offset + 4)?;
		offset += 8;
		if offset + length > bytes.len() {
			return Err(ImportError::Message("GLB chunk exceeds file length".to_string()));
		}
		let chunk = &bytes[offset..offset + length];
		match chunk_type {
			JSON_CHUNK_TYPE => {
				let end = chunk.iter().position(|b| *b == 0).unwrap_or(chunk.len());
				json = Some(serde_json::from_slice(&chunk[..end]).map_err(|e| ImportError::Message(format!("GLB JSON: {e}")))?);
			}
			BIN_CHUNK_TYPE => bin = chunk.to_vec(),
			_ => {}
		}
		offset += length;
	}
	Ok((
		json.ok_or_else(|| ImportError::Message("GLB JSON chunk is missing".to_string()))?,
		bin,
	))
}

fn extract_glb_buffer_views(root: &Value, bin: &[u8]) -> Result<Vec<GltfBufferViewBytes>, ImportError> {
	let array = root
		.get("bufferViews")
		.and_then(Value::as_array)
		.ok_or_else(|| ImportError::Message("GLB bufferViews array is missing".to_string()))?;
	let mut views = Vec::with_capacity(array.len());
	for view in array {
		let byte_offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
		let byte_length = view
			.get("byteLength")
			.and_then(Value::as_u64)
			.ok_or_else(|| ImportError::Message("bufferView without byteLength".to_string()))? as usize;
		if byte_offset + byte_length > bin.len() {
			return Err(ImportError::Message("bufferView exceeds BIN chunk".to_string()));
		}
		views.push(GltfBufferViewBytes {
			bytes: bin[byte_offset..byte_offset + byte_length].to_vec(),
			target: view.get("target").cloned(),
		});
	}
	Ok(views)
}

fn rebuild_glb(root: &mut Value, views: &[GltfBufferViewBytes]) -> Result<Vec<u8>, ImportError> {
	let mut bin = Vec::new();
	let mut buffer_views = Vec::with_capacity(views.len());
	for view in views {
		align_to_4(&mut bin, 0);
		let byte_offset = bin.len();
		bin.extend_from_slice(&view.bytes);
		let mut obj = serde_json::Map::new();
		obj.insert("buffer".to_string(), Value::from(0));
		obj.insert("byteOffset".to_string(), Value::from(byte_offset as u64));
		obj.insert("byteLength".to_string(), Value::from(view.bytes.len() as u64));
		if let Some(target) = &view.target {
			obj.insert("target".to_string(), target.clone());
		}
		buffer_views.push(Value::Object(obj));
	}
	align_to_4(&mut bin, 0);
	root["bufferViews"] = Value::Array(buffer_views);
	if let Some(buffer) = root
		.get_mut("buffers")
		.and_then(Value::as_array_mut)
		.and_then(|buffers| buffers.get_mut(0))
		.and_then(Value::as_object_mut)
	{
		buffer.insert("byteLength".to_string(), Value::from(bin.len() as u64));
	}

	let mut json = serde_json::to_vec(root).map_err(|e| ImportError::Message(format!("serialize GLB JSON: {e}")))?;
	align_to_4(&mut json, b' ');
	let total_length = 12 + 8 + json.len() + 8 + bin.len();
	let mut out = Vec::with_capacity(total_length);
	write_glb_u32(&mut out, GLB_MAGIC);
	write_glb_u32(&mut out, GLB_VERSION_2);
	write_glb_u32(&mut out, total_length as u32);
	write_glb_u32(&mut out, json.len() as u32);
	write_glb_u32(&mut out, JSON_CHUNK_TYPE);
	out.extend_from_slice(&json);
	write_glb_u32(&mut out, bin.len() as u32);
	write_glb_u32(&mut out, BIN_CHUNK_TYPE);
	out.extend_from_slice(&bin);
	Ok(out)
}

fn read_glb_u32(bytes: &[u8], offset: usize) -> Result<u32, ImportError> {
	let slice = bytes
		.get(offset..offset + 4)
		.ok_or_else(|| ImportError::Message("unexpected end of GLB".to_string()))?;
	Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn write_glb_u32(out: &mut Vec<u8>, value: u32) {
	out.extend_from_slice(&value.to_le_bytes());
}

fn align_to_4(bytes: &mut Vec<u8>, padding: u8) {
	while bytes.len() % 4 != 0 {
		bytes.push(padding);
	}
}

fn unavatar_extension_from_root(root: &Value) -> Option<UnaUnavatarExtension> {
	let ext = root.get("extensions")?.as_object()?.get(UN_AVATAR_EXTENSION_NAME)?.clone();
	let spec_version = ext.get("specVersion").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
	Some(UnaUnavatarExtension { spec_version, source: ext })
}

fn scene_node_paths(scene: &UnaSceneSnapshot) -> BTreeMap<String, usize> {
	fn visit(scene: &UnaSceneSnapshot, idx: usize, path: String, out: &mut BTreeMap<String, usize>) {
		out.insert(path.clone(), idx);
		let Some(node) = scene.nodes.get(idx) else { return };
		for &child in &node.children {
			let Some(child_node) = scene.nodes.get(child) else { continue };
			let child_name = child_node.name.as_deref().unwrap_or("");
			let child_path = if path.is_empty() {
				child_name.to_string()
			} else {
				format!("{path}/{child_name}")
			};
			visit(scene, child, child_path, out);
		}
	}

	let mut out = BTreeMap::new();
	for &root in &scene.roots {
		visit(scene, root, String::new(), &mut out);
	}
	out
}

fn normalize_unavatar_path(path: &str) -> String {
	path.split('/').map(normalize_unavatar_path_segment).collect::<Vec<_>>().join("/")
}

fn normalize_unavatar_path_segment(segment: &str) -> String {
	let mut out = segment.split_whitespace().collect::<Vec<_>>().join(" ");
	if let Some((prefix, _)) = out.split_once('$') {
		out = prefix.to_string();
	}
	if out
		.strip_prefix("Armature.")
		.is_some_and(|suffix| suffix.chars().all(|ch| ch.is_ascii_digit()))
	{
		out = "Armature".to_string();
	}
	out
}

fn scene_node_normalized_paths(scene: &UnaSceneSnapshot) -> BTreeMap<String, Vec<usize>> {
	let mut out = BTreeMap::new();
	for (path, idx) in scene_node_paths(scene) {
		out.entry(normalize_unavatar_path(&path)).or_insert_with(Vec::new).push(idx);
	}
	out
}

fn lookup_scene_path_all(paths: &BTreeMap<String, usize>, normalized_paths: &BTreeMap<String, Vec<usize>>, path: &str) -> Vec<usize> {
	if let Some(&idx) = paths.get(path) {
		return vec![idx];
	}
	let normalized = normalize_unavatar_path(path);
	if let Some(indices) = normalized_paths.get(&normalized) {
		return indices.clone();
	}
	let segments: Vec<&str> = normalized.split('/').filter(|segment| !segment.is_empty()).collect();
	for drop_count in 1..segments.len() {
		let suffix = segments[drop_count..].join("/");
		let mut matches = Vec::new();
		for (candidate, indices) in normalized_paths {
			if candidate == &suffix || candidate.ends_with(&format!("/{suffix}")) {
				matches.extend(indices.iter().copied());
			}
		}
		if !matches.is_empty() {
			matches.sort_unstable();
			matches.dedup();
			return matches;
		}
	}
	Vec::new()
}

fn scene_node_ids(scene: &UnaSceneSnapshot) -> BTreeMap<String, usize> {
	scene
		.nodes
		.iter()
		.enumerate()
		.filter_map(|(idx, node)| node.source_node_id.as_ref().map(|id| (id.clone(), idx)))
		.collect()
}

fn unavatar_node_registry_paths(unavatar: Option<&UnaUnavatarExtension>) -> BTreeMap<String, String> {
	let Some(unavatar) = unavatar else {
		return BTreeMap::new();
	};
	let Some(nodes) = unavatar.source.get("nodes").and_then(|v| v.as_array()) else {
		return BTreeMap::new();
	};
	nodes
		.iter()
		.filter_map(|node| {
			let id = node.get("nodeId").and_then(|v| v.as_str())?;
			let path = node.get("path").and_then(|v| v.as_str()).unwrap_or("");
			(!id.is_empty() && !path.is_empty()).then(|| (id.to_string(), path.to_string()))
		})
		.collect()
}

fn operation_target_node_id(op: &Value) -> Option<&str> {
	op.get("target")
		.and_then(|t| t.get("nodeId"))
		.or_else(|| op.get("nodeId"))
		.and_then(|v| v.as_str())
		.filter(|v| !v.is_empty())
}

fn operation_target_path(op: &Value) -> &str {
	op.get("target")
		.and_then(|t| t.get("path"))
		.or_else(|| op.get("path"))
		.and_then(|v| v.as_str())
		.unwrap_or("")
}

fn lookup_operation_targets_all(
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	op: &Value,
) -> Vec<usize> {
	if let Some(node_id) = operation_target_node_id(op) {
		if let Some(&idx) = node_ids.get(node_id) {
			return vec![idx];
		}
		if let Some(path) = registry_paths.get(node_id) {
			let resolved = lookup_scene_path_all(paths, normalized_paths, path);
			if !resolved.is_empty() {
				return resolved;
			}
		}
	}
	lookup_scene_path_all(paths, normalized_paths, operation_target_path(op))
}

fn lookup_operation_target(
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	op: &Value,
) -> Option<usize> {
	lookup_operation_targets_all(node_ids, registry_paths, paths, normalized_paths, op)
		.into_iter()
		.next()
}

fn apply_blend_shape_weight(scene: &mut UnaSceneSnapshot, node_idx: usize, name: &str, value: f32) -> bool {
	let Some(mesh_idx) = scene.nodes.get(node_idx).and_then(|node| node.mesh) else {
		return false;
	};
	let Some(primitives) = scene.meshes.get_mut(mesh_idx) else {
		return false;
	};
	let mut applied = false;
	let normalized = (value / 100.0).clamp(0.0, 1.0);
	for primitive in primitives {
		let Some(target_idx) = primitive.morph_target_names.iter().position(|candidate| candidate == name) else {
			continue;
		};
		if target_idx >= primitive.morph_targets.len() {
			continue;
		}
		if primitive.default_morph_weights.len() < primitive.morph_targets.len() {
			primitive.default_morph_weights.resize(primitive.morph_targets.len(), 0.0);
		}
		primitive.default_morph_weights[target_idx] = normalized;
		applied = true;
	}
	applied
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WardrobeApplyReport {
	pub visibility_applied: usize,
	pub visibility_missing: usize,
	pub blendshape_applied: usize,
	pub blendshape_missing: usize,
	pub missing_visibility_paths: Vec<String>,
	pub missing_blendshapes: Vec<String>,
}

fn apply_unavatar_wardrobe_operations(
	scene: &mut UnaSceneSnapshot,
	operations: &[Value],
	unavatar: Option<&UnaUnavatarExtension>,
) -> WardrobeApplyReport {
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(unavatar);
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let mut report = WardrobeApplyReport::default();
	for op in operations {
		let ty = op.get("type").or_else(|| op.get("op")).and_then(|v| v.as_str()).unwrap_or("");
		let path = operation_target_path(op);
		match ty {
			"subtreeEnabled" | "subtreeVisibility" | "nodeEnabled" | "nodeVisibility" | "rendererEnabled" | "rendererVisibility" => {
				let Some(visible) = op.get("visible").and_then(|v| v.as_bool()) else {
					continue;
				};
				let indices = lookup_operation_targets_all(&node_ids, &registry_paths, &paths, &normalized_paths, op);
				if !indices.is_empty() {
					for idx in indices {
						if let Some(node) = scene.nodes.get_mut(idx) {
							node.visible = visible;
						}
					}
					report.visibility_applied += 1;
				} else {
					report.visibility_missing += 1;
					report.missing_visibility_paths.push(path.to_string());
				}
			}
			"blendShapeWeight" => {
				let Some(value) = op.get("value").and_then(|v| v.as_f64()) else {
					continue;
				};
				let Some(name) = op.get("name").and_then(|v| v.as_str()) else {
					continue;
				};
				if let Some(idx) = lookup_operation_target(&node_ids, &registry_paths, &paths, &normalized_paths, op) {
					if apply_blend_shape_weight(scene, idx, name, value as f32) {
						report.blendshape_applied += 1;
					} else if value.abs() <= 0.001 {
						// Exporter may omit morph targets whose only recorded base value is zero.
						// Applying zero to a missing target is a no-op, not a compatibility failure.
						continue;
					} else {
						report.blendshape_missing += 1;
						report.missing_blendshapes.push(format!("{path}::{name}"));
					}
				} else if value.abs() <= 0.001 {
					continue;
				} else {
					report.blendshape_missing += 1;
					report.missing_blendshapes.push(format!("{path}::{name}"));
				}
			}
			_ => {}
		}
	}
	report
}

fn unavatar_wardrobe_set_operations<'a>(unavatar: &'a UnaUnavatarExtension, set_id: &str) -> Option<&'a [Value]> {
	let wardrobe = unavatar.source.get("wardrobe").and_then(|v| v.as_object())?;
	let sets = wardrobe.get("sets").and_then(|v| v.as_array())?;
	let set = sets.iter().find(|set| set.get("id").and_then(|v| v.as_str()) == Some(set_id))?;
	set.get("operations").and_then(|v| v.as_array()).map(Vec::as_slice)
}

fn unavatar_path_is_same_or_descendant(path: &str, ancestor: &str) -> bool {
	let path = normalize_unavatar_path(path);
	let ancestor = normalize_unavatar_path(ancestor);
	!ancestor.is_empty() && (path == ancestor || path.starts_with(&format!("{ancestor}/")))
}

fn unavatar_base_hidden_subtree_paths(unavatar: &UnaUnavatarExtension) -> Vec<String> {
	let Some(wardrobe) = unavatar.source.get("wardrobe").and_then(|v| v.as_object()) else {
		return Vec::new();
	};
	let base_set = wardrobe.get("baseSet").and_then(|v| v.as_str()).unwrap_or("base");
	let Some(sets) = wardrobe.get("sets").and_then(|v| v.as_array()) else {
		return Vec::new();
	};
	let Some(base) = sets.iter().find(|set| {
		set.get("id").and_then(|v| v.as_str()) == Some(base_set) || set.get("default").and_then(|v| v.as_bool()).unwrap_or(false)
	}) else {
		return Vec::new();
	};
	base.get("operations")
		.and_then(|v| v.as_array())
		.map(|operations| {
			operations
				.iter()
				.filter_map(|op| {
					let ty = op.get("type").or_else(|| op.get("op")).and_then(|v| v.as_str()).unwrap_or("");
					let is_visibility = matches!(
						ty,
						"subtreeEnabled"
							| "subtreeVisibility" | "nodeEnabled"
							| "nodeVisibility" | "rendererEnabled"
							| "rendererVisibility"
					);
					let hidden = op.get("visible").and_then(|v| v.as_bool()) == Some(false);
					let path = operation_target_path(op);
					(is_visibility && hidden && !path.is_empty()).then(|| path.to_owned())
				})
				.collect()
		})
		.unwrap_or_default()
}

fn current_state_operation_is_inherited_hidden_under_base(
	op: &Value,
	base_hidden_paths: &[String],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	paths_by_index: &[Option<String>],
) -> bool {
	let ty = op.get("type").or_else(|| op.get("op")).and_then(|v| v.as_str()).unwrap_or("");
	if !matches!(
		ty,
		"subtreeEnabled" | "subtreeVisibility" | "nodeEnabled" | "nodeVisibility" | "rendererEnabled" | "rendererVisibility"
	) || op.get("visible").and_then(|v| v.as_bool()) != Some(false)
	{
		return false;
	}
	let path = operation_target_path(op);
	if path.is_empty()
		|| !base_hidden_paths
			.iter()
			.any(|hidden| unavatar_path_is_same_or_descendant(path, hidden))
	{
		return false;
	}
	let resolved = lookup_operation_targets_all(node_ids, registry_paths, paths, normalized_paths, op);
	if resolved.is_empty() {
		return true;
	}
	resolved.iter().all(|idx| {
		paths_by_index.get(*idx).and_then(|p| p.as_deref()).is_some_and(|resolved_path| {
			base_hidden_paths
				.iter()
				.any(|hidden| unavatar_path_is_same_or_descendant(resolved_path, hidden))
		})
	})
}

fn apply_unavatar_initial_variant_state(scene: &mut UnaSceneSnapshot, unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	let Some(variants) = unavatar.source.get("variants").and_then(|v| v.as_array()) else {
		return;
	};
	let Some(current_state) = variants.iter().find(|variant| {
		variant.get("source").and_then(|v| v.as_str()) == Some("unity-active-state")
			|| variant.get("id").and_then(|v| v.as_str()) == Some("current-state")
	}) else {
		return;
	};
	let Some(operations) = current_state.get("operations").and_then(|v| v.as_array()) else {
		return;
	};
	let base_hidden_paths = unavatar_base_hidden_subtree_paths(unavatar);
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(Some(unavatar));
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let mut paths_by_index = vec![None; scene.nodes.len()];
	for (path, idx) in &paths {
		if let Some(slot) = paths_by_index.get_mut(*idx) {
			*slot = Some(path.clone());
		}
	}
	let filtered_operations: Vec<Value> = operations
		.iter()
		.filter(|op| {
			!current_state_operation_is_inherited_hidden_under_base(
				op,
				&base_hidden_paths,
				&node_ids,
				&registry_paths,
				&paths,
				&normalized_paths,
				&paths_by_index,
			)
		})
		.cloned()
		.collect();
	let skipped = operations.len().saturating_sub(filtered_operations.len());
	let applied = apply_unavatar_wardrobe_operations(scene, &filtered_operations, Some(unavatar));
	report.push_info(format!(
		".unavatar unity active state: visibility_applied={}, visibility_missing={}, blendshape_applied={}, blendshape_missing={}, inherited_hidden_skipped={}",
		applied.visibility_applied, applied.visibility_missing, applied.blendshape_applied, applied.blendshape_missing, skipped
	));
	if !applied.missing_visibility_paths.is_empty() {
		report.push_info(format!(
			".unavatar unity active state missing visibility paths: {}",
			applied.missing_visibility_paths.join(", ")
		));
	}
}

pub fn apply_unavatar_wardrobe_set(document: &mut UnaDocument, set_id: &str) -> Result<WardrobeApplyReport, String> {
	let Some(unavatar) = document.unavatar.as_ref() else {
		return Err("document has no .unavatar extension".to_string());
	};
	let Some(operations) = unavatar_wardrobe_set_operations(unavatar, set_id) else {
		return Err(format!(".unavatar wardrobe set not found: {set_id}"));
	};
	let Some(scene) = document.scene.as_mut() else {
		return Err("document has no scene".to_string());
	};
	Ok(apply_unavatar_wardrobe_operations(scene, operations, Some(unavatar)))
}

fn apply_unavatar_base_wardrobe(scene: &mut UnaSceneSnapshot, unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	let Some(wardrobe) = unavatar.source.get("wardrobe").and_then(|v| v.as_object()) else {
		return;
	};
	let base_set = wardrobe.get("baseSet").and_then(|v| v.as_str()).unwrap_or("base");
	let Some(sets) = wardrobe.get("sets").and_then(|v| v.as_array()) else {
		return;
	};
	let Some(base) = sets.iter().find(|set| {
		set.get("id").and_then(|v| v.as_str()) == Some(base_set) || set.get("default").and_then(|v| v.as_bool()).unwrap_or(false)
	}) else {
		return;
	};
	let Some(operations) = base.get("operations").and_then(|v| v.as_array()) else {
		return;
	};

	let applied = apply_unavatar_wardrobe_operations(scene, operations, Some(unavatar));
	if applied.visibility_applied > 0 || applied.visibility_missing > 0 || applied.blendshape_applied > 0 || applied.blendshape_missing > 0
	{
		report.push_info(format!(
			".unavatar wardrobe base: visibility_applied={}, visibility_missing={}, blendshape_applied={}, blendshape_missing={}",
			applied.visibility_applied, applied.visibility_missing, applied.blendshape_applied, applied.blendshape_missing
		));
	}
}

fn unavatar_humanoid_profile(
	scene: &UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
	report: &mut ImportReport,
) -> Option<HumanoidProfile> {
	let humanoid = unavatar.source.get("humanoid").and_then(|v| v.as_object())?;
	let paths = scene_node_paths(scene);
	let mut bone_node_indices = BTreeMap::new();
	let mut missing = 0usize;
	for (bone, value) in humanoid {
		let path = value.as_str().or_else(|| value.get("path").and_then(|v| v.as_str())).unwrap_or("");
		if path.is_empty() {
			continue;
		}
		if let Some(&idx) = paths.get(path) {
			bone_node_indices.insert(bone.to_ascii_lowercase(), idx);
		} else {
			missing += 1;
		}
	}
	if bone_node_indices.is_empty() {
		if missing > 0 {
			report.push_info(format!(".unavatar humanoid: no bones resolved, missing_targets={missing}"));
		}
		return None;
	}
	report.push_info(format!(
		".unavatar humanoid: resolved_bones={}, missing_targets={missing}",
		bone_node_indices.len()
	));
	Some(HumanoidProfile { bone_node_indices })
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
			let extras = unavatar_material_extras(&m);
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
			let unavatar_mtoon = extras.as_ref().and_then(unavatar_mtoon_from_extras);
			let shading = if unavatar_mtoon.is_some() {
				UnaShadingModel::MToonLike
			} else if m.unlit() {
				UnaShadingModel::Unlit
			} else {
				UnaShadingModel::LitLambert
			};
			let alpha_cutoff_opt = m.alpha_cutoff();
			let alpha_cutoff = alpha_cutoff_opt.unwrap_or(0.5);
			let gltf_alpha_mode = match m.alpha_mode() {
				gltf::material::AlphaMode::Opaque => UnaAlphaMode::Opaque,
				gltf::material::AlphaMode::Mask => UnaAlphaMode::Mask,
				gltf::material::AlphaMode::Blend => UnaAlphaMode::Blend,
			};
			let alpha_mode = unavatar_material_inferred_alpha_mode(extras.as_ref(), gltf_alpha_mode, alpha_cutoff_opt, tex.is_some())
				.unwrap_or(gltf_alpha_mode);
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
				mtoon: unavatar_mtoon,
				unavatar_material: extras,
			}
		})
		.collect()
}

fn unavatar_material_extras(material: &gltf::Material<'_>) -> Option<Value> {
	let raw = material.extras().as_ref()?;
	let value = serde_json::from_str::<Value>(raw.get()).ok()?;
	value.get("UN_avatar_material").cloned()
}

fn unavatar_node_id(node: &gltf::Node<'_>) -> Option<String> {
	let raw = node.extras().as_ref()?;
	let value = serde_json::from_str::<Value>(raw.get()).ok()?;
	value
		.get("UN_avatar_node")
		.and_then(|node| node.get("nodeId"))
		.and_then(|v| v.as_str())
		.filter(|id| !id.is_empty())
		.map(str::to_string)
}

fn unavatar_material_inferred_alpha_mode(
	extras: Option<&Value>,
	_gltf_alpha_mode: UnaAlphaMode,
	_alpha_cutoff: Option<f32>,
	_has_base_color_texture: bool,
) -> Option<UnaAlphaMode> {
	let extras = extras?;
	let family = extras.get("family").and_then(|v| v.as_str()).unwrap_or("");
	let source_shader = extras.get("sourceShader").and_then(|v| v.as_str()).unwrap_or("");
	if !family.eq_ignore_ascii_case("liltoon") && !source_shader.to_ascii_lowercase().contains("liltoon") {
		return None;
	}

	let shader = source_shader.to_ascii_lowercase();
	if let Some(render_queue) = json_i32(extras.get("renderQueue").or_else(|| extras.get("render_queue"))) {
		if render_queue >= 3000 {
			return Some(UnaAlphaMode::Blend);
		}
		if (2450..3000).contains(&render_queue) {
			return Some(UnaAlphaMode::Mask);
		}
	}
	if shader.contains("cutout") {
		Some(UnaAlphaMode::Mask)
	} else if shader.contains("transparent") || shader.contains("refraction") || shader.contains("fur") {
		Some(UnaAlphaMode::Blend)
	} else {
		None
	}
}

fn unavatar_material_is_ordinary_liltoon(material: &UnaMaterialPbr) -> bool {
	let Some(extras) = material.unavatar_material.as_ref() else {
		return false;
	};
	let family = extras.get("family").and_then(|v| v.as_str()).unwrap_or("");
	let source_shader = extras.get("sourceShader").and_then(|v| v.as_str()).unwrap_or("");
	if !family.eq_ignore_ascii_case("liltoon") && !source_shader.to_ascii_lowercase().contains("liltoon") {
		return false;
	}
	let shader = source_shader.to_ascii_lowercase();
	!(shader.contains("cutout") || shader.contains("transparent") || shader.contains("refraction") || shader.contains("fur"))
}

fn image_alpha_has_transparency(image: &UnaImageRgba) -> bool {
	match image.pixel_format {
		UnaImagePixelFormat::R8G8 | UnaImagePixelFormat::R8G8B8A8 => image.rgba8_compat_pixels().chunks_exact(4).any(|px| px[3] < 255),
		UnaImagePixelFormat::R16G16B16A16 | UnaImagePixelFormat::R16G16B16A16Float | UnaImagePixelFormat::R32G32B32A32Float => {
			image.rgba8_compat_pixels().chunks_exact(4).any(|px| px[3] < 255)
		}
		_ => false,
	}
}

fn image_alpha_has_translucency(image: &UnaImageRgba) -> bool {
	match image.pixel_format {
		UnaImagePixelFormat::R8G8 | UnaImagePixelFormat::R8G8B8A8 => {
			image.rgba8_compat_pixels().chunks_exact(4).any(|px| px[3] > 0 && px[3] < 255)
		}
		UnaImagePixelFormat::R16G16B16A16 | UnaImagePixelFormat::R16G16B16A16Float | UnaImagePixelFormat::R32G32B32A32Float => {
			image.rgba8_compat_pixels().chunks_exact(4).any(|px| px[3] > 0 && px[3] < 255)
		}
		_ => false,
	}
}

fn refine_liltoon_alpha_from_images(materials: &mut [UnaMaterialPbr], images: &[UnaImageRgba]) {
	for material in materials {
		if material.alpha_mode != UnaAlphaMode::Mask || material.alpha_cutoff > 0.5 || !unavatar_material_is_ordinary_liltoon(material) {
			continue;
		}
		let Some(image) = material.base_color_texture_index.and_then(|index| images.get(index)) else {
			continue;
		};
		let has_transparent_alpha = image_alpha_has_transparency(image);
		if !has_transparent_alpha {
			material.alpha_mode = UnaAlphaMode::Opaque;
			continue;
		}
		if material.alpha_cutoff <= 0.01 && image_alpha_has_translucency(image) {
			material.alpha_mode = UnaAlphaMode::Blend;
		}
	}
}

fn unavatar_mtoon_from_extras(extras: &Value) -> Option<UnaMtoonMaterial> {
	let mtoon = extras.get("mtoon")?;
	let family = extras.get("family").and_then(|v| v.as_str()).unwrap_or("");
	let source_shader = extras.get("sourceShader").and_then(|v| v.as_str()).unwrap_or("");
	let outline_width_unit = mtoon
		.get("outlineWidthFactorUnit")
		.or_else(|| mtoon.get("outline_width_factor_unit"))
		.and_then(|v| v.as_str())
		.unwrap_or("");
	let liltoon_outline_width_scale = if outline_width_unit.eq_ignore_ascii_case("meters") {
		1.0
	} else if family.eq_ignore_ascii_case("liltoon") || source_shader.to_ascii_lowercase().contains("liltoon") {
		0.01
	} else {
		1.0
	};
	let mut out = UnaMtoonMaterial::default();
	if let Some(value) = json_bool(mtoon.get("transparentWithZWrite").or_else(|| mtoon.get("transparent_with_z_write"))) {
		out.transparent_with_z_write = value;
	}
	if let Some(value) = json_vec3(mtoon.get("shadeColorFactor").or_else(|| mtoon.get("shade_color_factor"))) {
		out.shade_color_factor = value;
	}
	if let Some(value) = json_usize(
		mtoon
			.get("shadeMultiplyTextureIndex")
			.or_else(|| mtoon.get("shade_multiply_texture_index")),
	) {
		out.shade_multiply_texture_index = Some(value);
	}
	if let Some(value) = json_f32(mtoon.get("shadingShiftFactor").or_else(|| mtoon.get("shading_shift_factor"))) {
		out.shading_shift_factor = value;
	}
	if let Some(value) = json_f32(mtoon.get("shadingToonyFactor").or_else(|| mtoon.get("shading_toony_factor"))) {
		out.shading_toony_factor = value;
	}
	if let Some(value) = json_vec3(mtoon.get("matcapFactor").or_else(|| mtoon.get("matcap_factor"))) {
		out.matcap_factor = value;
	}
	if let Some(value) = json_usize(mtoon.get("matcapTextureIndex").or_else(|| mtoon.get("matcap_texture_index"))) {
		out.matcap_texture_index = Some(value);
	}
	if let Some(value) = json_vec3(
		mtoon
			.get("parametricRimColorFactor")
			.or_else(|| mtoon.get("parametric_rim_color_factor")),
	) {
		out.parametric_rim_color_factor = value;
	}
	if let Some(value) = json_usize(
		mtoon
			.get("rimMultiplyTextureIndex")
			.or_else(|| mtoon.get("rim_multiply_texture_index")),
	) {
		out.rim_multiply_texture_index = Some(value);
	}
	if let Some(value) = json_usize(
		mtoon
			.get("reflectionCubeTextureIndex")
			.or_else(|| mtoon.get("reflection_cube_texture_index")),
	) {
		out.reflection_cube_texture_index = Some(value);
	}
	if let Some(value) = json_f32(mtoon.get("rimLightingMixFactor").or_else(|| mtoon.get("rim_lighting_mix_factor"))) {
		out.rim_lighting_mix_factor = value;
	}
	if let Some(value) = json_f32(
		mtoon
			.get("parametricRimFresnelPowerFactor")
			.or_else(|| mtoon.get("parametric_rim_fresnel_power_factor")),
	) {
		out.parametric_rim_fresnel_power_factor = value;
	}
	if let Some(value) = json_f32(
		mtoon
			.get("parametricRimLiftFactor")
			.or_else(|| mtoon.get("parametric_rim_lift_factor")),
	) {
		out.parametric_rim_lift_factor = value;
	}
	if let Some(value) = mtoon
		.get("outlineWidthMode")
		.or_else(|| mtoon.get("outline_width_mode"))
		.and_then(|v| v.as_str())
	{
		out.outline_width_mode = match value {
			"world_coordinates" | "world" => UnaMtoonOutlineWidthMode::WorldCoordinates,
			"screen_coordinates" | "screen" => UnaMtoonOutlineWidthMode::ScreenCoordinates,
			_ => UnaMtoonOutlineWidthMode::None,
		};
	}
	if let Some(value) = json_f32(mtoon.get("outlineWidthFactor").or_else(|| mtoon.get("outline_width_factor"))) {
		out.outline_width_factor = value * liltoon_outline_width_scale;
	}
	if let Some(value) = json_usize(
		mtoon
			.get("outlineWidthMultiplyTextureIndex")
			.or_else(|| mtoon.get("outline_width_multiply_texture_index")),
	) {
		out.outline_width_multiply_texture_index = Some(value);
	}
	if let Some(value) = json_vec3(mtoon.get("outlineColorFactor").or_else(|| mtoon.get("outline_color_factor"))) {
		out.outline_color_factor = value;
	}
	if let Some(value) = json_f32(
		mtoon
			.get("outlineLightingMixFactor")
			.or_else(|| mtoon.get("outline_lighting_mix_factor")),
	) {
		out.outline_lighting_mix_factor = value;
	}
	Some(out)
}

fn json_bool(value: Option<&Value>) -> Option<bool> {
	value.and_then(Value::as_bool)
}

fn json_f32(value: Option<&Value>) -> Option<f32> {
	value.and_then(Value::as_f64).map(|v| v as f32)
}

fn json_usize(value: Option<&Value>) -> Option<usize> {
	value.and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok())
}

fn json_i32(value: Option<&Value>) -> Option<i32> {
	value
		.and_then(|v| v.as_i64().or_else(|| v.as_u64().and_then(|u| i64::try_from(u).ok())))
		.and_then(|v| i32::try_from(v).ok())
}

fn json_vec3(value: Option<&Value>) -> Option<[f32; 3]> {
	let array = value?.as_array()?;
	let x = array.first()?.as_f64()? as f32;
	let y = array.get(1)?.as_f64()? as f32;
	let z = array.get(2)?.as_f64()? as f32;
	Some([x, y, z])
}

fn mesh_target_names(mesh: gltf::Mesh<'_>) -> Vec<String> {
	let Some(raw) = mesh.extras().as_ref() else {
		return Vec::new();
	};
	let Ok(value) = serde_json::from_str::<Value>(raw.get()) else {
		return Vec::new();
	};
	value
		.get("targetNames")
		.and_then(|v| v.as_array())
		.map(|names| names.iter().filter_map(|name| name.as_str().map(str::to_owned)).collect())
		.unwrap_or_default()
}

fn read_primitive(
	prim: gltf::Primitive<'_>,
	buffers: &[gltf::buffer::Data],
	mesh_weights: Option<&[f32]>,
	mesh_target_names: &[String],
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
	let morph_target_names = if mesh_target_names.len() == morph_targets.len() {
		mesh_target_names.to_vec()
	} else {
		Vec::new()
	};

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
		morph_target_names,
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

	let image_sources = collect_image_source_metadata(document, buffers);
	let images = collect_images(image_data, report).map_err(ImportError::Message)?;
	refine_liltoon_alpha_from_images(&mut materials, &images);

	let skins = build_skins(document, buffers)?;

	let mut meshes: Vec<Vec<UnaMeshBuffers>> = vec![Vec::new(); document.meshes().len()];
	for mesh in document.meshes() {
		let mid = mesh.index();
		let mw = mesh.weights();
		let target_names = mesh_target_names(mesh.clone());
		for prim in mesh.primitives() {
			if let Some(buf) = read_primitive(prim, buffers, mw, &target_names, report)? {
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
			source_node_id: unavatar_node_id(&node),
			visible: true,
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
		image_sources,
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
			display_name: "glTF 2.0 / .unavatar".to_owned(),
			extensions: vec!["gltf".to_owned(), "glb".to_owned(), "unavatar".to_owned()],
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
		if s.ends_with(".unavatar") {
			return ImportProbeResult { confidence: 255 };
		}
		if s.ends_with(".glb") {
			return ImportProbeResult { confidence: 254 };
		}
		if s.ends_with(".gltf") {
			return ImportProbeResult { confidence: 240 };
		}
		ImportProbeResult { confidence: 0 }
	}

	fn import(&self, _ctx: &mut ImportContext, input: ImportInput, _options: ImportOptions) -> Result<ImportResult, ImportError> {
		let mut root_json: Option<Value> = None;
		let mut original_image_sources: Option<Vec<Option<UnaImageSourceMetadata>>> = None;
		let mut original_glb_bin: Option<Vec<u8>> = None;
		let (path_hint, document, buffers, image_data) = match input {
			ImportInput::Path(path) => {
				if path
					.extension()
					.and_then(|e| e.to_str())
					.is_some_and(|e| e.eq_ignore_ascii_case("unavatar"))
				{
					let bytes = std::fs::read(&path).map_err(|e| ImportError::Message(format!("{}: {e}", path.display())))?;
					if bytes.starts_with(b"glTF") {
						let (root, bin) = read_glb_json_and_bin(&bytes)?;
						original_image_sources = Some(collect_glb_image_source_metadata(&root, &bin));
						original_glb_bin = Some(bin);
						root_json = Some(root);
					} else {
						root_json = Some(gltf_root_json_from_bytes(&bytes)?);
					}
					let import_bytes = normalize_webp_glb_for_gltf_import(&bytes)?;
					let imported = gltf::import_slice(import_bytes.as_ref()).map_err(|e| ImportError::Message(e.to_string()))?;
					(Some(path), imported.0, imported.1, imported.2)
				} else {
					let imported = gltf::import(&path).map_err(|e| ImportError::Message(e.to_string()))?;
					(Some(path), imported.0, imported.1, imported.2)
				}
			}
			ImportInput::Bytes { bytes, path_hint } => {
				if path_hint
					.as_ref()
					.and_then(|p| p.extension().and_then(|e| e.to_str()))
					.is_some_and(|e| e.eq_ignore_ascii_case("unavatar"))
				{
					if bytes.as_ref().starts_with(b"glTF") {
						let (root, bin) = read_glb_json_and_bin(bytes.as_ref())?;
						original_image_sources = Some(collect_glb_image_source_metadata(&root, &bin));
						original_glb_bin = Some(bin);
						root_json = Some(root);
					} else {
						root_json = Some(gltf_root_json_from_bytes(bytes.as_ref())?);
					}
					let import_bytes = normalize_webp_glb_for_gltf_import(bytes.as_ref())?;
					let imported = gltf::import_slice(import_bytes.as_ref()).map_err(|e| ImportError::Message(e.to_string()))?;
					(path_hint, imported.0, imported.1, imported.2)
				} else {
					let imported = gltf::import_slice(bytes.as_ref()).map_err(|e| ImportError::Message(e.to_string()))?;
					(path_hint, imported.0, imported.1, imported.2)
				}
			}
		};

		let mut report = ImportReport {
			source_format: Some(self.descriptor().id.clone()),
			..Default::default()
		};

		let mut scene = scene_snapshot_from_gltf(&document, &buffers, image_data, &mut report)?;
		if let Some(original_image_sources) = original_image_sources {
			scene.image_sources = original_image_sources;
		}
		if let (Some(root), Some(bin)) = (root_json.as_ref(), original_glb_bin.as_deref()) {
			let asset_map = append_unavatar_texture_assets(&mut scene, root, bin, &mut report);
			apply_unavatar_material_texture_asset_refs(&mut scene, root, &asset_map);
		}
		let unavatar = root_json.as_ref().and_then(unavatar_extension_from_root);
		if let Some(unavatar) = &unavatar {
			apply_unavatar_initial_variant_state(&mut scene, unavatar, &mut report);
			apply_unavatar_base_wardrobe(&mut scene, unavatar, &mut report);
		}
		let humanoid_profile = unavatar
			.as_ref()
			.and_then(|unavatar| unavatar_humanoid_profile(&scene, unavatar, &mut report));

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
		if let Some(unavatar) = &unavatar {
			report.push_info(format!(".unavatar: UN_avatar specVersion={}", unavatar.spec_version));
		}

		Ok(ImportResult {
			document: UnaDocument {
				scene: Some(scene),
				unavatar,
				humanoid_profile,
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

	fn glb_bytes_with_bin(json: &str, bin: &[u8]) -> Vec<u8> {
		let mut json_bytes = json.as_bytes().to_vec();
		while json_bytes.len() % 4 != 0 {
			json_bytes.push(b' ');
		}
		let mut bin_bytes = bin.to_vec();
		while bin_bytes.len() % 4 != 0 {
			bin_bytes.push(0);
		}
		let bin_len = if bin_bytes.is_empty() { 0 } else { 8 + bin_bytes.len() };
		let total_len = 12 + 8 + json_bytes.len() + bin_len;
		let mut out = Vec::with_capacity(total_len);
		out.extend_from_slice(b"glTF");
		out.extend_from_slice(&2u32.to_le_bytes());
		out.extend_from_slice(&(total_len as u32).to_le_bytes());
		out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
		out.extend_from_slice(b"JSON");
		out.extend_from_slice(&json_bytes);
		if !bin_bytes.is_empty() {
			out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
			out.extend_from_slice(b"BIN\0");
			out.extend_from_slice(&bin_bytes);
		}
		out
	}

	#[test]
	fn converts_r16_rgb_pixels_to_rgba8() {
		let pixels = [0u16.to_ne_bytes(), 32768u16.to_ne_bytes(), 65535u16.to_ne_bytes()].concat();
		let image = UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: UnaImagePixelFormat::R16G16B16,
			pixels,
		};
		assert_eq!(image.rgba8_compat_pixels().as_ref(), &[0, 128, 255, 255]);
	}

	#[test]
	fn converts_r32_float_rgba_pixels_to_rgba8() {
		let pixels = [
			0.0_f32.to_ne_bytes(),
			0.5_f32.to_ne_bytes(),
			1.0_f32.to_ne_bytes(),
			2.0_f32.to_ne_bytes(),
		]
		.concat();
		let image = UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: UnaImagePixelFormat::R32G32B32A32Float,
			pixels,
		};
		assert_eq!(image.rgba8_compat_pixels().as_ref(), &[0, 128, 255, 255]);
	}

	#[test]
	fn collects_glb_image_source_metadata_without_decoding() {
		let root: Value = serde_json::from_str(
			r#"{
				"images": [{"name": "main", "bufferView": 0, "mimeType": "image/png"}],
				"bufferViews": [{"buffer": 0, "byteOffset": 4, "byteLength": 3}]
			}"#,
		)
		.unwrap();
		let metadata = collect_glb_image_source_metadata(&root, &[0, 0, 0, 0, 1, 2, 3, 4]);
		assert_eq!(metadata.len(), 1);
		let source = metadata[0].as_ref().unwrap();
		assert_eq!(source.name.as_deref(), Some("main"));
		assert_eq!(source.mime_type.as_deref(), Some("image/png"));
		assert_eq!(source.byte_length, 3);
		assert_eq!(source.source_hash, fnv1a64(&[1, 2, 3]));
	}

	#[test]
	fn decodes_unavatar_exr_texture_asset() {
		let image = image::Rgba32FImage::from_raw(1, 1, vec![0.25, 0.5, 1.0, 1.0]).unwrap();
		let mut exr = Vec::new();
		image::DynamicImage::ImageRgba32F(image)
			.write_to(&mut Cursor::new(&mut exr), image::ImageFormat::OpenExr)
			.unwrap();
		let decoded = decode_unavatar_texture_asset(&exr, "image/exr", Some("RGBA16F"), Some("rgba")).unwrap();
		assert_eq!(decoded.width, 1);
		assert_eq!(decoded.height, 1);
		assert_eq!(decoded.pixel_format, UnaImagePixelFormat::R16G16B16A16Float);
		assert_eq!(decoded.pixels.len(), 8);
	}

	#[test]
	fn decodes_unavatar_rgb_exr_texture_asset_without_alpha_channel() {
		let image = image::Rgb32FImage::from_raw(1, 1, vec![0.25, 0.5, 1.0]).unwrap();
		let mut exr = Vec::new();
		image::DynamicImage::ImageRgb32F(image)
			.write_to(&mut Cursor::new(&mut exr), image::ImageFormat::OpenExr)
			.unwrap();
		let decoded = decode_unavatar_texture_asset(&exr, "image/exr", Some("RGB16F"), Some("rgb")).unwrap();
		assert_eq!(decoded.width, 1);
		assert_eq!(decoded.height, 1);
		assert_eq!(decoded.pixel_format, UnaImagePixelFormat::R16G16B16Float);
		assert_eq!(decoded.pixels.len(), 6);
	}

	#[test]
	fn imports_unavatar_exr_texture_asset_and_material_ref() {
		let exr_image = image::Rgba32FImage::from_raw(1, 1, vec![0.25, 0.5, 1.0, 1.0]).unwrap();
		let mut exr = Vec::new();
		image::DynamicImage::ImageRgba32F(exr_image)
			.write_to(&mut Cursor::new(&mut exr), image::ImageFormat::OpenExr)
			.unwrap();
		let mut bin = vec![0; 12];
		let exr_offset = bin.len();
		bin.extend_from_slice(&exr);
		let json = format!(
			r#"{{
				"asset": {{"version": "2.0"}},
				"scene": 0,
				"scenes": [{{"nodes": [0]}}],
				"meshes": [{{
					"primitives": [{{
						"attributes": {{"POSITION": 0}},
						"material": 0
					}}]
				}}],
				"materials": [{{
					"name": "toon material",
					"pbrMetallicRoughness": {{"baseColorFactor": [1, 1, 1, 1]}},
					"extras": {{
						"UN_avatar_material": {{
							"sourceShader": "lilToon",
							"family": "liltoon",
							"unMaterialModel": "UNToon",
							"mtoon": {{
								"matcapTextureIndexAsset": "matcap-exr"
							}}
						}}
					}}
				}}],
				"accessors": [
					{{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3", "min": [0, 0, 0], "max": [0, 0, 0]}}
				],
				"bufferViews": [
					{{"buffer": 0, "byteOffset": 0, "byteLength": 12}},
					{{"buffer": 0, "byteOffset": {exr_offset}, "byteLength": {exr_len}}}
				],
				"buffers": [{{"byteLength": {bin_len}}}],
				"nodes": [
					{{"name": "root", "mesh": 0}}
				],
				"extensionsUsed": ["UN_avatar"],
				"extensions": {{
					"UN_avatar": {{
						"specVersion": "0.1-preview",
						"textureAssets": [{{
							"id": "matcap-exr",
							"name": "matcap",
							"mimeType": "image/exr",
							"sourcePixelFormat": "RGBA32F",
							"colorSpace": "linear",
							"channels": "rgba",
							"bufferView": 1
						}}]
					}}
				}}
			}}"#,
			exr_offset = exr_offset,
			exr_len = exr.len(),
			bin_len = bin.len()
		);
		let bytes = glb_bytes_with_bin(&json, &bin);
		let imp = GltfImporter;
		let mut ctx = ImportContext::dummy();
		let got = imp
			.import(
				&mut ctx,
				ImportInput::Bytes {
					bytes: bytes.into(),
					path_hint: Some(std::path::PathBuf::from("asset.unavatar")),
				},
				ImportOptions,
			)
			.unwrap();
		let scene = got.document.scene.as_ref().unwrap();
		assert_eq!(scene.images.len(), 1);
		assert_eq!(scene.image_sources[0].as_ref().unwrap().mime_type.as_deref(), Some("image/exr"));
		assert_eq!(
			scene.image_sources[0].as_ref().unwrap().source_pixel_format.as_deref(),
			Some("RGBA32F")
		);
		assert_eq!(scene.image_sources[0].as_ref().unwrap().channels.as_deref(), Some("rgba"));
		assert_eq!(scene.materials[0].mtoon.as_ref().unwrap().matcap_texture_index, Some(0));
	}

	#[test]
	fn imports_unavatar_reflection_texture_asset_ref() {
		let exr_image = image::Rgba32FImage::from_raw(1, 1, vec![1.0, 0.5, 0.25, 1.0]).unwrap();
		let mut exr = Vec::new();
		image::DynamicImage::ImageRgba32F(exr_image)
			.write_to(&mut Cursor::new(&mut exr), image::ImageFormat::OpenExr)
			.unwrap();
		let mut bin = vec![0; 12];
		let exr_offset = bin.len();
		bin.extend_from_slice(&exr);
		let json = format!(
			r#"{{
				"asset": {{"version": "2.0"}},
				"scene": 0,
				"scenes": [{{"nodes": [0]}}],
				"meshes": [{{
					"primitives": [{{
						"attributes": {{"POSITION": 0}},
						"material": 0
					}}]
				}}],
				"materials": [{{
					"name": "toon material",
					"pbrMetallicRoughness": {{"baseColorFactor": [1, 1, 1, 1]}},
					"extras": {{
						"UN_avatar_material": {{
							"sourceShader": "lilToon",
							"family": "liltoon",
							"unMaterialModel": "UNToon",
							"mtoon": {{
								"reflectionCubeTextureIndexAsset": "reflection-exr"
							}}
						}}
					}}
				}}],
				"accessors": [
					{{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3", "min": [0, 0, 0], "max": [0, 0, 0]}}
				],
				"bufferViews": [
					{{"buffer": 0, "byteOffset": 0, "byteLength": 12}},
					{{"buffer": 0, "byteOffset": {exr_offset}, "byteLength": {exr_len}}}
				],
				"buffers": [{{"byteLength": {bin_len}}}],
				"nodes": [
					{{"name": "root", "mesh": 0}}
				],
				"extensionsUsed": ["UN_avatar"],
				"extensions": {{
					"UN_avatar": {{
						"specVersion": "0.1-preview",
						"textureAssets": [{{
							"id": "reflection-exr",
							"name": "reflection",
							"mimeType": "image/exr",
							"bufferView": 1
						}}]
					}}
				}}
			}}"#,
			exr_offset = exr_offset,
			exr_len = exr.len(),
			bin_len = bin.len()
		);
		let imp = GltfImporter;
		let mut ctx = ImportContext::dummy();
		let got = imp
			.import(
				&mut ctx,
				ImportInput::Bytes {
					bytes: glb_bytes_with_bin(&json, &bin).into(),
					path_hint: Some(std::path::PathBuf::from("reflection.unavatar")),
				},
				ImportOptions,
			)
			.unwrap();
		let scene = got.document.scene.as_ref().unwrap();
		assert_eq!(scene.materials[0].mtoon.as_ref().unwrap().reflection_cube_texture_index, Some(0));
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

	#[test]
	fn imports_unavatar_extension_from_glb() {
		let dir = std::env::temp_dir().join(format!("un-avatar-unavatar-test-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("empty.unavatar");
		let json = r#"{
			"asset": {"version": "2.0"},
			"scene": 0,
			"scenes": [{"nodes": [0]}],
			"meshes": [{
				"extras": {"targetNames": ["Shrink"]},
				"primitives": [{
					"attributes": {"POSITION": 0},
					"material": 0,
					"targets": [{"POSITION": 1}]
				}]
			}],
			"materials": [{
				"name": "toon material",
				"pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1]},
				"extras": {
					"UN_avatar_material": {
						"sourceShader": "lilToon",
						"family": "liltoon",
						"unMaterialModel": "UNToon",
						"mtoon": {
							"shadeColorFactor": [0.7, 0.8, 0.9],
							"shadingShiftFactor": -0.1,
							"outlineWidthMode": "world_coordinates",
							"outlineWidthFactor": 0.03
						}
					}
				}
			}],
			"accessors": [
				{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3", "min": [0, 0, 0], "max": [0, 0, 0]},
				{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3"}
			],
			"bufferViews": [
				{"buffer": 0, "byteOffset": 0, "byteLength": 12},
				{"buffer": 0, "byteOffset": 0, "byteLength": 12}
			],
			"buffers": [{"byteLength": 12}],
			"nodes": [
				{"name": "root", "children": [1, 2]},
				{
					"name": "Hidden",
					"mesh": 0,
					"children": [3],
					"extras": {"UN_avatar_node": {"nodeId": "node_hidden", "path": "Hidden"}}
				},
				{"name": "UnityInactive", "mesh": 0},
				{
					"name": "HiddenChild",
					"mesh": 0,
					"extras": {"UN_avatar_node": {"nodeId": "node_hidden_child", "path": "Hidden/HiddenChild"}}
				}
			],
			"extensionsUsed": ["UN_avatar"],
			"extensions": {
				"UN_avatar": {
					"specVersion": "0.1-preview",
					"variants": [{
						"id": "current-state",
						"source": "unity-active-state",
						"operations": [{
							"op": "nodeEnabled",
							"path": "UnityInactive",
							"visible": false
						}, {
							"op": "nodeEnabled",
							"path": "Hidden/HiddenChild",
							"visible": false
						}]
					}],
					"humanoid": {
						"Hips": "Hidden"
					},
					"wardrobe": {
						"baseSet": "base",
						"sets": [{
							"id": "base",
							"operations": [
								{
									"type": "subtreeEnabled",
									"target": {"nodeId": "node_hidden", "path": "Wrong Path"},
									"visible": false
								},
								{
									"type": "subtreeEnabled",
									"target": {"nodeId": "node_hidden_child", "path": "Wrong Path"},
									"visible": false
								},
								{
									"type": "subtreeEnabled",
									"target": {"path": "Hidden"},
									"visible": false
								},
								{
									"type": "blendShapeWeight",
									"target": {"nodeId": "node_hidden", "path": "Wrong Path"},
									"name": "Shrink",
									"value": 50
								}
							]
						}, {
							"id": "visible",
							"operations": [
								{
									"type": "subtreeEnabled",
									"target": {"nodeId": "node_hidden", "path": "Wrong Path"},
									"visible": true
								},
								{
									"type": "blendShapeWeight",
									"target": {"nodeId": "node_hidden", "path": "Wrong Path"},
									"name": "Shrink",
									"value": 0
								}
							]
						}]
					}
				}
			}
		}"#;
		std::fs::write(&path, glb_bytes_with_bin(json, &[0; 12])).unwrap();

		let imp = GltfImporter;
		let mut ctx = ImportContext::dummy();
		let mut got = imp.import(&mut ctx, ImportInput::Path(path), ImportOptions).unwrap();
		let unavatar = got.document.unavatar.as_ref().expect("UN_avatar extension");
		assert_eq!(unavatar.spec_version, "0.1-preview");
		assert!(unavatar.source.get("wardrobe").is_some());
		let scene = got.document.scene.as_ref().unwrap();
		assert!(scene.nodes[0].visible);
		assert_eq!(scene.nodes[1].source_node_id.as_deref(), Some("node_hidden"));
		assert!(!scene.nodes[1].visible);
		assert!(!scene.nodes[3].visible);
		assert!(!scene.nodes[2].visible);
		assert_eq!(scene.meshes[0][0].morph_target_names, vec!["Shrink"]);
		assert_eq!(scene.meshes[0][0].default_morph_weights, vec![0.5]);
		assert_eq!(scene.materials[0].shading, UnaShadingModel::MToonLike);
		let mtoon = scene.materials[0].mtoon.as_ref().unwrap();
		assert_eq!(mtoon.shade_color_factor, [0.7, 0.8, 0.9]);
		assert_eq!(mtoon.shading_shift_factor, -0.1);
		assert_eq!(mtoon.outline_width_mode, UnaMtoonOutlineWidthMode::WorldCoordinates);
		assert!((mtoon.outline_width_factor - 0.0003).abs() < 1e-8);
		let applied = apply_unavatar_wardrobe_set(&mut got.document, "visible").unwrap();
		assert_eq!(applied.visibility_applied, 1);
		assert_eq!(applied.visibility_missing, 0);
		assert_eq!(applied.blendshape_applied, 1);
		assert_eq!(applied.blendshape_missing, 0);
		let scene = got.document.scene.as_ref().unwrap();
		assert!(scene.nodes[1].visible);
		assert!(!scene.nodes[3].visible);
		assert_eq!(scene.meshes[0][0].default_morph_weights, vec![0.0]);
		let humanoid = got.document.humanoid_profile.as_ref().expect("humanoid profile");
		assert_eq!(humanoid.bone_node_indices.get("hips"), Some(&1));
		assert!(got.report.messages.iter().any(|m| m.contains("UN_avatar specVersion=0.1-preview")));
		assert!(got
			.report
			.messages
			.iter()
			.any(|m| m.contains(".unavatar unity active state: visibility_applied=1")));
		assert!(got.report.messages.iter().any(|m| m.contains("inherited_hidden_skipped=1")));
		assert!(got
			.report
			.messages
			.iter()
			.any(|m| m.contains(".unavatar humanoid: resolved_bones=1")));
		let _ = std::fs::remove_dir_all(&dir);
	}

	#[test]
	fn infers_liltoon_alpha_mode_from_source_shader() {
		let transparent = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonTransparentOutline"
		});
		let cutout = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonCutout"
		});
		let opaque = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "lilToon"
		});
		let queue_cutout = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "lilToon",
			"renderQueue": 2450
		});
		let queue_transparent = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "lilToon",
			"renderQueue": 3000
		});

		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&transparent), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Blend)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&cutout), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Mask)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&opaque), UnaAlphaMode::Opaque, None, true),
			None
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&opaque), UnaAlphaMode::Opaque, Some(0.001), true),
			None
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&opaque), UnaAlphaMode::Mask, Some(0.001), true),
			None
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&opaque), UnaAlphaMode::Mask, Some(0.5), true),
			None
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&opaque), UnaAlphaMode::Opaque, Some(0.5), true),
			None
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&queue_cutout), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Mask)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&queue_transparent), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Blend)
		);
	}

	#[test]
	fn refines_ordinary_liltoon_mask_from_texture_alpha_shape() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonOutline"
		});
		let opaque_image = UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 255],
		};
		let transparent_image = UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 0],
		};
		let translucent_image = UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 128],
		};
		let mut materials = vec![
			UnaMaterialPbr {
				alpha_mode: UnaAlphaMode::Mask,
				alpha_cutoff: 0.5,
				base_color_texture_index: Some(0),
				unavatar_material: Some(extras.clone()),
				..Default::default()
			},
			UnaMaterialPbr {
				alpha_mode: UnaAlphaMode::Mask,
				alpha_cutoff: 0.5,
				base_color_texture_index: Some(1),
				unavatar_material: Some(extras),
				..Default::default()
			},
			UnaMaterialPbr {
				alpha_mode: UnaAlphaMode::Mask,
				alpha_cutoff: 0.001,
				base_color_texture_index: Some(2),
				unavatar_material: Some(serde_json::json!({
					"family": "liltoon",
					"sourceShader": "Hidden/lilToonOutline"
				})),
				..Default::default()
			},
		];

		refine_liltoon_alpha_from_images(&mut materials, &[opaque_image, transparent_image, translucent_image]);

		assert_eq!(materials[0].alpha_mode, UnaAlphaMode::Opaque);
		assert_eq!(materials[1].alpha_mode, UnaAlphaMode::Mask);
		assert_eq!(materials[2].alpha_mode, UnaAlphaMode::Blend);
	}
}
