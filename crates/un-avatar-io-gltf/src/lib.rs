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
	Approximation, ReportStatus, UnaAlphaMode, UnaBounds, UnaCullMode, UnaDocument, UnaImagePixelFormat, UnaImageRgba,
	UnaImageSourceMetadata, UnaLilToonLikeBlendMode, UnaLilToonLikeMaterial, UnaLilToonLikeSourceProfile, UnaMaterialPbr, UnaMeshBuffers,
	UnaMorphTargetDeltas, UnaMtoonMaterial, UnaMtoonOutlineWidthMode, UnaSceneNode, UnaSceneSnapshot, UnaShadingModel, UnaSkin,
	UnaTextureFilterMode, UnaTextureSampler, UnaTextureWrapMode, UnaUnavatarExtension,
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
	let samplers = image_samplers_from_document(document);
	document
		.images()
		.map(|image| {
			let sampler = samplers.get(image.index()).copied().flatten();
			let name = image.name().map(str::to_string);
			let image_metadata = image
				.extras()
				.as_ref()
				.and_then(|extras| unavatar_image_metadata_from_raw(extras.get()));
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
						source_pixel_format: image_metadata.as_ref().and_then(|metadata| {
							json_string(metadata.get("sourcePixelFormat").or_else(|| metadata.get("source_pixel_format")))
						}),
						channels: image_metadata.as_ref().and_then(|metadata| json_string(metadata.get("channels"))),
						color_space: image_metadata
							.as_ref()
							.and_then(|metadata| json_string(metadata.get("colorSpace").or_else(|| metadata.get("color_space")))),
						texture_type: image_metadata
							.as_ref()
							.and_then(|metadata| json_string(metadata.get("textureType").or_else(|| metadata.get("texture_type")))),
						texture_shape: image_metadata
							.as_ref()
							.and_then(|metadata| json_string(metadata.get("textureShape").or_else(|| metadata.get("texture_shape")))),
						srgb: image_metadata
							.as_ref()
							.and_then(|metadata| metadata.get("sRGB").or_else(|| metadata.get("srgb")).and_then(Value::as_bool)),
						sampler,
						byte_length: bytes.len() as u64,
						source_hash: fnv1a64(bytes),
					})
				}
				gltf::image::Source::Uri { uri, mime_type } => Some(UnaImageSourceMetadata {
					name,
					mime_type: mime_type.map(str::to_string),
					uri: Some(uri.to_string()),
					source_pixel_format: image_metadata.as_ref().and_then(|metadata| {
						json_string(metadata.get("sourcePixelFormat").or_else(|| metadata.get("source_pixel_format")))
					}),
					channels: image_metadata.as_ref().and_then(|metadata| json_string(metadata.get("channels"))),
					color_space: image_metadata
						.as_ref()
						.and_then(|metadata| json_string(metadata.get("colorSpace").or_else(|| metadata.get("color_space")))),
					texture_type: image_metadata
						.as_ref()
						.and_then(|metadata| json_string(metadata.get("textureType").or_else(|| metadata.get("texture_type")))),
					texture_shape: image_metadata
						.as_ref()
						.and_then(|metadata| json_string(metadata.get("textureShape").or_else(|| metadata.get("texture_shape")))),
					srgb: image_metadata
						.as_ref()
						.and_then(|metadata| metadata.get("sRGB").or_else(|| metadata.get("srgb")).and_then(Value::as_bool)),
					sampler,
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
	let samplers = image_samplers_from_root_json(root);
	images
		.iter()
		.enumerate()
		.map(|(image_index, image)| {
			let sampler = samplers.get(image_index).copied().flatten();
			let name = image.get("name").and_then(Value::as_str).map(str::to_string);
			let mime_type = image.get("mimeType").and_then(Value::as_str).map(str::to_string);
			let image_metadata = unavatar_image_metadata_from_image_json(image);
			if let Some(uri) = image.get("uri").and_then(Value::as_str) {
				return Some(UnaImageSourceMetadata {
					name,
					mime_type,
					uri: Some(uri.to_string()),
					source_pixel_format: image_metadata.as_ref().and_then(|metadata| {
						json_string(metadata.get("sourcePixelFormat").or_else(|| metadata.get("source_pixel_format")))
					}),
					channels: image_metadata.as_ref().and_then(|metadata| json_string(metadata.get("channels"))),
					color_space: image_metadata
						.as_ref()
						.and_then(|metadata| json_string(metadata.get("colorSpace").or_else(|| metadata.get("color_space")))),
					texture_type: image_metadata
						.as_ref()
						.and_then(|metadata| json_string(metadata.get("textureType").or_else(|| metadata.get("texture_type")))),
					texture_shape: image_metadata
						.as_ref()
						.and_then(|metadata| json_string(metadata.get("textureShape").or_else(|| metadata.get("texture_shape")))),
					srgb: image_metadata
						.as_ref()
						.and_then(|metadata| metadata.get("sRGB").or_else(|| metadata.get("srgb")).and_then(Value::as_bool)),
					sampler,
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
				source_pixel_format: image_metadata
					.as_ref()
					.and_then(|metadata| json_string(metadata.get("sourcePixelFormat").or_else(|| metadata.get("source_pixel_format")))),
				channels: image_metadata.as_ref().and_then(|metadata| json_string(metadata.get("channels"))),
				color_space: image_metadata
					.as_ref()
					.and_then(|metadata| json_string(metadata.get("colorSpace").or_else(|| metadata.get("color_space")))),
				texture_type: image_metadata
					.as_ref()
					.and_then(|metadata| json_string(metadata.get("textureType").or_else(|| metadata.get("texture_type")))),
				texture_shape: image_metadata
					.as_ref()
					.and_then(|metadata| json_string(metadata.get("textureShape").or_else(|| metadata.get("texture_shape")))),
				srgb: image_metadata
					.as_ref()
					.and_then(|metadata| metadata.get("sRGB").or_else(|| metadata.get("srgb")).and_then(Value::as_bool)),
				sampler,
				byte_length: bytes.len() as u64,
				source_hash: fnv1a64(bytes),
			})
		})
		.collect()
}

fn unavatar_image_metadata_from_raw(raw: &str) -> Option<Value> {
	let value = serde_json::from_str::<Value>(raw).ok()?;
	if let Some(metadata) = value.get("UN_avatar_image") {
		Some(metadata.clone())
	} else {
		Some(value)
	}
}

fn unavatar_image_metadata_from_image_json(image: &Value) -> Option<Value> {
	let extras = image.get("extras")?;
	if let Some(metadata) = extras.get("UN_avatar_image") {
		Some(metadata.clone())
	} else {
		Some(extras.clone())
	}
}

fn image_samplers_from_document(document: &gltf::Document) -> Vec<Option<UnaTextureSampler>> {
	let mut out = vec![None; document.images().len()];
	for texture in document.textures() {
		let image_index = texture.source().index();
		if out.get(image_index).is_some_and(Option::is_some) {
			continue;
		}
		if let Some(slot) = out.get_mut(image_index) {
			let sampler = texture.sampler();
			*slot = Some(UnaTextureSampler {
				mag_filter: match sampler.mag_filter() {
					Some(gltf::texture::MagFilter::Nearest) => UnaTextureFilterMode::Nearest,
					Some(gltf::texture::MagFilter::Linear) | None => UnaTextureFilterMode::Linear,
				},
				min_filter: sampler
					.min_filter()
					.map(gltf_min_filter_mode)
					.unwrap_or(UnaTextureFilterMode::Linear),
				wrap_s: gltf_wrap_mode(sampler.wrap_s()),
				wrap_t: gltf_wrap_mode(sampler.wrap_t()),
			});
		}
	}
	out
}

fn gltf_min_filter_mode(filter: gltf::texture::MinFilter) -> UnaTextureFilterMode {
	match filter {
		gltf::texture::MinFilter::Nearest
		| gltf::texture::MinFilter::NearestMipmapNearest
		| gltf::texture::MinFilter::NearestMipmapLinear => UnaTextureFilterMode::Nearest,
		gltf::texture::MinFilter::Linear | gltf::texture::MinFilter::LinearMipmapNearest | gltf::texture::MinFilter::LinearMipmapLinear => {
			UnaTextureFilterMode::Linear
		}
	}
}

fn gltf_wrap_mode(mode: gltf::texture::WrappingMode) -> UnaTextureWrapMode {
	match mode {
		gltf::texture::WrappingMode::ClampToEdge => UnaTextureWrapMode::ClampToEdge,
		gltf::texture::WrappingMode::MirroredRepeat => UnaTextureWrapMode::MirroredRepeat,
		gltf::texture::WrappingMode::Repeat => UnaTextureWrapMode::Repeat,
	}
}

fn image_samplers_from_root_json(root: &Value) -> Vec<Option<UnaTextureSampler>> {
	let image_count = root.get("images").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
	let mut out = vec![None; image_count];
	let Some(textures) = root.get("textures").and_then(Value::as_array) else {
		return out;
	};
	let samplers = root.get("samplers").and_then(Value::as_array);
	for texture in textures {
		let Some(source) = texture.get("source").and_then(Value::as_u64).map(|value| value as usize) else {
			continue;
		};
		if out.get(source).is_some_and(Option::is_some) {
			continue;
		}
		let sampler = texture
			.get("sampler")
			.and_then(Value::as_u64)
			.and_then(|index| samplers?.get(index as usize))
			.map(sampler_from_root_json)
			.unwrap_or_default();
		if let Some(slot) = out.get_mut(source) {
			*slot = Some(sampler);
		}
	}
	out
}

fn sampler_from_root_json(value: &Value) -> UnaTextureSampler {
	UnaTextureSampler {
		mag_filter: filter_from_gltf_constant(value.get("magFilter").and_then(Value::as_u64)),
		min_filter: filter_from_gltf_constant(value.get("minFilter").and_then(Value::as_u64)),
		wrap_s: wrap_from_gltf_constant(value.get("wrapS").and_then(Value::as_u64)),
		wrap_t: wrap_from_gltf_constant(value.get("wrapT").and_then(Value::as_u64)),
	}
}

fn filter_from_gltf_constant(value: Option<u64>) -> UnaTextureFilterMode {
	match value {
		Some(9728 | 9984 | 9986) => UnaTextureFilterMode::Nearest,
		_ => UnaTextureFilterMode::Linear,
	}
}

fn wrap_from_gltf_constant(value: Option<u64>) -> UnaTextureWrapMode {
	match value {
		Some(33071) => UnaTextureWrapMode::ClampToEdge,
		Some(33648) => UnaTextureWrapMode::MirroredRepeat,
		_ => UnaTextureWrapMode::Repeat,
	}
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
			texture_type: asset.get("textureType").and_then(Value::as_str).map(str::to_string),
			texture_shape: asset.get("textureShape").and_then(Value::as_str).map(str::to_string),
			srgb: asset.get("sRGB").or_else(|| asset.get("srgb")).and_then(Value::as_bool),
			sampler: asset.get("sampler").map(sampler_from_root_json),
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
		if let Some(image_index) = texture_asset_ref(mtoon, "shadowColorTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.shadow
				.color_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "shadowStrengthMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.shadow
				.strength_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "shadowBorderMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.shadow
				.border_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "shadowBlurMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.shadow
				.blur_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "normal2ndTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.normal
				.second_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "matcapTextureIndexAsset", asset_map) {
			scene_material.mtoon.get_or_insert_with(Default::default).matcap_texture_index = Some(image_index);
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.matcap
				.texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "matcapBlendMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.matcap
				.blend_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "matcap2ndTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.matcap
				.second_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "matcap2ndBlendMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.matcap
				.second_blend_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "rimMultiplyTextureIndexAsset", asset_map) {
			scene_material.mtoon.get_or_insert_with(Default::default).rim_multiply_texture_index = Some(image_index);
			scene_material.liltoon_like.get_or_insert_with(Default::default).rim.texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "emissionTextureIndexAsset", asset_map) {
			scene_material.emissive_texture_index = Some(image_index);
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.emission
				.texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "emissionGradationTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.emission
				.gradation_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "reflectionCubeTextureIndexAsset", asset_map) {
			scene_material
				.mtoon
				.get_or_insert_with(Default::default)
				.reflection_cube_texture_index = Some(image_index);
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.reflection
				.cube_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "reflectionColorTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.reflection
				.color_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "smoothnessTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.reflection
				.smoothness_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "metallicGlossTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.reflection
				.metallic_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "outlineWidthMultiplyTextureIndexAsset", asset_map) {
			scene_material
				.mtoon
				.get_or_insert_with(Default::default)
				.outline_width_multiply_texture_index = Some(image_index);
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.outline
				.width_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "outlineTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.outline
				.texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "alphaMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.alpha_mask
				.texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "gradationMapTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.gradation_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "anisotropyTangentTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.reflection
				.anisotropy_tangent_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "anisotropyScaleMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.reflection
				.anisotropy_scale_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "anisotropyShiftNoiseMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.reflection
				.anisotropy_shift_noise_mask_texture_index = Some(image_index);
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

fn base_operation_is_inherited_hidden_under_base(
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
	if path.is_empty() {
		return false;
	}
	let resolved = lookup_operation_targets_all(node_ids, registry_paths, paths, normalized_paths, op);
	if resolved.is_empty() {
		return base_hidden_paths.iter().any(|hidden| {
			let hidden = normalize_unavatar_path(hidden);
			let path = normalize_unavatar_path(path);
			hidden != path && !hidden.is_empty() && path.starts_with(&format!("{hidden}/"))
		});
	}
	resolved.iter().all(|idx| {
		paths_by_index.get(*idx).and_then(|p| p.as_deref()).is_some_and(|resolved_path| {
			base_hidden_paths.iter().any(|hidden| {
				let hidden = normalize_unavatar_path(hidden);
				let resolved_path = normalize_unavatar_path(resolved_path);
				hidden != resolved_path && !hidden.is_empty() && resolved_path.starts_with(&format!("{hidden}/"))
			})
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

fn scene_parent_indices(scene: &UnaSceneSnapshot) -> Vec<Option<usize>> {
	let mut parents = vec![None; scene.nodes.len()];
	for (idx, node) in scene.nodes.iter().enumerate() {
		for &child in &node.children {
			if child < parents.len() {
				parents[child] = Some(idx);
			}
		}
	}
	parents
}

fn scene_world_matrices(scene: &UnaSceneSnapshot) -> Vec<Mat4> {
	let mut world = vec![Mat4::IDENTITY; scene.nodes.len().max(1)];
	fn visit(scene: &UnaSceneSnapshot, idx: usize, parent: Mat4, world: &mut [Mat4]) {
		let Some(node) = scene.nodes.get(idx) else {
			return;
		};
		let current = parent * Mat4::from_cols_array(&node.transform);
		world[idx] = current;
		for &child in &node.children {
			if child < scene.nodes.len() {
				visit(scene, child, current, world);
			}
		}
	}
	for &root in &scene.roots {
		if root < scene.nodes.len() {
			visit(scene, root, Mat4::IDENTITY, &mut world);
		}
	}
	world
}

fn scene_is_descendant_of(parents: &[Option<usize>], mut node: usize, ancestor: usize) -> bool {
	while let Some(parent) = parents.get(node).copied().flatten() {
		if parent == ancestor {
			return true;
		}
		node = parent;
	}
	false
}

fn unavatar_node_ref_index(
	reference: &Value,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Option<usize> {
	if let Some(node_id) = reference.get("nodeId").and_then(|v| v.as_str()).filter(|v| !v.is_empty()) {
		if let Some(&idx) = node_ids.get(node_id) {
			return Some(idx);
		}
		if let Some(path) = registry_paths.get(node_id) {
			if let Some(idx) = lookup_scene_path_all(paths, normalized_paths, path).into_iter().next() {
				return Some(idx);
			}
		}
	}
	let path = reference.get("path").and_then(|v| v.as_str()).unwrap_or("");
	lookup_scene_path_all(paths, normalized_paths, path).into_iter().next()
}

fn decompose_finite(m: Mat4) -> (Vec3, Quat, Vec3) {
	let (scale, rotation, translation) = m.to_scale_rotation_translation();
	let scale = if scale.is_finite() { scale } else { Vec3::ONE };
	let rotation = if rotation.is_finite() { rotation } else { Quat::IDENTITY };
	let translation = if translation.is_finite() { translation } else { Vec3::ZERO };
	(scale, rotation, translation)
}

fn bone_proxy_local_transform(mode: &str, match_scale: bool, target_world: Mat4, old_world: Mat4) -> Mat4 {
	let target_inverse = inverse_finite_or_identity(target_world);
	let (_old_scale, old_rotation, old_translation) = decompose_finite(old_world);
	let local = match mode {
		"AsChildAtRoot" | "Unset" | "" => Mat4::IDENTITY,
		"AsChildKeepPosition" => target_inverse * Mat4::from_translation(old_translation),
		"AsChildKeepRotation" => target_inverse * Mat4::from_quat(old_rotation),
		"AsChildKeepWorldPose" => target_inverse * old_world,
		_ => target_inverse * old_world,
	};
	if match_scale {
		let (_scale, rotation, translation) = decompose_finite(local);
		Mat4::from_scale_rotation_translation(Vec3::ONE, rotation, translation)
	} else {
		local
	}
}

fn inverse_finite_or_identity(m: Mat4) -> Mat4 {
	let inverse = m.inverse();
	if inverse.to_cols_array().iter().all(|v| v.is_finite()) {
		inverse
	} else {
		Mat4::IDENTITY
	}
}

fn reparent_scene_node(scene: &mut UnaSceneSnapshot, child: usize, new_parent: usize, local: Mat4) -> bool {
	if child >= scene.nodes.len() || new_parent >= scene.nodes.len() || child == new_parent {
		return false;
	}
	let parents = scene_parent_indices(scene);
	if scene_is_descendant_of(&parents, new_parent, child) {
		return false;
	}
	if let Some(old_parent) = parents.get(child).copied().flatten() {
		if let Some(parent_node) = scene.nodes.get_mut(old_parent) {
			parent_node.children.retain(|&idx| idx != child);
		}
	} else {
		scene.roots.retain(|&idx| idx != child);
	}
	if let Some(parent_node) = scene.nodes.get_mut(new_parent) {
		if !parent_node.children.contains(&child) {
			parent_node.children.push(child);
		}
	}
	if let Some(node) = scene.nodes.get_mut(child) {
		node.transform = local.to_cols_array();
	}
	true
}

#[derive(Clone, Debug)]
struct BoneProxyResolverInfo {
	child: usize,
	new_parent: usize,
	old_world: Mat4,
	mode: String,
	match_scale: bool,
}

fn collect_merge_armature_bone_mappings(
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> (BTreeMap<usize, usize>, usize, usize) {
	let mut mappings = BTreeMap::new();
	let mut missing = 0usize;
	let mut skipped = 0usize;
	for component in components {
		if component.get("shortType").and_then(|v| v.as_str()) != Some("ModularAvatarMergeArmature") {
			continue;
		}
		if component.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
			skipped += 1;
			continue;
		}
		let Some(bone_mappings) = component.get("boneMappings").and_then(|v| v.as_array()) else {
			missing += 1;
			continue;
		};
		for mapping in bone_mappings {
			let Some(source_ref) = mapping.get("sourceBone") else {
				missing += 1;
				continue;
			};
			let Some(target_ref) = mapping.get("targetBone") else {
				missing += 1;
				continue;
			};
			let Some(source) = unavatar_node_ref_index(source_ref, node_ids, registry_paths, paths, normalized_paths) else {
				missing += 1;
				continue;
			};
			let Some(target) = unavatar_node_ref_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
				missing += 1;
				continue;
			};
			if source != target {
				mappings.insert(source, target);
			}
		}
	}
	(mappings, missing, skipped)
}

fn retarget_merge_armature_skins(scene: &mut UnaSceneSnapshot, mappings: &BTreeMap<usize, usize>) -> usize {
	if mappings.is_empty() {
		return 0;
	}
	let world = scene_world_matrices(scene);
	let mut retargeted = 0usize;
	for skin in &mut scene.skins {
		for joint_idx in 0..skin.joint_nodes.len() {
			let source_node = skin.joint_nodes[joint_idx];
			let Some(&target_node) = mappings.get(&source_node) else {
				continue;
			};
			let Some(source_world) = world.get(source_node).copied() else {
				continue;
			};
			let Some(target_world) = world.get(target_node).copied() else {
				continue;
			};
			let old_bind = skin
				.inverse_bind_matrices
				.get(joint_idx)
				.copied()
				.map(|m| Mat4::from_cols_array(&m))
				.unwrap_or(Mat4::IDENTITY);
			let new_bind = inverse_finite_or_identity(target_world) * source_world * old_bind;
			if let Some(bind) = skin.inverse_bind_matrices.get_mut(joint_idx) {
				*bind = new_bind.to_cols_array();
			}
			skin.joint_nodes[joint_idx] = target_node;
			retargeted += 1;
		}
		if let Some(skeleton_node) = skin.skeleton_node {
			if let Some(&target_node) = mappings.get(&skeleton_node) {
				skin.skeleton_node = Some(target_node);
			}
		}
	}
	retargeted
}

fn modular_avatar_reference_index(
	reference: &Value,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Option<usize> {
	if let Some(resolved) = reference.get("resolvedTarget") {
		if let Some(idx) = unavatar_node_ref_index(resolved, node_ids, registry_paths, paths, normalized_paths) {
			return Some(idx);
		}
	}
	if let Some(target) = reference.get("targetObject") {
		if let Some(idx) = unavatar_node_ref_index(target, node_ids, registry_paths, paths, normalized_paths) {
			return Some(idx);
		}
	}
	unavatar_node_ref_index(reference, node_ids, registry_paths, paths, normalized_paths)
}

fn mesh_settings_mode_sets(mode: Option<&str>) -> bool {
	matches!(mode, Some("Set") | Some("SetOrInherit"))
}

fn value_vec3(value: &Value) -> Option<[f32; 3]> {
	let values = value.as_array()?;
	if values.len() != 3 {
		return None;
	}
	Some([values[0].as_f64()? as f32, values[1].as_f64()? as f32, values[2].as_f64()? as f32])
}

fn mesh_settings_bounds(value: &Value) -> Option<UnaBounds> {
	let object = value.as_object()?;
	let center = object.get("center").and_then(value_vec3)?;
	let extents = object.get("extents").and_then(value_vec3).or_else(|| {
		object
			.get("size")
			.and_then(value_vec3)
			.map(|size| [(size[0] * 0.5).abs(), (size[1] * 0.5).abs(), (size[2] * 0.5).abs()])
	})?;
	Some(UnaBounds { center, extents })
}

fn apply_mesh_settings_to_subtree(
	scene: &mut UnaSceneSnapshot,
	target_root: usize,
	root_bone: Option<usize>,
	probe_anchor: Option<usize>,
	local_bounds: Option<UnaBounds>,
) -> usize {
	let parents = scene_parent_indices(scene);
	let mut applied = 0usize;
	for idx in 0..scene.nodes.len() {
		if idx != target_root && !scene_is_descendant_of(&parents, idx, target_root) {
			continue;
		}
		let Some(skin_idx) = scene.nodes.get(idx).and_then(|node| node.skin) else {
			continue;
		};
		let Some(skin) = scene.skins.get_mut(skin_idx) else {
			continue;
		};
		skin.skeleton_node = root_bone.or(Some(idx));
		if let Some(node) = scene.nodes.get_mut(idx) {
			node.probe_anchor_node = probe_anchor;
			node.local_bounds = local_bounds;
		}
		applied += 1;
	}
	applied
}

fn apply_unavatar_mesh_settings(
	scene: &mut UnaSceneSnapshot,
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> (usize, usize, usize, usize) {
	let mut root_bone_applied = 0usize;
	let mut probe_anchor_applied = 0usize;
	let mut bounds_applied = 0usize;
	let mut missing = 0usize;
	for component in components {
		if component.get("shortType").and_then(|v| v.as_str()) != Some("ModularAvatarMeshSettings") {
			continue;
		}
		if component.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
			continue;
		}
		let Some(target_ref) = component.get("target") else {
			missing += 1;
			continue;
		};
		let Some(target_root) = unavatar_node_ref_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
			missing += 1;
			continue;
		};
		let fields = component.get("fields").and_then(|v| v.as_object());
		let probe_anchor = if fields
			.and_then(|fields| fields.get("InheritProbeAnchor"))
			.and_then(|v| v.as_str())
			.is_some_and(|mode| mesh_settings_mode_sets(Some(mode)))
		{
			let probe_anchor = fields
				.and_then(|fields| fields.get("ProbeAnchor"))
				.and_then(|reference| modular_avatar_reference_index(reference, node_ids, registry_paths, paths, normalized_paths));
			if probe_anchor.is_some() {
				probe_anchor
			} else {
				missing += 1;
				None
			}
		} else {
			None
		};
		if !fields
			.and_then(|fields| fields.get("InheritBounds"))
			.and_then(|v| v.as_str())
			.is_some_and(|mode| mesh_settings_mode_sets(Some(mode)))
		{
			continue;
		}
		let root_bone = fields
			.and_then(|fields| fields.get("RootBone"))
			.and_then(|reference| modular_avatar_reference_index(reference, node_ids, registry_paths, paths, normalized_paths));
		let local_bounds = fields.and_then(|fields| fields.get("Bounds")).and_then(mesh_settings_bounds);
		let applied = apply_mesh_settings_to_subtree(scene, target_root, root_bone, probe_anchor, local_bounds);
		root_bone_applied += applied;
		if probe_anchor.is_some() {
			probe_anchor_applied += applied;
		}
		if local_bounds.is_some() {
			bounds_applied += applied;
		}
	}
	(root_bone_applied, probe_anchor_applied, bounds_applied, missing)
}

fn apply_unavatar_modular_avatar(scene: &mut UnaSceneSnapshot, unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	let Some(modular_avatar) = unavatar.source.get("modularAvatar").and_then(|v| v.as_object()) else {
		return;
	};
	let Some(components) = modular_avatar.get("components").and_then(|v| v.as_array()) else {
		return;
	};
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(Some(unavatar));
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let (mesh_settings_root_bones, mesh_settings_probe_anchors, mesh_settings_bounds, mesh_settings_missing) =
		apply_unavatar_mesh_settings(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths);
	if mesh_settings_root_bones > 0 || mesh_settings_probe_anchors > 0 || mesh_settings_bounds > 0 || mesh_settings_missing > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: mesh_settings_root_bones={}, mesh_settings_probe_anchors={}, mesh_settings_bounds={}, mesh_settings_missing={}",
			mesh_settings_root_bones, mesh_settings_probe_anchors, mesh_settings_bounds, mesh_settings_missing
		));
	}

	let (merge_mappings, merge_missing, merge_skipped) =
		collect_merge_armature_bone_mappings(components, &node_ids, &registry_paths, &paths, &normalized_paths);
	let merge_retargeted = retarget_merge_armature_skins(scene, &merge_mappings);
	if merge_retargeted > 0 || merge_missing > 0 || merge_skipped > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: merge_armature_mappings={}, mesh_retargeter_joints={}, merge_armature_missing={}, merge_armature_skipped={}",
			merge_mappings.len(),
			merge_retargeted,
			merge_missing,
			merge_skipped
		));
	}

	let node_ids = scene_node_ids(scene);
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let mut bone_proxy_applied = 0usize;
	let mut bone_proxy_missing = 0usize;
	let mut bone_proxy_skipped = 0usize;
	let initial_world = scene_world_matrices(scene);
	let mut bone_proxies = Vec::new();

	for component in components {
		if component.get("shortType").and_then(|v| v.as_str()) != Some("ModularAvatarBoneProxy") {
			continue;
		}
		if component.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
			bone_proxy_skipped += 1;
			continue;
		}
		let Some(target_ref) = component.get("target") else {
			bone_proxy_missing += 1;
			continue;
		};
		let Some(resolved_ref) = component.get("resolvedTarget") else {
			bone_proxy_missing += 1;
			continue;
		};
		let Some(child) = unavatar_node_ref_index(target_ref, &node_ids, &registry_paths, &paths, &normalized_paths) else {
			bone_proxy_missing += 1;
			continue;
		};
		let Some(new_parent) = unavatar_node_ref_index(resolved_ref, &node_ids, &registry_paths, &paths, &normalized_paths) else {
			bone_proxy_missing += 1;
			continue;
		};
		let fields = component.get("fields").and_then(|v| v.as_object());
		let mode = fields
			.and_then(|fields| fields.get("attachmentMode"))
			.and_then(|v| v.as_str())
			.unwrap_or("AsChildKeepWorldPose");
		let match_scale = fields
			.and_then(|fields| fields.get("matchScale"))
			.and_then(|v| v.as_bool())
			.unwrap_or(false);
		bone_proxies.push(BoneProxyResolverInfo {
			child,
			new_parent,
			old_world: initial_world.get(child).copied().unwrap_or(Mat4::IDENTITY),
			mode: mode.to_string(),
			match_scale,
		});
	}

	let reparent_world = scene_world_matrices(scene);
	for proxy in &bone_proxies {
		let parent_world = reparent_world.get(proxy.new_parent).copied().unwrap_or(Mat4::IDENTITY);
		let local = inverse_finite_or_identity(parent_world) * proxy.old_world;
		if reparent_scene_node(scene, proxy.child, proxy.new_parent, local) {
			bone_proxy_applied += 1;
		} else {
			bone_proxy_missing += 1;
		}
	}
	let parents = scene_parent_indices(scene);
	bone_proxies.sort_by_key(|proxy| {
		let mut depth = 0usize;
		let mut node = proxy.child;
		while let Some(parent) = parents.get(node).copied().flatten() {
			depth += 1;
			node = parent;
		}
		depth
	});
	for proxy in &bone_proxies {
		let world = scene_world_matrices(scene);
		let parent_world = world.get(proxy.new_parent).copied().unwrap_or(Mat4::IDENTITY);
		let local = bone_proxy_local_transform(&proxy.mode, proxy.match_scale, parent_world, proxy.old_world);
		if let Some(node) = scene.nodes.get_mut(proxy.child) {
			node.transform = local.to_cols_array();
		}
	}

	if bone_proxy_applied > 0 || bone_proxy_missing > 0 || bone_proxy_skipped > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: bone_proxy_applied={bone_proxy_applied}, bone_proxy_missing={bone_proxy_missing}, bone_proxy_skipped={bone_proxy_skipped}"
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
	let base_hidden_paths = operations
		.iter()
		.filter(|op| op.get("visible").and_then(|v| v.as_bool()) == Some(false))
		.flat_map(|op| {
			let resolved = lookup_operation_targets_all(&node_ids, &registry_paths, &paths, &normalized_paths, op);
			if resolved.is_empty() {
				vec![operation_target_path(op).to_string()]
			} else {
				resolved
					.into_iter()
					.filter_map(|idx| paths_by_index.get(idx).and_then(|p| p.clone()))
					.collect::<Vec<_>>()
			}
		})
		.filter(|path| !path.is_empty())
		.collect::<Vec<_>>();
	let filtered_operations: Vec<Value> = operations
		.iter()
		.filter(|op| {
			!base_operation_is_inherited_hidden_under_base(
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
	if applied.visibility_applied > 0 || applied.visibility_missing > 0 || applied.blendshape_applied > 0 || applied.blendshape_missing > 0
	{
		report.push_info(format!(
			".unavatar wardrobe base: visibility_applied={}, visibility_missing={}, blendshape_applied={}, blendshape_missing={}, inherited_hidden_skipped={}",
			applied.visibility_applied, applied.visibility_missing, applied.blendshape_applied, applied.blendshape_missing, skipped
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
			skeleton_node: skin.skeleton().map(|node| node.index()),
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
			let cull_mode = extras
				.as_ref()
				.and_then(unavatar_material_cull_mode_from_source_params)
				.unwrap_or(if double_sided { UnaCullMode::Off } else { UnaCullMode::Back });
			let pbr = m.pbr_metallic_roughness();
			let factor = pbr.base_color_factor();
			let tex = pbr.base_color_texture().map(|t| t.texture().source().index());
			let gltf_uv_offset_scale = pbr.base_color_texture().and_then(texture_info_uv_offset_scale);
			let normal_texture_index = m.normal_texture().map(|t| t.texture().source().index());
			let normal_texture_scale = m.normal_texture().map(|t| t.scale()).unwrap_or(1.0);
			let occlusion_texture_index = m.occlusion_texture().map(|t| t.texture().source().index());
			let occlusion_texture_strength = m.occlusion_texture().map(|t| t.strength()).unwrap_or(1.0);
			let emissive_factor = extras
				.as_ref()
				.and_then(unavatar_material_emissive_factor_from_source_params)
				.unwrap_or_else(|| m.emissive_factor());
			let emissive_texture_index = m.emissive_texture().map(|t| t.texture().source().index());
			let mut unavatar_liltoon_like = extras.as_ref().and_then(unavatar_liltoon_like_from_extras);
			if let Some(liltoon_like) = unavatar_liltoon_like.as_mut() {
				if liltoon_like.emission.texture_index.is_none() {
					liltoon_like.emission.texture_index = emissive_texture_index;
				}
			}
			let unavatar_mtoon = extras.as_ref().and_then(unavatar_mtoon_from_extras);
			let shading = if unavatar_liltoon_like.is_some() {
				UnaShadingModel::LilToonLike
			} else if unavatar_mtoon.is_some() {
				UnaShadingModel::MToonLike
			} else if m.unlit() {
				UnaShadingModel::Unlit
			} else {
				UnaShadingModel::LitLambert
			};
			let alpha_cutoff_opt = m
				.alpha_cutoff()
				.or_else(|| extras.as_ref().and_then(unavatar_material_alpha_cutoff_from_source_params));
			let alpha_cutoff = alpha_cutoff_opt.unwrap_or(0.5);
			let gltf_alpha_mode = match m.alpha_mode() {
				gltf::material::AlphaMode::Opaque => UnaAlphaMode::Opaque,
				gltf::material::AlphaMode::Mask => UnaAlphaMode::Mask,
				gltf::material::AlphaMode::Blend => UnaAlphaMode::Blend,
			};
			let alpha_mode = unavatar_material_inferred_alpha_mode(extras.as_ref(), gltf_alpha_mode, alpha_cutoff_opt, tex.is_some())
				.unwrap_or(gltf_alpha_mode);
			let uv_offset_scale = unavatar_mtoon
				.as_ref()
				.map(|mtoon| mtoon.uv_offset_scale)
				.or(gltf_uv_offset_scale)
				.unwrap_or([0.0, 0.0, 1.0, 1.0]);
			UnaMaterialPbr {
				name,
				double_sided,
				cull_mode,
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
				uv_offset_scale,
				mtoon: unavatar_mtoon,
				liltoon_like: unavatar_liltoon_like,
				unavatar_material: extras,
			}
		})
		.collect()
}

fn texture_info_uv_offset_scale(info: gltf::texture::Info<'_>) -> Option<[f32; 4]> {
	let transform = info.texture_transform()?;
	let offset = transform.offset();
	let scale = transform.scale();
	Some([offset[0], offset[1], scale[0], scale[1]])
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

	if let Some(mode) = unavatar_material_blend_mode_from_source_params(extras) {
		return Some(mode);
	}

	if let Some(mode @ (UnaAlphaMode::Mask | UnaAlphaMode::Blend)) = unavatar_material_alpha_mode_from_source_params(extras) {
		return Some(mode);
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
	} else if shader == "hidden/liltoonoutline" || shader == "hidden/liltoon" {
		Some(UnaAlphaMode::Opaque)
	} else {
		None
	}
}

fn unavatar_material_alpha_mode_from_source_params(extras: &Value) -> Option<UnaAlphaMode> {
	let mode = unavatar_material_float_param(extras, "_TransparentMode")
		.or_else(|| unavatar_material_float_param(extras, "_AlphaMode"))
		.or_else(|| unavatar_material_float_param(extras, "_BlendMode"))?;
	if mode >= 1.5 {
		Some(UnaAlphaMode::Blend)
	} else if mode >= 0.5 {
		Some(UnaAlphaMode::Mask)
	} else {
		Some(UnaAlphaMode::Opaque)
	}
}

fn unavatar_material_blend_mode_from_source_params(extras: &Value) -> Option<UnaAlphaMode> {
	let src = unavatar_material_float_param(extras, "_SrcBlend")?;
	let dst = unavatar_material_float_param(extras, "_DstBlend")?;
	let alpha_to_mask = unavatar_material_float_param(extras, "_AlphaToMask").unwrap_or(0.0);
	if alpha_to_mask >= 0.5 {
		return Some(UnaAlphaMode::Mask);
	}
	if dst != 0.0 {
		return Some(UnaAlphaMode::Blend);
	}
	if src == 1.0 && dst == 0.0 {
		return Some(UnaAlphaMode::Opaque);
	}
	None
}

fn unavatar_material_alpha_cutoff_from_source_params(extras: &Value) -> Option<f32> {
	unavatar_material_float_param(extras, "_Cutoff")
		.or_else(|| unavatar_material_float_param(extras, "_AlphaCutoff"))
		.map(|value| value.clamp(0.0, 1.0))
}

fn unavatar_material_cull_mode_from_source_params(extras: &Value) -> Option<UnaCullMode> {
	let value = unavatar_material_float_param(extras, "_Cull").or_else(|| unavatar_material_float_param(extras, "_CullMode"))?;
	if value < 0.5 {
		Some(UnaCullMode::Off)
	} else if value < 1.5 {
		Some(UnaCullMode::Front)
	} else {
		Some(UnaCullMode::Back)
	}
}

fn unavatar_material_emissive_factor_from_source_params(extras: &Value) -> Option<[f32; 3]> {
	if unavatar_material_feature_enabled(extras, "_UseEmission") == Some(false) {
		return Some([0.0, 0.0, 0.0]);
	}
	let color = unavatar_material_color_param_rgb(extras, "_EmissionColor")?;
	let strength = unavatar_material_float_param(extras, "_EmissionMainStrength")
		.or_else(|| unavatar_material_float_param(extras, "_EmissionBlend"))
		.unwrap_or(1.0)
		.max(0.0);
	Some([color[0] * strength, color[1] * strength, color[2] * strength])
}

fn unavatar_material_is_ordinary_liltoon(material: &UnaMaterialPbr) -> bool {
	let Some(extras) = material.unavatar_material.as_ref() else {
		return false;
	};
	let family = extras.get("family").and_then(|v| v.as_str()).unwrap_or("");
	let source_shader = extras.get("sourceShader").and_then(|v| v.as_str()).unwrap_or("");
	if !unavatar_material_source_is_liltoon(family, source_shader) {
		return false;
	}
	let shader = source_shader.to_ascii_lowercase();
	!(shader.contains("cutout") || shader.contains("transparent") || shader.contains("refraction") || shader.contains("fur"))
}

fn unavatar_material_is_liltoon(material: &UnaMaterialPbr) -> bool {
	let Some(extras) = material.unavatar_material.as_ref() else {
		return false;
	};
	let family = extras.get("family").and_then(|v| v.as_str()).unwrap_or("");
	let source_shader = extras.get("sourceShader").and_then(|v| v.as_str()).unwrap_or("");
	unavatar_material_source_is_liltoon(family, source_shader)
}

fn unavatar_material_source_is_liltoon(family: &str, source_shader: &str) -> bool {
	family.eq_ignore_ascii_case("liltoon") || source_shader.to_ascii_lowercase().contains("liltoon")
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
		if !unavatar_material_is_liltoon(material) {
			continue;
		}
		let Some(image) = material.base_color_texture_index.and_then(|index| images.get(index)) else {
			continue;
		};
		let has_transparent_alpha = image_alpha_has_transparency(image);
		if material.alpha_mode == UnaAlphaMode::Mask && has_transparent_alpha && material.alpha_cutoff <= 0.0 {
			material.alpha_cutoff = 1.0 / 255.0;
		}
		if !unavatar_material_is_ordinary_liltoon(material) {
			continue;
		}
		match material.alpha_mode {
			UnaAlphaMode::Mask if material.alpha_cutoff <= 0.5 => {
				if !has_transparent_alpha {
					material.alpha_mode = UnaAlphaMode::Opaque;
				} else if material.alpha_cutoff <= 0.01 && image_alpha_has_translucency(image) {
					material.alpha_mode = UnaAlphaMode::Blend;
				}
			}
			_ => {}
		}
	}
}

fn unavatar_liltoon_like_from_extras(extras: &Value) -> Option<UnaLilToonLikeMaterial> {
	let family = extras.get("family").and_then(|v| v.as_str()).unwrap_or("");
	let source_shader = extras.get("sourceShader").and_then(|v| v.as_str()).unwrap_or("");
	let source_is_liltoon = family.eq_ignore_ascii_case("liltoon") || source_shader.to_ascii_lowercase().contains("liltoon");
	if !source_is_liltoon {
		return None;
	}
	let mtoon = extras.get("mtoon");
	let outline_width_unit = mtoon
		.and_then(|m| m.get("outlineWidthFactorUnit").or_else(|| m.get("outline_width_factor_unit")))
		.and_then(|v| v.as_str())
		.unwrap_or("");
	let liltoon_outline_width_scale = if outline_width_unit.eq_ignore_ascii_case("meters") {
		1.0
	} else {
		0.01
	};
	let mut out = UnaLilToonLikeMaterial {
		source_profile: if source_shader.to_ascii_lowercase().contains("liltoongem") {
			UnaLilToonLikeSourceProfile::LiltoonGem
		} else {
			UnaLilToonLikeSourceProfile::Liltoon
		},
		..Default::default()
	};
	out.texture_uv_offset_scales = unavatar_material_uv_offset_scales(extras);
	out.texture_uv_mode_factors = unavatar_material_uv_mode_factors(extras);
	out.rendering.render_queue_number = json_i32(extras.get("renderQueue").or_else(|| extras.get("render_queue")));
	if let Some(value) = mtoon.and_then(|m| {
		json_vec4(
			m.get("mainTexHsvgFactor")
				.or_else(|| m.get("main_tex_hsvg_factor"))
				.or_else(|| m.get("mainTextureHsvgFactor"))
				.or_else(|| m.get("main_texture_hsvg_factor")),
		)
	}) {
		out.main_color.main_texture_hsvg_factor = value;
	}
	out.main_color.gradation_enabled_factor = unavatar_material_float_param(extras, "_UseGradationMap")
		.unwrap_or_else(|| {
			if mtoon
				.and_then(|m| json_usize(m.get("gradationMapTextureIndex").or_else(|| m.get("gradation_map_texture_index"))))
				.is_some()
			{
				1.0
			} else {
				0.0
			}
		})
		.clamp(0.0, 1.0);
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("gradationMapTextureIndex").or_else(|| m.get("gradation_map_texture_index"))))
	{
		out.main_color.gradation_texture_index = Some(value);
	}
	out.main_color.second_enabled_factor = unavatar_material_float_param(extras, "_UseMain2ndTex")
		.unwrap_or(0.0)
		.clamp(0.0, 1.0);
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("main2ndTextureIndex").or_else(|| m.get("main_2nd_texture_index")))) {
		out.main_color.second_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main2ndTexBlendMode").map(float_to_u32_saturating) {
		out.main_color.second_blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main2ndEnableLighting") {
		out.main_color.second_enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	out.main_color.third_enabled_factor = unavatar_material_float_param(extras, "_UseMain3rdTex")
		.unwrap_or(0.0)
		.clamp(0.0, 1.0);
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("main3rdTextureIndex").or_else(|| m.get("main_3rd_texture_index")))) {
		out.main_color.third_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main3rdTexBlendMode").map(float_to_u32_saturating) {
		out.main_color.third_blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main3rdEnableLighting") {
		out.main_color.third_enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_LightMinLimit") {
		out.rendering.light_min_limit_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_LightMaxLimit") {
		out.rendering.light_max_limit_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MonochromeLighting") {
		out.rendering.monochrome_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_VertexLightStrength") {
		out.rendering.vertex_light_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AAStrength") {
		out.rendering.aa_strength_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GSAAStrength") {
		out.rendering.gsaa_strength_factor = value.max(0.0);
	}
	out.normal.second_enabled_factor = unavatar_material_float_param(extras, "_UseBumpMap2nd")
		.or_else(|| unavatar_material_float_param(extras, "_UseNormalMap2nd"))
		.unwrap_or_else(|| {
			if mtoon
				.and_then(|m| {
					json_usize(
						m.get("normal2ndTextureIndex")
							.or_else(|| m.get("normal_2nd_texture_index"))
							.or_else(|| m.get("normalSecondTextureIndex"))
							.or_else(|| m.get("normal_second_texture_index")),
					)
				})
				.is_some()
			{
				1.0
			} else {
				0.0
			}
		})
		.clamp(0.0, 1.0);
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("normal2ndTextureIndex")
				.or_else(|| m.get("normal_2nd_texture_index"))
				.or_else(|| m.get("normalSecondTextureIndex"))
				.or_else(|| m.get("normal_second_texture_index")),
		)
	}) {
		out.normal.second_texture_index = Some(value);
	}
	if let Some(value) = mtoon
		.and_then(|m| {
			json_f32(
				m.get("normal2ndScaleFactor")
					.or_else(|| m.get("normal_2nd_scale_factor"))
					.or_else(|| m.get("normalSecondScaleFactor"))
					.or_else(|| m.get("normal_second_scale_factor")),
			)
		})
		.or_else(|| unavatar_material_float_param(extras, "_BumpScale2nd"))
		.or_else(|| unavatar_material_float_param(extras, "_NormalScale2nd"))
	{
		out.normal.second_scale_factor = value;
	}
	out.shadow.enabled_factor = unavatar_material_float_param(extras, "_UseShadow").unwrap_or(1.0).clamp(0.0, 1.0);
	if let Some(value) =
		unavatar_material_color_param_rgb(extras, "_ShadeColor").or_else(|| unavatar_material_color_param_rgb(extras, "_ShadowColor"))
	{
		out.shadow.color_factor = value;
	}
	if let Some(value) = mtoon
		.and_then(|m| json_usize(m.get("shadowColorTextureIndex").or_else(|| m.get("shadow_color_texture_index"))))
		.or_else(|| mtoon.and_then(|m| json_usize(m.get("shadeMultiplyTextureIndex").or_else(|| m.get("shade_multiply_texture_index")))))
	{
		out.shadow.color_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("shadowStrengthMaskTextureIndex")
				.or_else(|| m.get("shadow_strength_mask_texture_index")),
		)
	}) {
		out.shadow.strength_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("shadowBorderMaskTextureIndex")
				.or_else(|| m.get("shadow_border_mask_texture_index")),
		)
	}) {
		out.shadow.border_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("shadowBlurMaskTextureIndex")
				.or_else(|| m.get("shadow_blur_mask_texture_index")),
		)
	}) {
		out.shadow.blur_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon
		.and_then(|m| json_f32(m.get("shadowStrengthFactor").or_else(|| m.get("shadow_strength_factor"))))
		.or_else(|| unavatar_material_float_param(extras, "_ShadowStrength"))
	{
		out.shadow.strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon
		.and_then(|m| json_f32(m.get("shadowBorderFactor").or_else(|| m.get("shadow_border_factor"))))
		.or_else(|| unavatar_material_float_param(extras, "_ShadowBorder"))
	{
		out.shadow.border_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon
		.and_then(|m| json_f32(m.get("shadowBlurFactor").or_else(|| m.get("shadow_blur_factor"))))
		.or_else(|| unavatar_material_float_param(extras, "_ShadowBlur"))
	{
		out.shadow.blur_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon
		.and_then(|m| json_f32(m.get("shadowBorderRangeFactor").or_else(|| m.get("shadow_border_range_factor"))))
		.or_else(|| unavatar_material_float_param(extras, "_ShadowBorderRange"))
	{
		out.shadow.border_range_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ShadowMainStrength") {
		out.shadow.main_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ShadowEnvStrength") {
		out.shadow.env_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_color_param_rgb(extras, "_ShadowBorderColor") {
		out.shadow.border_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ShadowNormalStrength") {
		out.shadow.normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ShadowReceive") {
		out.shadow.receive_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_Shadow2ndColor") {
		out.shadow.second_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Shadow2ndBorder") {
		out.shadow.second_border_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Shadow2ndBlur") {
		out.shadow.second_blur_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Shadow2ndNormalStrength") {
		out.shadow.second_normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Shadow2ndReceive") {
		out.shadow.second_receive_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_Shadow3rdColor") {
		out.shadow.third_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Shadow3rdBorder") {
		out.shadow.third_border_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Shadow3rdBlur") {
		out.shadow.third_blur_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Shadow3rdNormalStrength") {
		out.shadow.third_normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Shadow3rdReceive") {
		out.shadow.third_receive_factor = value.clamp(0.0, 1.0);
	}

	out.matcap.enabled_factor = unavatar_material_float_param(extras, "_UseMatCap").unwrap_or(0.0).clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_MatCapColor") {
		out.matcap.color_factor = [value[0], value[1], value[2]];
		out.matcap.color_alpha_factor = value[3].clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("matcapTextureIndex").or_else(|| m.get("matcap_texture_index")))) {
		out.matcap.texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("matcapBlendMaskTextureIndex")
				.or_else(|| m.get("matcap_blend_mask_texture_index")),
		)
	}) {
		out.matcap.blend_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon
		.and_then(|m| json_f32(m.get("matcapBlendFactor").or_else(|| m.get("matcap_blend_factor"))))
		.or_else(|| unavatar_material_float_param(extras, "_MatCapBlend"))
	{
		out.matcap.blend_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon
		.and_then(|m| json_f32(m.get("matcapMainStrengthFactor").or_else(|| m.get("matcap_main_strength_factor"))))
		.or_else(|| unavatar_material_float_param(extras, "_MatCapMainStrength"))
	{
		out.matcap.main_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon
		.and_then(|m| {
			json_f32(
				m.get("matcapEnableLightingFactor")
					.or_else(|| m.get("matcap_enable_lighting_factor")),
			)
		})
		.or_else(|| unavatar_material_float_param(extras, "_MatCapEnableLighting"))
	{
		out.matcap.enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon
		.and_then(|m| json_u32(m.get("matcapBlendMode").or_else(|| m.get("matcap_blend_mode"))))
		.or_else(|| unavatar_material_float_param(extras, "_MatCapBlendMode").map(float_to_u32_saturating))
	{
		out.matcap.blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapNormalStrength") {
		out.matcap.normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapShadowMask") {
		out.matcap.shadow_mask_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapApplyTransparency") {
		out.matcap.apply_transparency_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapLod") {
		out.matcap.lod_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapBackfaceMask") {
		out.matcap.backface_mask_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapPerspective") {
		out.matcap.perspective_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapZRotCancel") {
		out.matcap.z_rotation_cancel_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapVRParallaxStrength") {
		out.matcap.vr_parallax_strength_factor = value.clamp(0.0, 1.0);
	}
	out.matcap.second_enabled_factor = unavatar_material_float_param(extras, "_UseMatCap2nd")
		.unwrap_or(0.0)
		.clamp(0.0, 1.0);
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("matcap2ndTextureIndex").or_else(|| m.get("matcap_2nd_texture_index")))) {
		out.matcap.second_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("matcap2ndBlendMaskTextureIndex")
				.or_else(|| m.get("matcap_2nd_blend_mask_texture_index")),
		)
	}) {
		out.matcap.second_blend_mask_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_MatCap2ndColor") {
		out.matcap.second_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndMainStrength") {
		out.matcap.second_main_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndBlend") {
		out.matcap.second_blend_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndEnableLighting") {
		out.matcap.second_enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndShadowMask") {
		out.matcap.second_shadow_mask_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndApplyTransparency") {
		out.matcap.second_apply_transparency_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndBlendMode").map(float_to_u32_saturating) {
		out.matcap.second_blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndNormalStrength") {
		out.matcap.second_normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndLod") {
		out.matcap.second_lod_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndBackfaceMask") {
		out.matcap.second_backface_mask_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndPerspective") {
		out.matcap.second_perspective_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndZRotCancel") {
		out.matcap.second_z_rotation_cancel_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndVRParallaxStrength") {
		out.matcap.second_vr_parallax_strength_factor = value.clamp(0.0, 1.0);
	}

	out.reflection.enabled_factor = unavatar_material_float_param(extras, "_UseReflection")
		.unwrap_or(0.0)
		.clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_ReflectionColor") {
		out.reflection.color_factor = value;
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("reflectionCubeTextureIndex")
				.or_else(|| m.get("reflection_cube_texture_index")),
		)
	}) {
		out.reflection.cube_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("reflectionColorTextureIndex")
				.or_else(|| m.get("reflection_color_texture_index")),
		)
	}) {
		out.reflection.color_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("smoothnessTextureIndex").or_else(|| m.get("smoothness_texture_index")))) {
		out.reflection.smoothness_texture_index = Some(value);
	}
	if let Some(value) =
		mtoon.and_then(|m| json_usize(m.get("metallicGlossTextureIndex").or_else(|| m.get("metallic_gloss_texture_index"))))
	{
		out.reflection.metallic_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("anisotropyTangentTextureIndex")
				.or_else(|| m.get("anisotropy_tangent_texture_index")),
		)
	}) {
		out.reflection.anisotropy_tangent_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("anisotropyScaleMaskTextureIndex")
				.or_else(|| m.get("anisotropy_scale_mask_texture_index")),
		)
	}) {
		out.reflection.anisotropy_scale_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("anisotropyShiftNoiseMaskTextureIndex")
				.or_else(|| m.get("anisotropy_shift_noise_mask_texture_index")),
		)
	}) {
		out.reflection.anisotropy_shift_noise_mask_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Smoothness") {
		out.reflection.smoothness_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Metallic") {
		out.reflection.metallic_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Reflectance") {
		out.reflection.reflectance_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ApplySpecular") {
		out.reflection.apply_specular_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ApplySpecularFA") {
		out.reflection.apply_specular_forward_add_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ApplyReflection") {
		out.reflection.apply_reflection_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ReflectionApplyTransparency") {
		out.reflection.apply_transparency_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_SpecularToon") {
		out.reflection.specular_toon_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_SpecularBorder") {
		out.reflection.specular_border_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_SpecularBlur") {
		out.reflection.specular_blur_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_SpecularNormalStrength") {
		out.reflection.specular_normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ReflectionNormalStrength") {
		out.reflection.reflection_normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ReflectionCubeEnableLighting") {
		out.reflection.cube_enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_ReflectionCubeColor") {
		out.reflection.cube_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ReflectionCubeOverride") {
		out.reflection.cube_override_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ReflectionBlendMode").map(float_to_u32_saturating) {
		out.reflection.blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_GemEnvColor") {
		out.reflection.gem_env_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GemEnvContrast") {
		out.reflection.gem_env_contrast_factor = value.max(0.0001);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RefractionFresnelPower") {
		out.reflection.gem_refraction_fresnel_power_factor = value.max(0.0001);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RefractionStrength") {
		out.reflection.gem_refraction_strength_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GemChromaticAberration") {
		out.reflection.gem_chromatic_aberration_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GemParticleLoop") {
		out.reflection.gem_particle_loop_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_GemParticleColor") {
		out.reflection.gem_particle_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GemVRParallaxStrength") {
		out.reflection.gem_vr_parallax_strength_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_UseAnisotropy") {
		out.reflection.anisotropy_enabled_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AnisotropyScale") {
		out.reflection.anisotropy_scale_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AnisotropyShift") {
		out.reflection.anisotropy_shift_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AnisotropyShiftNoiseScale") {
		out.reflection.anisotropy_shift_noise_scale_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AnisotropySpecularStrength") {
		out.reflection.anisotropy_specular_strength_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AnisotropyTangentWidth") {
		out.reflection.anisotropy_tangent_width_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AnisotropyBitangentWidth") {
		out.reflection.anisotropy_bitangent_width_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Anisotropy2Reflection") {
		out.reflection.anisotropy_to_reflection_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Anisotropy2MatCap") {
		out.reflection.anisotropy_to_matcap_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Anisotropy2MatCap2nd") {
		out.reflection.anisotropy_to_second_matcap_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Anisotropy2ndShift") {
		out.reflection.anisotropy_second_shift_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Anisotropy2ndShiftNoiseScale") {
		out.reflection.anisotropy_second_shift_noise_scale_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Anisotropy2ndSpecularStrength") {
		out.reflection.anisotropy_second_specular_strength_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Anisotropy2ndTangentWidth") {
		out.reflection.anisotropy_second_tangent_width_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Anisotropy2ndBitangentWidth") {
		out.reflection.anisotropy_second_bitangent_width_factor = value.max(0.0);
	}

	out.rim.enabled_factor = unavatar_material_float_param(extras, "_UseRim").unwrap_or(0.0).clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_RimColor") {
		out.rim.color_factor = value;
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("rimMultiplyTextureIndex").or_else(|| m.get("rim_multiply_texture_index")))) {
		out.rim.texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimMainStrength") {
		out.rim.main_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimBorder") {
		out.rim.border_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimBlur") {
		out.rim.blur_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimFresnelPower") {
		out.rim.fresnel_power_factor = value.clamp(0.01, 50.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimEnableLighting") {
		out.rim.enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon
		.and_then(|m| json_u32(m.get("rimBlendMode").or_else(|| m.get("rim_blend_mode"))))
		.or_else(|| unavatar_material_float_param(extras, "_RimBlendMode").map(float_to_u32_saturating))
	{
		out.rim.blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimShadowMask") {
		out.rim.shadow_mask_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimApplyTransparency") {
		out.rim.apply_transparency_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimNormalStrength") {
		out.rim.normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimBackfaceMask") {
		out.rim.backface_mask_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimDirStrength") {
		out.rim.directional_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimDirRange") {
		out.rim.directional_range_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_RimIndirColor") {
		out.rim.indirect_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimIndirRange") {
		out.rim.indirect_range_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimIndirBorder") {
		out.rim.indirect_border_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimIndirBlur") {
		out.rim.indirect_blur_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_UseRimShade") {
		out.rim.shade_enabled_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_RimShadeColor") {
		out.rim.shade_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimShadeBorder") {
		out.rim.shade_border_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimShadeBlur") {
		out.rim.shade_blur_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimShadeFresnelPower") {
		out.rim.shade_fresnel_power_factor = value.clamp(0.01, 50.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimShadeNormalStrength") {
		out.rim.shade_normal_strength_factor = value.clamp(0.0, 1.0);
	}

	out.backlight.enabled_factor = unavatar_material_float_param(extras, "_UseBacklight")
		.unwrap_or(0.0)
		.clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_BacklightColor") {
		out.backlight.color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BacklightMainStrength") {
		out.backlight.main_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BacklightNormalStrength") {
		out.backlight.normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BacklightBorder") {
		out.backlight.border_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BacklightBlur") {
		out.backlight.blur_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BacklightDirectivity") {
		out.backlight.directivity_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BacklightViewStrength") {
		out.backlight.view_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BacklightReceiveShadow") {
		out.backlight.receive_shadow_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BacklightBackfaceMask") {
		out.backlight.backface_mask_factor = value.clamp(0.0, 1.0);
	}

	out.emission.enabled_factor = unavatar_material_float_param(extras, "_UseEmission").unwrap_or(0.0).clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_EmissionColor") {
		out.emission.color_factor = value;
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("emissionTextureIndex").or_else(|| m.get("emission_texture_index")))) {
		out.emission.texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("emissionGradationTextureIndex")
				.or_else(|| m.get("emission_gradation_texture_index")),
		)
	}) {
		out.emission.gradation_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_EmissionMainStrength") {
		out.emission.main_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_EmissionBlend") {
		out.emission.blend_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_EmissionBlendMode").map(float_to_u32_saturating) {
		out.emission.blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_EmissionUseGrad") {
		out.emission.gradation_enabled_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_EmissionGradSpeed") {
		out.emission.gradation_speed_factor = value;
	}

	out.outline.enabled_factor = unavatar_material_float_param(extras, "_UseOutline")
		.unwrap_or_else(|| {
			if source_shader.to_ascii_lowercase().contains("outline") {
				1.0
			} else {
				0.0
			}
		})
		.clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_OutlineColor") {
		out.outline.color_factor = value;
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_OutlineLitColor") {
		out.outline.lit_color_factor = value;
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("outlineTextureIndex").or_else(|| m.get("outline_texture_index")))) {
		out.outline.texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("outlineWidthMultiplyTextureIndex")
				.or_else(|| m.get("outline_width_multiply_texture_index")),
		)
	}) {
		out.outline.width_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon
		.and_then(|m| json_f32(m.get("outlineWidthFactor").or_else(|| m.get("outline_width_factor"))))
		.or_else(|| unavatar_material_float_param(extras, "_OutlineWidth"))
	{
		out.outline.width_factor = value * liltoon_outline_width_scale;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_OutlineFixWidth") {
		out.outline.fix_width_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_OutlineEnableLighting") {
		out.outline.enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_OutlineLitScale") {
		out.outline.lit_scale_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_OutlineLitOffset") {
		out.outline.lit_offset_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_OutlineLitApplyTex") {
		out.outline.lit_apply_tex_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_OutlineLitShadowReceive") {
		out.outline.lit_shadow_receive_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_OutlineZBias") {
		out.outline.z_bias_factor = value;
	}
	if unavatar_material_liltoon_alpha_mask_enabled(extras) {
		if let Some(value) = unavatar_material_float_param(extras, "_AlphaMaskMode") {
			out.alpha_mask.mode_factor = value.clamp(0.0, 4.0);
		}
		if let Some(value) = mtoon.and_then(|m| json_usize(m.get("alphaMaskTextureIndex").or_else(|| m.get("alpha_mask_texture_index")))) {
			out.alpha_mask.texture_index = Some(value);
		}
		if let Some(value) = unavatar_material_float_param(extras, "_AlphaMaskScale") {
			out.alpha_mask.scale_factor = value;
		}
		if let Some(value) = unavatar_material_float_param(extras, "_AlphaMaskValue") {
			out.alpha_mask.value_factor = value;
		}
	}
	if let Some(value) = unavatar_material_float_param(extras, "_SrcBlend") {
		out.blend_state.source_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_DstBlend") {
		out.blend_state.destination_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BlendOp") {
		out.blend_state.operation_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_SrcBlendAlpha") {
		out.blend_state.alpha_source_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_DstBlendAlpha") {
		out.blend_state.alpha_destination_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BlendOpAlpha") {
		out.blend_state.alpha_operation_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_SrcBlendAlphaFA") {
		out.blend_state.forward_add_alpha_source_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_DstBlendAlphaFA") {
		out.blend_state.forward_add_alpha_destination_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_BlendOpAlphaFA") {
		out.blend_state.forward_add_alpha_operation_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AlphaBoostFA") {
		out.blend_state.alpha_boost_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_SubpassCutoff") {
		out.blend_state.subpass_cutoff_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AlphaToMask") {
		out.blend_state.alpha_to_mask_factor = value.clamp(0.0, 1.0);
	}
	Some(out)
}

fn float_to_u32_saturating(value: f32) -> u32 {
	value.round().max(0.0) as u32
}

fn liltoon_like_blend_mode(value: u32) -> UnaLilToonLikeBlendMode {
	match value {
		0 => UnaLilToonLikeBlendMode::Normal,
		2 => UnaLilToonLikeBlendMode::Screen,
		3 => UnaLilToonLikeBlendMode::Multiply,
		_ => UnaLilToonLikeBlendMode::Add,
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
	} else if let Some(value) =
		unavatar_material_float_param(extras, "_ZWrite").or_else(|| unavatar_material_float_param(extras, "_ZWriteMode"))
	{
		out.transparent_with_z_write = value > 0.5;
	} else if source_shader.to_ascii_lowercase().contains("twopass") {
		out.transparent_with_z_write = true;
	}
	if let Some(value) = json_vec3(mtoon.get("shadeColorFactor").or_else(|| mtoon.get("shade_color_factor"))).or_else(|| {
		unavatar_material_feature_enabled(extras, "_UseShadow")
			.unwrap_or(true)
			.then(|| {
				unavatar_material_color_param_rgb(extras, "_ShadeColor")
					.or_else(|| unavatar_material_color_param_rgb(extras, "_ShadowColor"))
			})
			.flatten()
	}) {
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
	if let Some(value) = json_vec3(mtoon.get("matcapFactor").or_else(|| mtoon.get("matcap_factor"))).or_else(|| {
		unavatar_material_feature_enabled(extras, "_UseMatCap")
			.unwrap_or(true)
			.then(|| unavatar_material_color_param_rgb(extras, "_MatCapColor"))
			.flatten()
	}) {
		out.matcap_factor = value;
	}
	if let Some(value) = json_usize(mtoon.get("matcapTextureIndex").or_else(|| mtoon.get("matcap_texture_index"))) {
		out.matcap_texture_index = Some(value);
	}
	if let Some(value) = json_vec3(
		mtoon
			.get("parametricRimColorFactor")
			.or_else(|| mtoon.get("parametric_rim_color_factor")),
	)
	.or_else(|| {
		unavatar_material_feature_enabled(extras, "_UseRim")
			.unwrap_or(true)
			.then(|| {
				unavatar_material_color_param_rgb(extras, "_RimColor").map(|color| {
					let strength = unavatar_material_float_param(extras, "_RimMainStrength").unwrap_or(1.0).max(0.0);
					[color[0] * strength, color[1] * strength, color[2] * strength]
				})
			})
			.flatten()
	}) {
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
	if let Some(value) = json_vec3(mtoon.get("outlineColorFactor").or_else(|| mtoon.get("outline_color_factor")))
		.or_else(|| unavatar_material_color_param_rgb(extras, "_OutlineColor"))
	{
		out.outline_color_factor = value;
	}
	if let Some(value) = json_f32(
		mtoon
			.get("outlineLightingMixFactor")
			.or_else(|| mtoon.get("outline_lighting_mix_factor")),
	) {
		out.outline_lighting_mix_factor = value;
	}
	if let Some(value) = json_usize(
		mtoon
			.get("uvAnimationMaskTextureIndex")
			.or_else(|| mtoon.get("uv_animation_mask_texture_index")),
	) {
		out.uv_animation_mask_texture_index = Some(value);
	}
	if let Some(value) = json_vec4(mtoon.get("uvOffsetScale").or_else(|| mtoon.get("uv_offset_scale"))) {
		out.uv_offset_scale = value;
	}
	if let Some(value) = json_f32(
		mtoon
			.get("uvAnimationScrollXSpeedFactor")
			.or_else(|| mtoon.get("uv_animation_scroll_x_speed_factor")),
	) {
		out.uv_animation_scroll_x_speed_factor = value;
	}
	if let Some(value) = json_f32(
		mtoon
			.get("uvAnimationScrollYSpeedFactor")
			.or_else(|| mtoon.get("uv_animation_scroll_y_speed_factor")),
	) {
		out.uv_animation_scroll_y_speed_factor = value;
	}
	if let Some(value) = json_f32(
		mtoon
			.get("uvAnimationRotationSpeedFactor")
			.or_else(|| mtoon.get("uv_animation_rotation_speed_factor")),
	) {
		out.uv_animation_rotation_speed_factor = value;
	}
	Some(out)
}

fn json_bool(value: Option<&Value>) -> Option<bool> {
	value.and_then(Value::as_bool)
}

fn json_string(value: Option<&Value>) -> Option<String> {
	value.and_then(Value::as_str).filter(|value| !value.is_empty()).map(str::to_string)
}

fn unavatar_material_float_param(extras: &Value, name: &str) -> Option<f32> {
	extras
		.get("floatParams")
		.or_else(|| extras.get("float_params"))
		.and_then(|params| params.get(name))
		.and_then(json_number_f32)
}

fn unavatar_material_feature_enabled(extras: &Value, name: &str) -> Option<bool> {
	unavatar_material_float_param(extras, name).map(|value| value > 0.5)
}

fn unavatar_material_liltoon_alpha_mask_enabled(extras: &Value) -> bool {
	unavatar_material_has_enabled_keyword(extras, &["_COLOROVERLAY_ON", "LIL_FEATURE_ALPHAMASK", "LIL_FEATURE_AlphaMask"])
}

fn unavatar_material_has_enabled_keyword(extras: &Value, names: &[&str]) -> bool {
	const KEYWORD_FIELDS: &[&str] = &[
		"enabledKeywords",
		"enabled_keywords",
		"shaderKeywords",
		"shader_keywords",
		"keywords",
	];
	KEYWORD_FIELDS
		.iter()
		.any(|field| unavatar_material_keyword_field_has_any(extras.get(*field), names))
}

fn unavatar_material_keyword_field_has_any(value: Option<&Value>, names: &[&str]) -> bool {
	match value {
		Some(Value::Array(values)) => values.iter().any(|value| {
			value
				.as_str()
				.is_some_and(|keyword| names.iter().any(|name| keyword.eq_ignore_ascii_case(name)))
		}),
		Some(Value::Object(values)) => values
			.iter()
			.any(|(keyword, value)| value.as_bool().unwrap_or(false) && names.iter().any(|name| keyword.eq_ignore_ascii_case(name))),
		_ => false,
	}
}

fn unavatar_material_color_param_rgb(extras: &Value, name: &str) -> Option<[f32; 3]> {
	extras
		.get("colorParams")
		.or_else(|| extras.get("color_params"))
		.and_then(|params| params.get(name))
		.and_then(|value| json_vec3(Some(value)))
}

fn unavatar_material_color_param_rgba(extras: &Value, name: &str) -> Option<[f32; 4]> {
	extras
		.get("colorParams")
		.or_else(|| extras.get("color_params"))
		.and_then(|params| params.get(name))
		.and_then(|value| json_vec4(Some(value)))
}

fn unavatar_material_uv_offset_scales(extras: &Value) -> BTreeMap<String, [f32; 4]> {
	let mut out = BTreeMap::new();
	let Some(values) = extras
		.get("textureUvOffsetScales")
		.or_else(|| extras.get("texture_uv_offset_scales"))
		.and_then(Value::as_object)
	else {
		return out;
	};
	for (key, value) in values {
		if let Some(offset_scale) = json_vec4(Some(value)) {
			out.insert(key.clone(), offset_scale);
		}
	}
	out
}

fn unavatar_material_uv_mode_factors(extras: &Value) -> BTreeMap<String, f32> {
	let mut out = BTreeMap::new();
	let Some(values) = extras
		.get("textureUvModeFactors")
		.or_else(|| extras.get("texture_uv_mode_factors"))
		.and_then(Value::as_object)
	else {
		return out;
	};
	for (key, value) in values {
		if let Some(mode) = json_number_f32(value) {
			out.insert(key.clone(), mode);
		}
	}
	out
}

fn json_number_f32(value: &Value) -> Option<f32> {
	value.as_f64().or_else(|| value.as_i64().map(|v| v as f64)).map(|v| v as f32)
}

fn json_f32(value: Option<&Value>) -> Option<f32> {
	value.and_then(Value::as_f64).map(|v| v as f32)
}

fn json_usize(value: Option<&Value>) -> Option<usize> {
	value.and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok())
}

fn json_u32(value: Option<&Value>) -> Option<u32> {
	value.and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok())
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

fn json_vec4(value: Option<&Value>) -> Option<[f32; 4]> {
	let array = value?.as_array()?;
	let x = array.first()?.as_f64()? as f32;
	let y = array.get(1)?.as_f64()? as f32;
	let z = array.get(2)?.as_f64()? as f32;
	let w = array.get(3)?.as_f64()? as f32;
	Some([x, y, z, w])
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
	let tangents = reader.read_tangents().map(|it| it.collect());
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
		tangents,
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
			probe_anchor_node: None,
			local_bounds: None,
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
			apply_unavatar_modular_avatar(&mut scene, unavatar, &mut report);
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
	fn root_json_texture_samplers_map_to_image_sources() {
		let root = serde_json::json!({
			"images": [
				{
					"uri": "normal.png",
					"extras": {
						"UN_avatar_image": {
							"colorSpace": "linear",
							"textureType": "NormalMap",
							"textureShape": "2D",
							"sRGB": false
						}
					}
				},
				{"uri": "base.png"}
			],
			"textures": [
				{ "source": 0, "sampler": 0 },
				{ "source": 1 }
			],
			"samplers": [
				{ "magFilter": 9728, "minFilter": 9987, "wrapS": 33071, "wrapT": 33648 }
			]
		});

		let samplers = image_samplers_from_root_json(&root);

		assert_eq!(samplers.len(), 2);
		assert_eq!(
			samplers[0],
			Some(UnaTextureSampler {
				mag_filter: UnaTextureFilterMode::Nearest,
				min_filter: UnaTextureFilterMode::Linear,
				wrap_s: UnaTextureWrapMode::ClampToEdge,
				wrap_t: UnaTextureWrapMode::MirroredRepeat,
			})
		);
		assert_eq!(samplers[1], Some(UnaTextureSampler::default()));
		let metadata = collect_glb_image_source_metadata(&root, &[]);
		let source = metadata[0].as_ref().unwrap();
		assert_eq!(source.color_space.as_deref(), Some("linear"));
		assert_eq!(source.texture_type.as_deref(), Some("NormalMap"));
		assert_eq!(source.texture_shape.as_deref(), Some("2D"));
		assert_eq!(source.srgb, Some(false));
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
							"unMaterialModel": "liltoon_like",
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
							"sampler": {{"magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 10497}},
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
		assert_eq!(
			scene.image_sources[0].as_ref().unwrap().sampler,
			Some(UnaTextureSampler {
				mag_filter: UnaTextureFilterMode::Nearest,
				min_filter: UnaTextureFilterMode::Nearest,
				wrap_s: UnaTextureWrapMode::ClampToEdge,
				wrap_t: UnaTextureWrapMode::Repeat,
			})
		);
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
							"unMaterialModel": "liltoon_like",
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
						"unMaterialModel": "liltoon_like",
					"mtoon": {
						"shadowColorTextureIndex": 8,
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
		assert!(scene.nodes[3].visible);
		assert!(!scene.nodes[2].visible);
		assert_eq!(scene.meshes[0][0].morph_target_names, vec!["Shrink"]);
		assert_eq!(scene.meshes[0][0].default_morph_weights, vec![0.5]);
		assert_eq!(scene.materials[0].shading, UnaShadingModel::LilToonLike);
		assert!(scene.materials[0].liltoon_like.is_some());
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
		assert!(scene.nodes[3].visible);
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
		let hidden_opaque = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonOutline"
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
		let source_param_blend = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "lilToon",
			"floatParams": { "_TransparentMode": 2.0 }
		});
		let source_param_cutout = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "lilToon",
			"floatParams": { "_AlphaMode": 1.0 }
		});
		let transparent_shader_with_opaque_source_param = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonTransparent",
			"floatParams": { "_TransparentMode": 0.0 }
		});
		let alpha_blend_state = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonTwoPassTransparentOutline",
			"floatParams": { "_SrcBlend": 1.0, "_DstBlend": 10.0, "_TransparentMode": 0.0 }
		});
		let opaque_blend_state = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonOutline",
			"floatParams": { "_SrcBlend": 1.0, "_DstBlend": 0.0, "_Cutoff": 0.5 }
		});
		let alpha_to_mask_state = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonOutline",
			"floatParams": { "_SrcBlend": 1.0, "_DstBlend": 0.0, "_AlphaToMask": 1.0 }
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
			unavatar_material_inferred_alpha_mode(Some(&hidden_opaque), UnaAlphaMode::Blend, Some(0.001), true),
			Some(UnaAlphaMode::Opaque)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&queue_cutout), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Mask)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&queue_transparent), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Blend)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&source_param_blend), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Blend)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&source_param_cutout), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Mask)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&transparent_shader_with_opaque_source_param), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Blend)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&alpha_blend_state), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Blend)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&opaque_blend_state), UnaAlphaMode::Mask, Some(0.5), true),
			Some(UnaAlphaMode::Opaque)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&alpha_to_mask_state), UnaAlphaMode::Opaque, None, true),
			Some(UnaAlphaMode::Mask)
		);
		assert_eq!(
			unavatar_material_alpha_cutoff_from_source_params(&serde_json::json!({
				"floatParams": { "_Cutoff": 0.25 }
			})),
			Some(0.25)
		);
		assert_eq!(
			unavatar_material_cull_mode_from_source_params(&serde_json::json!({
				"floatParams": { "_Cull": 0.0 }
			})),
			Some(UnaCullMode::Off)
		);
		assert_eq!(
			unavatar_material_cull_mode_from_source_params(&serde_json::json!({
				"floatParams": { "_Cull": 1.0 }
			})),
			Some(UnaCullMode::Front)
		);
		assert_eq!(
			unavatar_material_cull_mode_from_source_params(&serde_json::json!({
				"floatParams": { "_CullMode": 2.0 }
			})),
			Some(UnaCullMode::Back)
		);
		assert_eq!(
			unavatar_material_emissive_factor_from_source_params(&serde_json::json!({
				"floatParams": { "_UseEmission": 1.0, "_EmissionMainStrength": 2.0 },
				"colorParams": { "_EmissionColor": [0.1, 0.2, 0.3, 1.0] }
			})),
			Some([0.2, 0.4, 0.6])
		);
		assert_eq!(
			unavatar_material_emissive_factor_from_source_params(&serde_json::json!({
				"floatParams": { "_UseEmission": 0.0 },
				"colorParams": { "_EmissionColor": [0.1, 0.2, 0.3, 1.0] }
			})),
			Some([0.0, 0.0, 0.0])
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
			UnaMaterialPbr {
				alpha_mode: UnaAlphaMode::Opaque,
				alpha_cutoff: 0.001,
				base_color_texture_index: Some(1),
				unavatar_material: Some(serde_json::json!({
					"family": "liltoon",
					"sourceShader": "Hidden/lilToonOutline"
				})),
				..Default::default()
			},
			UnaMaterialPbr {
				alpha_mode: UnaAlphaMode::Mask,
				alpha_cutoff: 0.0,
				base_color_texture_index: Some(1),
				unavatar_material: Some(serde_json::json!({
					"family": "liltoon",
					"sourceShader": "Hidden/lilToonTransparentOutline"
				})),
				..Default::default()
			},
		];

		refine_liltoon_alpha_from_images(&mut materials, &[opaque_image, transparent_image, translucent_image]);

		assert_eq!(materials[0].alpha_mode, UnaAlphaMode::Opaque);
		assert_eq!(materials[1].alpha_mode, UnaAlphaMode::Mask);
		assert_eq!(materials[2].alpha_mode, UnaAlphaMode::Blend);
		assert_eq!(materials[3].alpha_mode, UnaAlphaMode::Opaque);
		assert_eq!(materials[4].alpha_mode, UnaAlphaMode::Mask);
		assert_eq!(materials[4].alpha_cutoff, 1.0 / 255.0);
	}

	#[test]
	fn infers_liltoon_twopass_transparent_zwrite_from_shader_name() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonTwoPassTransparentOutline",
			"mtoon": {
				"shadowColorTextureIndex": 8,
				"shadowStrengthMaskTextureIndex": 9
			}
		});

		let mtoon = unavatar_mtoon_from_extras(&extras).expect("mtoon material");

		assert!(mtoon.transparent_with_z_write);
	}

	#[test]
	fn source_zwrite_param_overrides_twopass_shader_name() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonTwoPassTransparentOutline",
			"floatParams": { "_ZWrite": 0.0 },
			"mtoon": {
				"shadowColorTextureIndex": 8
			}
		});

		let mtoon = unavatar_mtoon_from_extras(&extras).expect("mtoon material");

		assert!(!mtoon.transparent_with_z_write);
	}

	#[test]
	fn source_color_params_fill_untoon_v2_fields() {
		let extras: Value = serde_json::from_str(
			r#"{
				"family": "liltoon",
				"sourceShader": "lilToon",
				"renderQueue": 2461,
				"enabledKeywords": ["_COLOROVERLAY_ON"],
				"textureUvOffsetScales": {
					"_EmissionMap": [0.1, 0.2, 2.0, 3.0],
					"_MatCapTex": [0.0, 0.25, 1.0, 0.5]
				},
				"textureUvModeFactors": {
					"_EmissionMap": 1.0,
					"_Bump2ndMap": 2.0
				},
				"floatParams": {
					"_UseShadow": 1.0,
					"_UseMatCap": 1.0,
					"_UseReflection": 1.0,
					"_UseRim": 1.0,
					"_UseBumpMap2nd": 1.0,
					"_BumpScale2nd": 0.33,
					"_ShadowStrength": 0.75,
					"_ShadowBorder": 0.42,
					"_ShadowBlur": 0.18,
					"_ShadowBorderRange": 0.08,
					"_ShadowMainStrength": 0.35,
					"_ShadowEnvStrength": 0.45,
					"_ShadowNormalStrength": 0.55,
					"_ShadowReceive": 0.65,
					"_Shadow2ndBorder": 0.31,
					"_Shadow2ndBlur": 0.21,
					"_Shadow2ndNormalStrength": 0.71,
					"_Shadow2ndReceive": 0.81,
					"_Shadow3rdBorder": 0.41,
					"_Shadow3rdBlur": 0.32,
					"_Shadow3rdNormalStrength": 0.72,
					"_Shadow3rdReceive": 0.82,
					"_MatCapMainStrength": 0.5,
					"_MatCapBlend": 0.25,
					"_MatCapEnableLighting": 0.75,
					"_MatCapBlendMode": 2.0,
					"_MatCapNormalStrength": 0.66,
					"_MatCapShadowMask": 0.57,
					"_MatCapApplyTransparency": 0.47,
					"_MatCapLod": 2.5,
					"_MatCapBackfaceMask": 0.35,
					"_MatCapPerspective": 0.64,
					"_MatCapZRotCancel": 0.74,
					"_MatCapVRParallaxStrength": 0.84,
					"_UseMatCap2nd": 1.0,
					"_MatCap2ndMainStrength": 0.58,
					"_MatCap2ndBlend": 0.68,
					"_MatCap2ndEnableLighting": 0.78,
					"_MatCap2ndShadowMask": 0.48,
					"_MatCap2ndApplyTransparency": 0.38,
					"_MatCap2ndBlendMode": 1.0,
					"_MatCap2ndNormalStrength": 0.88,
					"_MatCap2ndLod": 1.5,
					"_MatCap2ndBackfaceMask": 0.45,
					"_MatCap2ndPerspective": 0.54,
					"_MatCap2ndZRotCancel": 0.44,
					"_MatCap2ndVRParallaxStrength": 0.34,
					"_Smoothness": 0.6,
					"_Metallic": 0.2,
					"_Reflectance": 0.4,
					"_ApplySpecular": 0.8,
					"_ApplySpecularFA": 0.9,
					"_ApplyReflection": 0.7,
					"_ReflectionApplyTransparency": 0.67,
					"_SpecularToon": 1.0,
					"_SpecularBorder": 0.37,
					"_SpecularBlur": 0.12,
					"_SpecularNormalStrength": 0.88,
					"_ReflectionNormalStrength": 0.77,
					"_ReflectionCubeEnableLighting": 0.69,
					"_ReflectionCubeOverride": 1.0,
					"_ReflectionBlendMode": 3.0,
					"_UseAnisotropy": 1.0,
					"_AnisotropyScale": 0.8,
					"_AnisotropyShift": -0.2,
					"_AnisotropyShiftNoiseScale": 0.3,
					"_AnisotropySpecularStrength": 0.7,
					"_AnisotropyTangentWidth": 0.4,
					"_AnisotropyBitangentWidth": 0.5,
					"_Anisotropy2Reflection": 0.6,
					"_Anisotropy2MatCap": 0.7,
					"_Anisotropy2MatCap2nd": 0.8,
					"_Anisotropy2ndShift": 0.2,
					"_Anisotropy2ndShiftNoiseScale": 0.35,
					"_Anisotropy2ndSpecularStrength": 0.45,
					"_Anisotropy2ndTangentWidth": 0.55,
					"_Anisotropy2ndBitangentWidth": 0.65,
					"_RimMainStrength": 0.4,
					"_RimBorder": 0.3,
					"_RimBlur": 0.2,
					"_RimFresnelPower": 4.0,
					"_RimEnableLighting": 0.6,
					"_RimBlendMode": 2.0,
					"_RimShadowMask": 0.91,
					"_RimApplyTransparency": 0.83,
					"_RimNormalStrength": 0.82,
					"_RimBackfaceMask": 0.73,
					"_RimDirStrength": 0.52,
					"_RimDirRange": 0.42,
					"_RimIndirRange": 0.32,
					"_RimIndirBorder": 0.22,
					"_RimIndirBlur": 0.12,
					"_UseRimShade": 1.0,
					"_RimShadeBorder": 0.44,
					"_RimShadeBlur": 0.22,
					"_RimShadeFresnelPower": 2.5,
					"_RimShadeNormalStrength": 0.62,
					"_UseBacklight": 1.0,
					"_BacklightMainStrength": 0.72,
					"_BacklightNormalStrength": 0.82,
					"_BacklightBorder": 0.32,
					"_BacklightBlur": 0.23,
					"_BacklightDirectivity": 7.0,
					"_BacklightViewStrength": 0.62,
					"_BacklightReceiveShadow": 0.52,
					"_BacklightBackfaceMask": 0.42,
					"_UseEmission": 1.0,
					"_EmissionMainStrength": 0.45,
					"_EmissionBlend": 0.55,
					"_EmissionBlendMode": 3.0,
					"_EmissionUseGrad": 1.0,
					"_EmissionGradSpeed": 1.5,
					"_UseOutline": 1.0,
					"_OutlineWidth": 0.03,
					"_OutlineFixWidth": 0.25,
					"_OutlineEnableLighting": 0.65,
					"_OutlineLitScale": 9.0,
					"_OutlineLitOffset": -7.0,
					"_OutlineLitApplyTex": 1.0,
					"_OutlineLitShadowReceive": 1.0,
					"_OutlineZBias": -0.01,
					"_AlphaMaskMode": 2.0,
					"_AlphaMaskScale": 0.8,
					"_AlphaMaskValue": 0.1,
					"_SrcBlend": 1.0,
					"_DstBlend": 10.0,
					"_BlendOp": 0.0,
					"_SrcBlendAlpha": 1.0,
					"_DstBlendAlpha": 10.0,
					"_BlendOpAlpha": 0.0,
					"_SrcBlendAlphaFA": 0.0,
					"_DstBlendAlphaFA": 1.0,
					"_BlendOpAlphaFA": 4.0,
					"_AlphaBoostFA": 10.0,
					"_SubpassCutoff": 0.4,
					"_AlphaToMask": 1.0,
					"_LightMinLimit": 0.06,
					"_LightMaxLimit": 0.9,
					"_MonochromeLighting": 0.25,
					"_VertexLightStrength": 0.35,
					"_AAStrength": 1.25,
					"_GSAAStrength": 0.5,
					"_UseMain2ndTex": 1.0,
					"_Main2ndTexBlendMode": 1.0,
					"_Main2ndEnableLighting": 0.25,
					"_UseMain3rdTex": 1.0,
					"_Main3rdTexBlendMode": 3.0,
					"_Main3rdEnableLighting": 0.75
				},
				"colorParams": {
					"_ShadeColor": [0.7, 0.8, 0.9, 1.0],
					"_ShadowBorderColor": [0.2, 0.3, 0.4, 1.0],
					"_Shadow2ndColor": [0.4, 0.5, 0.6, 0.7],
					"_Shadow3rdColor": [0.3, 0.4, 0.5, 0.6],
					"_MatCapColor": [0.2, 0.4, 0.6, 0.7],
					"_MatCap2ndColor": [0.3, 0.5, 0.7, 0.9],
					"_ReflectionColor": [0.9, 0.8, 0.7, 0.6],
					"_ReflectionCubeColor": [0.6, 0.7, 0.8, 1.0],
					"_RimColor": [0.1, 0.2, 0.3, 1.0],
					"_RimIndirColor": [0.4, 0.5, 0.6, 0.7],
					"_RimShadeColor": [0.6, 0.5, 0.4, 0.7],
					"_BacklightColor": [1.1, 1.2, 1.3, 0.8],
					"_EmissionColor": [0.5, 0.4, 0.3, 0.8],
					"_OutlineColor": [0.01, 0.02, 0.03, 1.0],
					"_OutlineLitColor": [1.0, 0.2, 0.0, 0.4]
				},
				"mtoon": {
					"shadowColorTextureIndex": 8,
					"shadowStrengthMaskTextureIndex": 9,
					"shadowBorderMaskTextureIndex": 10,
					"shadowBlurMaskTextureIndex": 11,
					"rimMultiplyTextureIndex": 12,
					"emissionTextureIndex": 13,
					"emissionGradationTextureIndex": 29,
					"outlineWidthMultiplyTextureIndex": 14,
					"outlineTextureIndex": 15,
					"reflectionColorTextureIndex": 16,
					"smoothnessTextureIndex": 17,
					"metallicGlossTextureIndex": 18,
					"main2ndTextureIndex": 30,
					"main3rdTextureIndex": 31,
					"matcapTextureIndex": 19,
					"matcapBlendMaskTextureIndex": 20,
					"matcap2ndTextureIndex": 22,
					"matcap2ndBlendMaskTextureIndex": 23,
					"normal2ndTextureIndex": 24,
					"alphaMaskTextureIndex": 21,
					"gradationMapTextureIndex": 25,
					"anisotropyTangentTextureIndex": 26,
					"anisotropyScaleMaskTextureIndex": 27,
					"anisotropyShiftNoiseMaskTextureIndex": 28,
					"mainTexHsvgFactor": [0.12, 0.8, 1.2, 0.9]
				}
			}"#,
		)
		.expect("test extras JSON");

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon_like material");
		let mtoon = unavatar_mtoon_from_extras(&extras).expect("legacy mtoon material");

		assert_eq!(liltoon_like.source_profile, UnaLilToonLikeSourceProfile::Liltoon);
		assert_eq!(liltoon_like.main_color.main_texture_hsvg_factor, [0.12, 0.8, 1.2, 0.9]);
		assert_eq!(liltoon_like.main_color.gradation_enabled_factor, 1.0);
		assert_eq!(liltoon_like.main_color.gradation_texture_index, Some(25));
		assert_eq!(liltoon_like.main_color.second_enabled_factor, 1.0);
		assert_eq!(liltoon_like.main_color.second_texture_index, Some(30));
		assert_eq!(liltoon_like.main_color.second_blend_mode, UnaLilToonLikeBlendMode::Add);
		assert_eq!(liltoon_like.main_color.second_enable_lighting_factor, 0.25);
		assert_eq!(liltoon_like.main_color.third_enabled_factor, 1.0);
		assert_eq!(liltoon_like.main_color.third_texture_index, Some(31));
		assert_eq!(liltoon_like.main_color.third_blend_mode, UnaLilToonLikeBlendMode::Multiply);
		assert_eq!(liltoon_like.main_color.third_enable_lighting_factor, 0.75);
		assert_eq!(
			liltoon_like.texture_uv_offset_scales.get("_EmissionMap"),
			Some(&[0.1, 0.2, 2.0, 3.0])
		);
		assert_eq!(
			liltoon_like.texture_uv_offset_scales.get("_MatCapTex"),
			Some(&[0.0, 0.25, 1.0, 0.5])
		);
		assert_eq!(liltoon_like.texture_uv_mode_factors.get("_EmissionMap"), Some(&1.0));
		assert_eq!(liltoon_like.texture_uv_mode_factors.get("_Bump2ndMap"), Some(&2.0));
		assert_eq!(liltoon_like.rendering.render_queue_number, Some(2461));
		assert_eq!(liltoon_like.rendering.light_min_limit_factor, 0.06);
		assert_eq!(liltoon_like.rendering.light_max_limit_factor, 0.9);
		assert_eq!(liltoon_like.rendering.monochrome_lighting_factor, 0.25);
		assert_eq!(liltoon_like.rendering.vertex_light_strength_factor, 0.35);
		assert_eq!(liltoon_like.rendering.aa_strength_factor, 1.25);
		assert_eq!(liltoon_like.rendering.gsaa_strength_factor, 0.5);
		assert_eq!(liltoon_like.normal.second_enabled_factor, 1.0);
		assert_eq!(liltoon_like.normal.second_texture_index, Some(24));
		assert_eq!(liltoon_like.normal.second_scale_factor, 0.33);
		assert_eq!(liltoon_like.shadow.color_factor, [0.7, 0.8, 0.9]);
		assert_eq!(liltoon_like.shadow.color_texture_index, Some(8));
		assert_eq!(liltoon_like.shadow.strength_mask_texture_index, Some(9));
		assert_eq!(liltoon_like.shadow.border_mask_texture_index, Some(10));
		assert_eq!(liltoon_like.shadow.blur_mask_texture_index, Some(11));
		assert_eq!(liltoon_like.shadow.strength_factor, 0.75);
		assert_eq!(liltoon_like.shadow.border_factor, 0.42);
		assert_eq!(liltoon_like.shadow.blur_factor, 0.18);
		assert_eq!(liltoon_like.shadow.border_range_factor, 0.08);
		assert_eq!(liltoon_like.shadow.main_strength_factor, 0.35);
		assert_eq!(liltoon_like.shadow.env_strength_factor, 0.45);
		assert_eq!(liltoon_like.shadow.border_color_factor, [0.2, 0.3, 0.4]);
		assert_eq!(liltoon_like.shadow.normal_strength_factor, 0.55);
		assert_eq!(liltoon_like.shadow.receive_factor, 0.65);
		assert_eq!(liltoon_like.shadow.second_color_factor, [0.4, 0.5, 0.6, 0.7]);
		assert_eq!(liltoon_like.shadow.second_border_factor, 0.31);
		assert_eq!(liltoon_like.shadow.second_blur_factor, 0.21);
		assert_eq!(liltoon_like.shadow.second_normal_strength_factor, 0.71);
		assert_eq!(liltoon_like.shadow.second_receive_factor, 0.81);
		assert_eq!(liltoon_like.shadow.third_color_factor, [0.3, 0.4, 0.5, 0.6]);
		assert_eq!(liltoon_like.shadow.third_border_factor, 0.41);
		assert_eq!(liltoon_like.shadow.third_blur_factor, 0.32);
		assert_eq!(liltoon_like.shadow.third_normal_strength_factor, 0.72);
		assert_eq!(liltoon_like.shadow.third_receive_factor, 0.82);
		assert_eq!(liltoon_like.matcap.color_factor, [0.2, 0.4, 0.6]);
		assert_eq!(liltoon_like.matcap.color_alpha_factor, 0.7);
		assert_eq!(liltoon_like.matcap.texture_index, Some(19));
		assert_eq!(liltoon_like.matcap.blend_mask_texture_index, Some(20));
		assert_eq!(liltoon_like.matcap.main_strength_factor, 0.5);
		assert_eq!(liltoon_like.matcap.blend_factor, 0.25);
		assert_eq!(liltoon_like.matcap.enable_lighting_factor, 0.75);
		assert_eq!(liltoon_like.matcap.blend_mode, UnaLilToonLikeBlendMode::Screen);
		assert_eq!(liltoon_like.matcap.normal_strength_factor, 0.66);
		assert_eq!(liltoon_like.matcap.shadow_mask_factor, 0.57);
		assert_eq!(liltoon_like.matcap.apply_transparency_factor, 0.47);
		assert_eq!(liltoon_like.matcap.lod_factor, 2.5);
		assert_eq!(liltoon_like.matcap.backface_mask_factor, 0.35);
		assert_eq!(liltoon_like.matcap.perspective_factor, 0.64);
		assert_eq!(liltoon_like.matcap.z_rotation_cancel_factor, 0.74);
		assert_eq!(liltoon_like.matcap.vr_parallax_strength_factor, 0.84);
		assert_eq!(liltoon_like.matcap.second_enabled_factor, 1.0);
		assert_eq!(liltoon_like.matcap.second_texture_index, Some(22));
		assert_eq!(liltoon_like.matcap.second_blend_mask_texture_index, Some(23));
		assert_eq!(liltoon_like.matcap.second_color_factor, [0.3, 0.5, 0.7, 0.9]);
		assert_eq!(liltoon_like.matcap.second_main_strength_factor, 0.58);
		assert_eq!(liltoon_like.matcap.second_blend_factor, 0.68);
		assert_eq!(liltoon_like.matcap.second_enable_lighting_factor, 0.78);
		assert_eq!(liltoon_like.matcap.second_shadow_mask_factor, 0.48);
		assert_eq!(liltoon_like.matcap.second_apply_transparency_factor, 0.38);
		assert_eq!(liltoon_like.matcap.second_blend_mode, UnaLilToonLikeBlendMode::Add);
		assert_eq!(liltoon_like.matcap.second_normal_strength_factor, 0.88);
		assert_eq!(liltoon_like.matcap.second_lod_factor, 1.5);
		assert_eq!(liltoon_like.matcap.second_backface_mask_factor, 0.45);
		assert_eq!(liltoon_like.matcap.second_perspective_factor, 0.54);
		assert_eq!(liltoon_like.matcap.second_z_rotation_cancel_factor, 0.44);
		assert_eq!(liltoon_like.matcap.second_vr_parallax_strength_factor, 0.34);
		assert_eq!(liltoon_like.reflection.enabled_factor, 1.0);
		assert_eq!(liltoon_like.reflection.color_factor, [0.9, 0.8, 0.7, 0.6]);
		assert_eq!(liltoon_like.reflection.smoothness_factor, 0.6);
		assert_eq!(liltoon_like.reflection.metallic_factor, 0.2);
		assert_eq!(liltoon_like.reflection.reflectance_factor, 0.4);
		assert_eq!(liltoon_like.reflection.apply_specular_factor, 0.8);
		assert_eq!(liltoon_like.reflection.apply_specular_forward_add_factor, 0.9);
		assert_eq!(liltoon_like.reflection.apply_reflection_factor, 0.7);
		assert_eq!(liltoon_like.reflection.apply_transparency_factor, 0.67);
		assert_eq!(liltoon_like.reflection.specular_toon_factor, 1.0);
		assert_eq!(liltoon_like.reflection.specular_border_factor, 0.37);
		assert_eq!(liltoon_like.reflection.specular_blur_factor, 0.12);
		assert_eq!(liltoon_like.reflection.specular_normal_strength_factor, 0.88);
		assert_eq!(liltoon_like.reflection.reflection_normal_strength_factor, 0.77);
		assert_eq!(liltoon_like.reflection.cube_enable_lighting_factor, 0.69);
		assert_eq!(liltoon_like.reflection.cube_color_factor, [0.6, 0.7, 0.8, 1.0]);
		assert_eq!(liltoon_like.reflection.cube_override_factor, 1.0);
		assert_eq!(liltoon_like.reflection.blend_mode, UnaLilToonLikeBlendMode::Multiply);
		assert_eq!(liltoon_like.reflection.color_texture_index, Some(16));
		assert_eq!(liltoon_like.reflection.smoothness_texture_index, Some(17));
		assert_eq!(liltoon_like.reflection.metallic_texture_index, Some(18));
		assert_eq!(liltoon_like.reflection.anisotropy_enabled_factor, 1.0);
		assert_eq!(liltoon_like.reflection.anisotropy_scale_factor, 0.8);
		assert_eq!(liltoon_like.reflection.anisotropy_shift_factor, -0.2);
		assert_eq!(liltoon_like.reflection.anisotropy_shift_noise_scale_factor, 0.3);
		assert_eq!(liltoon_like.reflection.anisotropy_specular_strength_factor, 0.7);
		assert_eq!(liltoon_like.reflection.anisotropy_tangent_width_factor, 0.4);
		assert_eq!(liltoon_like.reflection.anisotropy_bitangent_width_factor, 0.5);
		assert_eq!(liltoon_like.reflection.anisotropy_to_reflection_factor, 0.6);
		assert_eq!(liltoon_like.reflection.anisotropy_to_matcap_factor, 0.7);
		assert_eq!(liltoon_like.reflection.anisotropy_to_second_matcap_factor, 0.8);
		assert_eq!(liltoon_like.reflection.anisotropy_second_shift_factor, 0.2);
		assert_eq!(liltoon_like.reflection.anisotropy_second_shift_noise_scale_factor, 0.35);
		assert_eq!(liltoon_like.reflection.anisotropy_second_specular_strength_factor, 0.45);
		assert_eq!(liltoon_like.reflection.anisotropy_second_tangent_width_factor, 0.55);
		assert_eq!(liltoon_like.reflection.anisotropy_second_bitangent_width_factor, 0.65);
		assert_eq!(liltoon_like.reflection.anisotropy_tangent_texture_index, Some(26));
		assert_eq!(liltoon_like.reflection.anisotropy_scale_mask_texture_index, Some(27));
		assert_eq!(liltoon_like.reflection.anisotropy_shift_noise_mask_texture_index, Some(28));
		assert_eq!(liltoon_like.rim.enabled_factor, 1.0);
		assert_eq!(liltoon_like.rim.color_factor, [0.1, 0.2, 0.3, 1.0]);
		assert_eq!(liltoon_like.rim.texture_index, Some(12));
		assert_eq!(liltoon_like.rim.main_strength_factor, 0.4);
		assert_eq!(liltoon_like.rim.border_factor, 0.3);
		assert_eq!(liltoon_like.rim.blur_factor, 0.2);
		assert_eq!(liltoon_like.rim.fresnel_power_factor, 4.0);
		assert_eq!(liltoon_like.rim.enable_lighting_factor, 0.6);
		assert_eq!(liltoon_like.rim.blend_mode, UnaLilToonLikeBlendMode::Screen);
		assert_eq!(liltoon_like.rim.shadow_mask_factor, 0.91);
		assert_eq!(liltoon_like.rim.apply_transparency_factor, 0.83);
		assert_eq!(liltoon_like.rim.normal_strength_factor, 0.82);
		assert_eq!(liltoon_like.rim.backface_mask_factor, 0.73);
		assert_eq!(liltoon_like.rim.directional_strength_factor, 0.52);
		assert_eq!(liltoon_like.rim.directional_range_factor, 0.42);
		assert_eq!(liltoon_like.rim.indirect_color_factor, [0.4, 0.5, 0.6, 0.7]);
		assert_eq!(liltoon_like.rim.indirect_range_factor, 0.32);
		assert_eq!(liltoon_like.rim.indirect_border_factor, 0.22);
		assert_eq!(liltoon_like.rim.indirect_blur_factor, 0.12);
		assert_eq!(liltoon_like.rim.shade_enabled_factor, 1.0);
		assert_eq!(liltoon_like.rim.shade_color_factor, [0.6, 0.5, 0.4, 0.7]);
		assert_eq!(liltoon_like.rim.shade_border_factor, 0.44);
		assert_eq!(liltoon_like.rim.shade_blur_factor, 0.22);
		assert_eq!(liltoon_like.rim.shade_fresnel_power_factor, 2.5);
		assert_eq!(liltoon_like.rim.shade_normal_strength_factor, 0.62);
		assert_eq!(liltoon_like.backlight.enabled_factor, 1.0);
		assert_eq!(liltoon_like.backlight.color_factor, [1.1, 1.2, 1.3, 0.8]);
		assert_eq!(liltoon_like.backlight.main_strength_factor, 0.72);
		assert_eq!(liltoon_like.backlight.normal_strength_factor, 0.82);
		assert_eq!(liltoon_like.backlight.border_factor, 0.32);
		assert_eq!(liltoon_like.backlight.blur_factor, 0.23);
		assert_eq!(liltoon_like.backlight.directivity_factor, 7.0);
		assert_eq!(liltoon_like.backlight.view_strength_factor, 0.62);
		assert_eq!(liltoon_like.backlight.receive_shadow_factor, 0.52);
		assert_eq!(liltoon_like.backlight.backface_mask_factor, 0.42);
		assert_eq!(liltoon_like.emission.enabled_factor, 1.0);
		assert_eq!(liltoon_like.emission.color_factor, [0.5, 0.4, 0.3, 0.8]);
		assert_eq!(liltoon_like.emission.texture_index, Some(13));
		assert_eq!(liltoon_like.emission.main_strength_factor, 0.45);
		assert_eq!(liltoon_like.emission.blend_factor, 0.55);
		assert_eq!(liltoon_like.emission.blend_mode, UnaLilToonLikeBlendMode::Multiply);
		assert_eq!(liltoon_like.emission.gradation_enabled_factor, 1.0);
		assert_eq!(liltoon_like.emission.gradation_texture_index, Some(29));
		assert_eq!(liltoon_like.emission.gradation_speed_factor, 1.5);
		assert_eq!(liltoon_like.outline.enabled_factor, 1.0);
		assert_eq!(liltoon_like.outline.color_factor, [0.01, 0.02, 0.03, 1.0]);
		assert_eq!(liltoon_like.outline.lit_color_factor, [1.0, 0.2, 0.0, 0.4]);
		assert_eq!(liltoon_like.outline.width_mask_texture_index, Some(14));
		assert_eq!(liltoon_like.outline.texture_index, Some(15));
		assert!((liltoon_like.outline.width_factor - 0.0003).abs() < 1e-8);
		assert_eq!(liltoon_like.outline.fix_width_factor, 0.25);
		assert_eq!(liltoon_like.outline.enable_lighting_factor, 0.65);
		assert_eq!(liltoon_like.outline.lit_scale_factor, 9.0);
		assert_eq!(liltoon_like.outline.lit_offset_factor, -7.0);
		assert_eq!(liltoon_like.outline.lit_apply_tex_factor, 1.0);
		assert_eq!(liltoon_like.outline.lit_shadow_receive_factor, 1.0);
		assert_eq!(liltoon_like.outline.z_bias_factor, -0.01);
		assert_eq!(liltoon_like.alpha_mask.mode_factor, 2.0);
		assert_eq!(liltoon_like.alpha_mask.texture_index, Some(21));
		assert_eq!(liltoon_like.alpha_mask.scale_factor, 0.8);
		assert_eq!(liltoon_like.alpha_mask.value_factor, 0.1);
		assert_eq!(liltoon_like.blend_state.source_factor, 1.0);
		assert_eq!(liltoon_like.blend_state.destination_factor, 10.0);
		assert_eq!(liltoon_like.blend_state.operation_factor, 0.0);
		assert_eq!(liltoon_like.blend_state.alpha_source_factor, 1.0);
		assert_eq!(liltoon_like.blend_state.alpha_destination_factor, 10.0);
		assert_eq!(liltoon_like.blend_state.alpha_operation_factor, 0.0);
		assert_eq!(liltoon_like.blend_state.forward_add_alpha_source_factor, 0.0);
		assert_eq!(liltoon_like.blend_state.forward_add_alpha_destination_factor, 1.0);
		assert_eq!(liltoon_like.blend_state.forward_add_alpha_operation_factor, 4.0);
		assert_eq!(liltoon_like.blend_state.alpha_boost_factor, 10.0);
		assert_eq!(liltoon_like.blend_state.subpass_cutoff_factor, 0.4);
		assert_eq!(liltoon_like.blend_state.alpha_to_mask_factor, 1.0);
		assert_eq!(mtoon.parametric_rim_color_factor, [0.040000003, 0.080000006, 0.120000005]);
		assert_eq!(mtoon.outline_color_factor, [0.01, 0.02, 0.03]);
	}

	#[test]
	fn source_alpha_mask_params_require_liltoon_feature_keyword() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonTransparentOutline",
			"floatParams": {
				"_AlphaMaskMode": 1.0,
				"_AlphaMaskScale": 1.0,
				"_AlphaMaskValue": 0.13
			},
			"mtoon": {
				"alphaMaskTextureIndex": 21
			}
		});

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon_like material");

		assert_eq!(liltoon_like.alpha_mask.mode_factor, 0.0);
		assert_eq!(liltoon_like.alpha_mask.texture_index, None);
		assert_eq!(liltoon_like.alpha_mask.scale_factor, 1.0);
		assert_eq!(liltoon_like.alpha_mask.value_factor, 0.0);
	}

	#[test]
	fn source_alpha_mask_params_accept_liltoon_feature_keyword() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonTransparentOutline",
			"enabledKeywords": ["_COLOROVERLAY_ON"],
			"floatParams": {
				"_AlphaMaskMode": 1.0,
				"_AlphaMaskScale": 1.0,
				"_AlphaMaskValue": 0.13
			},
			"mtoon": {
				"alphaMaskTextureIndex": 21
			}
		});

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon_like material");

		assert_eq!(liltoon_like.alpha_mask.mode_factor, 1.0);
		assert_eq!(liltoon_like.alpha_mask.texture_index, Some(21));
		assert_eq!(liltoon_like.alpha_mask.scale_factor, 1.0);
		assert_eq!(liltoon_like.alpha_mask.value_factor, 0.13);
	}

	#[test]
	fn source_color_params_respect_liltoon_feature_toggles() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "lilToon",
			"floatParams": {
				"_UseShadow": 0.0,
				"_UseMatCap": 0.0,
				"_UseRim": 0.0,
				"_UseEmission": 0.0,
				"_UseOutline": 0.0
			},
			"colorParams": {
				"_ShadeColor": [0.7, 0.8, 0.9, 1.0],
				"_MatCapColor": [0.2, 0.4, 0.6, 1.0],
				"_RimColor": [0.1, 0.2, 0.3, 1.0],
				"_EmissionColor": [1.0, 1.0, 1.0, 1.0]
			},
			"mtoon": {}
		});

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon_like material");
		let mtoon = unavatar_mtoon_from_extras(&extras).expect("legacy mtoon material");

		assert_eq!(mtoon.shade_color_factor, UnaMtoonMaterial::default().shade_color_factor);
		assert_eq!(mtoon.matcap_factor, UnaMtoonMaterial::default().matcap_factor);
		assert_eq!(liltoon_like.shadow.enabled_factor, 0.0);
		assert_eq!(liltoon_like.matcap.enabled_factor, 0.0);
		assert_eq!(liltoon_like.rim.enabled_factor, 0.0);
		assert_eq!(liltoon_like.emission.enabled_factor, 0.0);
		assert_eq!(liltoon_like.outline.enabled_factor, 0.0);
		assert_eq!(
			mtoon.parametric_rim_color_factor,
			UnaMtoonMaterial::default().parametric_rim_color_factor
		);
	}

	#[test]
	fn imports_liltoon_gem_source_profile_and_reflection_params() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonGem",
			"floatParams": {
				"_GemEnvContrast": 2.5,
				"_RefractionFresnelPower": 4.25,
				"_RefractionStrength": 0.45,
				"_GemChromaticAberration": 0.03,
				"_GemParticleLoop": 6.0,
				"_GemVRParallaxStrength": 0.8
			},
			"colorParams": {
				"_GemEnvColor": [0.8, 0.9, 1.0, 0.7],
				"_GemParticleColor": [2.0, 3.0, 4.0, 0.5]
			},
			"mtoon": {}
		});

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon gem material");

		assert_eq!(liltoon_like.source_profile, UnaLilToonLikeSourceProfile::LiltoonGem);
		assert_eq!(liltoon_like.reflection.gem_env_color_factor, [0.8, 0.9, 1.0, 0.7]);
		assert_eq!(liltoon_like.reflection.gem_env_contrast_factor, 2.5);
		assert_eq!(liltoon_like.reflection.gem_refraction_fresnel_power_factor, 4.25);
		assert_eq!(liltoon_like.reflection.gem_refraction_strength_factor, 0.45);
		assert_eq!(liltoon_like.reflection.gem_chromatic_aberration_factor, 0.03);
		assert_eq!(liltoon_like.reflection.gem_particle_loop_factor, 6.0);
		assert_eq!(liltoon_like.reflection.gem_particle_color_factor, [2.0, 3.0, 4.0, 0.5]);
		assert_eq!(liltoon_like.reflection.gem_vr_parallax_strength_factor, 0.8);
	}

	#[test]
	fn imports_mtoon_uv_animation_fields() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "lilToon",
			"mtoon": {
				"uvOffsetScale": [0.1, 0.2, 2.0, 3.0],
				"uvAnimationMaskTextureIndex": 7,
				"uvAnimationScrollXSpeedFactor": 0.25,
				"uvAnimationScrollYSpeedFactor": -0.5,
				"uvAnimationRotationSpeedFactor": 0.75
			}
		});

		let mtoon = unavatar_mtoon_from_extras(&extras).expect("mtoon material");

		assert_eq!(mtoon.uv_offset_scale, [0.1, 0.2, 2.0, 3.0]);
		assert_eq!(mtoon.uv_animation_mask_texture_index, Some(7));
		assert_eq!(mtoon.uv_animation_scroll_x_speed_factor, 0.25);
		assert_eq!(mtoon.uv_animation_scroll_y_speed_factor, -0.5);
		assert_eq!(mtoon.uv_animation_rotation_speed_factor, 0.75);
	}

	#[test]
	fn imports_khr_texture_transform_as_material_uv_offset_scale() {
		let json = r#"{
			"asset": {"version": "2.0"},
			"materials": [{
				"pbrMetallicRoughness": {
					"baseColorTexture": {
						"index": 0,
						"extensions": {
							"KHR_texture_transform": {
								"offset": [0.25, -0.5],
								"scale": [2.0, 3.0]
							}
						}
					}
				}
			}],
			"textures": [{"source": 0}],
			"images": [{"uri": "white.png"}]
		}"#;
		let gltf = gltf::Gltf::from_slice(&glb_bytes_with_bin(json, &[])).expect("gltf parses");
		let materials = build_materials(&gltf.document);

		assert_eq!(materials[0].uv_offset_scale, [0.25, -0.5, 2.0, 3.0]);
	}

	#[test]
	fn modular_avatar_bone_proxy_reparents_keep_world_pose() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					source_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![1, 2],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Head".to_string()),
					source_node_id: Some("node_head".to_string()),
					visible: true,
					transform: Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0)).to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Proxy".to_string()),
					source_node_id: Some("node_proxy".to_string()),
					visible: true,
					transform: Mat4::from_translation(Vec3::new(0.0, 2.0, 0.0)).to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [{
						"shortType": "ModularAvatarBoneProxy",
						"enabled": true,
						"target": {"nodeId": "node_proxy", "path": "Proxy"},
						"resolvedTarget": {"nodeId": "node_head", "path": "Head"},
						"fields": {
							"attachmentMode": "AsChildKeepWorldPose",
							"matchScale": false
						}
					}]
				}
			}),
		};
		let before = scene_world_matrices(&scene);
		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);
		let after = scene_world_matrices(&scene);
		assert_eq!(scene.nodes[0].children, vec![1]);
		assert_eq!(scene.nodes[1].children, vec![2]);
		assert_eq!(after[2].transform_point3(Vec3::ZERO), before[2].transform_point3(Vec3::ZERO));
		assert!(report.messages.iter().any(|m| m.contains("bone_proxy_applied=1")));
	}

	#[test]
	fn modular_avatar_nested_bone_proxy_uses_prepass_world_pose() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					source_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![1, 2],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Head".to_string()),
					source_node_id: Some("node_head".to_string()),
					visible: true,
					transform: Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0)).to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("ParentProxy".to_string()),
					source_node_id: Some("node_parent_proxy".to_string()),
					visible: true,
					transform: Mat4::from_translation(Vec3::new(0.0, 10.0, 0.0)).to_cols_array(),
					children: vec![3],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("ChildProxy".to_string()),
					source_node_id: Some("node_child_proxy".to_string()),
					visible: true,
					transform: Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0)).to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let before = scene_world_matrices(&scene);
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [
						{
							"shortType": "ModularAvatarBoneProxy",
							"enabled": true,
							"target": {"nodeId": "node_parent_proxy", "path": "ParentProxy"},
							"resolvedTarget": {"nodeId": "node_head", "path": "Head"},
							"fields": {
								"attachmentMode": "AsChildAtRoot",
								"matchScale": false
							}
						},
						{
							"shortType": "ModularAvatarBoneProxy",
							"enabled": true,
							"target": {"nodeId": "node_child_proxy", "path": "ParentProxy/ChildProxy"},
							"resolvedTarget": {"nodeId": "node_head", "path": "Head"},
							"fields": {
								"attachmentMode": "AsChildKeepWorldPose",
								"matchScale": false
							}
						}
					]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);
		let after = scene_world_matrices(&scene);

		assert_eq!(scene.nodes[1].children, vec![2, 3]);
		assert_eq!(after[2].transform_point3(Vec3::ZERO), before[1].transform_point3(Vec3::ZERO));
		assert_eq!(after[3].transform_point3(Vec3::ZERO), before[3].transform_point3(Vec3::ZERO));
		assert!(report.messages.iter().any(|m| m.contains("bone_proxy_applied=2")));
	}

	#[test]
	fn modular_avatar_merge_armature_retargets_skin_bindposes() {
		let source_world = Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0));
		let target_world = Mat4::from_translation(Vec3::new(0.0, 3.0, 0.0));
		let old_bind = Mat4::from_translation(Vec3::new(0.0, 0.0, 5.0));
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					source_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![1, 2],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("TargetBone".to_string()),
					source_node_id: Some("node_target".to_string()),
					visible: true,
					transform: target_world.to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("SourceBone".to_string()),
					source_node_id: Some("node_source".to_string()),
					visible: true,
					transform: source_world.to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			skins: vec![UnaSkin {
				joint_nodes: vec![2],
				inverse_bind_matrices: vec![old_bind.to_cols_array()],
				skeleton_node: Some(2),
			}],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [{
						"shortType": "ModularAvatarMergeArmature",
						"enabled": true,
						"target": {"nodeId": "node_source", "path": "SourceBone"},
						"boneMappings": [{
							"sourceBone": {"nodeId": "node_source", "path": "SourceBone"},
							"targetBone": {"nodeId": "node_target", "path": "TargetBone"}
						}]
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		assert_eq!(scene.skins[0].joint_nodes, vec![1]);
		let expected = target_world.inverse() * source_world * old_bind;
		let actual = Mat4::from_cols_array(&scene.skins[0].inverse_bind_matrices[0]);
		for (a, e) in actual.to_cols_array().iter().zip(expected.to_cols_array()) {
			assert!((a - e).abs() < 0.0001, "actual={actual:?} expected={expected:?}");
		}
		assert!(report
			.messages
			.iter()
			.any(|m| { m.contains("merge_armature_mappings=1") && m.contains("mesh_retargeter_joints=1") }));
	}

	#[test]
	fn modular_avatar_mesh_settings_sets_skin_skeleton_node() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					source_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![1, 2, 3],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Outfit".to_string()),
					source_node_id: Some("node_outfit".to_string()),
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![4],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Hips".to_string()),
					source_node_id: Some("node_hips".to_string()),
					visible: true,
					transform: Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0)).to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("OtherMesh".to_string()),
					source_node_id: Some("node_other_mesh".to_string()),
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: Vec::new(),
					mesh: Some(0),
					skin: Some(1),
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("OutfitMesh".to_string()),
					source_node_id: Some("node_outfit_mesh".to_string()),
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: Vec::new(),
					mesh: Some(0),
					skin: Some(0),
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			skins: vec![
				UnaSkin {
					joint_nodes: vec![2],
					inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array()],
					skeleton_node: None,
				},
				UnaSkin {
					joint_nodes: vec![2],
					inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array()],
					skeleton_node: None,
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [{
						"shortType": "ModularAvatarMeshSettings",
						"enabled": true,
						"target": {"nodeId": "node_outfit", "path": "Outfit"},
						"fields": {
							"InheritProbeAnchor": "SetOrInherit",
							"ProbeAnchor": {
								"resolvedTarget": {"nodeId": "node_hips", "path": "Hips"}
							},
							"InheritBounds": "SetOrInherit",
							"RootBone": {
								"resolvedTarget": {"nodeId": "node_hips", "path": "Hips"}
							},
							"Bounds": {
								"center": [0.0, 0.0, 0.0],
								"extents": [1.0, 1.0, 1.0]
							}
						}
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		assert_eq!(scene.skins[0].skeleton_node, Some(2));
		assert_eq!(scene.skins[1].skeleton_node, None);
		assert_eq!(scene.nodes[4].probe_anchor_node, Some(2));
		assert_eq!(
			scene.nodes[4].local_bounds,
			Some(UnaBounds {
				center: [0.0, 0.0, 0.0],
				extents: [1.0, 1.0, 1.0],
			})
		);
		assert!(report.messages.iter().any(|m| {
			m.contains("mesh_settings_root_bones=1") && m.contains("mesh_settings_probe_anchors=1") && m.contains("mesh_settings_bounds=1")
		}));
	}

	#[test]
	fn wardrobe_set_lookup_uses_exact_id() {
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"wardrobe": {
					"sets": [{
						"id": "field_drape",
						"displayName": "Field Drape",
						"operations": [{
							"type": "nodeEnabled",
							"target": {"path": "Hair"},
							"visible": true
						}]
					}]
				}
			}),
		};

		assert!(unavatar_wardrobe_set_operations(&unavatar, "field_drape").is_some());
		assert!(unavatar_wardrobe_set_operations(&unavatar, "Field Drape").is_none());
	}
}
