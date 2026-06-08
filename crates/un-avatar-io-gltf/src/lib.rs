//! glTF 2.0 インポート（静的メッシュ + スキニング。Morph・スパースアクセサは読み飛ばし／レポート記録）。
//!
//! 設計正本: `docs/development-plan.md` Commit 1.3〜1.4

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;

use exr::prelude::{f16, pixel_vec::PixelVec, read, ReadChannels, ReadLayers};
use glam::{Mat4, Quat, Vec3};
use serde_json::Value;
use un_avatar_core::{
	apply_runtime_material_color, apply_runtime_material_scalar, modular_avatar_component_support_kind, Approximation, ReportStatus,
	UnaAlphaMode, UnaBounds, UnaCullMode, UnaDocument, UnaDynamicsCollider, UnaDynamicsColliderShape, UnaDynamicsInteraction,
	UnaDynamicsLimit, UnaDynamicsSourceKind, UnaExpressionCatalog, UnaExpressionPreset, UnaExpressionWeights, UnaImagePixelFormat,
	UnaImageRgba, UnaImageSourceMetadata, UnaLilToonLikeBlendMode, UnaLilToonLikeMaterial, UnaLilToonLikeSourceProfile, UnaMaterialPbr,
	UnaMeshBuffers, UnaMorphTargetBind, UnaMorphTargetDeltas, UnaMtoonMaterial, UnaMtoonOutlineWidthMode, UnaRuntimeAction,
	UnaRuntimeActionEffect, UnaRuntimeActionSet, UnaRuntimeActionTrigger, UnaRuntimeDynamicsMut, UnaRuntimeMaterialSlotTarget,
	UnaRuntimeMaterialTarget, UnaRuntimeNodeTarget, UnaSceneNode, UnaSceneSnapshot, UnaShadingModel, UnaSkin, UnaSpringBoneGroup,
	UnaSpringBoneSettings, UnaTextureFilterMode, UnaTextureSampler, UnaTextureWrapMode, UnaUnavatarExtension,
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
						source_layout: image_metadata
							.as_ref()
							.and_then(|metadata| json_string(metadata.get("sourceLayout").or_else(|| metadata.get("source_layout")))),
						unity_generate_cubemap: image_metadata.as_ref().and_then(|metadata| {
							json_string(
								metadata
									.get("unityGenerateCubemap")
									.or_else(|| metadata.get("unity_generate_cubemap")),
							)
						}),
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
					source_layout: image_metadata
						.as_ref()
						.and_then(|metadata| json_string(metadata.get("sourceLayout").or_else(|| metadata.get("source_layout")))),
					unity_generate_cubemap: image_metadata.as_ref().and_then(|metadata| {
						json_string(
							metadata
								.get("unityGenerateCubemap")
								.or_else(|| metadata.get("unity_generate_cubemap")),
						)
					}),
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
					source_layout: image_metadata
						.as_ref()
						.and_then(|metadata| json_string(metadata.get("sourceLayout").or_else(|| metadata.get("source_layout")))),
					unity_generate_cubemap: image_metadata.as_ref().and_then(|metadata| {
						json_string(
							metadata
								.get("unityGenerateCubemap")
								.or_else(|| metadata.get("unity_generate_cubemap")),
						)
					}),
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
				source_layout: image_metadata
					.as_ref()
					.and_then(|metadata| json_string(metadata.get("sourceLayout").or_else(|| metadata.get("source_layout")))),
				unity_generate_cubemap: image_metadata.as_ref().and_then(|metadata| {
					json_string(
						metadata
							.get("unityGenerateCubemap")
							.or_else(|| metadata.get("unity_generate_cubemap")),
					)
				}),
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
			source_layout: asset.get("sourceLayout").and_then(Value::as_str).map(str::to_string),
			unity_generate_cubemap: asset.get("unityGenerateCubemap").and_then(Value::as_str).map(str::to_string),
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
		"image/vnd.radiance" => {
			let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Hdr).map_err(|e| format!("HDR decode: {e}"))?;
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
		if let Some(image_index) = texture_asset_ref(mtoon, "shadow2ndColorTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.shadow
				.second_color_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "shadow3rdColorTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.shadow
				.third_color_texture_index = Some(image_index);
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
		if let Some(image_index) = texture_asset_ref(mtoon, "normal2ndScaleMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.normal
				.second_scale_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "main2ndTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.second_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "mainColorAdjustMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.main_color_adjust_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "main2ndBlendMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.second_blend_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "main2ndDissolveMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.second_dissolve
				.mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "main2ndDissolveNoiseMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.second_dissolve
				.noise_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "main3rdTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.third_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "main3rdBlendMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.third_blend_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "main3rdDissolveMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.third_dissolve
				.mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "main3rdDissolveNoiseMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.main_color
				.third_dissolve
				.noise_mask_texture_index = Some(image_index);
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
		if let Some(image_index) = texture_asset_ref(mtoon, "matcapBumpTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.matcap
				.bump_texture_index = Some(image_index);
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
		if let Some(image_index) = texture_asset_ref(mtoon, "matcap2ndBumpTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.matcap
				.second_bump_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "rimMultiplyTextureIndexAsset", asset_map) {
			scene_material.mtoon.get_or_insert_with(Default::default).rim_multiply_texture_index = Some(image_index);
			scene_material.liltoon_like.get_or_insert_with(Default::default).rim.texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "rimShadeMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.rim
				.shade_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "backlightColorTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.backlight
				.texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "glitterColorTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.glitter
				.color_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "glitterShapeTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.glitter
				.shape_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "dissolveMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.dissolve
				.mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "dissolveNoiseMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.dissolve
				.noise_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "parallaxTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.parallax
				.texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "emissionTextureIndexAsset", asset_map) {
			scene_material.emissive_texture_index = Some(image_index);
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.emission
				.texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "emissionBlendMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.emission
				.blend_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "emissionGradationTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.emission
				.gradation_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "emission2ndTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.emission
				.second_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "emission2ndBlendMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.emission
				.second_blend_mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "emission2ndGradationTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.emission
				.second_gradation_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "audioLinkMaskTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.audio_link
				.mask_texture_index = Some(image_index);
		}
		if let Some(image_index) = texture_asset_ref(mtoon, "audioLinkLocalMapTextureIndexAsset", asset_map) {
			scene_material
				.liltoon_like
				.get_or_insert_with(Default::default)
				.audio_link
				.local_map_texture_index = Some(image_index);
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
	let bin_capacity = views.iter().map(|view| view.bytes.len().saturating_add(3)).sum();
	let mut bin = Vec::with_capacity(bin_capacity);
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

fn scene_node_paths_all(scene: &UnaSceneSnapshot) -> BTreeMap<String, Vec<usize>> {
	fn visit(scene: &UnaSceneSnapshot, idx: usize, path: String, out: &mut BTreeMap<String, Vec<usize>>) {
		out.entry(path.clone()).or_default().push(idx);
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
	let mut out = String::with_capacity(path.len());
	for segment in path.split('/') {
		if !out.is_empty() {
			out.push('/');
		}
		out.push_str(&normalize_unavatar_path_segment(segment));
	}
	out
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
	for (path, indices) in scene_node_paths_all(scene) {
		out.entry(normalize_unavatar_path(&path)).or_insert_with(Vec::new).extend(indices);
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

fn report_unavatar_path_diagnostics(scene: &UnaSceneSnapshot, unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	let exact_paths = scene_node_paths_all(scene);
	let exact_duplicate_paths = exact_paths.values().filter(|indices| indices.len() > 1).count();
	let normalized_paths = scene_node_normalized_paths(scene);
	let normalized_ambiguous_paths = normalized_paths.values().filter(|indices| indices.len() > 1).count();
	let registry_paths = unavatar_node_registry_paths(Some(unavatar));
	let paths = scene_node_paths(scene);
	let registry_ambiguous_paths = registry_paths
		.values()
		.filter(|path| lookup_scene_path_all(&paths, &normalized_paths, path).len() > 1)
		.count();
	if exact_duplicate_paths > 0 || normalized_ambiguous_paths > 0 || registry_ambiguous_paths > 0 {
		report.push_info(format!(
			".unavatar path diagnostics: exact_duplicate_paths={exact_duplicate_paths}, normalized_ambiguous_paths={normalized_ambiguous_paths}, registry_ambiguous_paths={registry_ambiguous_paths}"
		));
	}
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

fn normalized_path_is_same_or_descendant(path: &str, ancestor: &str) -> bool {
	if ancestor.is_empty() {
		return false;
	}
	let path = normalize_unavatar_path(path);
	let ancestor = normalize_unavatar_path(ancestor);
	path == ancestor || path.starts_with(&format!("{ancestor}/"))
}

fn operation_target_registry_path<'a>(registry_paths: &'a BTreeMap<String, String>, op: &'a Value) -> &'a str {
	operation_target_node_id(op)
		.and_then(|node_id| registry_paths.get(node_id).map(String::as_str))
		.filter(|path| !path.is_empty())
		.unwrap_or_else(|| operation_target_path(op))
}

fn collect_current_subtree(scene: &UnaSceneSnapshot, root: usize, out: &mut BTreeSet<usize>) {
	if root >= scene.nodes.len() || !out.insert(root) {
		return;
	}
	if let Some(node) = scene.nodes.get(root) {
		for &child in &node.children {
			collect_current_subtree(scene, child, out);
		}
	}
}

fn lookup_operation_subtree_targets_all(
	scene: &UnaSceneSnapshot,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	op: &Value,
) -> Vec<usize> {
	let mut out = BTreeSet::new();
	for root in lookup_operation_targets_all(node_ids, registry_paths, paths, normalized_paths, op) {
		collect_current_subtree(scene, root, &mut out);
	}
	let target_path = operation_target_registry_path(registry_paths, op);
	if !target_path.is_empty() {
		for (idx, node) in scene.nodes.iter().enumerate() {
			let Some(source_node_id) = node.source_node_id.as_deref() else {
				continue;
			};
			let Some(source_path) = registry_paths.get(source_node_id) else {
				continue;
			};
			if normalized_path_is_same_or_descendant(source_path, target_path) {
				out.insert(idx);
			}
		}
	}
	out.into_iter().collect()
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

fn unavatar_dynamics_source_kind(value: &Value) -> UnaDynamicsSourceKind {
	let source = value
		.get("source")
		.or_else(|| value.get("sourceKind"))
		.or_else(|| value.get("source_kind"))
		.and_then(Value::as_str)
		.unwrap_or("");
	match source {
		source if source.eq_ignore_ascii_case("vrc_physbone") || source.eq_ignore_ascii_case("physbone") => {
			UnaDynamicsSourceKind::VrcPhysBone
		}
		source
			if source.eq_ignore_ascii_case("vrm_spring_bone")
				|| source.eq_ignore_ascii_case("vrm_springbone")
				|| source.eq_ignore_ascii_case("spring_bone")
				|| source.eq_ignore_ascii_case("springbone") =>
		{
			UnaDynamicsSourceKind::VrmSpringBone
		}
		_ => UnaDynamicsSourceKind::Unknown,
	}
}

fn unavatar_dynamics_root_index(
	value: &Value,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Option<usize> {
	if let Some(index) = json_usize(Some(value)) {
		return Some(index);
	}
	if let Some(node_id) = value.as_str().filter(|value| !value.is_empty()) {
		return node_ids.get(node_id).copied();
	}
	unavatar_node_ref_index(value, node_ids, registry_paths, paths, normalized_paths)
}

fn unavatar_dynamics_node_index_set(
	value: Option<&Value>,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> BTreeSet<usize> {
	let mut indices = BTreeSet::new();
	let Some(value) = value else {
		return indices;
	};
	let values: Cow<'_, [Value]> = if let Some(array) = value.as_array() {
		Cow::Borrowed(array.as_slice())
	} else {
		Cow::Owned(vec![value.clone()])
	};
	for item in values.iter() {
		if let Some(index) = unavatar_dynamics_root_index(item, node_ids, registry_paths, paths, normalized_paths) {
			indices.insert(index);
		}
	}
	indices
}

fn unavatar_dynamics_multi_child_ignore(value: &Value) -> bool {
	let value = value
		.get("multiChildType")
		.or_else(|| value.get("multi_child_type"))
		.or_else(|| value.get("multiChild"))
		.or_else(|| value.get("multi_child"))
		.and_then(Value::as_str)
		.unwrap_or("");
	value.eq_ignore_ascii_case("ignore") || value.eq_ignore_ascii_case("ignored")
}

fn unavatar_dynamics_collider_shape(value: &Value) -> UnaDynamicsColliderShape {
	let shape = value
		.get("shapeType")
		.or_else(|| value.get("shape_type"))
		.or_else(|| value.get("shape"))
		.and_then(Value::as_str)
		.unwrap_or("");
	if shape.eq_ignore_ascii_case("sphere") {
		UnaDynamicsColliderShape::Sphere
	} else if shape.eq_ignore_ascii_case("capsule") {
		UnaDynamicsColliderShape::Capsule
	} else {
		UnaDynamicsColliderShape::Unknown
	}
}

fn unity_vec3_to_unavatar_runtime(value: [f32; 3]) -> [f32; 3] {
	[-value[0], value[1], value[2]]
}

fn unity_quat_to_unavatar_runtime(value: [f32; 4]) -> [f32; 4] {
	[value[0], -value[1], -value[2], value[3]]
}

fn unavatar_dynamics_colliders(
	value: &Value,
	source_kind: UnaDynamicsSourceKind,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Vec<UnaDynamicsCollider> {
	let source_params = unavatar_dynamics_source_params(value);
	if unavatar_dynamics_source_value(value, source_params, "allowCollision", "allow_collision").and_then(Value::as_bool) == Some(false) {
		return Vec::new();
	}
	let colliders = unavatar_dynamics_source_value(value, source_params, "colliders", "colliders").and_then(Value::as_array);
	let Some(colliders) = colliders else {
		return Vec::new();
	};
	colliders
		.iter()
		.filter_map(|collider| {
			let inside_bounds = collider
				.get("insideBounds")
				.or_else(|| collider.get("inside_bounds"))
				.and_then(Value::as_bool)
				.unwrap_or(false);
			let root = collider
				.get("root")
				.or_else(|| collider.get("node"))
				.or_else(|| collider.get("component"))?;
			let node = unavatar_node_ref_index(root, node_ids, registry_paths, paths, normalized_paths)?;
			let radius = json_f32(collider.get("radius")).unwrap_or(0.0);
			if !radius.is_finite() || radius <= 0.0 {
				return None;
			}
			Some(UnaDynamicsCollider {
				source_kind,
				node,
				shape: unavatar_dynamics_collider_shape(collider),
				radius,
				height: json_f32(collider.get("height")).unwrap_or(0.0).max(0.0),
				position: unity_vec3_to_unavatar_runtime(json_vec3(collider.get("position")).unwrap_or([0.0; 3])),
				rotation: unity_quat_to_unavatar_runtime(json_vec4(collider.get("rotation")).unwrap_or([0.0, 0.0, 0.0, 1.0])),
				inside_bounds,
			})
		})
		.collect()
}

fn unavatar_dynamics_endpoint_position(value: &Value) -> Option<[f32; 3]> {
	let source_params = unavatar_dynamics_source_params(value);
	let endpoint = unavatar_dynamics_source_value(value, source_params, "endpointPosition", "endpoint_position");
	let endpoint = unity_vec3_to_unavatar_runtime(json_vec3(endpoint)?);
	let length_sq = endpoint[0] * endpoint[0] + endpoint[1] * endpoint[1] + endpoint[2] * endpoint[2];
	(length_sq > 1e-12).then_some(endpoint)
}

fn ensure_unavatar_dynamics_endpoint_child(
	scene: &mut UnaSceneSnapshot,
	root_idx: usize,
	item: &Value,
	ignored_nodes: &BTreeSet<usize>,
) -> bool {
	if root_idx >= scene.nodes.len() {
		return false;
	}
	if scene.nodes[root_idx].children.iter().any(|child| !ignored_nodes.contains(child)) {
		return false;
	}
	let Some(endpoint) = unavatar_dynamics_endpoint_position(item) else {
		return false;
	};
	let endpoint_idx = scene.nodes.len();
	let root_name = scene.nodes.get(root_idx).and_then(|node| node.name.as_deref()).unwrap_or("Root");
	scene.nodes.push(UnaSceneNode {
		name: Some(format!("{root_name} Endpoint")),
		source_node_id: None,
		resolved_node_id: None,
		visible: true,
		transform: Mat4::from_translation(Vec3::from(endpoint)).to_cols_array(),
		children: Vec::new(),
		mesh: None,
		skin: None,
		probe_anchor_node: None,
		local_bounds: None,
	});
	scene.nodes[root_idx].children.push(endpoint_idx);
	true
}

fn collect_scene_child_chains(
	scene: &UnaSceneSnapshot,
	root_idx: usize,
	ignored_nodes: &BTreeSet<usize>,
	multi_child_ignore: bool,
) -> Vec<Vec<usize>> {
	if root_idx >= scene.nodes.len() {
		return Vec::new();
	}
	let mut chains = Vec::new();
	let mut stack = vec![(root_idx, vec![root_idx])];
	while let Some((current, chain)) = stack.pop() {
		let Some(node) = scene.nodes.get(current) else {
			continue;
		};
		let mut child_count = 0usize;
		for &child in &node.children {
			if child >= scene.nodes.len() || chain.contains(&child) {
				continue;
			}
			if ignored_nodes.contains(&child) {
				continue;
			}
			child_count += 1;
			let mut next_chain = chain.clone();
			next_chain.push(child);
			if next_chain.len() > 64 {
				chains.push(next_chain);
			} else {
				stack.push((child, next_chain));
			}
			if multi_child_ignore {
				break;
			}
		}
		if child_count == 0 {
			chains.push(chain);
		}
	}
	chains
}

fn unavatar_dynamics_gravity(value: &Value) -> (f32, [f32; 3]) {
	let gravity = json_vec3(
		value
			.get("gravity")
			.or_else(|| value.get("gravityVector"))
			.or_else(|| value.get("gravity_vector")),
	)
	.unwrap_or([0.0, -1.0, 0.0]);
	let gravity_vec = Vec3::from(gravity);
	let vector_power = gravity_vec.length();
	let explicit_power = json_f32(value.get("gravityPower").or_else(|| value.get("gravity_power")));
	let power = explicit_power.unwrap_or(vector_power);
	let dir = if gravity_vec.length_squared() > 1e-12 {
		gravity_vec.normalize().to_array()
	} else {
		[0.0, -1.0, 0.0]
	};
	(power, dir)
}

fn unavatar_dynamics_source_params(value: &Value) -> Option<&Value> {
	value.get("sourceParams").or_else(|| value.get("source_params"))
}

fn unavatar_dynamics_source_value<'a>(
	value: &'a Value,
	source_params: Option<&'a Value>,
	camel_key: &str,
	snake_key: &str,
) -> Option<&'a Value> {
	source_params
		.and_then(|params| params.get(camel_key).or_else(|| params.get(snake_key)))
		.or_else(|| value.get(camel_key).or_else(|| value.get(snake_key)))
}

fn unavatar_dynamics_limit(value: &Value) -> Option<UnaDynamicsLimit> {
	let source_params = unavatar_dynamics_source_params(value);
	let limit_type = unavatar_dynamics_source_value(value, source_params, "limitType", "limit_type")
		.and_then(Value::as_str)
		.unwrap_or("")
		.to_string();
	let max_angle_x = unavatar_dynamics_source_value(value, source_params, "maxAngleX", "max_angle_x")
		.and_then(|value| json_f32(Some(value)))
		.unwrap_or(0.0);
	let max_angle_z = unavatar_dynamics_source_value(value, source_params, "maxAngleZ", "max_angle_z")
		.and_then(|value| json_f32(Some(value)))
		.unwrap_or(0.0);
	let max_stretch = unavatar_dynamics_source_value(value, source_params, "maxStretch", "max_stretch")
		.and_then(|value| json_f32(Some(value)))
		.unwrap_or(0.0);
	if limit_type.is_empty() && max_angle_x == 0.0 && max_angle_z == 0.0 && max_stretch == 0.0 {
		None
	} else {
		Some(UnaDynamicsLimit {
			limit_type,
			max_angle_x,
			max_angle_z,
			max_stretch,
		})
	}
}

fn unavatar_dynamics_interaction(value: &Value) -> Option<UnaDynamicsInteraction> {
	let source_params = unavatar_dynamics_source_params(value);
	let allow_grabbing = unavatar_dynamics_source_value(value, source_params, "allowGrabbing", "allow_grabbing").and_then(Value::as_bool);
	let allow_posing = unavatar_dynamics_source_value(value, source_params, "allowPosing", "allow_posing").and_then(Value::as_bool);
	if allow_grabbing.is_none() && allow_posing.is_none() {
		None
	} else {
		Some(UnaDynamicsInteraction {
			allow_grabbing,
			allow_posing,
		})
	}
}

fn unavatar_dynamics_settings(
	scene: &mut UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
	report: &mut ImportReport,
) -> Option<UnaSpringBoneSettings> {
	let dynamics = unavatar.source.get("dynamics").and_then(Value::as_array)?;
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(Some(unavatar));
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let mut groups = Vec::new();
	let mut missing_roots = 0usize;
	let mut short_chains = 0usize;
	let mut ignored_transform_count = 0usize;
	let mut multi_child_ignore_count = 0usize;
	let mut endpoint_child_count = 0usize;
	let mut colliders = Vec::new();

	for item in dynamics {
		if item.get("enabled").and_then(Value::as_bool) == Some(false) {
			continue;
		}
		let Some(roots) = item.get("roots").or_else(|| item.get("root")).or_else(|| item.get("rootNode")) else {
			missing_roots += 1;
			continue;
		};
		let root_values: Cow<'_, [Value]> = if let Some(array) = roots.as_array() {
			Cow::Borrowed(array.as_slice())
		} else {
			Cow::Owned(vec![roots.clone()])
		};
		let source_kind = unavatar_dynamics_source_kind(item);
		let category = item
			.get("category")
			.and_then(Value::as_str)
			.filter(|value| !value.is_empty())
			.unwrap_or("")
			.to_string();
		let source_id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
		let comment = item.get("name").and_then(Value::as_str).unwrap_or(source_id.as_str()).to_string();
		let stiffness = json_f32(item.get("stiffness").or_else(|| item.get("spring")).or_else(|| item.get("pull"))).unwrap_or(1.0);
		let drag_force = json_f32(
			item.get("drag")
				.or_else(|| item.get("dragForce"))
				.or_else(|| item.get("drag_force")),
		)
		.unwrap_or(0.4);
		let hit_radius = json_f32(
			item.get("radius")
				.or_else(|| item.get("hitRadius"))
				.or_else(|| item.get("hit_radius")),
		)
		.unwrap_or(0.02);
		let (gravity_power, gravity_dir) = unavatar_dynamics_gravity(item);
		let limit = unavatar_dynamics_limit(item);
		let interaction = unavatar_dynamics_interaction(item);
		let ignored_nodes = unavatar_dynamics_node_index_set(
			item.get("ignoreTransforms")
				.or_else(|| item.get("ignore_transforms"))
				.or_else(|| item.get("ignoredTransforms"))
				.or_else(|| item.get("ignored_transforms")),
			&node_ids,
			&registry_paths,
			&paths,
			&normalized_paths,
		);
		ignored_transform_count += ignored_nodes.len();
		let multi_child_ignore = unavatar_dynamics_multi_child_ignore(item);
		if multi_child_ignore {
			multi_child_ignore_count += 1;
		}
		colliders.extend(unavatar_dynamics_colliders(
			item,
			source_kind,
			&node_ids,
			&registry_paths,
			&paths,
			&normalized_paths,
		));

		for root in root_values.iter() {
			let Some(root_idx) = unavatar_dynamics_root_index(root, &node_ids, &registry_paths, &paths, &normalized_paths) else {
				missing_roots += 1;
				continue;
			};
			if ensure_unavatar_dynamics_endpoint_child(scene, root_idx, item, &ignored_nodes) {
				endpoint_child_count += 1;
			}
			for chain in collect_scene_child_chains(scene, root_idx, &ignored_nodes, multi_child_ignore) {
				if chain.len() < 2 {
					short_chains += 1;
					continue;
				}
				groups.push(UnaSpringBoneGroup {
					source_kind,
					// VRC PhysBone is imported as source metadata and an action target, but the
					// current SpringBone-like solver is not a faithful PhysBone implementation.
					// Keep it opt-in to avoid visibly deforming authored VRC clothing at rest.
					enabled: source_kind != UnaDynamicsSourceKind::VrcPhysBone,
					source_id: source_id.clone(),
					comment: comment.clone(),
					category: category.clone(),
					stiffness,
					gravity_power,
					gravity_dir,
					drag_force,
					center_node: None,
					hit_radius,
					limit: limit.clone(),
					interaction: interaction.clone(),
					bone_node_indices: chain,
				});
			}
		}
	}

	if missing_roots > 0 || short_chains > 0 {
		report.push_info(format!(
			".unavatar dynamics: skipped missing_roots={missing_roots} short_chains={short_chains}"
		));
	}
	if ignored_transform_count > 0 || multi_child_ignore_count > 0 {
		report.push_info(format!(
			".unavatar dynamics: source_hints ignored_transforms={ignored_transform_count} multi_child_ignore={multi_child_ignore_count}"
		));
	}
	if endpoint_child_count > 0 {
		report.push_info(format!(".unavatar dynamics: synthesized_endpoint_children={endpoint_child_count}"));
	}
	if groups.is_empty() {
		None
	} else {
		report.push_info(format!(
			".unavatar dynamics: lowered_groups={} lowered_colliders={}",
			groups.len(),
			colliders.len()
		));
		Some(UnaSpringBoneSettings { groups, colliders })
	}
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

fn expression_catalog_from_morph_target_names(scene: &UnaSceneSnapshot) -> Option<UnaExpressionCatalog> {
	let mut binds_by_name: BTreeMap<String, Vec<UnaMorphTargetBind>> = BTreeMap::new();
	for (mesh_index, primitives) in scene.meshes.iter().enumerate() {
		for (primitive_index, primitive) in primitives.iter().enumerate() {
			for (morph_target_index, name) in primitive.morph_target_names.iter().enumerate() {
				if name.is_empty() || morph_target_index >= primitive.morph_targets.len() {
					continue;
				}
				binds_by_name.entry(name.clone()).or_default().push(UnaMorphTargetBind {
					mesh_index,
					primitive_index,
					morph_target_index,
					weight_scale: 1.0,
				});
			}
		}
	}
	if binds_by_name.is_empty() {
		return None;
	}
	Some(UnaExpressionCatalog {
		presets: binds_by_name
			.into_iter()
			.map(|(name, binds)| UnaExpressionPreset { name, binds })
			.collect(),
	})
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WardrobeApplyReport {
	pub active_asset_groups: Vec<String>,
	pub visibility_applied: usize,
	pub visibility_missing: usize,
	pub blendshape_applied: usize,
	pub blendshape_missing: usize,
	pub dynamics_applied: usize,
	pub dynamics_missing: usize,
	pub material_applied: usize,
	pub material_missing: usize,
	pub material_slot_applied: usize,
	pub material_slot_missing: usize,
	pub missing_visibility_paths: Vec<String>,
	pub missing_blendshapes: Vec<String>,
	pub missing_dynamics_ids: Vec<String>,
	pub missing_materials: Vec<String>,
	pub missing_material_slots: Vec<String>,
}

fn apply_unavatar_wardrobe_operations(
	scene: &mut UnaSceneSnapshot,
	dynamics: Option<&mut UnaRuntimeDynamicsMut<'_>>,
	operations: &[Value],
	unavatar: Option<&UnaUnavatarExtension>,
) -> WardrobeApplyReport {
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(unavatar);
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let mut report = WardrobeApplyReport::default();
	let mut dynamics = dynamics;
	for op in operations {
		let ty = op.get("type").or_else(|| op.get("op")).and_then(|v| v.as_str()).unwrap_or("");
		let path = operation_target_path(op);
		match ty {
			"subtreeEnabled" | "subtreeVisibility" => {
				let Some(visible) = op.get("visible").and_then(|v| v.as_bool()) else {
					continue;
				};
				let indices = lookup_operation_subtree_targets_all(scene, &node_ids, &registry_paths, &paths, &normalized_paths, op);
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
			"nodeEnabled" | "nodeVisibility" | "rendererEnabled" | "rendererVisibility" => {
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
			"dynamicsEnable" => {
				let Some(enabled) = op.get("enabled").or_else(|| op.get("visible")).and_then(|value| value.as_bool()) else {
					continue;
				};
				let Some(target_id) = operation_dynamics_target_id(op) else {
					continue;
				};
				let Some(dynamics) = dynamics.as_deref_mut() else {
					report.dynamics_missing += 1;
					report.missing_dynamics_ids.push(target_id.to_string());
					continue;
				};
				if dynamics.set_group_enabled_by_source_id(target_id, enabled) {
					report.dynamics_applied += 1;
				} else {
					report.dynamics_missing += 1;
					report.missing_dynamics_ids.push(target_id.to_string());
				}
			}
			"materialColor" | "material_color" => {
				if apply_unavatar_material_color_operation(scene, op) {
					report.material_applied += 1;
				} else {
					report.material_missing += 1;
					report.missing_materials.push(path.to_string());
				}
			}
			"materialScalar" | "material_scalar" => {
				if apply_unavatar_material_scalar_operation(scene, op) {
					report.material_applied += 1;
				} else {
					report.material_missing += 1;
					report.missing_materials.push(path.to_string());
				}
			}
			"materialSlot" | "material_slot" => {
				if apply_unavatar_material_slot_operation(scene, op) {
					report.material_slot_applied += 1;
				} else {
					report.material_slot_missing += 1;
					report.missing_material_slots.push(path.to_string());
				}
			}
			_ => {}
		}
	}
	report
}

fn apply_unavatar_material_slot_operation(scene: &mut UnaSceneSnapshot, op: &Value) -> bool {
	let Some(target) = unavatar_runtime_material_slot_target(op) else {
		return false;
	};
	let Some(material) = unavatar_runtime_material_slot_material(op) else {
		return false;
	};
	let material_index = if let Some(material) = material {
		let Some(material_index) = unavatar_scene_material_index(scene, &material) else {
			return false;
		};
		Some(material_index)
	} else {
		None
	};
	let Some(node_index) = resolve_unavatar_runtime_node_target(scene, &target.node) else {
		return false;
	};
	let Some(mesh_index) = scene.nodes.get(node_index).and_then(|node| node.mesh) else {
		return false;
	};
	let primitive_index = target.primitive_index.unwrap_or(0);
	let Some(primitive) = scene.meshes.get_mut(mesh_index).and_then(|mesh| mesh.get_mut(primitive_index)) else {
		return false;
	};
	primitive.material_index = material_index;
	true
}

fn apply_unavatar_material_color_operation(scene: &mut UnaSceneSnapshot, op: &Value) -> bool {
	let Some(target) = unavatar_runtime_material_target(op) else {
		return false;
	};
	let Some(material_index) = unavatar_scene_material_index(scene, &target) else {
		return false;
	};
	let Some(color) = op.get("color").or_else(|| op.get("value")).and_then(value_vec4) else {
		return false;
	};
	let parameter = op
		.get("parameter")
		.or_else(|| op.get("property"))
		.or_else(|| op.get("name"))
		.and_then(Value::as_str)
		.unwrap_or("_Color");
	scene
		.materials
		.get_mut(material_index)
		.is_some_and(|material| apply_runtime_material_color(material, parameter, color).is_ok())
}

fn apply_unavatar_material_scalar_operation(scene: &mut UnaSceneSnapshot, op: &Value) -> bool {
	let Some(target) = unavatar_runtime_material_target(op) else {
		return false;
	};
	let Some(material_index) = unavatar_scene_material_index(scene, &target) else {
		return false;
	};
	let Some(value) = op.get("value").and_then(Value::as_f64).map(|value| value as f32) else {
		return false;
	};
	let parameter = op
		.get("parameter")
		.or_else(|| op.get("property"))
		.or_else(|| op.get("name"))
		.and_then(Value::as_str)
		.unwrap_or("_Alpha");
	scene
		.materials
		.get_mut(material_index)
		.is_some_and(|material| apply_runtime_material_scalar(material, parameter, value).is_ok())
}

fn resolve_unavatar_runtime_node_target(scene: &UnaSceneSnapshot, target: &UnaRuntimeNodeTarget) -> Option<usize> {
	if let Some(source_node_id) = target.source_node_id.as_deref().filter(|value| !value.is_empty()) {
		if let Some((index, _)) = scene
			.nodes
			.iter()
			.enumerate()
			.find(|(_, node)| node.source_node_id.as_deref() == Some(source_node_id))
		{
			return Some(index);
		}
	}
	if let Some(resolved_node_id) = target.resolved_node_id.as_deref().filter(|value| !value.is_empty()) {
		if let Some((index, _)) = scene
			.nodes
			.iter()
			.enumerate()
			.find(|(_, node)| node.resolved_node_id.as_deref() == Some(resolved_node_id))
		{
			return Some(index);
		}
	}
	if let Some(path) = target.path.as_deref().filter(|value| !value.is_empty()) {
		let paths = scene_node_paths(scene);
		let normalized_paths = scene_node_normalized_paths(scene);
		if let Some(index) = lookup_scene_path_all(&paths, &normalized_paths, path).into_iter().next() {
			return Some(index);
		}
	}
	target.node_index.filter(|index| *index < scene.nodes.len())
}

fn operation_dynamics_target_id(op: &Value) -> Option<&str> {
	let target = op.get("target");
	target
		.and_then(|target| {
			target
				.get("dynamicsId")
				.or_else(|| target.get("dynamics_id"))
				.or_else(|| target.get("sourceId"))
				.or_else(|| target.get("source_id"))
				.or_else(|| target.get("id"))
		})
		.or_else(|| op.get("dynamicsId").or_else(|| op.get("dynamics_id")))
		.or_else(|| op.get("sourceId").or_else(|| op.get("source_id")))
		.or_else(|| op.get("dynamics"))
		.or(target)
		.and_then(Value::as_str)
		.filter(|id| !id.is_empty())
}

fn reset_runtime_dynamics_enabled(dynamics: Option<&mut UnaRuntimeDynamicsMut<'_>>) {
	let Some(dynamics) = dynamics else {
		return;
	};
	dynamics.reset_enabled();
}

fn unavatar_wardrobe_set_operations<'a>(unavatar: &'a UnaUnavatarExtension, set_id: &str) -> Option<&'a [Value]> {
	let wardrobe = unavatar.source.get("wardrobe").and_then(|v| v.as_object())?;
	let sets = wardrobe.get("sets").and_then(|v| v.as_array())?;
	let set = sets.iter().find(|set| set.get("id").and_then(|v| v.as_str()) == Some(set_id))?;
	set.get("operations").and_then(|v| v.as_array()).map(Vec::as_slice)
}

fn unavatar_wardrobe_set_asset_groups(unavatar: &UnaUnavatarExtension, set_id: &str) -> Vec<String> {
	let Some(wardrobe) = unavatar.source.get("wardrobe").and_then(|v| v.as_object()) else {
		return Vec::new();
	};
	let Some(sets) = wardrobe.get("sets").and_then(|v| v.as_array()) else {
		return Vec::new();
	};
	let Some(set) = sets.iter().find(|set| set.get("id").and_then(|v| v.as_str()) == Some(set_id)) else {
		return Vec::new();
	};
	let mut seen = BTreeSet::new();
	set.get("assetGroups")
		.or_else(|| set.get("asset_groups"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.filter(|group| !group.is_empty())
		.filter_map(|group| {
			if seen.insert(group.to_string()) {
				Some(group.to_string())
			} else {
				None
			}
		})
		.collect()
}

fn unavatar_base_wardrobe_set<'a>(unavatar: &'a UnaUnavatarExtension) -> Option<(&'a str, &'a [Value])> {
	let wardrobe = unavatar.source.get("wardrobe").and_then(|v| v.as_object())?;
	let base_set = wardrobe.get("baseSet").and_then(|v| v.as_str()).unwrap_or("base");
	let sets = wardrobe.get("sets").and_then(|v| v.as_array())?;
	let base = sets.iter().find(|set| {
		set.get("id").and_then(|v| v.as_str()) == Some(base_set) || set.get("default").and_then(|v| v.as_bool()).unwrap_or(false)
	})?;
	let id = base.get("id").and_then(|v| v.as_str()).unwrap_or(base_set);
	let operations = base.get("operations").and_then(|v| v.as_array()).map(Vec::as_slice)?;
	Some((id, operations))
}

fn unavatar_runtime_action_set(unavatar: &UnaUnavatarExtension, scene: Option<&UnaSceneSnapshot>) -> Option<UnaRuntimeActionSet> {
	let mut actions = Vec::new();
	if let Some(wardrobe) = unavatar.source.get("wardrobe").and_then(|v| v.as_object()) {
		if let Some(sets) = wardrobe.get("sets").and_then(|v| v.as_array()) {
			let base_id = unavatar_base_wardrobe_set(unavatar).map(|(id, _)| id);
			for set in sets {
				let Some(set_id) = set.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()) else {
					continue;
				};
				if base_id == Some(set_id) {
					continue;
				}
				let label = set
					.get("name")
					.or_else(|| set.get("displayName"))
					.and_then(Value::as_str)
					.unwrap_or(set_id);
				actions.push(UnaRuntimeAction {
					id: format!("wardrobe:{set_id}"),
					label: label.to_string(),
					triggers: vec![UnaRuntimeActionTrigger::SupervisorCommand {
						command: set_id.to_string(),
					}],
					effects: vec![UnaRuntimeActionEffect::WardrobeSet {
						set_id: set_id.to_string(),
					}],
				});
			}
		}
	}
	if let Some(variants) = unavatar.source.get("variants").and_then(|v| v.as_array()) {
		for variant in variants {
			let Some(action) = unavatar_variant_runtime_action(variant) else {
				continue;
			};
			if actions.iter().any(|existing| existing.id == action.id) {
				continue;
			}
			actions.push(action);
		}
	}
	if let Some(components) = unavatar
		.source
		.get("modularAvatar")
		.and_then(|v| v.get("components"))
		.and_then(Value::as_array)
	{
		for (component_index, component) in components.iter().enumerate() {
			let Some(action) = unavatar_modular_avatar_component_runtime_action(component, component_index, scene, unavatar) else {
				continue;
			};
			if actions.iter().any(|existing| existing.id == action.id) {
				continue;
			}
			actions.push(action);
		}
	}
	(!actions.is_empty()).then_some(UnaRuntimeActionSet { actions })
}

fn unavatar_variant_runtime_action(variant: &Value) -> Option<UnaRuntimeAction> {
	let id = variant.get("id").and_then(Value::as_str).filter(|id| !id.is_empty())?;
	if id == "current-state" {
		return None;
	}
	let operations = variant.get("operations").and_then(Value::as_array)?;
	let mut effects = Vec::new();
	for op in operations {
		let ty = op.get("type").or_else(|| op.get("op")).and_then(Value::as_str).unwrap_or("");
		match ty {
			"subtreeEnabled" | "subtreeVisibility" | "nodeEnabled" | "nodeVisibility" | "rendererEnabled" | "rendererVisibility" => {
				let Some(visible) = op.get("visible").and_then(Value::as_bool) else {
					continue;
				};
				let path = operation_target_path(op);
				let target = op.get("target").and_then(Value::as_object);
				let source_node_id = target
					.and_then(|target| target.get("nodeId").or_else(|| target.get("sourceNodeId")))
					.and_then(Value::as_str)
					.filter(|value| !value.is_empty())
					.map(str::to_string);
				let path = (!path.is_empty()).then(|| path.to_string());
				if source_node_id.is_none() && path.is_none() {
					continue;
				}
				effects.push(UnaRuntimeActionEffect::NodeVisibility {
					target: UnaRuntimeNodeTarget {
						node_index: None,
						source_node_id,
						resolved_node_id: None,
						path,
					},
					visible,
				});
			}
			"materialColor" | "material_color" => {
				let Some(target) = unavatar_runtime_material_target(op) else {
					continue;
				};
				let Some(color) = op.get("color").or_else(|| op.get("value")).and_then(value_vec4) else {
					continue;
				};
				let parameter = op
					.get("parameter")
					.or_else(|| op.get("property"))
					.or_else(|| op.get("name"))
					.and_then(Value::as_str)
					.unwrap_or("_Color")
					.to_string();
				effects.push(UnaRuntimeActionEffect::MaterialColor { target, parameter, color });
			}
			"materialScalar" | "material_scalar" => {
				let Some(target) = unavatar_runtime_material_target(op) else {
					continue;
				};
				let Some(value) = op.get("value").and_then(Value::as_f64).map(|value| value as f32) else {
					continue;
				};
				let parameter = op
					.get("parameter")
					.or_else(|| op.get("property"))
					.or_else(|| op.get("name"))
					.and_then(Value::as_str)
					.unwrap_or("_Alpha")
					.to_string();
				effects.push(UnaRuntimeActionEffect::MaterialScalar { target, parameter, value });
			}
			"materialSlot" | "material_slot" => {
				let Some(target) = unavatar_runtime_material_slot_target(op) else {
					continue;
				};
				let Some(material) = unavatar_runtime_material_slot_material(op) else {
					continue;
				};
				effects.push(UnaRuntimeActionEffect::MaterialSlot { target, material });
			}
			"expressionWeight" | "expression_weight" | "blendShapeWeight" => {
				let Some(name) = op
					.get("expression")
					.or_else(|| op.get("preset"))
					.or_else(|| op.get("name"))
					.and_then(Value::as_str)
					.filter(|value| !value.is_empty())
				else {
					continue;
				};
				let Some(weight) = op
					.get("weight")
					.or_else(|| op.get("value"))
					.and_then(Value::as_f64)
					.map(|value| value as f32)
				else {
					continue;
				};
				effects.push(UnaRuntimeActionEffect::ExpressionWeight {
					name: name.to_string(),
					weight,
				});
			}
			"dynamicsEnable" | "dynamics_enabled" | "dynamicsEnabled" => {
				let Some(enabled) = op.get("enabled").or_else(|| op.get("visible")).and_then(Value::as_bool) else {
					continue;
				};
				let Some(source_id) = operation_dynamics_target_id(op) else {
					continue;
				};
				effects.push(UnaRuntimeActionEffect::DynamicsEnabled {
					source_id: source_id.to_string(),
					enabled,
				});
			}
			_ => {}
		}
	}
	if effects.is_empty() {
		return None;
	}
	let label = variant
		.get("name")
		.or_else(|| variant.get("displayName"))
		.and_then(Value::as_str)
		.unwrap_or(id)
		.to_string();
	let expression_menu_path = unavatar_variant_expression_menu_path(variant, &label);
	Some(UnaRuntimeAction {
		id: format!("variant:{id}"),
		label: label.clone(),
		triggers: vec![
			UnaRuntimeActionTrigger::SupervisorCommand { command: id.to_string() },
			UnaRuntimeActionTrigger::ExpressionMenu {
				path: expression_menu_path,
			},
		],
		effects,
	})
}

fn unavatar_modular_avatar_component_runtime_action(
	component: &Value,
	component_index: usize,
	scene: Option<&UnaSceneSnapshot>,
	unavatar: &UnaUnavatarExtension,
) -> Option<UnaRuntimeAction> {
	if component.get("enabled").and_then(Value::as_bool) == Some(false) {
		return None;
	}
	let short_type = component.get("shortType").and_then(Value::as_str).unwrap_or("");
	match short_type {
		"ModularAvatarMaterialSetter" => unavatar_material_setter_runtime_action(component, component_index, scene, unavatar),
		"ModularAvatarMaterialSwap" => unavatar_material_swap_runtime_action(component, component_index, scene?, unavatar),
		_ => None,
	}
}

fn unavatar_material_setter_runtime_action(
	component: &Value,
	component_index: usize,
	scene: Option<&UnaSceneSnapshot>,
	unavatar: &UnaUnavatarExtension,
) -> Option<UnaRuntimeAction> {
	let objects = component
		.get("fields")
		.and_then(|fields| {
			fields
				.get("Objects")
				.or_else(|| fields.get("objects"))
				.or_else(|| fields.get("m_objects"))
				.or_else(|| fields.get("materialSwitchObjects"))
				.or_else(|| fields.get("material_switch_objects"))
		})
		.or_else(|| component.get("objects").or_else(|| component.get("m_objects")))
		.and_then(Value::as_array)?;
	let mut effects = Vec::new();
	for object in objects {
		let Some(target) = unavatar_material_setter_slot_target(object, scene, unavatar) else {
			continue;
		};
		let Some(material) = object
			.get("Material")
			.or_else(|| object.get("material"))
			.or_else(|| object.get("to"))
			.and_then(unavatar_runtime_material_ref_nullable)
		else {
			continue;
		};
		effects.push(UnaRuntimeActionEffect::MaterialSlot { target, material });
	}
	if effects.is_empty() {
		return None;
	}
	let component_id = component
		.get("id")
		.or_else(|| component.get("componentId"))
		.or_else(|| component.get("component_id"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string)
		.unwrap_or_else(|| component_index.to_string());
	let label = unavatar_modular_avatar_component_label(component, "Material Setter");
	let command = format!("ma:material_setter:{component_id}");
	Some(UnaRuntimeAction {
		id: command.clone(),
		label,
		triggers: unavatar_modular_avatar_component_triggers(component, command),
		effects,
	})
}

fn unavatar_material_setter_slot_target(
	object: &Value,
	scene: Option<&UnaSceneSnapshot>,
	unavatar: &UnaUnavatarExtension,
) -> Option<UnaRuntimeMaterialSlotTarget> {
	let target_ref = object
		.get("Object")
		.or_else(|| object.get("object"))
		.or_else(|| object.get("target"))
		.or_else(|| object.get("renderer"))
		.unwrap_or(object);
	let source_node_id = target_ref
		.get("nodeId")
		.or_else(|| target_ref.get("sourceNodeId"))
		.or_else(|| target_ref.get("source_node_id"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string);
	let path = target_ref
		.get("path")
		.or_else(|| target_ref.get("referencePath"))
		.or_else(|| target_ref.get("reference_path"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string);
	if source_node_id.is_none() && path.is_none() {
		return None;
	}
	let primitive_index = object
		.get("MaterialIndex")
		.or_else(|| object.get("materialIndex"))
		.or_else(|| object.get("material_index"))
		.or_else(|| object.get("slot"))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok());
	if let Some(scene) = scene {
		let node_ids = scene_node_ids(scene);
		let registry_paths = unavatar_node_registry_paths(Some(unavatar));
		let paths = scene_node_paths(scene);
		let normalized_paths = scene_node_normalized_paths(scene);
		let node_index = modular_avatar_reference_index(target_ref, &node_ids, &registry_paths, &paths, &normalized_paths)?;
		let node = scene.nodes.get(node_index)?;
		return Some(UnaRuntimeMaterialSlotTarget {
			node: UnaRuntimeNodeTarget {
				node_index: None,
				source_node_id: node.source_node_id.clone(),
				resolved_node_id: None,
				path: scene_node_path_for_index(scene, node_index),
			},
			primitive_index,
		});
	}
	Some(UnaRuntimeMaterialSlotTarget {
		node: UnaRuntimeNodeTarget {
			node_index: None,
			source_node_id,
			resolved_node_id: None,
			path,
		},
		primitive_index,
	})
}

fn unavatar_material_swap_runtime_action(
	component: &Value,
	component_index: usize,
	scene: &UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
) -> Option<UnaRuntimeAction> {
	let swaps = component
		.get("fields")
		.and_then(|fields| {
			fields
				.get("Swaps")
				.or_else(|| fields.get("swaps"))
				.or_else(|| fields.get("m_swaps"))
		})
		.or_else(|| component.get("swaps").or_else(|| component.get("m_swaps")))
		.and_then(Value::as_array)?;
	let root = component
		.get("fields")
		.and_then(|fields| fields.get("Root").or_else(|| fields.get("root")).or_else(|| fields.get("m_root")))
		.or_else(|| component.get("root").or_else(|| component.get("m_root")));
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(Some(unavatar));
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let root_index = match root {
		Some(root) if unavatar_reference_has_target(root) => Some(modular_avatar_reference_index(
			root,
			&node_ids,
			&registry_paths,
			&paths,
			&normalized_paths,
		)?),
		_ => None,
	};
	let parents = scene_parent_indices(scene);
	let mut effects = Vec::new();
	for swap in swaps {
		let Some(from) = swap
			.get("From")
			.or_else(|| swap.get("from"))
			.and_then(unavatar_runtime_material_ref_nullable)
			.and_then(|target| target.map_or(Some(None), |target| unavatar_scene_material_index(scene, &target).map(Some)))
		else {
			continue;
		};
		let Some(material) = swap
			.get("To")
			.or_else(|| swap.get("to"))
			.or_else(|| swap.get("material"))
			.and_then(unavatar_runtime_material_ref_nullable)
		else {
			continue;
		};
		for (node_index, node) in scene.nodes.iter().enumerate() {
			if let Some(root_index) = root_index {
				if node_index != root_index && !scene_is_descendant_of(&parents, node_index, root_index) {
					continue;
				}
			}
			let Some(mesh_index) = node.mesh else {
				continue;
			};
			let Some(mesh) = scene.meshes.get(mesh_index) else {
				continue;
			};
			for (primitive_index, primitive) in mesh.iter().enumerate() {
				if primitive.material_index != from {
					continue;
				}
				effects.push(UnaRuntimeActionEffect::MaterialSlot {
					target: UnaRuntimeMaterialSlotTarget {
						node: UnaRuntimeNodeTarget {
							node_index: None,
							source_node_id: node.source_node_id.clone(),
							resolved_node_id: None,
							path: scene_node_path_for_index(scene, node_index),
						},
						primitive_index: Some(primitive_index),
					},
					material: material.clone(),
				});
			}
		}
	}
	if effects.is_empty() {
		return None;
	}
	let component_id = component
		.get("id")
		.or_else(|| component.get("componentId"))
		.or_else(|| component.get("component_id"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string)
		.unwrap_or_else(|| component_index.to_string());
	let label = unavatar_modular_avatar_component_label(component, "Material Swap");
	let command = format!("ma:material_swap:{component_id}");
	Some(UnaRuntimeAction {
		id: command.clone(),
		label,
		triggers: unavatar_modular_avatar_component_triggers(component, command),
		effects,
	})
}

fn unavatar_modular_avatar_component_triggers(component: &Value, command: String) -> Vec<UnaRuntimeActionTrigger> {
	let mut triggers = vec![UnaRuntimeActionTrigger::SupervisorCommand { command }];
	if let Some(path) = unavatar_modular_avatar_component_expression_menu_path(component) {
		triggers.push(UnaRuntimeActionTrigger::ExpressionMenu { path });
	}
	if let Some((name, value)) = unavatar_modular_avatar_component_parameter_value(component) {
		triggers.push(UnaRuntimeActionTrigger::ParameterValue { name, value });
	}
	triggers
}

fn unavatar_modular_avatar_component_label(component: &Value, fallback: &str) -> String {
	unavatar_named_value(component)
		.or_else(|| component.get("fields").and_then(unavatar_named_value))
		.or_else(|| {
			component.get("fields").and_then(|fields| {
				fields
					.get("menuItem")
					.or_else(|| fields.get("menu_item"))
					.and_then(unavatar_named_value)
			})
		})
		.or_else(|| {
			component
				.get("menuItem")
				.or_else(|| component.get("menu_item"))
				.and_then(unavatar_named_value)
		})
		.unwrap_or(fallback)
		.to_string()
}

fn unavatar_named_value(value: &Value) -> Option<&str> {
	value
		.get("name")
		.or_else(|| value.get("displayName"))
		.or_else(|| value.get("display_name"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
}

fn unavatar_modular_avatar_component_expression_menu_path(component: &Value) -> Option<String> {
	unavatar_explicit_expression_menu_path(component)
		.or_else(|| {
			component.get("fields").and_then(|fields| {
				unavatar_explicit_expression_menu_path(fields)
					.or_else(|| fields.get("menuItem").and_then(unavatar_explicit_expression_menu_path))
					.or_else(|| fields.get("menu_item").and_then(unavatar_explicit_expression_menu_path))
			})
		})
		.or_else(|| {
			component
				.get("menuItem")
				.or_else(|| component.get("menu_item"))
				.and_then(unavatar_explicit_expression_menu_path)
		})
}

fn unavatar_explicit_expression_menu_path(value: &Value) -> Option<String> {
	value
		.get("expressionMenuPath")
		.or_else(|| value.get("expression_menu_path"))
		.or_else(|| value.get("menuPath"))
		.or_else(|| value.get("menu_path"))
		.and_then(Value::as_str)
		.filter(|path| !path.is_empty())
		.map(str::to_string)
}

fn unavatar_modular_avatar_component_parameter_value(component: &Value) -> Option<(String, f32)> {
	unavatar_explicit_parameter_value(component)
		.or_else(|| {
			component.get("fields").and_then(|fields| {
				unavatar_explicit_parameter_value(fields)
					.or_else(|| fields.get("control").and_then(unavatar_explicit_parameter_value))
					.or_else(|| fields.get("Control").and_then(unavatar_explicit_parameter_value))
					.or_else(|| fields.get("menuItem").and_then(unavatar_explicit_parameter_value))
					.or_else(|| fields.get("menu_item").and_then(unavatar_explicit_parameter_value))
					.or_else(|| fields.get("menuItem").and_then(unavatar_menu_item_parameter_value))
					.or_else(|| fields.get("menu_item").and_then(unavatar_menu_item_parameter_value))
			})
		})
		.or_else(|| {
			component
				.get("control")
				.or_else(|| component.get("Control"))
				.and_then(unavatar_explicit_parameter_value)
		})
		.or_else(|| {
			component
				.get("menuItem")
				.or_else(|| component.get("menu_item"))
				.and_then(unavatar_menu_item_parameter_value)
		})
}

fn unavatar_menu_item_parameter_value(menu_item: &Value) -> Option<(String, f32)> {
	unavatar_explicit_parameter_value(menu_item).or_else(|| {
		menu_item
			.get("control")
			.or_else(|| menu_item.get("Control"))
			.and_then(unavatar_explicit_parameter_value)
	})
}

fn unavatar_explicit_parameter_value(value: &Value) -> Option<(String, f32)> {
	let name = value
		.get("parameter")
		.and_then(|parameter| {
			parameter
				.as_str()
				.or_else(|| parameter.get("name").and_then(Value::as_str))
				.or_else(|| parameter.get("Name").and_then(Value::as_str))
		})
		.or_else(|| {
			value
				.get("parameterName")
				.or_else(|| value.get("parameter_name"))
				.and_then(Value::as_str)
		})
		.filter(|value| !value.is_empty())?
		.to_string();
	let value = json_f32(value.get("value").or_else(|| value.get("Value")))?;
	Some((name, value))
}

fn unavatar_scene_material_index(scene: &UnaSceneSnapshot, target: &UnaRuntimeMaterialTarget) -> Option<usize> {
	if let Some(index) = target.material_index.filter(|index| *index < scene.materials.len()) {
		return Some(index);
	}
	let name = target.name.as_deref().filter(|value| !value.is_empty())?;
	scene.materials.iter().position(|material| material.name.as_deref() == Some(name))
}

fn unavatar_reference_has_target(value: &Value) -> bool {
	value
		.get("nodeId")
		.or_else(|| value.get("sourceNodeId"))
		.or_else(|| value.get("source_node_id"))
		.or_else(|| value.get("path"))
		.or_else(|| value.get("referencePath"))
		.or_else(|| value.get("reference_path"))
		.and_then(Value::as_str)
		.is_some_and(|value| !value.is_empty())
		|| value.get("resolvedTarget").is_some_and(unavatar_reference_has_target)
		|| value.get("targetObject").is_some_and(unavatar_reference_has_target)
}

fn scene_node_path_for_index(scene: &UnaSceneSnapshot, target_index: usize) -> Option<String> {
	let parents = scene_parent_indices(scene);
	let mut parts = Vec::new();
	let mut current = Some(target_index);
	while let Some(index) = current {
		let node = scene.nodes.get(index)?;
		parts.push(node.name.as_deref().unwrap_or("").to_string());
		current = parents.get(index).copied().flatten();
	}
	parts.reverse();
	let path = parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("/");
	(!path.is_empty()).then_some(path)
}

fn unavatar_variant_expression_menu_path(variant: &Value, fallback_label: &str) -> String {
	let metadata_path = variant.get("operations").and_then(Value::as_array).and_then(|operations| {
		operations.iter().find_map(|op| {
			let ty = op.get("type").or_else(|| op.get("op")).and_then(Value::as_str).unwrap_or("");
			if ty != "metadata" {
				return None;
			}
			op.get("expressionMenuPath")
				.or_else(|| op.get("expression_menu_path"))
				.or_else(|| op.get("menuPath"))
				.or_else(|| op.get("menu_path"))
				.or_else(|| op.get("path"))
				.and_then(Value::as_str)
				.filter(|path| !path.is_empty())
		})
	});
	metadata_path.unwrap_or(fallback_label).to_string()
}

fn unavatar_runtime_material_target(op: &Value) -> Option<UnaRuntimeMaterialTarget> {
	let target = op.get("target");
	let material_index = target
		.and_then(|target| target.get("materialIndex").or_else(|| target.get("material_index")))
		.or_else(|| op.get("materialIndex").or_else(|| op.get("material_index")))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok());
	let name = target
		.and_then(|target| {
			target
				.get("materialName")
				.or_else(|| target.get("material_name"))
				.or_else(|| target.get("name"))
		})
		.or_else(|| {
			op.get("materialName")
				.or_else(|| op.get("material_name"))
				.or_else(|| op.get("material"))
		})
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string);
	if material_index.is_none() && name.is_none() {
		return None;
	}
	Some(UnaRuntimeMaterialTarget { material_index, name })
}

fn unavatar_runtime_material_ref(value: &Value) -> Option<UnaRuntimeMaterialTarget> {
	if let Some(name) = value.as_str().filter(|value| !value.is_empty()) {
		return Some(UnaRuntimeMaterialTarget {
			material_index: None,
			name: Some(name.to_string()),
		});
	}
	let material_index = value
		.get("materialIndex")
		.or_else(|| value.get("material_index"))
		.or_else(|| value.get("index"))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok());
	let name = value
		.get("materialName")
		.or_else(|| value.get("material_name"))
		.or_else(|| value.get("name"))
		.or_else(|| value.get("material"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string);
	if material_index.is_none() && name.is_none() {
		return None;
	}
	Some(UnaRuntimeMaterialTarget { material_index, name })
}

fn unavatar_runtime_material_ref_nullable(value: &Value) -> Option<Option<UnaRuntimeMaterialTarget>> {
	if value.is_null() {
		return Some(None);
	}
	unavatar_runtime_material_ref(value).map(Some)
}

fn unavatar_runtime_material_slot_target(op: &Value) -> Option<UnaRuntimeMaterialSlotTarget> {
	let target = op.get("target").unwrap_or(op);
	let source_node_id = target
		.get("nodeId")
		.or_else(|| target.get("sourceNodeId"))
		.or_else(|| target.get("source_node_id"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string);
	let path = operation_target_path(op);
	let path = (!path.is_empty()).then(|| path.to_string());
	if source_node_id.is_none() && path.is_none() {
		return None;
	}
	let primitive_index = target
		.get("primitiveIndex")
		.or_else(|| target.get("primitive_index"))
		.or_else(|| target.get("materialSlot"))
		.or_else(|| target.get("material_slot"))
		.or_else(|| target.get("slot"))
		.or_else(|| op.get("primitiveIndex"))
		.or_else(|| op.get("primitive_index"))
		.or_else(|| op.get("materialSlot"))
		.or_else(|| op.get("material_slot"))
		.or_else(|| op.get("slot"))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok());
	Some(UnaRuntimeMaterialSlotTarget {
		node: UnaRuntimeNodeTarget {
			node_index: None,
			source_node_id,
			resolved_node_id: None,
			path,
		},
		primitive_index,
	})
}

fn unavatar_runtime_material_slot_material(op: &Value) -> Option<Option<UnaRuntimeMaterialTarget>> {
	if let Some(material) = op
		.get("material")
		.or_else(|| op.get("toMaterial"))
		.or_else(|| op.get("to_material"))
		.or_else(|| op.get("to"))
		.or_else(|| op.get("replacementMaterial"))
		.or_else(|| op.get("replacement_material"))
	{
		if let Some(target) = unavatar_runtime_material_ref_nullable(material) {
			return Some(target);
		}
	}
	unavatar_runtime_material_target(op).map(Some)
}

fn value_vec4(value: &Value) -> Option<[f32; 4]> {
	let values = value.as_array()?;
	if values.len() != 4 {
		return None;
	}
	Some([
		values[0].as_f64()? as f32,
		values[1].as_f64()? as f32,
		values[2].as_f64()? as f32,
		values[3].as_f64()? as f32,
	])
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
	let applied = apply_unavatar_wardrobe_operations(scene, None, &filtered_operations, Some(unavatar));
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
	let rotation = if rotation.is_finite() && rotation.length_squared() > 0.0 {
		rotation.normalize()
	} else {
		Quat::IDENTITY
	};
	let translation = if translation.is_finite() { translation } else { Vec3::ZERO };
	(scale, rotation, translation)
}

fn bone_proxy_local_transform(mode: &str, match_scale: bool, target_world: Mat4, old_world: Mat4) -> Mat4 {
	let target_inverse = inverse_finite_or_identity(target_world);
	let (preserved_local_scale, _, _) = decompose_finite(target_inverse * old_world);
	let (_, old_rotation, old_translation) = decompose_finite(old_world);
	let (local_scale, local_rotation, local_translation) = match mode {
		"AsChildAtRoot" | "Unset" | "" => (preserved_local_scale, Quat::IDENTITY, Vec3::ZERO),
		"AsChildKeepPosition" => (
			preserved_local_scale,
			Quat::IDENTITY,
			target_inverse.transform_point3(old_translation),
		),
		"AsChildKeepRotation" => {
			let (_, target_rotation, _) = decompose_finite(target_world);
			(preserved_local_scale, target_rotation.inverse() * old_rotation, Vec3::ZERO)
		}
		"AsChildKeepWorldPose" => {
			let (scale, rotation, translation) = decompose_finite(target_inverse * old_world);
			(scale, rotation, translation)
		}
		_ => {
			let (scale, rotation, translation) = decompose_finite(target_inverse * old_world);
			(scale, rotation, translation)
		}
	};
	if match_scale {
		Mat4::from_scale_rotation_translation(Vec3::ONE, local_rotation, local_translation)
	} else {
		Mat4::from_scale_rotation_translation(local_scale, local_rotation, local_translation)
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

fn make_unique_child_name(scene: &UnaSceneSnapshot, parent: usize, child: usize) -> Option<String> {
	let base_name = scene
		.nodes
		.get(child)
		.and_then(|node| node.name.as_deref())
		.filter(|name| !name.is_empty())?;
	let Some(parent_node) = scene.nodes.get(parent) else {
		return Some(base_name.to_string());
	};
	let mut suffix = String::new();
	let mut suffix_index = 1usize;
	while parent_node.children.iter().any(|&sibling| {
		scene
			.nodes
			.get(sibling)
			.and_then(|node| node.name.as_deref())
			.is_some_and(|name| name == format!("{base_name}{suffix}"))
	}) {
		suffix = format!(" ({suffix_index})");
		suffix_index += 1;
	}
	Some(format!("{base_name}{suffix}"))
}

fn reparent_bone_proxy_node(scene: &mut UnaSceneSnapshot, child: usize, new_parent: usize, local: Mat4) -> bool {
	let Some(unique_name) = make_unique_child_name(scene, new_parent, child) else {
		return reparent_scene_node(scene, child, new_parent, local);
	};
	if let Some(node) = scene.nodes.get_mut(child) {
		node.name = Some(unique_name);
	}
	reparent_scene_node(scene, child, new_parent, local)
}

fn replace_scene_child(parent_children: &mut Vec<usize>, old_child: usize, new_child: usize, insert_index: usize) {
	parent_children.retain(|&idx| idx != old_child && idx != new_child);
	let index = insert_index.min(parent_children.len());
	parent_children.insert(index, new_child);
}

fn remap_scene_node_references(scene: &mut UnaSceneSnapshot, old_node: usize, new_node: usize) {
	for skin in &mut scene.skins {
		for joint_node in &mut skin.joint_nodes {
			if *joint_node == old_node {
				*joint_node = new_node;
			}
		}
		if skin.skeleton_node == Some(old_node) {
			skin.skeleton_node = Some(new_node);
		}
	}
	for node in &mut scene.nodes {
		if node.probe_anchor_node == Some(old_node) {
			node.probe_anchor_node = Some(new_node);
		}
	}
	for constraint in &mut scene.node_constraints {
		if constraint.source_node == old_node {
			constraint.source_node = new_node;
		}
		if constraint.target_node == old_node {
			constraint.target_node = new_node;
		}
	}
}

fn replace_object_resolved_node_id(scene: &UnaSceneSnapshot, original: usize, replacement: usize) -> String {
	let original_id = scene
		.nodes
		.get(original)
		.and_then(|node| node.source_node_id.as_deref())
		.filter(|value| !value.is_empty())
		.map(str::to_string)
		.unwrap_or_else(|| format!("node#{original}"));
	let replacement_id = scene
		.nodes
		.get(replacement)
		.and_then(|node| node.source_node_id.as_deref())
		.filter(|value| !value.is_empty())
		.map(str::to_string)
		.unwrap_or_else(|| format!("node#{replacement}"));
	format!("ma:replace_object:{original_id}:{replacement_id}")
}

fn replace_scene_object(scene: &mut UnaSceneSnapshot, original: usize, replacement: usize, initial_world: &[Mat4]) -> bool {
	if original >= scene.nodes.len() || replacement >= scene.nodes.len() || original == replacement {
		return false;
	}
	let parents = scene_parent_indices(scene);
	if scene_is_descendant_of(&parents, replacement, original) {
		return false;
	}
	let original_parent = parents.get(original).copied().flatten();
	let replacement_parent = parents.get(replacement).copied().flatten();
	let sibling_index = original_parent
		.and_then(|parent| scene.nodes.get(parent))
		.and_then(|parent| parent.children.iter().position(|&child| child == original))
		.unwrap_or_else(|| scene.roots.iter().position(|&root| root == original).unwrap_or(scene.roots.len()));
	let replacement_world = initial_world.get(replacement).copied().unwrap_or(Mat4::IDENTITY);
	if let Some(parent) = replacement_parent {
		if let Some(parent_node) = scene.nodes.get_mut(parent) {
			parent_node.children.retain(|&idx| idx != replacement);
		}
	} else {
		scene.roots.retain(|&idx| idx != replacement);
	}
	let parent_world = original_parent
		.and_then(|parent| initial_world.get(parent).copied())
		.unwrap_or(Mat4::IDENTITY);
	let resolved_node_id = replace_object_resolved_node_id(scene, original, replacement);
	if let Some(node) = scene.nodes.get_mut(replacement) {
		node.transform = (inverse_finite_or_identity(parent_world) * replacement_world).to_cols_array();
		node.resolved_node_id = Some(resolved_node_id);
	}
	if let Some(parent) = original_parent {
		if let Some(parent_node) = scene.nodes.get_mut(parent) {
			replace_scene_child(&mut parent_node.children, original, replacement, sibling_index);
		}
	} else {
		replace_scene_child(&mut scene.roots, original, replacement, sibling_index);
	}
	let child_world_parent = inverse_finite_or_identity(replacement_world);
	let original_children = scene.nodes.get(original).map(|node| node.children.clone()).unwrap_or_default();
	for child in original_children {
		let old_world = initial_world.get(child).copied().unwrap_or(Mat4::IDENTITY);
		if let Some(node) = scene.nodes.get_mut(child) {
			node.transform = (child_world_parent * old_world).to_cols_array();
		}
		if let Some(replacement_node) = scene.nodes.get_mut(replacement) {
			if !replacement_node.children.contains(&child) {
				replacement_node.children.push(child);
			}
		}
	}
	if let Some(node) = scene.nodes.get_mut(original) {
		node.children.clear();
		node.visible = false;
	}
	remap_scene_node_references(scene, original, replacement);
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

fn collect_primary_humanoid_name_targets(scene: &UnaSceneSnapshot, humanoid: &HumanoidProfile) -> BTreeMap<String, usize> {
	let mut targets = BTreeMap::new();
	for &node_index in humanoid.bone_node_indices.values() {
		let Some(name) = scene
			.nodes
			.get(node_index)
			.and_then(|node| node.name.as_deref())
			.filter(|name| !name.is_empty())
		else {
			continue;
		};
		targets.entry(name.to_string()).or_insert(node_index);
	}
	targets
}

fn collect_same_name_humanoid_armature_mappings(scene: &UnaSceneSnapshot, humanoid: &HumanoidProfile) -> BTreeMap<usize, usize> {
	let targets = collect_primary_humanoid_name_targets(scene, humanoid);
	if targets.is_empty() {
		return BTreeMap::new();
	}
	let mut mappings = BTreeMap::new();
	for skin in &scene.skins {
		for &joint_node in &skin.joint_nodes {
			let Some(name) = scene.nodes.get(joint_node).and_then(|node| node.name.as_deref()) else {
				continue;
			};
			let Some(&target_node) = targets.get(name) else {
				continue;
			};
			if joint_node != target_node {
				mappings.entry(joint_node).or_insert(target_node);
			}
		}
		if let Some(skeleton_node) = skin.skeleton_node {
			let Some(name) = scene.nodes.get(skeleton_node).and_then(|node| node.name.as_deref()) else {
				continue;
			};
			let Some(&target_node) = targets.get(name) else {
				continue;
			};
			if skeleton_node != target_node {
				mappings.entry(skeleton_node).or_insert(target_node);
			}
		}
	}
	mappings
}

fn subtree_contains_mapped_node(scene: &UnaSceneSnapshot, node: usize, mappings: &BTreeMap<usize, usize>) -> bool {
	if mappings.contains_key(&node) {
		return true;
	}
	let Some(scene_node) = scene.nodes.get(node) else {
		return false;
	};
	scene_node
		.children
		.iter()
		.any(|&child| child < scene.nodes.len() && subtree_contains_mapped_node(scene, child, mappings))
}

fn reparent_merge_armature_auxiliary_bones(scene: &mut UnaSceneSnapshot, mappings: &BTreeMap<usize, usize>) -> usize {
	if mappings.is_empty() {
		return 0;
	}
	let initial_world = scene_world_matrices(scene);
	let mut reparent_ops = Vec::new();
	for (&source_node, &target_node) in mappings {
		let Some(source) = scene.nodes.get(source_node) else {
			continue;
		};
		for &child in &source.children {
			if child >= scene.nodes.len() || subtree_contains_mapped_node(scene, child, mappings) {
				continue;
			}
			let old_world = initial_world.get(child).copied().unwrap_or(Mat4::IDENTITY);
			reparent_ops.push((child, target_node, old_world));
		}
	}
	let reparent_world = scene_world_matrices(scene);
	let mut reparented = 0usize;
	for (child, new_parent, old_world) in reparent_ops {
		let parent_world = reparent_world.get(new_parent).copied().unwrap_or(Mat4::IDENTITY);
		let local = inverse_finite_or_identity(parent_world) * old_world;
		if reparent_scene_node(scene, child, new_parent, local) {
			reparented += 1;
		}
	}
	reparented
}

fn retarget_same_name_humanoid_armature_skins(scene: &mut UnaSceneSnapshot, humanoid: &HumanoidProfile) -> (usize, usize, usize) {
	let mappings = collect_same_name_humanoid_armature_mappings(scene, humanoid);
	let auxiliary_reparented = reparent_merge_armature_auxiliary_bones(scene, &mappings);
	let retargeted = retarget_merge_armature_skins(scene, &mappings);
	(mappings.len(), retargeted, auxiliary_reparented)
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

fn replace_object_target_ref(component: &Value) -> Option<&Value> {
	component
		.get("fields")
		.and_then(|fields| fields.get("targetObject"))
		.or_else(|| component.get("targetObject"))
		.or_else(|| component.get("resolvedTarget"))
		.or_else(|| component.get("original"))
}

fn apply_unavatar_replace_objects(
	scene: &mut UnaSceneSnapshot,
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> (usize, usize, usize, usize) {
	let mut replacements = Vec::new();
	let mut replaced_originals = BTreeSet::new();
	let mut applied = 0usize;
	let mut missing = 0usize;
	let mut skipped = 0usize;
	let mut invalid = 0usize;
	let parents = scene_parent_indices(scene);
	for component in components {
		if component.get("shortType").and_then(|v| v.as_str()) != Some("ModularAvatarReplaceObject") {
			continue;
		}
		if component.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
			skipped += 1;
			continue;
		}
		let Some(replacement_ref) = component.get("target") else {
			missing += 1;
			continue;
		};
		let Some(original_ref) = replace_object_target_ref(component) else {
			missing += 1;
			continue;
		};
		let Some(replacement) = unavatar_node_ref_index(replacement_ref, node_ids, registry_paths, paths, normalized_paths) else {
			missing += 1;
			continue;
		};
		let Some(original) = modular_avatar_reference_index(original_ref, node_ids, registry_paths, paths, normalized_paths) else {
			missing += 1;
			continue;
		};
		if original == replacement || scene_is_descendant_of(&parents, replacement, original) || !replaced_originals.insert(original) {
			invalid += 1;
			continue;
		}
		replacements.push((original, replacement));
	}
	let initial_world = scene_world_matrices(scene);
	for (original, replacement) in replacements {
		if replace_scene_object(scene, original, replacement, &initial_world) {
			applied += 1;
		} else {
			invalid += 1;
		}
	}
	(applied, missing, skipped, invalid)
}

fn modular_avatar_remove_vertex_color_removes(component: &Value) -> bool {
	let Some(mode) = component
		.get("fields")
		.and_then(|fields| fields.get("Mode").or_else(|| fields.get("mode")))
		.or_else(|| component.get("Mode").or_else(|| component.get("mode")))
	else {
		return true;
	};
	match mode {
		Value::String(mode) => !matches!(mode.as_str(), "DontRemove" | "dontRemove" | "dont_remove" | "1"),
		Value::Number(mode) => mode.as_u64() != Some(1),
		_ => true,
	}
}

fn apply_unavatar_remove_vertex_color(
	scene: &mut UnaSceneSnapshot,
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> (usize, usize, usize, usize) {
	let mut removers = BTreeMap::<usize, bool>::new();
	let mut missing = 0usize;
	let mut skipped = 0usize;
	for component in components {
		if component.get("shortType").and_then(Value::as_str) != Some("ModularAvatarRemoveVertexColor") {
			continue;
		}
		if component.get("enabled").and_then(Value::as_bool) == Some(false) {
			skipped += 1;
			continue;
		}
		let Some(target_ref) = component.get("target").or_else(|| component.get("resolvedTarget")) else {
			missing += 1;
			continue;
		};
		let Some(target) = modular_avatar_reference_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
			missing += 1;
			continue;
		};
		let remove = modular_avatar_remove_vertex_color_removes(component);
		removers.insert(target, remove);
	}
	if removers.is_empty() {
		return (0, 0, missing, skipped);
	}

	let parents = scene_parent_indices(scene);
	let mesh_user_counts = scene
		.nodes
		.iter()
		.filter_map(|node| node.mesh)
		.fold(BTreeMap::<usize, usize>::new(), |mut counts, mesh| {
			*counts.entry(mesh).or_default() += 1;
			counts
		});
	let mut removed_nodes = 0usize;
	let mut removed_primitives = 0usize;
	for node_idx in 0..scene.nodes.len() {
		let mut cursor = Some(node_idx);
		let mut nearest_remove = None;
		while let Some(idx) = cursor {
			if let Some(&remove) = removers.get(&idx) {
				nearest_remove = Some(remove);
				break;
			}
			cursor = parents.get(idx).copied().flatten();
		}
		if nearest_remove != Some(true) {
			continue;
		}
		let Some(mesh_idx) = scene.nodes.get(node_idx).and_then(|node| node.mesh) else {
			continue;
		};
		let target_mesh_idx = if mesh_user_counts.get(&mesh_idx).copied().unwrap_or(0) > 1 {
			let Some(mesh) = scene.meshes.get(mesh_idx).cloned() else {
				continue;
			};
			scene.meshes.push(mesh);
			let cloned_idx = scene.meshes.len() - 1;
			if let Some(node) = scene.nodes.get_mut(node_idx) {
				node.mesh = Some(cloned_idx);
			}
			cloned_idx
		} else {
			mesh_idx
		};
		let Some(mesh) = scene.meshes.get_mut(target_mesh_idx) else {
			continue;
		};
		let mut node_removed = false;
		for primitive in mesh {
			if primitive.colors_0.take().is_some() {
				removed_primitives += 1;
				node_removed = true;
			}
		}
		if node_removed {
			removed_nodes += 1;
		}
	}
	(removed_nodes, removed_primitives, missing, skipped)
}

fn report_unavatar_modular_avatar_component_catalog(components: &[Value], report: &mut ImportReport) {
	if components.is_empty() {
		return;
	}
	let mut resolver_supported = 0usize;
	let mut runtime_action_supported = 0usize;
	let mut unsupported = 0usize;
	let mut disabled = 0usize;
	let mut unsupported_types = BTreeMap::<String, usize>::new();
	let mut unsupported_active_types = BTreeMap::<String, usize>::new();
	for component in components {
		let component_disabled = component.get("enabled").and_then(Value::as_bool) == Some(false);
		if component_disabled {
			disabled += 1;
		}
		let short_type = component
			.get("shortType")
			.and_then(Value::as_str)
			.filter(|value| !value.is_empty())
			.unwrap_or("unknown");
		match modular_avatar_component_support_kind(short_type) {
			"resolver" => resolver_supported += 1,
			"runtime_action" => runtime_action_supported += 1,
			_ => {
				unsupported += 1;
				*unsupported_types.entry(short_type.to_string()).or_default() += 1;
				if !component_disabled {
					*unsupported_active_types.entry(short_type.to_string()).or_default() += 1;
				}
			}
		}
	}
	for (short_type, count) in &unsupported_active_types {
		report.push_warning(format!(
			".unavatar Modular Avatar unsupported component: type={short_type}, count={count}"
		));
		report.lost_features.push(un_avatar_core::LostFeature {
			feature: format!("ModularAvatar.{short_type}"),
			detail: Some(format!(
				"{count} unsupported Modular Avatar component(s) were preserved as source payload but not applied"
			)),
		});
	}
	let unsupported_types = unsupported_types
		.into_iter()
		.map(|(ty, count)| format!("{ty}:{count}"))
		.collect::<Vec<_>>()
		.join(",");
	report.push_info(format!(
		".unavatar Modular Avatar components: total={}, resolver_supported={}, runtime_action_supported={}, unsupported={}, disabled={}, unsupported_types={}",
		components.len(),
		resolver_supported,
		runtime_action_supported,
		unsupported,
		disabled,
		unsupported_types
	));
}

fn apply_unavatar_modular_avatar(scene: &mut UnaSceneSnapshot, unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	let Some(modular_avatar) = unavatar.source.get("modularAvatar").and_then(|v| v.as_object()) else {
		return;
	};
	let Some(components) = modular_avatar.get("components").and_then(|v| v.as_array()) else {
		return;
	};
	report_unavatar_modular_avatar_component_catalog(components, report);
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(Some(unavatar));
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let (remove_vcol_nodes, remove_vcol_primitives, remove_vcol_missing, remove_vcol_skipped) =
		apply_unavatar_remove_vertex_color(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths);
	if remove_vcol_nodes > 0 || remove_vcol_primitives > 0 || remove_vcol_missing > 0 || remove_vcol_skipped > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: remove_vertex_color_nodes={remove_vcol_nodes}, remove_vertex_color_primitives={remove_vcol_primitives}, remove_vertex_color_missing={remove_vcol_missing}, remove_vertex_color_skipped={remove_vcol_skipped}"
		));
	}
	let (mesh_settings_root_bones, mesh_settings_probe_anchors, mesh_settings_bounds, mesh_settings_missing) =
		apply_unavatar_mesh_settings(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths);
	if mesh_settings_root_bones > 0 || mesh_settings_probe_anchors > 0 || mesh_settings_bounds > 0 || mesh_settings_missing > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: mesh_settings_root_bones={}, mesh_settings_probe_anchors={}, mesh_settings_bounds={}, mesh_settings_missing={}",
			mesh_settings_root_bones, mesh_settings_probe_anchors, mesh_settings_bounds, mesh_settings_missing
		));
	}

	let (replace_object_applied, replace_object_missing, replace_object_skipped, replace_object_invalid) =
		apply_unavatar_replace_objects(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths);
	if replace_object_applied > 0 || replace_object_missing > 0 || replace_object_skipped > 0 || replace_object_invalid > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: replace_object_applied={replace_object_applied}, replace_object_missing={replace_object_missing}, replace_object_skipped={replace_object_skipped}, replace_object_invalid={replace_object_invalid}"
		));
	}

	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let (merge_mappings, merge_missing, merge_skipped) =
		collect_merge_armature_bone_mappings(components, &node_ids, &registry_paths, &paths, &normalized_paths);
	let merge_auxiliary_reparented = reparent_merge_armature_auxiliary_bones(scene, &merge_mappings);
	let merge_retargeted = retarget_merge_armature_skins(scene, &merge_mappings);
	if merge_retargeted > 0 || merge_auxiliary_reparented > 0 || merge_missing > 0 || merge_skipped > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: merge_armature_mappings={}, mesh_retargeter_joints={}, merge_armature_auxiliary_bones={}, merge_armature_missing={}, merge_armature_skipped={}",
			merge_mappings.len(),
			merge_retargeted,
			merge_auxiliary_reparented,
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
		if reparent_bone_proxy_node(scene, proxy.child, proxy.new_parent, local) {
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
	let Some(unavatar) = document.unavatar.clone() else {
		return Err("document has no .unavatar extension".to_string());
	};
	let Some(operations) = unavatar_wardrobe_set_operations(&unavatar, set_id) else {
		return Err(format!(".unavatar wardrobe set not found: {set_id}"));
	};
	let active_asset_groups = unavatar_wardrobe_set_asset_groups(&unavatar, set_id);
	let Some(mut runtime) = document.runtime_scene_and_dynamics_mut() else {
		return Err("document has no scene".to_string());
	};
	reset_runtime_dynamics_enabled(Some(&mut runtime.dynamics));
	let base_id = unavatar_base_wardrobe_set(&unavatar).map(|(id, _)| id.to_string());
	if base_id.as_deref() == Some(set_id) {
		let Some((base_operations, _skipped, reset_operations)) = filtered_unavatar_base_wardrobe_operations(runtime.scene, &unavatar)
		else {
			drop(runtime);
			document.runtime_model_mut().set_active_wardrobe_set(Some(set_id.to_string()));
			document.runtime_model_mut().set_active_asset_groups(active_asset_groups);
			return Ok(WardrobeApplyReport {
				active_asset_groups: document.runtime_model().active_asset_groups().to_vec(),
				..Default::default()
			});
		};
		reset_scene_visibility(runtime.scene);
		let _ = apply_unavatar_wardrobe_operations(runtime.scene, Some(&mut runtime.dynamics), &reset_operations, Some(&unavatar));
		let mut report = apply_unavatar_wardrobe_operations(runtime.scene, Some(&mut runtime.dynamics), &base_operations, Some(&unavatar));
		report.active_asset_groups = active_asset_groups.clone();
		drop(runtime);
		document.runtime_model_mut().set_active_wardrobe_set(Some(set_id.to_string()));
		document.runtime_model_mut().set_active_asset_groups(active_asset_groups);
		return Ok(report);
	}
	if base_id.as_deref() != Some(set_id) {
		if let Some((base_operations, _skipped, reset_operations)) = filtered_unavatar_base_wardrobe_operations(runtime.scene, &unavatar) {
			reset_scene_visibility(runtime.scene);
			let _ = apply_unavatar_wardrobe_operations(runtime.scene, Some(&mut runtime.dynamics), &reset_operations, Some(&unavatar));
			let _ = apply_unavatar_wardrobe_operations(runtime.scene, Some(&mut runtime.dynamics), &base_operations, Some(&unavatar));
		}
	}
	let mut report = apply_unavatar_wardrobe_operations(runtime.scene, Some(&mut runtime.dynamics), operations, Some(&unavatar));
	report.active_asset_groups = active_asset_groups.clone();
	drop(runtime);
	document.runtime_model_mut().set_active_wardrobe_set(Some(set_id.to_string()));
	document.runtime_model_mut().set_active_asset_groups(active_asset_groups);
	Ok(report)
}

fn apply_unavatar_base_wardrobe(scene: &mut UnaSceneSnapshot, unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	let Some((filtered_operations, skipped, reset_operations)) = filtered_unavatar_base_wardrobe_operations(scene, unavatar) else {
		return;
	};
	reset_scene_visibility(scene);
	let _ = apply_unavatar_wardrobe_operations(scene, None, &reset_operations, Some(unavatar));
	let applied = apply_unavatar_wardrobe_operations(scene, None, &filtered_operations, Some(unavatar));
	if applied.visibility_applied > 0
		|| applied.visibility_missing > 0
		|| applied.blendshape_applied > 0
		|| applied.blendshape_missing > 0
		|| applied.dynamics_applied > 0
		|| applied.dynamics_missing > 0
		|| applied.material_applied > 0
		|| applied.material_missing > 0
		|| applied.material_slot_applied > 0
		|| applied.material_slot_missing > 0
	{
		report.push_info(format!(
			".unavatar wardrobe base: visibility_applied={}, visibility_missing={}, blendshape_applied={}, blendshape_missing={}, dynamics_applied={}, dynamics_missing={}, material_applied={}, material_missing={}, material_slot_applied={}, material_slot_missing={}, inherited_hidden_skipped={}",
			applied.visibility_applied,
			applied.visibility_missing,
			applied.blendshape_applied,
			applied.blendshape_missing,
			applied.dynamics_applied,
			applied.dynamics_missing,
			applied.material_applied,
			applied.material_missing,
			applied.material_slot_applied,
			applied.material_slot_missing,
			skipped
		));
	}
}

fn reset_scene_visibility(scene: &mut UnaSceneSnapshot) {
	for node in &mut scene.nodes {
		node.visible = true;
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
	let mut out = Vec::with_capacity(document.skins().len());
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

fn filtered_unavatar_base_wardrobe_operations(
	scene: &UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
) -> Option<(Vec<Value>, usize, Vec<Value>)> {
	let (_, operations) = unavatar_base_wardrobe_set(unavatar)?;
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
	let mut parent_by_index = vec![None; scene.nodes.len()];
	for (parent, node) in scene.nodes.iter().enumerate() {
		for &child in &node.children {
			if let Some(slot) = parent_by_index.get_mut(child) {
				*slot = Some(parent);
			}
		}
	}
	let base_hidden_indices = operations
		.iter()
		.filter(|op| op.get("visible").and_then(|v| v.as_bool()) == Some(false))
		.flat_map(|op| lookup_operation_subtree_targets_all(scene, &node_ids, &registry_paths, &paths, &normalized_paths, op))
		.collect::<BTreeSet<_>>();
	let base_hidden_paths = operations
		.iter()
		.filter(|op| op.get("visible").and_then(|v| v.as_bool()) == Some(false))
		.flat_map(|op| {
			let resolved = lookup_operation_subtree_targets_all(scene, &node_ids, &registry_paths, &paths, &normalized_paths, op);
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
	let mut filtered_operations = Vec::with_capacity(operations.len());
	let mut reset_operations = Vec::new();
	for op in operations {
		let mut skip_inherited_hidden = false;
		let ty = op.get("type").or_else(|| op.get("op")).and_then(|v| v.as_str()).unwrap_or("");
		if matches!(
			ty,
			"subtreeEnabled" | "subtreeVisibility" | "nodeEnabled" | "nodeVisibility" | "rendererEnabled" | "rendererVisibility"
		) && op.get("visible").and_then(|v| v.as_bool()) == Some(false)
		{
			let resolved = lookup_operation_targets_all(&node_ids, &registry_paths, &paths, &normalized_paths, op);
			if !resolved.is_empty()
				&& resolved.iter().all(|idx| {
					let mut parent = parent_by_index.get(*idx).copied().flatten();
					while let Some(parent_idx) = parent {
						if base_hidden_indices.contains(&parent_idx) {
							return true;
						}
						parent = parent_by_index.get(parent_idx).copied().flatten();
					}
					false
				}) {
				skip_inherited_hidden = true;
			}
		}
		if !skip_inherited_hidden {
			skip_inherited_hidden = base_operation_is_inherited_hidden_under_base(
				op,
				&base_hidden_paths,
				&node_ids,
				&registry_paths,
				&paths,
				&normalized_paths,
				&paths_by_index,
			);
		}
		if skip_inherited_hidden {
			let ty = op.get("type").or_else(|| op.get("op")).and_then(|v| v.as_str()).unwrap_or("");
			if matches!(
				ty,
				"subtreeEnabled" | "subtreeVisibility" | "nodeEnabled" | "nodeVisibility" | "rendererEnabled" | "rendererVisibility"
			) && op.get("visible").and_then(|v| v.as_bool()) == Some(false)
			{
				let mut reset = op.clone();
				if let Some(object) = reset.as_object_mut() {
					object.insert("visible".to_string(), Value::Bool(true));
				}
				reset_operations.push(reset);
			}
			continue;
		}
		filtered_operations.push(op.clone());
	}
	let skipped = operations.len().saturating_sub(filtered_operations.len());
	Some((filtered_operations, skipped, reset_operations))
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
	if shader.contains("refraction") || shader.contains("liltoonref") {
		return Some(UnaAlphaMode::Opaque);
	}
	if shader.contains("liltoongem") {
		return Some(UnaAlphaMode::Blend);
	}
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
	} else if shader.contains("transparent") || shader.contains("fur") {
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
	let source_shader_lower = source_shader.to_ascii_lowercase();
	let mut out = UnaLilToonLikeMaterial {
		source_profile: if source_shader_lower.contains("liltoongem") {
			UnaLilToonLikeSourceProfile::LiltoonGem
		} else if source_shader_lower.contains("liltoonref") || source_shader_lower.contains("liltoonmultirefraction") {
			UnaLilToonLikeSourceProfile::LiltoonRefraction
		} else {
			UnaLilToonLikeSourceProfile::Liltoon
		},
		..Default::default()
	};
	if source_shader_lower.contains("transparent") && !source_shader_lower.contains("twopass") {
		out.blend_state.pre_zwrite_factor = 0.0;
	}
	out.texture_uv_offset_scales = unavatar_material_uv_offset_scales(extras);
	out.texture_uv_mode_factors = unavatar_material_uv_mode_factors(extras);
	out.flip_backface_normal_factor = unavatar_material_float_param(extras, "_FlipNormal").unwrap_or(0.0).clamp(0.0, 1.0);
	out.rendering.render_queue_number = json_i32(extras.get("renderQueue").or_else(|| extras.get("render_queue")));
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_BackfaceColor") {
		out.rendering.backface_color_factor = value;
	}
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
	let main_gradation_strength = unavatar_material_float_param(extras, "_MainGradationStrength").unwrap_or(0.0);
	out.main_color.gradation_strength_factor = main_gradation_strength.clamp(0.0, 1.0);
	out.main_color.gradation_enabled_factor = unavatar_material_float_param(extras, "_UseGradationMap")
		.unwrap_or_else(|| {
			if main_gradation_strength > 0.0
				&& mtoon
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
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("mainColorAdjustMaskTextureIndex")
				.or_else(|| m.get("main_color_adjust_mask_texture_index")),
		)
	}) {
		out.main_color.main_color_adjust_mask_texture_index = Some(value);
	}
	out.main_color.second_enabled_factor = unavatar_material_float_param(extras, "_UseMain2ndTex")
		.unwrap_or(0.0)
		.clamp(0.0, 1.0);
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("main2ndTextureIndex").or_else(|| m.get("main_2nd_texture_index")))) {
		out.main_color.second_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("main2ndBlendMaskTextureIndex")
				.or_else(|| m.get("main_2nd_blend_mask_texture_index")),
		)
	}) {
		out.main_color.second_blend_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("main2ndDissolveMaskTextureIndex")
				.or_else(|| m.get("main_2nd_dissolve_mask_texture_index")),
		)
	}) {
		out.main_color.second_dissolve.mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("main2ndDissolveNoiseMaskTextureIndex")
				.or_else(|| m.get("main_2nd_dissolve_noise_mask_texture_index")),
		)
	}) {
		out.main_color.second_dissolve.noise_mask_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_Color2nd") {
		out.main_color.second_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main2ndTexBlendMode").map(float_to_u32_saturating) {
		out.main_color.second_blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main2ndEnableLighting") {
		out.main_color.second_enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main2ndTexAlphaMode") {
		out.main_color.second_alpha_mode_factor = value.clamp(0.0, 4.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main2ndTex_Cull") {
		out.main_color.second_cull_factor = value.clamp(0.0, 2.0);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main2ndDistanceFade") {
		out.main_color.second_distance_fade_factor = value;
	}
	out.main_color.second_decal_flags_factor = [
		unavatar_material_float_param(extras, "_Main2ndTexIsDecal")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		unavatar_material_float_param(extras, "_Main2ndTexIsLeftOnly")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		unavatar_material_float_param(extras, "_Main2ndTexIsRightOnly")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		unavatar_material_float_param(extras, "_Main2ndTexShouldCopy")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
	];
	out.main_color.second_decal_transform_factor = [
		unavatar_material_float_param(extras, "_Main2ndTexAngle").unwrap_or(0.0),
		unavatar_material_float_param(extras, "_Main2ndTexShouldFlipMirror")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		unavatar_material_float_param(extras, "_Main2ndTexShouldFlipCopy")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		0.0,
	];
	if let Some(value) = unavatar_material_vector_param(extras, "_Main2ndTexDecalAnimation") {
		out.main_color.second_decal_animation_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main2ndTexDecalSubParam") {
		out.main_color.second_decal_sub_param_factor = value;
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_Main2ndDissolveColor") {
		out.main_color.second_dissolve.color_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main2ndDissolveParams") {
		out.main_color.second_dissolve.params_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main2ndDissolvePos") {
		out.main_color.second_dissolve.position_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main2ndDissolveNoiseStrength") {
		out.main_color.second_dissolve.noise_strength_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main2ndDissolveNoiseMask_ScrollRotate") {
		out.main_color.second_dissolve.noise_uv_scroll_rotate_factor = value;
	}
	out.main_color.third_enabled_factor = unavatar_material_float_param(extras, "_UseMain3rdTex")
		.unwrap_or(0.0)
		.clamp(0.0, 1.0);
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("main3rdTextureIndex").or_else(|| m.get("main_3rd_texture_index")))) {
		out.main_color.third_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("main3rdBlendMaskTextureIndex")
				.or_else(|| m.get("main_3rd_blend_mask_texture_index")),
		)
	}) {
		out.main_color.third_blend_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("main3rdDissolveMaskTextureIndex")
				.or_else(|| m.get("main_3rd_dissolve_mask_texture_index")),
		)
	}) {
		out.main_color.third_dissolve.mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("main3rdDissolveNoiseMaskTextureIndex")
				.or_else(|| m.get("main_3rd_dissolve_noise_mask_texture_index")),
		)
	}) {
		out.main_color.third_dissolve.noise_mask_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_Color3rd") {
		out.main_color.third_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main3rdTexBlendMode").map(float_to_u32_saturating) {
		out.main_color.third_blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main3rdEnableLighting") {
		out.main_color.third_enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main3rdTexAlphaMode") {
		out.main_color.third_alpha_mode_factor = value.clamp(0.0, 4.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main3rdTex_Cull") {
		out.main_color.third_cull_factor = value.clamp(0.0, 2.0);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main3rdDistanceFade") {
		out.main_color.third_distance_fade_factor = value;
	}
	out.main_color.third_decal_flags_factor = [
		unavatar_material_float_param(extras, "_Main3rdTexIsDecal")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		unavatar_material_float_param(extras, "_Main3rdTexIsLeftOnly")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		unavatar_material_float_param(extras, "_Main3rdTexIsRightOnly")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		unavatar_material_float_param(extras, "_Main3rdTexShouldCopy")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
	];
	out.main_color.third_decal_transform_factor = [
		unavatar_material_float_param(extras, "_Main3rdTexAngle").unwrap_or(0.0),
		unavatar_material_float_param(extras, "_Main3rdTexShouldFlipMirror")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		unavatar_material_float_param(extras, "_Main3rdTexShouldFlipCopy")
			.unwrap_or(0.0)
			.clamp(0.0, 1.0),
		0.0,
	];
	if let Some(value) = unavatar_material_vector_param(extras, "_Main3rdTexDecalAnimation") {
		out.main_color.third_decal_animation_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main3rdTexDecalSubParam") {
		out.main_color.third_decal_sub_param_factor = value;
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_Main3rdDissolveColor") {
		out.main_color.third_dissolve.color_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main3rdDissolveParams") {
		out.main_color.third_dissolve.params_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main3rdDissolvePos") {
		out.main_color.third_dissolve.position_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Main3rdDissolveNoiseStrength") {
		out.main_color.third_dissolve.noise_strength_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Main3rdDissolveNoiseMask_ScrollRotate") {
		out.main_color.third_dissolve.noise_uv_scroll_rotate_factor = value;
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
	if let Some(value) = unavatar_material_float_param(extras, "_AsUnlit") {
		out.rendering.as_unlit_factor = value.clamp(0.0, 1.0);
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
	if let Some(value) = unavatar_material_vector_param(extras, "_DistanceFade") {
		out.rendering.distance_fade_factor = value;
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_DistanceFadeColor") {
		out.rendering.distance_fade_color_factor = value;
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_DistanceFadeRimColor") {
		out.rendering.distance_fade_rim_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_DistanceFadeRimFresnelPower") {
		out.rendering.distance_fade_rim_fresnel_power_factor = value.max(0.00001);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_DistanceFadeMode") {
		out.rendering.distance_fade_mode_factor = value.clamp(0.0, 1.0);
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
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("normal2ndScaleMaskTextureIndex")
				.or_else(|| m.get("normal_2nd_scale_mask_texture_index"))
				.or_else(|| m.get("normalSecondScaleMaskTextureIndex"))
				.or_else(|| m.get("normal_second_scale_mask_texture_index")),
		)
	}) {
		out.normal.second_scale_mask_texture_index = Some(value);
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
			m.get("shadow2ndColorTextureIndex")
				.or_else(|| m.get("shadow_2nd_color_texture_index")),
		)
	}) {
		out.shadow.second_color_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("shadow3rdColorTextureIndex")
				.or_else(|| m.get("shadow_3rd_color_texture_index")),
		)
	}) {
		out.shadow.third_color_texture_index = Some(value);
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
	if let Some(value) = unavatar_material_float_param(extras, "_ShadowPostAO") {
		out.shadow.post_ao_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_ShadowAOShift") {
		out.shadow.ao_shift_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_ShadowAOShift2") {
		out.shadow.ao_shift2_factor = value;
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
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("matcapBumpTextureIndex").or_else(|| m.get("matcap_bump_texture_index")))) {
		out.matcap.bump_texture_index = Some(value);
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
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapCustomNormal") {
		out.matcap.custom_normal_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCapBumpScale") {
		out.matcap.bump_scale_factor = value;
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
	if let Some(value) = unavatar_material_vector_param(extras, "_MatCapBlendUV1") {
		out.matcap.blend_uv1_factor = [value[0].clamp(0.0, 1.0), value[1].clamp(0.0, 1.0)];
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
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("matcap2ndBumpTextureIndex")
				.or_else(|| m.get("matcap_2nd_bump_texture_index")),
		)
	}) {
		out.matcap.second_bump_texture_index = Some(value);
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
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndCustomNormal") {
		out.matcap.second_custom_normal_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_MatCap2ndBumpScale") {
		out.matcap.second_bump_scale_factor = value;
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
	if let Some(value) = unavatar_material_vector_param(extras, "_MatCap2ndBlendUV1") {
		out.matcap.second_blend_uv1_factor = [value[0].clamp(0.0, 1.0), value[1].clamp(0.0, 1.0)];
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
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_RefractionColor") {
		out.reflection.refraction_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RefractionColorFromMain") {
		out.reflection.refraction_color_from_main_factor = value.clamp(0.0, 1.0);
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
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("rimShadeMaskTextureIndex").or_else(|| m.get("rim_shade_mask_texture_index"))))
	{
		out.rim.shade_mask_texture_index = Some(value);
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
		out.rim.directional_range_factor = value.clamp(-1.0, 1.0);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_RimIndirColor") {
		out.rim.indirect_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_RimIndirRange") {
		out.rim.indirect_range_factor = value.clamp(-1.0, 1.0);
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
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("backlightColorTextureIndex")
				.or_else(|| m.get("backlight_color_texture_index")),
		)
	}) {
		out.backlight.texture_index = Some(value);
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

	out.glitter.enabled_factor = unavatar_material_float_param(extras, "_UseGlitter").unwrap_or(0.0).clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_GlitterColor") {
		out.glitter.color_factor = value;
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("glitterColorTextureIndex").or_else(|| m.get("glitter_color_texture_index"))))
	{
		out.glitter.color_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("glitterShapeTextureIndex").or_else(|| m.get("glitter_shape_texture_index"))))
	{
		out.glitter.shape_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_GlitterParams1") {
		out.glitter.params1_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_GlitterParams2") {
		out.glitter.params2_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_GlitterAtras") {
		out.glitter.atlas_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterMainStrength") {
		out.glitter.main_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterNormalStrength") {
		out.glitter.normal_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterPostContrast") {
		out.glitter.post_contrast_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterSensitivity") {
		out.glitter.sensitivity_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterEnableLighting") {
		out.glitter.enable_lighting_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterShadowMask") {
		out.glitter.shadow_mask_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterApplyTransparency") {
		out.glitter.apply_transparency_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterBackfaceMask") {
		out.glitter.backface_mask_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterScaleRandomize") {
		out.glitter.scale_randomize_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterUVMode") {
		out.glitter.uv_mode_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterColorTex_UVMode") {
		out.glitter.color_texture_uv_mode_factor = value.clamp(0.0, 3.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterApplyShape") {
		out.glitter.apply_shape_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterAngleRandomize") {
		out.glitter.angle_randomize_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_GlitterVRParallaxStrength") {
		out.glitter.vr_parallax_strength_factor = value.clamp(0.0, 1.0);
	}

	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("dissolveMaskTextureIndex").or_else(|| m.get("dissolve_mask_texture_index"))))
	{
		out.dissolve.mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("dissolveNoiseMaskTextureIndex")
				.or_else(|| m.get("dissolve_noise_mask_texture_index")),
		)
	}) {
		out.dissolve.noise_mask_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_DissolveColor") {
		out.dissolve.color_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_DissolveParams") {
		out.dissolve.params_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_DissolvePos") {
		out.dissolve.position_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_DissolveNoiseStrength") {
		out.dissolve.noise_strength_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_DissolveNoiseMask_ScrollRotate") {
		out.dissolve.noise_uv_scroll_rotate_factor = value;
	}

	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("parallaxTextureIndex").or_else(|| m.get("parallax_texture_index")))) {
		out.parallax.texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_UseParallax") {
		out.parallax.enabled_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_UsePOM") {
		out.parallax.pom_enabled_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Parallax") {
		out.parallax.scale_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_ParallaxOffset") {
		out.parallax.offset_factor = value;
	}

	if let Some(value) = unavatar_material_float_param(extras, "_IDMaskCompile") {
		out.id_mask.compile_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_IDMaskFrom") {
		out.id_mask.from_factor = value.clamp(0.0, 8.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_IDMaskIsBitmap") {
		out.id_mask.is_bitmap_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_IDMaskControlsDissolve") {
		out.id_mask.controls_dissolve_factor = value.clamp(0.0, 1.0);
	}
	for index in 0..8 {
		let number = index + 1;
		if let Some(value) = unavatar_material_float_param(extras, &format!("_IDMask{number}")) {
			out.id_mask.flags_factor[index] = value.clamp(0.0, 1.0);
		}
		if let Some(value) = unavatar_material_float_param(extras, &format!("_IDMaskPrior{number}")) {
			out.id_mask.prior_flags_factor[index] = value.clamp(0.0, 1.0);
		}
		if let Some(value) = unavatar_material_float_param(extras, &format!("_IDMaskIndex{number}")) {
			out.id_mask.indices_factor[index] = value.round() as i32;
		}
	}
	if let Some(value) = unavatar_material_float_param(extras, "_UDIMDiscardCompile") {
		out.udim_discard.compile_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_UDIMDiscardMode") {
		out.udim_discard.mode_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_UDIMDiscardUV") {
		out.udim_discard.uv_factor = value.clamp(0.0, 3.0);
	}
	for row in 0..4 {
		for column in 0..4 {
			if let Some(value) = unavatar_material_float_param(extras, &format!("_UDIMDiscardRow{row}_{column}")) {
				match row {
					0 => out.udim_discard.row0_factor[column] = value.clamp(0.0, 1.0),
					1 => out.udim_discard.row1_factor[column] = value.clamp(0.0, 1.0),
					2 => out.udim_discard.row2_factor[column] = value.clamp(0.0, 1.0),
					_ => out.udim_discard.row3_factor[column] = value.clamp(0.0, 1.0),
				}
			}
		}
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
			m.get("emissionBlendMaskTextureIndex")
				.or_else(|| m.get("emission_blend_mask_texture_index")),
		)
	}) {
		out.emission.blend_mask_texture_index = Some(value);
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
	if let Some(value) = unavatar_material_vector_param(extras, "_EmissionBlink") {
		out.emission.blink_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_EmissionFluorescence") {
		out.emission.fluorescence_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_EmissionParallaxDepth") {
		out.emission.parallax_depth_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_EmissionMap_ScrollRotate") {
		out.emission.uv_scroll_rotate_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_EmissionBlendMask_ScrollRotate") {
		out.emission.blend_mask_uv_scroll_rotate_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_EmissionUseGrad") {
		out.emission.gradation_enabled_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_EmissionGradSpeed") {
		out.emission.gradation_speed_factor = value;
	}
	out.emission.second_enabled_factor = unavatar_material_float_param(extras, "_UseEmission2nd")
		.unwrap_or_else(|| {
			if mtoon
				.and_then(|m| json_usize(m.get("emission2ndTextureIndex").or_else(|| m.get("emission_2nd_texture_index"))))
				.is_some()
			{
				1.0
			} else {
				0.0
			}
		})
		.clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_Emission2ndColor") {
		out.emission.second_color_factor = value;
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("emission2ndTextureIndex").or_else(|| m.get("emission_2nd_texture_index")))) {
		out.emission.second_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("emission2ndBlendMaskTextureIndex")
				.or_else(|| m.get("emission_2nd_blend_mask_texture_index")),
		)
	}) {
		out.emission.second_blend_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("emission2ndGradationTextureIndex")
				.or_else(|| m.get("emission_2nd_gradation_texture_index")),
		)
	}) {
		out.emission.second_gradation_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Emission2ndBlend") {
		out.emission.second_blend_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Emission2ndBlendMode").map(float_to_u32_saturating) {
		out.emission.second_blend_mode = liltoon_like_blend_mode(value);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Emission2ndBlink") {
		out.emission.second_blink_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Emission2ndFluorescence") {
		out.emission.second_fluorescence_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Emission2ndParallaxDepth") {
		out.emission.second_parallax_depth_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Emission2ndMap_ScrollRotate") {
		out.emission.second_uv_scroll_rotate_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_Emission2ndBlendMask_ScrollRotate") {
		out.emission.second_blend_mask_uv_scroll_rotate_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Emission2ndMainStrength") {
		out.emission.second_main_strength_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Emission2ndUseGrad") {
		out.emission.second_gradation_enabled_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_Emission2ndGradSpeed") {
		out.emission.second_gradation_speed_factor = value;
	}

	out.audio_link.enabled_factor = unavatar_material_float_param(extras, "_UseAudioLink")
		.unwrap_or(0.0)
		.clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_vector_param(extras, "_AudioLinkDefaultValue") {
		out.audio_link.default_value_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLinkUVMode") {
		out.audio_link.uv_mode_factor = value.clamp(0.0, 5.0);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_AudioLinkUVParams") {
		out.audio_link.uv_params_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_AudioLinkStart") {
		out.audio_link.start_factor = value;
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("audioLinkMaskTextureIndex")
				.or_else(|| m.get("audio_link_mask_texture_index")),
		)
	}) {
		out.audio_link.mask_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_AudioLinkMask_ScrollRotate") {
		out.audio_link.mask_uv_scroll_rotate_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLinkMask_UVMode") {
		out.audio_link.mask_uv_mode_factor = value.clamp(0.0, 3.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLink2Main2nd") {
		out.audio_link.to_main_second_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLink2Main3rd") {
		out.audio_link.to_main_third_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLink2Emission") {
		out.audio_link.to_emission_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLink2EmissionGrad") {
		out.audio_link.to_emission_gradation_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLink2Emission2nd") {
		out.audio_link.to_emission_second_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLink2Emission2ndGrad") {
		out.audio_link.to_emission_second_gradation_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLink2Vertex") {
		out.audio_link.to_vertex_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLinkVertexUVMode") {
		out.audio_link.vertex_uv_mode_factor = value.clamp(0.0, 3.0);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_AudioLinkVertexUVParams") {
		out.audio_link.vertex_uv_params_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_AudioLinkVertexStart") {
		out.audio_link.vertex_start_factor = value;
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_AudioLinkVertexStrength") {
		out.audio_link.vertex_strength_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_AudioLinkAsLocal") {
		out.audio_link.as_local_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("audioLinkLocalMapTextureIndex")
				.or_else(|| m.get("audio_link_local_map_texture_index")),
		)
	}) {
		out.audio_link.local_map_texture_index = Some(value);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_AudioLinkLocalMapParams") {
		out.audio_link.local_map_params_factor = value;
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
	let source_shader_lower = source_shader.to_ascii_lowercase();
	out.fur.enabled_factor = unavatar_material_float_param(extras, "_UseFur")
		.unwrap_or_else(|| if source_shader_lower.contains("fur") { 1.0 } else { 0.0 })
		.clamp(0.0, 1.0);
	if let Some(value) = unavatar_material_float_param(extras, "_FurLayerNum") {
		out.fur.layer_count_factor = value.clamp(1.0, 3.0);
	}
	if let Some(value) = unavatar_material_vector_param(extras, "_FurVector") {
		out.fur.vector_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_VertexColor2FurVector") {
		out.fur.vertex_color_to_vector_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurVectorScale") {
		out.fur.vector_scale_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurGravity") {
		out.fur.gravity_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurAO") {
		out.fur.shell_ao_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurRootOffset") {
		out.fur.root_offset_factor = value.clamp(-1.0, 0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurCutoutLength") {
		out.fur.cutout_length_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurRandomize") {
		out.fur.randomize_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurNoiseTiling") {
		out.fur.noise_tiling_factor = value.max(0.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurNoiseOffset") {
		out.fur.noise_offset_factor = value;
	}
	if let Some(value) = unavatar_material_color_param_rgba(extras, "_FurRimColor") {
		out.fur.rim_color_factor = value;
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurRimFresnelPower") {
		out.fur.rim_fresnel_power_factor = value.clamp(0.01, 50.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_FurRimAntiLight") {
		out.fur.rim_anti_light_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("furVectorTextureIndex").or_else(|| m.get("fur_vector_texture_index")))) {
		out.fur.vector_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| {
		json_usize(
			m.get("furLengthMaskTextureIndex")
				.or_else(|| m.get("fur_length_mask_texture_index")),
		)
	}) {
		out.fur.length_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("furNoiseMaskTextureIndex").or_else(|| m.get("fur_noise_mask_texture_index"))))
	{
		out.fur.noise_mask_texture_index = Some(value);
	}
	if let Some(value) = mtoon.and_then(|m| json_usize(m.get("furMaskTextureIndex").or_else(|| m.get("fur_mask_texture_index")))) {
		out.fur.mask_texture_index = Some(value);
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
	if let Some(value) = unavatar_material_float_param(extras, "_PreCutoff") {
		out.blend_state.pre_cutoff_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_PreZWrite") {
		out.blend_state.pre_zwrite_factor = value.clamp(0.0, 1.0);
	}
	if let Some(value) = unavatar_material_float_param(extras, "_PreCull") {
		out.blend_state.pre_cull_factor = value.clamp(0.0, 2.0);
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
		|| unavatar_material_float_param(extras, "_AlphaMaskMode").is_some_and(|value| value.round() != 0.0)
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

fn unavatar_material_vector_param(extras: &Value, name: &str) -> Option<[f32; 4]> {
	extras
		.get("vectorParams")
		.or_else(|| extras.get("vector_params"))
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
	value.and_then(|value| {
		value
			.as_f64()
			.or_else(|| value.as_str().and_then(|value| value.trim().parse::<f64>().ok()))
			.map(|v| v as f32)
	})
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
		.map(|names| {
			let mut target_names = Vec::with_capacity(names.len());
			target_names.extend(names.iter().filter_map(|name| name.as_str().map(str::to_owned)));
			target_names
		})
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
	let tex_coords_1 = reader.read_tex_coords(1).map(|tc| tc.into_f32().collect());
	let tex_coords_2 = reader.read_tex_coords(2).map(|tc| tc.into_f32().collect());
	let tex_coords_3 = reader.read_tex_coords(3).map(|tc| tc.into_f32().collect());
	let colors_0 = reader.read_colors(0).map(|colors| colors.into_rgba_f32().collect());
	let indices = reader.read_indices().map(|idx| idx.into_u32().collect());
	let material_index = prim.material().index();
	let (joints, weights) = joints_weights;

	let morph_target_iter = reader.read_morph_targets();
	let (morph_target_lower, morph_target_upper) = morph_target_iter.size_hint();
	let mut morph_targets: Vec<UnaMorphTargetDeltas> = Vec::with_capacity(morph_target_upper.unwrap_or(morph_target_lower));
	for (pos_d, norm_d, _tan_d) in morph_target_iter {
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
		tex_coords_1,
		tex_coords_2,
		tex_coords_3,
		colors_0,
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

	let mut meshes: Vec<Vec<UnaMeshBuffers>> = document.meshes().map(|mesh| Vec::with_capacity(mesh.primitives().len())).collect();
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

	let mut nodes = Vec::with_capacity(document.nodes().len());
	for node in document.nodes() {
		let children: Vec<usize> = node.children().map(|c| c.index()).collect();
		nodes.push(UnaSceneNode {
			name: node.name().map(|s| s.to_string()),
			source_node_id: unavatar_node_id(&node),
			resolved_node_id: None,
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
				let extension = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
				if matches!(extension.as_deref(), Some("unavatar" | "glb")) {
					let bytes = std::fs::read(&path).map_err(|e| ImportError::Message(format!("{}: {e}", path.display())))?;
					if bytes.starts_with(b"glTF") {
						let (root, bin) = read_glb_json_and_bin(&bytes)?;
						original_image_sources = Some(collect_glb_image_source_metadata(&root, &bin));
						original_glb_bin = Some(bin);
						root_json = Some(root);
					} else if extension.as_deref() == Some("unavatar") {
						root_json = Some(gltf_root_json_from_bytes(&bytes)?);
					}
					let import_bytes = normalize_webp_glb_for_gltf_import(&bytes)?;
					let imported = gltf::import_slice(import_bytes.as_ref()).map_err(|e| ImportError::Message(e.to_string()))?;
					(Some(path), imported.0, imported.1, imported.2)
				} else if path
					.extension()
					.and_then(|e| e.to_str())
					.is_some_and(|e| e.eq_ignore_ascii_case("gltf"))
				{
					let bytes = std::fs::read(&path).map_err(|e| ImportError::Message(format!("{}: {e}", path.display())))?;
					root_json = Some(gltf_root_json_from_bytes(&bytes)?);
					let imported = gltf::import(&path).map_err(|e| ImportError::Message(e.to_string()))?;
					(Some(path), imported.0, imported.1, imported.2)
				} else {
					let imported = gltf::import(&path).map_err(|e| ImportError::Message(e.to_string()))?;
					(Some(path), imported.0, imported.1, imported.2)
				}
			}
			ImportInput::Bytes { bytes, path_hint } => {
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
		let humanoid_profile = unavatar
			.as_ref()
			.and_then(|unavatar| unavatar_humanoid_profile(&scene, unavatar, &mut report));
		if let Some(unavatar) = &unavatar {
			report_unavatar_path_diagnostics(&scene, unavatar, &mut report);
			apply_unavatar_modular_avatar(&mut scene, unavatar, &mut report);
			if let Some(humanoid_profile) = &humanoid_profile {
				let (same_name_mappings, same_name_retargeted, same_name_auxiliary_reparented) =
					retarget_same_name_humanoid_armature_skins(&mut scene, humanoid_profile);
				if same_name_mappings > 0 || same_name_retargeted > 0 || same_name_auxiliary_reparented > 0 {
					report.push_info(format!(
						".unavatar humanoid armature fallback: same_name_mappings={}, skin_joints={}, auxiliary_bones={}",
						same_name_mappings, same_name_retargeted, same_name_auxiliary_reparented
					));
				}
			}
			apply_unavatar_initial_variant_state(&mut scene, unavatar, &mut report);
			apply_unavatar_base_wardrobe(&mut scene, unavatar, &mut report);
		}
		let expression_catalog = if unavatar.is_some() {
			expression_catalog_from_morph_target_names(&scene)
		} else {
			None
		};
		let spring_bones = unavatar
			.as_ref()
			.and_then(|unavatar| unavatar_dynamics_settings(&mut scene, unavatar, &mut report));
		let runtime_actions = unavatar
			.as_ref()
			.and_then(|unavatar| unavatar_runtime_action_set(unavatar, Some(&scene)));
		if let Some(catalog) = &expression_catalog {
			report.push_info(format!(".unavatar expressions: morph_target_presets={}", catalog.presets.len()));
		}
		if let Some(actions) = &runtime_actions {
			report.push_info(format!(".unavatar runtime actions: {}", actions.actions.len()));
		}

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
				expression_weights: expression_catalog.as_ref().map(|_| UnaExpressionWeights::default()),
				expression_catalog,
				runtime_actions,
				spring_bones,
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

	fn test_scene_node(id: &str, children: Vec<usize>) -> UnaSceneNode {
		UnaSceneNode {
			name: Some(id.to_string()),
			source_node_id: Some(id.to_string()),
			resolved_node_id: None,
			visible: true,
			transform: Mat4::IDENTITY.to_cols_array(),
			children,
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	#[test]
	fn unavatar_dynamics_lowers_vrc_physbone_to_runtime_group() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![test_scene_node("node_root", vec![1]), test_scene_node("node_tip", Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"},
					{"nodeId": "node_tip", "path": "Root/Tip"}
				],
				"dynamics": [{
					"id": "hair_front",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"stiffness": 0.35,
					"drag": 0.2,
					"gravity": [0.0, -0.4, 0.0],
					"radius": 0.03,
					"sourceParams": {
						"allowCollision": true,
						"allowGrabbing": true,
						"allowPosing": false,
						"limitType": "Angle",
						"maxAngleX": 45.0,
						"maxAngleZ": 30.0,
						"maxStretch": 0.2,
						"colliders": [{
							"root": {"nodeId": "node_root", "path": "Root"},
							"shapeType": "Sphere",
							"radius": 0.08,
							"height": 0.3,
							"position": [0.1, 0.2, 0.3],
							"rotation": [0.0, 0.5, 0.0, 0.8660254],
							"insideBounds": false
						}, {
							"root": {"nodeId": "node_root", "path": "Root"},
							"shapeType": "Sphere",
							"radius": 0.2,
							"insideBounds": true
						}]
					}
				}, {
					"id": "no_collision_tail",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"allowCollision": false,
					"sourceParams": {
						"colliders": [{
							"root": {"nodeId": "node_root", "path": "Root"},
							"shapeType": "Sphere",
							"radius": 0.4
						}]
					}
				}, {
					"id": "disabled_tail",
					"source": "vrc_physbone",
					"enabled": false,
					"roots": [{"nodeId": "node_root", "path": "Root"}]
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(settings.groups.len(), 2);
		assert_eq!(settings.groups[0].source_kind, UnaDynamicsSourceKind::VrcPhysBone);
		assert!(!settings.groups[0].enabled);
		assert_eq!(settings.groups[0].source_id, "hair_front");
		assert_eq!(settings.groups[0].comment, "hair_front");
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 1]);
		assert_eq!(settings.groups[0].hit_radius, 0.03);
		assert!((settings.groups[0].gravity_power - 0.4).abs() < 1e-6);
		let limit = settings.groups[0].limit.as_ref().expect("limit");
		assert_eq!(limit.limit_type, "Angle");
		assert_eq!(limit.max_angle_x, 45.0);
		assert_eq!(limit.max_angle_z, 30.0);
		assert_eq!(limit.max_stretch, 0.2);
		let interaction = settings.groups[0].interaction.as_ref().expect("interaction");
		assert_eq!(interaction.allow_grabbing, Some(true));
		assert_eq!(interaction.allow_posing, Some(false));
		assert_eq!(settings.colliders.len(), 2);
		assert_eq!(settings.colliders[0].source_kind, UnaDynamicsSourceKind::VrcPhysBone);
		assert_eq!(settings.colliders[0].node, 0);
		assert_eq!(settings.colliders[0].shape, UnaDynamicsColliderShape::Sphere);
		assert_eq!(settings.colliders[0].radius, 0.08);
		assert_eq!(settings.colliders[0].position, [-0.1, 0.2, 0.3]);
		assert_eq!(settings.colliders[0].rotation, [0.0, -0.5, -0.0, 0.8660254]);
		assert!(!settings.colliders[0].inside_bounds);
		assert_eq!(settings.colliders[1].radius, 0.2);
		assert!(settings.colliders[1].inside_bounds);
	}

	#[test]
	fn unavatar_dynamics_lowers_branching_root_to_multiple_runtime_groups() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("node_root", vec![1, 3]),
				test_scene_node("node_left_mid", vec![2]),
				test_scene_node("node_left_tip", Vec::new()),
				test_scene_node("node_right_tip", Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"},
					{"nodeId": "node_left_mid", "path": "Root/LeftMid"},
					{"nodeId": "node_left_tip", "path": "Root/LeftMid/LeftTip"},
					{"nodeId": "node_right_tip", "path": "Root/RightTip"}
				],
				"dynamics": [{
					"id": "branched_hair",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}]
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");
		let mut chains: Vec<Vec<usize>> = settings.groups.iter().map(|group| group.bone_node_indices.clone()).collect();
		chains.sort();

		assert_eq!(chains, vec![vec![0, 1, 2], vec![0, 3]]);
		assert!(settings
			.groups
			.iter()
			.all(|group| group.source_kind == UnaDynamicsSourceKind::VrcPhysBone));
	}

	#[test]
	fn unavatar_dynamics_respects_ignored_transforms_and_multi_child_ignore() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("node_root", vec![1, 3, 4]),
				test_scene_node("node_ignored", vec![2]),
				test_scene_node("node_ignored_tip", Vec::new()),
				test_scene_node("node_kept_tip", Vec::new()),
				test_scene_node("node_extra_tip", Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"},
					{"nodeId": "node_ignored", "path": "Root/Ignored"},
					{"nodeId": "node_ignored_tip", "path": "Root/Ignored/Tip"},
					{"nodeId": "node_kept_tip", "path": "Root/KeptTip"},
					{"nodeId": "node_extra_tip", "path": "Root/ExtraTip"}
				],
				"dynamics": [{
					"id": "ignored_branch",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"ignoreTransforms": [{"nodeId": "node_ignored", "path": "Root/Ignored"}],
					"multiChildType": "Ignore"
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(settings.groups.len(), 1);
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 3]);
		assert!(report.messages.iter().any(|message| message.contains("ignored_transforms=1")));
		assert!(report.messages.iter().any(|message| message.contains("multi_child_ignore=1")));
	}

	#[test]
	fn unavatar_dynamics_synthesizes_endpoint_child_for_leaf_root() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![test_scene_node("node_root", Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"}
				],
				"dynamics": [{
					"id": "leaf_tail",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"sourceParams": {
						"endpointPosition": [0.1, 0.2, 0.3]
					}
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(scene.nodes.len(), 2);
		assert_eq!(scene.nodes[0].children, vec![1]);
		let (_, _, endpoint_translation) = Mat4::from_cols_array(&scene.nodes[1].transform).to_scale_rotation_translation();
		assert_eq!(endpoint_translation.to_array(), [-0.1, 0.2, 0.3]);
		assert_eq!(settings.groups.len(), 1);
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 1]);
		assert!(report
			.messages
			.iter()
			.any(|message| message.contains("synthesized_endpoint_children=1")));
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
	fn import_marks_enabled_unsupported_modular_avatar_component_partial_success() {
		let bin = triangle_bin_bytes();
		let json = format!(
			r#"{{
				"asset": {{"version": "2.0"}},
				"scene": 0,
				"scenes": [{{"nodes": [0]}}],
				"meshes": [{{
					"primitives": [{{
						"attributes": {{"POSITION": 0}},
						"indices": 1
					}}]
				}}],
				"accessors": [
					{{"bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "min": [-1, 0, 0], "max": [1, 1, 0]}},
					{{"bufferView": 1, "componentType": 5125, "count": 3, "type": "SCALAR"}}
				],
				"bufferViews": [
					{{"buffer": 0, "byteOffset": 0, "byteLength": 36}},
					{{"buffer": 0, "byteOffset": 36, "byteLength": 12}}
				],
				"buffers": [{{"byteLength": {bin_len}}}],
				"nodes": [{{"name": "Root", "mesh": 0}}],
				"extensionsUsed": ["UN_avatar"],
				"extensions": {{
					"UN_avatar": {{
						"specVersion": "0.1-preview",
						"modularAvatar": {{
							"schemaVersion": "0.1-preview",
							"components": [{{
								"shortType": "ModularAvatarMeshCutter",
								"enabled": true
							}}]
						}}
					}}
				}}
			}}"#,
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
					path_hint: Some(std::path::PathBuf::from("unsupported-ma.glb")),
				},
				ImportOptions,
			)
			.unwrap();

		assert_eq!(got.report.status, ReportStatus::PartialSuccess);
		assert_eq!(got.report.lost_features.len(), 1);
		assert_eq!(got.report.lost_features[0].feature, "ModularAvatar.ModularAvatarMeshCutter");
		assert!(got.report.diagnostics.iter().any(|diagnostic| {
			diagnostic.severity == un_avatar_core::ReportSeverity::Warning && diagnostic.text.contains("ModularAvatarMeshCutter")
		}));
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
	fn decodes_unavatar_hdr_texture_asset() {
		let mut hdr = Vec::new();
		image::codecs::hdr::HdrEncoder::new(Cursor::new(&mut hdr))
			.encode(&[image::Rgb([0.25, 0.5, 1.0])], 1, 1)
			.unwrap();
		let decoded = decode_unavatar_texture_asset(&hdr, "image/vnd.radiance", Some("RGBE8"), Some("rgb")).unwrap();
		assert_eq!(decoded.width, 1);
		assert_eq!(decoded.height, 1);
		assert_eq!(decoded.pixel_format, UnaImagePixelFormat::R32G32B32Float);
		assert_eq!(decoded.pixels.len(), 12);
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
	fn imports_unavatar_extension_from_gltf_path() {
		let dir = std::env::temp_dir().join(format!("un-avatar-gltf-extension-{}", std::process::id()));
		let _ = std::fs::remove_dir_all(&dir);
		std::fs::create_dir_all(&dir).unwrap();
		let gltf_path = dir.join("triangle.gltf");
		let mut root: Value = serde_json::from_str(include_str!("../tests/fixtures/triangle.gltf")).unwrap();
		root["extensionsUsed"] = serde_json::json!(["UN_avatar"]);
		root["extensions"] = serde_json::json!({
			"UN_avatar": {
				"specVersion": "0.1-preview",
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [{
						"shortType": "ModularAvatarMeshCutter",
						"enabled": true
					}]
				}
			}
		});
		std::fs::File::create(&gltf_path)
			.unwrap()
			.write_all(serde_json::to_string(&root).unwrap().as_bytes())
			.unwrap();
		std::fs::write(dir.join("triangle.bin"), triangle_bin_bytes()).unwrap();

		let imp = GltfImporter;
		let mut ctx = ImportContext::dummy();
		let got = imp.import(&mut ctx, ImportInput::Path(gltf_path), ImportOptions).unwrap();

		assert!(got.document.unavatar.is_some());
		assert_eq!(got.report.status, ReportStatus::PartialSuccess);
		assert_eq!(got.report.lost_features[0].feature, "ModularAvatar.ModularAvatarMeshCutter");
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
	fn imports_single_bone_skin_unavatar_through_common_scene_skinning() {
		let mut root: Value = serde_json::from_str(include_str!("../tests/fixtures/skin_one_bone.gltf")).unwrap();
		if let Some(buffer) = root
			.get_mut("buffers")
			.and_then(Value::as_array_mut)
			.and_then(|buffers| buffers.get_mut(0))
			.and_then(Value::as_object_mut)
		{
			buffer.remove("uri");
		}
		root["extensionsUsed"] = serde_json::json!(["UN_avatar"]);
		root["extensions"] = serde_json::json!({
			"UN_avatar": {
				"specVersion": "0.1-preview",
				"generator": "test"
			}
		});
		let bin = skin_one_bone_bin_bytes();
		let views = root
			.get("bufferViews")
			.and_then(Value::as_array)
			.unwrap()
			.iter()
			.map(|view| {
				let byte_offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
				let byte_length = view.get("byteLength").and_then(Value::as_u64).unwrap() as usize;
				GltfBufferViewBytes {
					bytes: bin[byte_offset..byte_offset + byte_length].to_vec(),
					target: view.get("target").cloned(),
				}
			})
			.collect::<Vec<_>>();
		let bytes = rebuild_glb(&mut root, &views).unwrap();

		let imp = GltfImporter;
		let mut ctx = ImportContext::dummy();
		let got = imp
			.import(
				&mut ctx,
				ImportInput::Bytes {
					bytes: bytes.into(),
					path_hint: Some(std::path::PathBuf::from("skin.unavatar")),
				},
				ImportOptions,
			)
			.unwrap();
		let sc = got.document.scene.as_ref().unwrap();
		assert!(got.document.unavatar.is_some());
		assert_eq!(sc.skins.len(), 1);
		assert_eq!(sc.nodes[0].skin, Some(0));
		assert_eq!(sc.skins[0].joint_nodes, vec![1]);
		assert_eq!(sc.skins[0].inverse_bind_matrices.len(), 1);
		let prim = &sc.meshes[0][0];
		assert_eq!(prim.joints.as_ref().unwrap().len(), 3);
		assert_eq!(prim.weights.as_ref().unwrap().len(), 3);
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
					}, {
						"id": "ma-object-toggle-0",
						"name": "Hidden Toggle",
						"source": "modular-avatar-object-toggle",
						"operations": [{
							"op": "metadata",
							"path": "Menu/Hidden Toggle",
							"controlType": "Toggle",
							"parameter": "HiddenToggle",
							"value": "1"
						}, {
							"op": "nodeEnabled",
							"target": {"nodeId": "node_hidden", "path": "Wrong Path"},
							"visible": true
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
									"target": {"path": "UnityInactive"},
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
						}, {
							"id": "child_hidden",
							"operations": [
								{
									"type": "subtreeEnabled",
									"target": {"nodeId": "node_hidden", "path": "Wrong Path"},
									"visible": true
								},
								{
									"type": "subtreeEnabled",
									"target": {"nodeId": "node_hidden_child", "path": "Wrong Path"},
									"visible": false
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
		let expressions = got.document.expression_catalog.as_ref().expect("expression catalog");
		assert_eq!(expressions.presets.len(), 1);
		assert_eq!(expressions.presets[0].name, "Shrink");
		assert_eq!(expressions.presets[0].binds.len(), 1);
		assert!(got.document.expression_weights.is_some());
		let runtime_actions = got.document.runtime_actions.as_ref().expect("runtime actions");
		assert_eq!(runtime_actions.actions.len(), 3);
		assert_eq!(runtime_actions.actions[0].id, "wardrobe:visible");
		assert_eq!(
			runtime_actions.actions[0].effects,
			vec![UnaRuntimeActionEffect::WardrobeSet {
				set_id: "visible".to_string()
			}]
		);
		assert_eq!(runtime_actions.actions[2].id, "variant:ma-object-toggle-0");
		assert_eq!(
			runtime_actions.actions[2].triggers,
			vec![
				UnaRuntimeActionTrigger::SupervisorCommand {
					command: "ma-object-toggle-0".to_string()
				},
				UnaRuntimeActionTrigger::ExpressionMenu {
					path: "Menu/Hidden Toggle".to_string()
				}
			]
		);
		assert_eq!(
			runtime_actions.actions[2].effects,
			vec![UnaRuntimeActionEffect::NodeVisibility {
				target: UnaRuntimeNodeTarget {
					node_index: None,
					source_node_id: Some("node_hidden".to_string()),
					resolved_node_id: None,
					path: Some("Wrong Path".to_string()),
				},
				visible: true,
			}]
		);
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
		let applied = apply_unavatar_wardrobe_set(&mut got.document, "base").unwrap();
		assert_eq!(applied.visibility_applied, 3);
		assert_eq!(applied.visibility_missing, 0);
		assert_eq!(applied.blendshape_applied, 1);
		assert_eq!(applied.blendshape_missing, 0);
		let scene = got.document.scene.as_ref().unwrap();
		assert!(!scene.nodes[1].visible);
		assert!(!scene.nodes[3].visible);
		assert_eq!(scene.meshes[0][0].default_morph_weights, vec![0.5]);
		let applied = apply_unavatar_wardrobe_set(&mut got.document, "child_hidden").unwrap();
		assert_eq!(applied.visibility_applied, 2);
		assert_eq!(applied.visibility_missing, 0);
		assert_eq!(applied.blendshape_applied, 0);
		assert_eq!(applied.blendshape_missing, 0);
		let scene = got.document.scene.as_ref().unwrap();
		assert!(scene.nodes[1].visible);
		assert!(!scene.nodes[3].visible);
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
			.any(|m| m.contains(".unavatar unity active state: visibility_applied=0")));
		assert!(got.report.messages.iter().any(|m| m.contains("inherited_hidden_skipped=2")));
		assert!(got
			.report
			.messages
			.iter()
			.any(|m| m.contains(".unavatar humanoid: resolved_bones=1")));
		let _ = std::fs::remove_dir_all(&dir);
	}

	#[test]
	fn unavatar_variant_runtime_action_imports_non_visibility_effects() {
		let variant = serde_json::json!({
			"id": "fx-toggle",
			"name": "FX Toggle",
			"operations": [{
				"op": "metadata",
				"menuPath": "FX/Toggle"
			}, {
				"op": "materialColor",
				"target": {"materialName": "Accent"},
				"parameter": "_EmissionColor",
				"color": [1.0, 0.5, 0.25, 1.0]
			}, {
				"op": "materialScalar",
				"materialIndex": 2,
				"parameter": "_Smoothness",
				"value": 0.75
			}, {
				"op": "materialSlot",
				"target": {
					"nodeId": "node_renderer",
					"path": "Root/Renderer",
					"primitiveIndex": 1
				},
				"material": {"name": "Alt"}
			}, {
				"op": "expressionWeight",
				"name": "Blink",
				"weight": 0.5
			}, {
				"op": "dynamicsEnable",
				"target": {"dynamicsId": "physbone:hair"},
				"enabled": false
			}]
		});

		let action = unavatar_variant_runtime_action(&variant).expect("runtime action");

		assert_eq!(action.id, "variant:fx-toggle");
		assert_eq!(
			action.triggers,
			vec![
				UnaRuntimeActionTrigger::SupervisorCommand {
					command: "fx-toggle".to_string()
				},
				UnaRuntimeActionTrigger::ExpressionMenu {
					path: "FX/Toggle".to_string()
				}
			]
		);
		assert_eq!(
			action.effects,
			vec![
				UnaRuntimeActionEffect::MaterialColor {
					target: UnaRuntimeMaterialTarget {
						material_index: None,
						name: Some("Accent".to_string()),
					},
					parameter: "_EmissionColor".to_string(),
					color: [1.0, 0.5, 0.25, 1.0],
				},
				UnaRuntimeActionEffect::MaterialScalar {
					target: UnaRuntimeMaterialTarget {
						material_index: Some(2),
						name: None,
					},
					parameter: "_Smoothness".to_string(),
					value: 0.75,
				},
				UnaRuntimeActionEffect::MaterialSlot {
					target: UnaRuntimeMaterialSlotTarget {
						node: UnaRuntimeNodeTarget {
							node_index: None,
							source_node_id: Some("node_renderer".to_string()),
							resolved_node_id: None,
							path: Some("Root/Renderer".to_string()),
						},
						primitive_index: Some(1),
					},
					material: Some(UnaRuntimeMaterialTarget {
						material_index: None,
						name: Some("Alt".to_string()),
					}),
				},
				UnaRuntimeActionEffect::ExpressionWeight {
					name: "Blink".to_string(),
					weight: 0.5,
				},
				UnaRuntimeActionEffect::DynamicsEnabled {
					source_id: "physbone:hair".to_string(),
					enabled: false,
				},
			]
		);
	}

	#[test]
	fn unavatar_runtime_actions_import_modular_avatar_material_setter() {
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarMaterialSetter",
						"enabled": true,
						"id": "mat-setter",
						"name": "Jacket Color",
						"menuPath": "Clothes/Jacket Color",
						"fields": {
							"menuItem": {
								"control": {
									"parameter": {"name": "JacketColor"},
									"value": "1"
								}
							},
							"objects": [{
								"object": {
									"nodeId": "node_jacket",
									"path": "Root/Jacket"
								},
								"MaterialIndex": 1,
								"Material": {
									"materialName": "Jacket Red"
								}
							}]
						}
					}]
				}
			}),
		};

		let actions = unavatar_runtime_action_set(&unavatar, None).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].id, "ma:material_setter:mat-setter");
		assert_eq!(actions.actions[0].label, "Jacket Color");
		assert_eq!(
			actions.actions[0].triggers,
			vec![
				UnaRuntimeActionTrigger::SupervisorCommand {
					command: "ma:material_setter:mat-setter".to_string()
				},
				UnaRuntimeActionTrigger::ExpressionMenu {
					path: "Clothes/Jacket Color".to_string()
				},
				UnaRuntimeActionTrigger::ParameterValue {
					name: "JacketColor".to_string(),
					value: 1.0
				}
			]
		);
		assert_eq!(
			actions.actions[0].effects,
			vec![UnaRuntimeActionEffect::MaterialSlot {
				target: UnaRuntimeMaterialSlotTarget {
					node: UnaRuntimeNodeTarget {
						node_index: None,
						source_node_id: Some("node_jacket".to_string()),
						resolved_node_id: None,
						path: Some("Root/Jacket".to_string()),
					},
					primitive_index: Some(1),
				},
				material: Some(UnaRuntimeMaterialTarget {
					material_index: None,
					name: Some("Jacket Red".to_string()),
				}),
			}]
		);
	}

	#[test]
	fn unavatar_runtime_actions_resolve_modular_avatar_material_setter_targets_from_scene() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Jacket".to_string()),
					source_node_id: Some("node_jacket".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarMaterialSetter",
						"enabled": true,
						"id": "mat-setter",
						"fields": {
							"objects": [{
								"object": {
									"nodeId": "node_jacket",
									"path": "Outdated/Jacket"
								},
								"MaterialIndex": 0,
								"Material": {
									"materialName": "Jacket Red"
								}
							}]
						}
					}]
				}
			}),
		};

		let actions = unavatar_runtime_action_set(&unavatar, Some(&scene)).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(
			actions.actions[0].effects,
			vec![UnaRuntimeActionEffect::MaterialSlot {
				target: UnaRuntimeMaterialSlotTarget {
					node: UnaRuntimeNodeTarget {
						node_index: None,
						source_node_id: Some("node_jacket".to_string()),
						resolved_node_id: None,
						path: Some("Root/Jacket".to_string()),
					},
					primitive_index: Some(0),
				},
				material: Some(UnaRuntimeMaterialTarget {
					material_index: None,
					name: Some("Jacket Red".to_string()),
				}),
			}]
		);
	}

	#[test]
	fn unavatar_runtime_actions_use_modular_avatar_menu_item_label_fallback() {
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarMaterialSetter",
						"enabled": true,
						"id": "mat-setter",
						"fields": {
							"menuItem": {
								"name": "Jacket Red"
							},
							"objects": [{
								"object": {
									"nodeId": "node_jacket",
									"path": "Root/Jacket"
								},
								"MaterialIndex": 0,
								"Material": {
									"materialName": "Jacket Red"
								}
							}]
						}
					}]
				}
			}),
		};

		let actions = unavatar_runtime_action_set(&unavatar, None).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].label, "Jacket Red");
	}

	#[test]
	fn unavatar_runtime_actions_skip_modular_avatar_material_setter_when_target_is_missing() {
		let scene = UnaSceneSnapshot {
			nodes: vec![UnaSceneNode {
				name: Some("Root".to_string()),
				..test_node(Vec::new())
			}],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarMaterialSetter",
						"enabled": true,
						"id": "mat-setter",
						"fields": {
							"objects": [{
								"object": {
									"nodeId": "node_missing",
									"path": "Missing/Jacket"
								},
								"MaterialIndex": 0,
								"Material": {
									"materialName": "Jacket Red"
								}
							}]
						}
					}]
				}
			}),
		};

		assert!(unavatar_runtime_action_set(&unavatar, Some(&scene)).is_none());
	}

	#[test]
	fn unavatar_runtime_actions_import_modular_avatar_material_swap_from_scene_slots() {
		let primitive_base = UnaMeshBuffers {
			name: None,
			positions: vec![[0.0; 3]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: Some(0),
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let primitive_other = UnaMeshBuffers {
			material_index: Some(2),
			..primitive_base.clone()
		};
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![primitive_base.clone(), primitive_other], vec![primitive_base]],
			materials: vec![
				UnaMaterialPbr {
					name: Some("Base Blue".to_string()),
					..Default::default()
				},
				UnaMaterialPbr {
					name: Some("Base Red".to_string()),
					..Default::default()
				},
				UnaMaterialPbr {
					name: Some("Unchanged".to_string()),
					..Default::default()
				},
			],
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("OutfitRoot".to_string()),
					source_node_id: Some("node_outfit_root".to_string()),
					resolved_node_id: None,
					children: vec![2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Jacket".to_string()),
					source_node_id: Some("node_jacket".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Outside".to_string()),
					source_node_id: Some("node_outside".to_string()),
					resolved_node_id: None,
					mesh: Some(1),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarMaterialSwap",
						"enabled": true,
						"id": "mat-swap",
						"name": "Swap Jacket",
						"fields": {
							"root": {"nodeId": "node_outfit_root", "path": "Root/OutfitRoot"},
							"swaps": [{
								"from": {"materialName": "Base Blue"},
								"to": {"materialName": "Base Red"}
							}]
						}
					}]
				}
			}),
		};

		let actions = unavatar_runtime_action_set(&unavatar, Some(&scene)).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].id, "ma:material_swap:mat-swap");
		assert_eq!(
			actions.actions[0].triggers,
			vec![UnaRuntimeActionTrigger::SupervisorCommand {
				command: "ma:material_swap:mat-swap".to_string()
			}]
		);
		assert_eq!(
			actions.actions[0].effects,
			vec![UnaRuntimeActionEffect::MaterialSlot {
				target: UnaRuntimeMaterialSlotTarget {
					node: UnaRuntimeNodeTarget {
						node_index: None,
						source_node_id: Some("node_jacket".to_string()),
						resolved_node_id: None,
						path: Some("Root/OutfitRoot/Jacket".to_string()),
					},
					primitive_index: Some(0),
				},
				material: Some(UnaRuntimeMaterialTarget {
					material_index: None,
					name: Some("Base Red".to_string()),
				}),
			}]
		);
	}

	#[test]
	fn unavatar_runtime_actions_import_modular_avatar_material_swap_null_slots() {
		let primitive_null = UnaMeshBuffers {
			name: None,
			positions: vec![[0.0; 3]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: None,
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let primitive_blue = UnaMeshBuffers {
			material_index: Some(0),
			..primitive_null.clone()
		};
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![primitive_null, primitive_blue]],
			materials: vec![
				UnaMaterialPbr {
					name: Some("Base Blue".to_string()),
					..Default::default()
				},
				UnaMaterialPbr {
					name: Some("Base Red".to_string()),
					..Default::default()
				},
			],
			nodes: vec![UnaSceneNode {
				name: Some("Renderer".to_string()),
				source_node_id: Some("node_renderer".to_string()),
				resolved_node_id: None,
				mesh: Some(0),
				..test_node(Vec::new())
			}],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarMaterialSwap",
						"enabled": true,
						"id": "mat-swap",
						"fields": {
							"swaps": [
								{"from": null, "to": {"materialName": "Base Red"}},
								{"from": {"materialName": "Base Blue"}, "to": null}
							]
						}
					}]
				}
			}),
		};

		let actions = unavatar_runtime_action_set(&unavatar, Some(&scene)).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(
			actions.actions[0].effects,
			vec![
				UnaRuntimeActionEffect::MaterialSlot {
					target: UnaRuntimeMaterialSlotTarget {
						node: UnaRuntimeNodeTarget {
							node_index: None,
							source_node_id: Some("node_renderer".to_string()),
							resolved_node_id: None,
							path: Some("Renderer".to_string()),
						},
						primitive_index: Some(0),
					},
					material: Some(UnaRuntimeMaterialTarget {
						material_index: None,
						name: Some("Base Red".to_string()),
					}),
				},
				UnaRuntimeActionEffect::MaterialSlot {
					target: UnaRuntimeMaterialSlotTarget {
						node: UnaRuntimeNodeTarget {
							node_index: None,
							source_node_id: Some("node_renderer".to_string()),
							resolved_node_id: None,
							path: Some("Renderer".to_string()),
						},
						primitive_index: Some(1),
					},
					material: None,
				},
			]
		);
	}

	#[test]
	fn unavatar_runtime_actions_skip_material_swap_when_explicit_root_is_missing() {
		let primitive = UnaMeshBuffers {
			name: None,
			positions: vec![[0.0; 3]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: Some(0),
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![primitive]],
			materials: vec![
				UnaMaterialPbr {
					name: Some("Base Blue".to_string()),
					..Default::default()
				},
				UnaMaterialPbr {
					name: Some("Base Red".to_string()),
					..Default::default()
				},
			],
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Jacket".to_string()),
					source_node_id: Some("node_jacket".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarMaterialSwap",
						"enabled": true,
						"id": "mat-swap",
						"fields": {
							"root": {"nodeId": "missing_root", "path": "MissingRoot"},
							"swaps": [{
								"from": {"materialName": "Base Blue"},
								"to": {"materialName": "Base Red"}
							}]
						}
					}]
				}
			}),
		};

		assert!(unavatar_runtime_action_set(&unavatar, Some(&scene)).is_none());
	}

	#[test]
	fn wardrobe_subtree_visibility_uses_original_export_paths_after_reparent() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					source_node_id: None,
					resolved_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![1, 2],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Outfit".to_string()),
					source_node_id: Some("node_outfit".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Head".to_string()),
					source_node_id: Some("node_head".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![3],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Hat".to_string()),
					source_node_id: Some("node_hat".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: Vec::new(),
					mesh: Some(0),
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
				"nodes": [
					{"nodeId": "node_outfit", "path": "Outfit"},
					{"nodeId": "node_head", "path": "Armature/Head"},
					{"nodeId": "node_hat", "path": "Outfit/Armature/Head/Hat"}
				],
				"wardrobe": {
					"baseSet": "base",
					"sets": [{
						"id": "base",
						"operations": [{
							"type": "subtreeEnabled",
							"target": {"nodeId": "node_outfit", "path": "Outfit"},
							"visible": false
						}]
					}, {
						"id": "outfit",
						"operations": [{
							"type": "subtreeEnabled",
							"target": {"nodeId": "node_outfit", "path": "Outfit"},
							"visible": true
						}]
					}]
				}
			}),
		};

		let base = unavatar_wardrobe_set_operations(&unavatar, "base").unwrap();
		let applied = apply_unavatar_wardrobe_operations(&mut scene, None, base, Some(&unavatar));
		assert_eq!(applied.visibility_applied, 1);
		assert!(!scene.nodes[1].visible);
		assert!(!scene.nodes[3].visible);

		let outfit = unavatar_wardrobe_set_operations(&unavatar, "outfit").unwrap();
		let applied = apply_unavatar_wardrobe_operations(&mut scene, None, outfit, Some(&unavatar));
		assert_eq!(applied.visibility_applied, 1);
		assert!(scene.nodes[1].visible);
		assert!(scene.nodes[3].visible);
	}

	#[test]
	fn wardrobe_material_slot_operation_replaces_primitive_material() {
		let primitive = UnaMeshBuffers {
			name: None,
			positions: vec![[0.0; 3]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: Some(0),
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		let mut scene = UnaSceneSnapshot {
			meshes: vec![vec![primitive.clone(), primitive]],
			materials: vec![
				UnaMaterialPbr {
					name: Some("Base".to_string()),
					..Default::default()
				},
				UnaMaterialPbr {
					name: Some("Alt".to_string()),
					..Default::default()
				},
			],
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Renderer".to_string()),
					source_node_id: Some("node_renderer".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let operations = vec![
			serde_json::json!({
				"op": "materialSlot",
				"target": {
					"nodeId": "node_renderer",
					"path": "Root/Renderer",
					"primitiveIndex": 1
				},
				"material": {"materialName": "Alt"}
			}),
			serde_json::json!({
				"op": "materialSlot",
				"target": {
					"path": "Root/Renderer",
					"primitiveIndex": 2
				},
				"material": {"materialName": "Missing"}
			}),
		];

		let applied = apply_unavatar_wardrobe_operations(&mut scene, None, &operations, None);

		assert_eq!(applied.material_slot_applied, 1);
		assert_eq!(applied.material_slot_missing, 1);
		assert_eq!(scene.meshes[0][0].material_index, Some(0));
		assert_eq!(scene.meshes[0][1].material_index, Some(1));
	}

	#[test]
	fn wardrobe_material_operations_apply_color_and_scalar_overrides() {
		let mut scene = UnaSceneSnapshot {
			materials: vec![
				UnaMaterialPbr {
					name: Some("Body".to_string()),
					..Default::default()
				},
				UnaMaterialPbr {
					name: Some("Accent".to_string()),
					..Default::default()
				},
			],
			..Default::default()
		};
		let operations = vec![
			serde_json::json!({
				"op": "materialColor",
				"target": {"materialName": "Accent"},
				"parameter": "_EmissionColor",
				"color": [2.0, 0.5, -1.0, 1.0]
			}),
			serde_json::json!({
				"op": "materialScalar",
				"materialIndex": 1,
				"parameter": "_Smoothness",
				"value": 0.75
			}),
			serde_json::json!({
				"op": "materialScalar",
				"materialIndex": 1,
				"parameter": "_Unsupported",
				"value": 1.0
			}),
		];

		let applied = apply_unavatar_wardrobe_operations(&mut scene, None, &operations, None);

		assert_eq!(applied.material_applied, 2);
		assert_eq!(applied.material_missing, 1);
		assert_eq!(scene.materials[1].emissive_factor, [2.0, 0.5, 0.0]);
		assert_eq!(scene.materials[1].roughness_factor, 0.25);
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
		let queue_refraction = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonRef",
			"renderQueue": 2900
		});
		let queue_gem = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonGem",
			"renderQueue": 2900
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
			unavatar_material_inferred_alpha_mode(Some(&queue_refraction), UnaAlphaMode::Blend, None, true),
			Some(UnaAlphaMode::Opaque)
		);
		assert_eq!(
			unavatar_material_inferred_alpha_mode(Some(&queue_gem), UnaAlphaMode::Mask, Some(0.001), true),
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
	fn liltoon_onepass_transparent_disables_backpass_pre_zwrite_by_default() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonTransparent",
			"floatParams": { "_ZWrite": 1.0 },
			"mtoon": {}
		});

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon_like material");
		let mtoon = unavatar_mtoon_from_extras(&extras).expect("mtoon material");

		assert!(mtoon.transparent_with_z_write);
		assert_eq!(liltoon_like.blend_state.pre_zwrite_factor, 0.0);
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
	fn liltoon_rim_direction_ranges_preserve_signed_values() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "lilToon",
			"floatParams": {
				"_UseRim": 1.0,
				"_RimDirRange": -0.75,
				"_RimIndirRange": -0.25
			}
		});

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon_like material");

		assert_eq!(liltoon_like.rim.directional_range_factor, -0.75);
		assert_eq!(liltoon_like.rim.indirect_range_factor, -0.25);
	}

	#[test]
	fn imports_liltoon_audio_link_params() {
		let extras: Value = serde_json::from_str(
			r#"{
				"family": "liltoon",
				"sourceShader": "lilToon",
				"floatParams": {
					"_UseAudioLink": 1.0,
					"_AudioLinkUVMode": 2.0,
					"_AudioLinkMask_UVMode": 3.0,
					"_AudioLink2Main2nd": 1.0,
					"_AudioLink2Main3rd": 1.0,
					"_AudioLink2Emission": 1.0,
					"_AudioLink2EmissionGrad": 1.0,
					"_AudioLink2Emission2nd": 1.0,
					"_AudioLink2Emission2ndGrad": 1.0,
					"_AudioLink2Vertex": 1.0,
					"_AudioLinkVertexUVMode": 3.0,
					"_AudioLinkAsLocal": 1.0
				},
				"vectorParams": {
					"_AudioLinkDefaultValue": [0.4, 0.5, 3.0, 0.2],
					"_AudioLinkUVParams": [0.6, 0.1, 0.25, 0.75],
					"_AudioLinkStart": [1.0, 2.0, 3.0, 0.0],
					"_AudioLinkMask_ScrollRotate": [0.01, 0.02, 0.03, 0.04],
					"_AudioLinkVertexUVParams": [0.7, 0.2, 0.3, 0.4],
					"_AudioLinkVertexStart": [4.0, 5.0, 6.0, 0.0],
					"_AudioLinkVertexStrength": [0.1, 0.2, 0.3, 0.4],
					"_AudioLinkLocalMapParams": [128.0, 2.0, 0.5, 0.0]
				}
			}"#,
		)
		.unwrap();

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("lilToon material");

		assert_eq!(liltoon_like.audio_link.enabled_factor, 1.0);
		assert_eq!(liltoon_like.audio_link.default_value_factor, [0.4, 0.5, 3.0, 0.2]);
		assert_eq!(liltoon_like.audio_link.uv_mode_factor, 2.0);
		assert_eq!(liltoon_like.audio_link.uv_params_factor, [0.6, 0.1, 0.25, 0.75]);
		assert_eq!(liltoon_like.audio_link.start_factor, [1.0, 2.0, 3.0, 0.0]);
		assert_eq!(liltoon_like.audio_link.mask_uv_scroll_rotate_factor, [0.01, 0.02, 0.03, 0.04]);
		assert_eq!(liltoon_like.audio_link.mask_uv_mode_factor, 3.0);
		assert_eq!(liltoon_like.audio_link.to_main_second_factor, 1.0);
		assert_eq!(liltoon_like.audio_link.to_main_third_factor, 1.0);
		assert_eq!(liltoon_like.audio_link.to_emission_factor, 1.0);
		assert_eq!(liltoon_like.audio_link.to_emission_gradation_factor, 1.0);
		assert_eq!(liltoon_like.audio_link.to_emission_second_factor, 1.0);
		assert_eq!(liltoon_like.audio_link.to_emission_second_gradation_factor, 1.0);
		assert_eq!(liltoon_like.audio_link.to_vertex_factor, 1.0);
		assert_eq!(liltoon_like.audio_link.vertex_uv_mode_factor, 3.0);
		assert_eq!(liltoon_like.audio_link.vertex_uv_params_factor, [0.7, 0.2, 0.3, 0.4]);
		assert_eq!(liltoon_like.audio_link.vertex_start_factor, [4.0, 5.0, 6.0, 0.0]);
		assert_eq!(liltoon_like.audio_link.vertex_strength_factor, [0.1, 0.2, 0.3, 0.4]);
		assert_eq!(liltoon_like.audio_link.as_local_factor, 1.0);
		assert_eq!(liltoon_like.audio_link.local_map_params_factor, [128.0, 2.0, 0.5, 0.0]);
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
					"_FlipNormal": 1.0,
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
					"_ShadowPostAO": 1.0,
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
					"_MatCapCustomNormal": 1.0,
					"_MatCapBumpScale": 0.76,
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
					"_MatCap2ndCustomNormal": 1.0,
					"_MatCap2ndBumpScale": 0.98,
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
					"_UseGlitter": 1.0,
					"_GlitterMainStrength": 0.2,
					"_GlitterNormalStrength": 0.8,
					"_GlitterPostContrast": 1.4,
					"_GlitterSensitivity": 0.35,
					"_GlitterEnableLighting": 0.6,
					"_GlitterShadowMask": 0.7,
					"_GlitterApplyTransparency": 0.8,
					"_GlitterBackfaceMask": 1.0,
					"_GlitterScaleRandomize": 0.3,
					"_GlitterUVMode": 1.0,
					"_GlitterColorTex_UVMode": 2.0,
					"_GlitterApplyShape": 1.0,
					"_GlitterAngleRandomize": 1.0,
					"_GlitterVRParallaxStrength": 0.4,
					"_DissolveNoiseStrength": 0.25,
					"_UseParallax": 1.0,
					"_UsePOM": 1.0,
					"_Parallax": 0.07,
					"_ParallaxOffset": 0.35,
					"_IDMaskCompile": 1.0,
					"_IDMaskFrom": 8.0,
					"_IDMaskIsBitmap": 1.0,
					"_IDMaskControlsDissolve": 1.0,
					"_IDMask1": 1.0,
					"_IDMask2": 0.0,
					"_IDMask3": 1.0,
					"_IDMask4": 0.0,
					"_IDMask5": 1.0,
					"_IDMask6": 0.0,
					"_IDMask7": 1.0,
					"_IDMask8": 0.0,
					"_IDMaskPrior1": 0.0,
					"_IDMaskPrior2": 1.0,
					"_IDMaskPrior3": 0.0,
					"_IDMaskPrior4": 1.0,
					"_IDMaskPrior5": 0.0,
					"_IDMaskPrior6": 1.0,
					"_IDMaskPrior7": 0.0,
					"_IDMaskPrior8": 1.0,
					"_IDMaskIndex1": 10.0,
					"_IDMaskIndex2": 20.0,
					"_IDMaskIndex3": 30.0,
					"_IDMaskIndex4": 40.0,
					"_IDMaskIndex5": 50.0,
					"_IDMaskIndex6": 60.0,
					"_IDMaskIndex7": 70.0,
					"_IDMaskIndex8": 80.0,
					"_UDIMDiscardCompile": 1.0,
					"_UDIMDiscardMode": 1.0,
					"_UDIMDiscardUV": 2.0,
					"_UDIMDiscardRow0_1": 1.0,
					"_UDIMDiscardRow2_3": 1.0,
					"_UseEmission": 1.0,
					"_EmissionMainStrength": 0.45,
					"_EmissionBlend": 0.55,
					"_EmissionBlendMode": 3.0,
					"_EmissionUseGrad": 1.0,
					"_EmissionGradSpeed": 1.5,
					"_UseEmission2nd": 1.0,
					"_Emission2ndBlend": 0.64,
					"_Emission2ndBlendMode": 2.0,
					"_Emission2ndMainStrength": 0.74,
					"_Emission2ndUseGrad": 1.0,
					"_Emission2ndGradSpeed": 2.5,
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
					"_UseFur": 1.0,
					"_FurLayerNum": 3.0,
					"_VertexColor2FurVector": 1.0,
					"_FurVectorScale": 1.75,
					"_FurGravity": 0.35,
					"_FurAO": 0.6,
					"_FurRootOffset": -0.35,
					"_FurCutoutLength": 0.9,
					"_FurRandomize": 0.45,
					"_FurNoiseTiling": 2.0,
					"_FurNoiseOffset": 0.25,
					"_FurRimFresnelPower": 4.5,
					"_FurRimAntiLight": 0.75,
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
					"_PreCutoff": 0.3,
					"_PreZWrite": 0.0,
					"_PreCull": 1.0,
					"_AlphaToMask": 1.0,
					"_LightMinLimit": 0.06,
					"_LightMaxLimit": 0.9,
					"_MonochromeLighting": 0.25,
					"_AsUnlit": 0.4,
					"_VertexLightStrength": 0.35,
					"_AAStrength": 1.25,
					"_GSAAStrength": 0.5,
					"_DistanceFadeMode": 1.0,
					"_DistanceFadeRimFresnelPower": 6.5,
					"_MainGradationStrength": 0.6,
					"_UseMain2ndTex": 1.0,
					"_Main2ndTexBlendMode": 1.0,
					"_Main2ndEnableLighting": 0.25,
					"_Main2ndTexAlphaMode": 2.0,
					"_Main2ndTex_Cull": 1.0,
					"_Main2ndTexIsDecal": 1.0,
					"_Main2ndTexIsLeftOnly": 1.0,
					"_Main2ndTexIsRightOnly": 0.0,
					"_Main2ndTexShouldCopy": 1.0,
					"_Main2ndTexAngle": 0.25,
					"_Main2ndTexShouldFlipMirror": 1.0,
					"_Main2ndTexShouldFlipCopy": 0.0,
					"_Main2ndDissolveNoiseStrength": 0.26,
					"_UseMain3rdTex": 1.0,
					"_Main3rdTexBlendMode": 3.0,
					"_Main3rdEnableLighting": 0.75,
					"_Main3rdTexAlphaMode": 4.0,
					"_Main3rdTex_Cull": 2.0,
					"_Main3rdTexIsDecal": 1.0,
					"_Main3rdTexIsLeftOnly": 0.0,
					"_Main3rdTexIsRightOnly": 1.0,
					"_Main3rdTexShouldCopy": 0.0,
					"_Main3rdTexAngle": 0.5,
					"_Main3rdTexShouldFlipMirror": 0.0,
					"_Main3rdTexShouldFlipCopy": 1.0,
					"_Main3rdDissolveNoiseStrength": 0.36
				},
				"colorParams": {
					"_BackfaceColor": [0.9, 0.8, 0.7, 0.6],
					"_Color2nd": [0.11, 0.22, 0.33, 0.44],
					"_Color3rd": [0.55, 0.66, 0.77, 0.88],
					"_Main2ndDissolveColor": [0.2, 0.3, 0.4, 0.5],
					"_Main3rdDissolveColor": [0.6, 0.7, 0.8, 0.9],
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
					"_FurRimColor": [0.21, 0.31, 0.41, 0.51],
					"_BacklightColor": [1.1, 1.2, 1.3, 0.8],
					"_GlitterColor": [0.8, 0.7, 0.6, 0.5],
					"_DissolveColor": [1.2, 1.1, 1.0, 0.9],
					"_EmissionColor": [0.5, 0.4, 0.3, 0.8],
					"_Emission2ndColor": [0.15, 0.25, 0.35, 0.45],
					"_OutlineColor": [0.01, 0.02, 0.03, 1.0],
					"_OutlineLitColor": [1.0, 0.2, 0.0, 0.4],
					"_DistanceFadeColor": [0.2, 0.3, 0.4, 0.6],
					"_DistanceFadeRimColor": [0.7, 0.6, 0.5, 0.4]
				},
				"vectorParams": {
					"_ShadowAOShift": [3.0, 0.1, 2.0, 0.2],
					"_ShadowAOShift2": [1.5, 0.3, 0.0, 0.0],
					"_MatCapBlendUV1": [0.12, 0.34, 0.0, 0.0],
					"_MatCap2ndBlendUV1": [0.56, 0.78, 0.0, 0.0],
					"_GlitterParams1": [512.0, 513.0, 0.08, 2.0],
					"_GlitterParams2": [0.6, 0.7, 0.8, 0.9],
					"_GlitterAtras": [3.0, 4.0, 0.0, 0.0],
					"_DistanceFade": [0.2, 5.0, 0.75, 1.0],
					"_DissolveParams": [1.0, 0.0, 0.45, 0.12],
					"_DissolvePos": [0.25, 0.75, 0.0, 0.5],
					"_DissolveNoiseMask_ScrollRotate": [0.01, 0.02, 0.03, 0.04],
					"_Main2ndDissolveParams": [1.0, 0.0, 0.25, 0.05],
					"_Main2ndDissolvePos": [0.11, 0.22, 0.33, 0.44],
					"_Main2ndDissolveNoiseMask_ScrollRotate": [0.05, 0.06, 0.07, 0.08],
					"_Main2ndDistanceFade": [1.0, 6.0, 0.4, 0.0],
					"_Main2ndTexDecalAnimation": [4.0, 2.0, 0.0, 0.0],
					"_Main2ndTexDecalSubParam": [1.0, 1.0, 0.5, 0.0],
					"_Main3rdDissolveParams": [2.0, 1.0, 0.35, 0.06],
					"_Main3rdDissolvePos": [0.55, 0.66, 0.77, 0.88],
					"_Main3rdDissolveNoiseMask_ScrollRotate": [0.09, 0.10, 0.11, 0.12],
					"_Main3rdDistanceFade": [2.0, 7.0, 0.5, 0.0],
					"_Main3rdTexDecalAnimation": [3.0, 3.0, 0.0, 0.0],
					"_Main3rdTexDecalSubParam": [0.5, 0.5, 0.25, 0.0],
					"_FurVector": [0.1, 0.2, 0.3, 0.4]
				},
				"mtoon": {
					"shadowColorTextureIndex": 8,
					"shadow2ndColorTextureIndex": 38,
					"shadow3rdColorTextureIndex": 39,
					"shadowStrengthMaskTextureIndex": 9,
					"shadowBorderMaskTextureIndex": 10,
					"shadowBlurMaskTextureIndex": 11,
					"rimMultiplyTextureIndex": 12,
					"backlightColorTextureIndex": 37,
					"glitterColorTextureIndex": 40,
					"glitterShapeTextureIndex": 41,
					"dissolveMaskTextureIndex": 42,
					"dissolveNoiseMaskTextureIndex": 43,
					"parallaxTextureIndex": 44,
					"emissionTextureIndex": 13,
					"emissionGradationTextureIndex": 29,
					"emission2ndTextureIndex": 34,
					"emission2ndBlendMaskTextureIndex": 35,
					"emission2ndGradationTextureIndex": 36,
					"outlineWidthMultiplyTextureIndex": 14,
					"outlineTextureIndex": 15,
					"reflectionColorTextureIndex": 16,
					"smoothnessTextureIndex": 17,
					"metallicGlossTextureIndex": 18,
					"main2ndTextureIndex": 30,
					"main2ndBlendMaskTextureIndex": 32,
					"main2ndDissolveMaskTextureIndex": 45,
					"main2ndDissolveNoiseMaskTextureIndex": 46,
					"main3rdTextureIndex": 31,
					"main3rdBlendMaskTextureIndex": 33,
					"main3rdDissolveMaskTextureIndex": 47,
					"main3rdDissolveNoiseMaskTextureIndex": 48,
					"matcapTextureIndex": 19,
					"matcapBlendMaskTextureIndex": 20,
					"matcapBumpTextureIndex": 66,
					"matcap2ndTextureIndex": 22,
					"matcap2ndBlendMaskTextureIndex": 23,
					"matcap2ndBumpTextureIndex": 67,
					"normal2ndTextureIndex": 24,
					"normal2ndScaleMaskTextureIndex": 65,
					"alphaMaskTextureIndex": 21,
					"gradationMapTextureIndex": 25,
					"anisotropyTangentTextureIndex": 26,
					"anisotropyScaleMaskTextureIndex": 27,
					"anisotropyShiftNoiseMaskTextureIndex": 28,
					"furVectorTextureIndex": 61,
					"furLengthMaskTextureIndex": 62,
					"furNoiseMaskTextureIndex": 63,
					"furMaskTextureIndex": 64,
					"mainColorAdjustMaskTextureIndex": 65,
					"mainTexHsvgFactor": [0.12, 0.8, 1.2, 0.9]
				}
			}"#,
		)
		.expect("test extras JSON");

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon_like material");
		let mtoon = unavatar_mtoon_from_extras(&extras).expect("legacy mtoon material");

		assert_eq!(liltoon_like.source_profile, UnaLilToonLikeSourceProfile::Liltoon);
		assert_eq!(liltoon_like.flip_backface_normal_factor, 1.0);
		assert_eq!(liltoon_like.rendering.backface_color_factor, [0.9, 0.8, 0.7, 0.6]);
		assert_eq!(liltoon_like.main_color.main_texture_hsvg_factor, [0.12, 0.8, 1.2, 0.9]);
		assert_eq!(liltoon_like.main_color.main_color_adjust_mask_texture_index, Some(65));
		assert_eq!(liltoon_like.main_color.gradation_enabled_factor, 1.0);
		assert_eq!(liltoon_like.main_color.gradation_texture_index, Some(25));
		assert_eq!(liltoon_like.main_color.gradation_strength_factor, 0.6);
		assert_eq!(liltoon_like.main_color.second_enabled_factor, 1.0);
		assert_eq!(liltoon_like.main_color.second_texture_index, Some(30));
		assert_eq!(liltoon_like.main_color.second_blend_mask_texture_index, Some(32));
		assert_eq!(liltoon_like.main_color.second_color_factor, [0.11, 0.22, 0.33, 0.44]);
		assert_eq!(liltoon_like.main_color.second_blend_mode, UnaLilToonLikeBlendMode::Add);
		assert_eq!(liltoon_like.main_color.second_enable_lighting_factor, 0.25);
		assert_eq!(liltoon_like.main_color.second_alpha_mode_factor, 2.0);
		assert_eq!(liltoon_like.main_color.second_cull_factor, 1.0);
		assert_eq!(liltoon_like.main_color.second_distance_fade_factor, [1.0, 6.0, 0.4, 0.0]);
		assert_eq!(liltoon_like.main_color.second_decal_flags_factor, [1.0, 1.0, 0.0, 1.0]);
		assert_eq!(liltoon_like.main_color.second_decal_transform_factor, [0.25, 1.0, 0.0, 0.0]);
		assert_eq!(liltoon_like.main_color.second_decal_animation_factor, [4.0, 2.0, 0.0, 0.0]);
		assert_eq!(liltoon_like.main_color.second_decal_sub_param_factor, [1.0, 1.0, 0.5, 0.0]);
		assert_eq!(liltoon_like.main_color.second_dissolve.mask_texture_index, Some(45));
		assert_eq!(liltoon_like.main_color.second_dissolve.noise_mask_texture_index, Some(46));
		assert_eq!(liltoon_like.main_color.second_dissolve.color_factor, [0.2, 0.3, 0.4, 0.5]);
		assert_eq!(liltoon_like.main_color.second_dissolve.params_factor, [1.0, 0.0, 0.25, 0.05]);
		assert_eq!(liltoon_like.main_color.second_dissolve.position_factor, [0.11, 0.22, 0.33, 0.44]);
		assert_eq!(liltoon_like.main_color.second_dissolve.noise_strength_factor, 0.26);
		assert_eq!(
			liltoon_like.main_color.second_dissolve.noise_uv_scroll_rotate_factor,
			[0.05, 0.06, 0.07, 0.08]
		);
		assert_eq!(liltoon_like.main_color.third_enabled_factor, 1.0);
		assert_eq!(liltoon_like.main_color.third_texture_index, Some(31));
		assert_eq!(liltoon_like.main_color.third_blend_mask_texture_index, Some(33));
		assert_eq!(liltoon_like.main_color.third_color_factor, [0.55, 0.66, 0.77, 0.88]);
		assert_eq!(liltoon_like.main_color.third_blend_mode, UnaLilToonLikeBlendMode::Multiply);
		assert_eq!(liltoon_like.main_color.third_enable_lighting_factor, 0.75);
		assert_eq!(liltoon_like.main_color.third_alpha_mode_factor, 4.0);
		assert_eq!(liltoon_like.main_color.third_cull_factor, 2.0);
		assert_eq!(liltoon_like.main_color.third_distance_fade_factor, [2.0, 7.0, 0.5, 0.0]);
		assert_eq!(liltoon_like.main_color.third_decal_flags_factor, [1.0, 0.0, 1.0, 0.0]);
		assert_eq!(liltoon_like.main_color.third_decal_transform_factor, [0.5, 0.0, 1.0, 0.0]);
		assert_eq!(liltoon_like.main_color.third_decal_animation_factor, [3.0, 3.0, 0.0, 0.0]);
		assert_eq!(liltoon_like.main_color.third_decal_sub_param_factor, [0.5, 0.5, 0.25, 0.0]);
		assert_eq!(liltoon_like.main_color.third_dissolve.mask_texture_index, Some(47));
		assert_eq!(liltoon_like.main_color.third_dissolve.noise_mask_texture_index, Some(48));
		assert_eq!(liltoon_like.main_color.third_dissolve.color_factor, [0.6, 0.7, 0.8, 0.9]);
		assert_eq!(liltoon_like.main_color.third_dissolve.params_factor, [2.0, 1.0, 0.35, 0.06]);
		assert_eq!(liltoon_like.main_color.third_dissolve.position_factor, [0.55, 0.66, 0.77, 0.88]);
		assert_eq!(liltoon_like.main_color.third_dissolve.noise_strength_factor, 0.36);
		assert_eq!(
			liltoon_like.main_color.third_dissolve.noise_uv_scroll_rotate_factor,
			[0.09, 0.10, 0.11, 0.12]
		);
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
		assert_eq!(liltoon_like.rendering.as_unlit_factor, 0.4);
		assert_eq!(liltoon_like.rendering.vertex_light_strength_factor, 0.35);
		assert_eq!(liltoon_like.rendering.aa_strength_factor, 1.25);
		assert_eq!(liltoon_like.rendering.gsaa_strength_factor, 0.5);
		assert_eq!(liltoon_like.rendering.distance_fade_factor, [0.2, 5.0, 0.75, 1.0]);
		assert_eq!(liltoon_like.rendering.distance_fade_color_factor, [0.2, 0.3, 0.4, 0.6]);
		assert_eq!(liltoon_like.rendering.distance_fade_rim_color_factor, [0.7, 0.6, 0.5, 0.4]);
		assert_eq!(liltoon_like.rendering.distance_fade_rim_fresnel_power_factor, 6.5);
		assert_eq!(liltoon_like.rendering.distance_fade_mode_factor, 1.0);
		assert_eq!(liltoon_like.normal.second_enabled_factor, 1.0);
		assert_eq!(liltoon_like.normal.second_texture_index, Some(24));
		assert_eq!(liltoon_like.normal.second_scale_mask_texture_index, Some(65));
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
		assert_eq!(liltoon_like.shadow.post_ao_factor, 1.0);
		assert_eq!(liltoon_like.shadow.ao_shift_factor, [3.0, 0.1, 2.0, 0.2]);
		assert_eq!(liltoon_like.shadow.ao_shift2_factor, [1.5, 0.3, 0.0, 0.0]);
		assert_eq!(liltoon_like.shadow.normal_strength_factor, 0.55);
		assert_eq!(liltoon_like.shadow.receive_factor, 0.65);
		assert_eq!(liltoon_like.shadow.second_color_factor, [0.4, 0.5, 0.6, 0.7]);
		assert_eq!(liltoon_like.shadow.second_color_texture_index, Some(38));
		assert_eq!(liltoon_like.shadow.second_border_factor, 0.31);
		assert_eq!(liltoon_like.shadow.second_blur_factor, 0.21);
		assert_eq!(liltoon_like.shadow.second_normal_strength_factor, 0.71);
		assert_eq!(liltoon_like.shadow.second_receive_factor, 0.81);
		assert_eq!(liltoon_like.shadow.third_color_factor, [0.3, 0.4, 0.5, 0.6]);
		assert_eq!(liltoon_like.shadow.third_color_texture_index, Some(39));
		assert_eq!(liltoon_like.shadow.third_border_factor, 0.41);
		assert_eq!(liltoon_like.shadow.third_blur_factor, 0.32);
		assert_eq!(liltoon_like.shadow.third_normal_strength_factor, 0.72);
		assert_eq!(liltoon_like.shadow.third_receive_factor, 0.82);
		assert_eq!(liltoon_like.matcap.color_factor, [0.2, 0.4, 0.6]);
		assert_eq!(liltoon_like.matcap.color_alpha_factor, 0.7);
		assert_eq!(liltoon_like.matcap.texture_index, Some(19));
		assert_eq!(liltoon_like.matcap.blend_mask_texture_index, Some(20));
		assert_eq!(liltoon_like.matcap.bump_texture_index, Some(66));
		assert_eq!(liltoon_like.matcap.main_strength_factor, 0.5);
		assert_eq!(liltoon_like.matcap.blend_factor, 0.25);
		assert_eq!(liltoon_like.matcap.enable_lighting_factor, 0.75);
		assert_eq!(liltoon_like.matcap.blend_mode, UnaLilToonLikeBlendMode::Screen);
		assert_eq!(liltoon_like.matcap.normal_strength_factor, 0.66);
		assert_eq!(liltoon_like.matcap.custom_normal_factor, 1.0);
		assert_eq!(liltoon_like.matcap.bump_scale_factor, 0.76);
		assert_eq!(liltoon_like.matcap.shadow_mask_factor, 0.57);
		assert_eq!(liltoon_like.matcap.apply_transparency_factor, 0.47);
		assert_eq!(liltoon_like.matcap.lod_factor, 2.5);
		assert_eq!(liltoon_like.matcap.backface_mask_factor, 0.35);
		assert_eq!(liltoon_like.matcap.perspective_factor, 0.64);
		assert_eq!(liltoon_like.matcap.z_rotation_cancel_factor, 0.74);
		assert_eq!(liltoon_like.matcap.vr_parallax_strength_factor, 0.84);
		assert_eq!(liltoon_like.matcap.blend_uv1_factor, [0.12, 0.34]);
		assert_eq!(liltoon_like.matcap.second_enabled_factor, 1.0);
		assert_eq!(liltoon_like.matcap.second_texture_index, Some(22));
		assert_eq!(liltoon_like.matcap.second_blend_mask_texture_index, Some(23));
		assert_eq!(liltoon_like.matcap.second_bump_texture_index, Some(67));
		assert_eq!(liltoon_like.matcap.second_color_factor, [0.3, 0.5, 0.7, 0.9]);
		assert_eq!(liltoon_like.matcap.second_main_strength_factor, 0.58);
		assert_eq!(liltoon_like.matcap.second_blend_factor, 0.68);
		assert_eq!(liltoon_like.matcap.second_enable_lighting_factor, 0.78);
		assert_eq!(liltoon_like.matcap.second_shadow_mask_factor, 0.48);
		assert_eq!(liltoon_like.matcap.second_apply_transparency_factor, 0.38);
		assert_eq!(liltoon_like.matcap.second_blend_mode, UnaLilToonLikeBlendMode::Add);
		assert_eq!(liltoon_like.matcap.second_normal_strength_factor, 0.88);
		assert_eq!(liltoon_like.matcap.second_custom_normal_factor, 1.0);
		assert_eq!(liltoon_like.matcap.second_bump_scale_factor, 0.98);
		assert_eq!(liltoon_like.matcap.second_lod_factor, 1.5);
		assert_eq!(liltoon_like.matcap.second_backface_mask_factor, 0.45);
		assert_eq!(liltoon_like.matcap.second_perspective_factor, 0.54);
		assert_eq!(liltoon_like.matcap.second_z_rotation_cancel_factor, 0.44);
		assert_eq!(liltoon_like.matcap.second_vr_parallax_strength_factor, 0.34);
		assert_eq!(liltoon_like.matcap.second_blend_uv1_factor, [0.56, 0.78]);
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
		assert_eq!(liltoon_like.backlight.texture_index, Some(37));
		assert_eq!(liltoon_like.backlight.main_strength_factor, 0.72);
		assert_eq!(liltoon_like.backlight.normal_strength_factor, 0.82);
		assert_eq!(liltoon_like.backlight.border_factor, 0.32);
		assert_eq!(liltoon_like.backlight.blur_factor, 0.23);
		assert_eq!(liltoon_like.backlight.directivity_factor, 7.0);
		assert_eq!(liltoon_like.backlight.view_strength_factor, 0.62);
		assert_eq!(liltoon_like.backlight.receive_shadow_factor, 0.52);
		assert_eq!(liltoon_like.backlight.backface_mask_factor, 0.42);
		assert_eq!(liltoon_like.glitter.enabled_factor, 1.0);
		assert_eq!(liltoon_like.glitter.color_factor, [0.8, 0.7, 0.6, 0.5]);
		assert_eq!(liltoon_like.glitter.color_texture_index, Some(40));
		assert_eq!(liltoon_like.glitter.shape_texture_index, Some(41));
		assert_eq!(liltoon_like.glitter.params1_factor, [512.0, 513.0, 0.08, 2.0]);
		assert_eq!(liltoon_like.glitter.params2_factor, [0.6, 0.7, 0.8, 0.9]);
		assert_eq!(liltoon_like.glitter.atlas_factor, [3.0, 4.0, 0.0, 0.0]);
		assert_eq!(liltoon_like.glitter.main_strength_factor, 0.2);
		assert_eq!(liltoon_like.glitter.normal_strength_factor, 0.8);
		assert_eq!(liltoon_like.glitter.post_contrast_factor, 1.4);
		assert_eq!(liltoon_like.glitter.sensitivity_factor, 0.35);
		assert_eq!(liltoon_like.glitter.enable_lighting_factor, 0.6);
		assert_eq!(liltoon_like.glitter.shadow_mask_factor, 0.7);
		assert_eq!(liltoon_like.glitter.apply_transparency_factor, 0.8);
		assert_eq!(liltoon_like.glitter.backface_mask_factor, 1.0);
		assert_eq!(liltoon_like.glitter.scale_randomize_factor, 0.3);
		assert_eq!(liltoon_like.glitter.uv_mode_factor, 1.0);
		assert_eq!(liltoon_like.glitter.color_texture_uv_mode_factor, 2.0);
		assert_eq!(liltoon_like.glitter.apply_shape_factor, 1.0);
		assert_eq!(liltoon_like.glitter.angle_randomize_factor, 1.0);
		assert_eq!(liltoon_like.glitter.vr_parallax_strength_factor, 0.4);
		assert_eq!(liltoon_like.dissolve.mask_texture_index, Some(42));
		assert_eq!(liltoon_like.dissolve.noise_mask_texture_index, Some(43));
		assert_eq!(liltoon_like.dissolve.color_factor, [1.2, 1.1, 1.0, 0.9]);
		assert_eq!(liltoon_like.dissolve.params_factor, [1.0, 0.0, 0.45, 0.12]);
		assert_eq!(liltoon_like.dissolve.position_factor, [0.25, 0.75, 0.0, 0.5]);
		assert_eq!(liltoon_like.dissolve.noise_strength_factor, 0.25);
		assert_eq!(liltoon_like.dissolve.noise_uv_scroll_rotate_factor, [0.01, 0.02, 0.03, 0.04]);
		assert_eq!(liltoon_like.parallax.texture_index, Some(44));
		assert_eq!(liltoon_like.parallax.enabled_factor, 1.0);
		assert_eq!(liltoon_like.parallax.pom_enabled_factor, 1.0);
		assert_eq!(liltoon_like.parallax.scale_factor, 0.07);
		assert_eq!(liltoon_like.parallax.offset_factor, 0.35);
		assert_eq!(liltoon_like.id_mask.compile_factor, 1.0);
		assert_eq!(liltoon_like.id_mask.from_factor, 8.0);
		assert_eq!(liltoon_like.id_mask.is_bitmap_factor, 1.0);
		assert_eq!(liltoon_like.id_mask.controls_dissolve_factor, 1.0);
		assert_eq!(liltoon_like.id_mask.flags_factor, [1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0]);
		assert_eq!(liltoon_like.id_mask.prior_flags_factor, [0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
		assert_eq!(liltoon_like.id_mask.indices_factor, [10, 20, 30, 40, 50, 60, 70, 80]);
		assert_eq!(liltoon_like.udim_discard.compile_factor, 1.0);
		assert_eq!(liltoon_like.udim_discard.mode_factor, 1.0);
		assert_eq!(liltoon_like.udim_discard.uv_factor, 2.0);
		assert_eq!(liltoon_like.udim_discard.row0_factor, [0.0, 1.0, 0.0, 0.0]);
		assert_eq!(liltoon_like.udim_discard.row2_factor, [0.0, 0.0, 0.0, 1.0]);
		assert_eq!(liltoon_like.emission.enabled_factor, 1.0);
		assert_eq!(liltoon_like.emission.color_factor, [0.5, 0.4, 0.3, 0.8]);
		assert_eq!(liltoon_like.emission.texture_index, Some(13));
		assert_eq!(liltoon_like.emission.main_strength_factor, 0.45);
		assert_eq!(liltoon_like.emission.blend_factor, 0.55);
		assert_eq!(liltoon_like.emission.blend_mode, UnaLilToonLikeBlendMode::Multiply);
		assert_eq!(liltoon_like.emission.gradation_enabled_factor, 1.0);
		assert_eq!(liltoon_like.emission.gradation_texture_index, Some(29));
		assert_eq!(liltoon_like.emission.gradation_speed_factor, 1.5);
		assert_eq!(liltoon_like.emission.second_enabled_factor, 1.0);
		assert_eq!(liltoon_like.emission.second_color_factor, [0.15, 0.25, 0.35, 0.45]);
		assert_eq!(liltoon_like.emission.second_texture_index, Some(34));
		assert_eq!(liltoon_like.emission.second_blend_mask_texture_index, Some(35));
		assert_eq!(liltoon_like.emission.second_gradation_texture_index, Some(36));
		assert_eq!(liltoon_like.emission.second_blend_factor, 0.64);
		assert_eq!(liltoon_like.emission.second_blend_mode, UnaLilToonLikeBlendMode::Screen);
		assert_eq!(liltoon_like.emission.second_main_strength_factor, 0.74);
		assert_eq!(liltoon_like.emission.second_gradation_enabled_factor, 1.0);
		assert_eq!(liltoon_like.emission.second_gradation_speed_factor, 2.5);
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
		assert_eq!(liltoon_like.fur.enabled_factor, 1.0);
		assert_eq!(liltoon_like.fur.layer_count_factor, 3.0);
		assert_eq!(liltoon_like.fur.vector_factor, [0.1, 0.2, 0.3, 0.4]);
		assert_eq!(liltoon_like.fur.vertex_color_to_vector_factor, 1.0);
		assert_eq!(liltoon_like.fur.vector_scale_factor, 1.75);
		assert_eq!(liltoon_like.fur.gravity_factor, 0.35);
		assert_eq!(liltoon_like.fur.shell_ao_factor, 0.6);
		assert_eq!(liltoon_like.fur.root_offset_factor, -0.35);
		assert_eq!(liltoon_like.fur.cutout_length_factor, 0.9);
		assert_eq!(liltoon_like.fur.randomize_factor, 0.45);
		assert_eq!(liltoon_like.fur.noise_tiling_factor, 2.0);
		assert_eq!(liltoon_like.fur.noise_offset_factor, 0.25);
		assert_eq!(liltoon_like.fur.rim_color_factor, [0.21, 0.31, 0.41, 0.51]);
		assert_eq!(liltoon_like.fur.rim_fresnel_power_factor, 4.5);
		assert_eq!(liltoon_like.fur.rim_anti_light_factor, 0.75);
		assert_eq!(liltoon_like.fur.vector_texture_index, Some(61));
		assert_eq!(liltoon_like.fur.length_mask_texture_index, Some(62));
		assert_eq!(liltoon_like.fur.noise_mask_texture_index, Some(63));
		assert_eq!(liltoon_like.fur.mask_texture_index, Some(64));
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
		assert_eq!(liltoon_like.blend_state.pre_cutoff_factor, 0.3);
		assert_eq!(liltoon_like.blend_state.pre_zwrite_factor, 0.0);
		assert_eq!(liltoon_like.blend_state.pre_cull_factor, 1.0);
		assert_eq!(liltoon_like.blend_state.alpha_to_mask_factor, 1.0);
		assert_eq!(mtoon.parametric_rim_color_factor, [0.040000003, 0.080000006, 0.120000005]);
		assert_eq!(mtoon.outline_color_factor, [0.01, 0.02, 0.03]);
	}

	#[test]
	fn source_alpha_mask_params_accept_authored_mode_without_keyword() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonRefraction",
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
	fn imports_liltoon_refraction_source_profile_and_params() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonRef",
			"floatParams": {
				"_RefractionFresnelPower": 0.75,
				"_RefractionStrength": -0.25,
				"_RefractionColorFromMain": 1.0
			},
			"colorParams": {
				"_RefractionColor": [0.8, 0.9, 1.0, 0.6]
			},
			"mtoon": {}
		});

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon refraction material");

		assert_eq!(liltoon_like.source_profile, UnaLilToonLikeSourceProfile::LiltoonRefraction);
		assert_eq!(liltoon_like.reflection.gem_refraction_fresnel_power_factor, 0.75);
		assert_eq!(liltoon_like.reflection.gem_refraction_strength_factor, -0.25);
		assert_eq!(liltoon_like.reflection.refraction_color_factor, [0.8, 0.9, 1.0, 0.6]);
		assert_eq!(liltoon_like.reflection.refraction_color_from_main_factor, 1.0);
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

	fn assert_vec3_near(actual: Vec3, expected: Vec3) {
		assert!(actual.abs_diff_eq(expected, 0.0001), "actual={actual:?} expected={expected:?}");
	}

	fn assert_quat_near(actual: Quat, expected: Quat) {
		assert!(
			actual.abs_diff_eq(expected, 0.0001) || actual.abs_diff_eq(-expected, 0.0001),
			"actual={actual:?} expected={expected:?}"
		);
	}

	fn test_node(children: Vec<usize>) -> UnaSceneNode {
		UnaSceneNode {
			name: None,
			source_node_id: None,
			resolved_node_id: None,
			visible: true,
			transform: Mat4::IDENTITY.to_cols_array(),
			children,
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		}
	}

	fn test_colored_primitive() -> UnaMeshBuffers {
		UnaMeshBuffers {
			name: None,
			positions: vec![[0.0, 0.0, 0.0]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: Some(vec![[0.25, 0.5, 0.75, 1.0]]),
			joints: None,
			weights: None,
			indices: None,
			material_index: None,
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		}
	}

	#[test]
	fn unavatar_path_diagnostics_reports_ambiguous_paths() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Item".to_string()),
					source_node_id: Some("node_item_a".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Item".to_string()),
					source_node_id: Some("node_item_b".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [{
					"nodeId": "node_item_ref",
					"path": "Root/Item"
				}]
			}),
		};
		let mut report = ImportReport::default();

		report_unavatar_path_diagnostics(&scene, &unavatar, &mut report);

		assert!(report.messages.iter().any(|message| {
			message.contains("exact_duplicate_paths=1")
				&& message.contains("normalized_ambiguous_paths=1")
				&& message.contains("registry_ambiguous_paths=1")
		}));

		scene.nodes[2].name = Some("Other".to_string());
		let mut report = ImportReport::default();
		report_unavatar_path_diagnostics(&scene, &unavatar, &mut report);
		assert!(!report.messages.iter().any(|message| message.contains(".unavatar path diagnostics")));
	}

	#[test]
	fn modular_avatar_bone_proxy_attachment_modes_match_processor_semantics() {
		let target_rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
		let old_rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_4);
		let target_world = Mat4::from_scale_rotation_translation(Vec3::new(2.0, 2.0, 2.0), target_rotation, Vec3::new(1.0, 2.0, 3.0));
		let old_world = Mat4::from_scale_rotation_translation(Vec3::new(0.5, 0.75, 1.25), old_rotation, Vec3::new(5.0, 7.0, 11.0));

		let at_root = target_world * bone_proxy_local_transform("AsChildAtRoot", false, target_world, old_world);
		let (_scale, rotation, translation) = decompose_finite(at_root);
		assert_vec3_near(translation, Vec3::new(1.0, 2.0, 3.0));
		assert_quat_near(rotation, target_rotation);

		let keep_position = target_world * bone_proxy_local_transform("AsChildKeepPosition", false, target_world, old_world);
		let (_scale, rotation, translation) = decompose_finite(keep_position);
		assert_vec3_near(translation, Vec3::new(5.0, 7.0, 11.0));
		assert_quat_near(rotation, target_rotation);

		let keep_rotation = target_world * bone_proxy_local_transform("AsChildKeepRotation", false, target_world, old_world);
		let (_scale, rotation, translation) = decompose_finite(keep_rotation);
		assert_vec3_near(translation, Vec3::new(1.0, 2.0, 3.0));
		assert_quat_near(rotation, old_rotation);

		let keep_world = target_world * bone_proxy_local_transform("AsChildKeepWorldPose", false, target_world, old_world);
		let (_scale, rotation, translation) = decompose_finite(keep_world);
		assert_vec3_near(translation, Vec3::new(5.0, 7.0, 11.0));
		assert_quat_near(rotation, old_rotation);
	}

	#[test]
	fn modular_avatar_bone_proxy_match_scale_forces_local_scale_one() {
		let target_world = Mat4::from_scale_rotation_translation(
			Vec3::new(2.0, 3.0, 4.0),
			Quat::from_rotation_x(std::f32::consts::FRAC_PI_4),
			Vec3::new(1.0, 2.0, 3.0),
		);
		let old_world = Mat4::from_scale_rotation_translation(Vec3::new(0.5, 0.75, 1.25), Quat::IDENTITY, Vec3::new(5.0, 7.0, 11.0));
		let local = bone_proxy_local_transform("AsChildKeepWorldPose", true, target_world, old_world);
		let (scale, _, _) = decompose_finite(local);
		assert_vec3_near(scale, Vec3::ONE);
	}

	#[test]
	fn modular_avatar_component_catalog_reports_support_classification() {
		let components = vec![
			serde_json::json!({"shortType": "ModularAvatarBoneProxy", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarMaterialSwap", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarMeshCutter", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarWorldFixedObject", "enabled": false}),
		];
		let mut report = ImportReport::default();
		report_unavatar_modular_avatar_component_catalog(&components, &mut report);

		let message = report
			.messages
			.iter()
			.find(|message| message.contains("Modular Avatar components"))
			.unwrap();
		assert!(message.contains("total=4"));
		assert!(message.contains("resolver_supported=1"));
		assert!(message.contains("runtime_action_supported=1"));
		assert!(message.contains("unsupported=2"));
		assert!(message.contains("disabled=1"));
		assert!(message.contains("ModularAvatarMeshCutter:1"));
		assert!(message.contains("ModularAvatarWorldFixedObject:1"));
		assert_eq!(report.lost_features.len(), 1);
		assert_eq!(report.lost_features[0].feature, "ModularAvatar.ModularAvatarMeshCutter");
		assert!(report.diagnostics.iter().any(|diagnostic| {
			diagnostic.severity == un_avatar_core::ReportSeverity::Warning && diagnostic.text.contains("ModularAvatarMeshCutter")
		}));
	}

	#[test]
	fn modular_avatar_remove_vertex_color_clones_shared_mesh_for_target_subtree() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Remove".to_string()),
					source_node_id: Some("node_remove".to_string()),
					resolved_node_id: None,
					children: vec![2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("TargetRenderer".to_string()),
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("OutsideRenderer".to_string()),
					mesh: Some(0),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![vec![test_colored_primitive()]],
			..Default::default()
		};
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarRemoveVertexColor",
			"enabled": true,
			"target": {"nodeId": "node_remove", "path": "Root/Remove"},
			"fields": {"Mode": "Remove"}
		})];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		let (nodes, primitives, missing, skipped) =
			apply_unavatar_remove_vertex_color(&mut scene, &components, &node_ids, &registry_paths, &paths, &normalized_paths);

		assert_eq!((nodes, primitives, missing, skipped), (1, 1, 0, 0));
		assert_eq!(scene.nodes[2].mesh, Some(1));
		assert_eq!(scene.nodes[3].mesh, Some(0));
		assert!(scene.meshes[0][0].colors_0.is_some());
		assert!(scene.meshes[1][0].colors_0.is_none());
	}

	#[test]
	fn modular_avatar_remove_vertex_color_honors_nearest_dont_remove() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Remove".to_string()),
					source_node_id: Some("node_remove".to_string()),
					resolved_node_id: None,
					children: vec![2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Keep".to_string()),
					source_node_id: Some("node_keep".to_string()),
					resolved_node_id: None,
					children: vec![3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Renderer".to_string()),
					mesh: Some(0),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![vec![test_colored_primitive()]],
			..Default::default()
		};
		let components = vec![
			serde_json::json!({
				"shortType": "ModularAvatarRemoveVertexColor",
				"enabled": true,
				"target": {"nodeId": "node_remove", "path": "Root/Remove"},
				"fields": {"Mode": "Remove"}
			}),
			serde_json::json!({
				"shortType": "ModularAvatarRemoveVertexColor",
				"enabled": true,
				"target": {"nodeId": "node_keep", "path": "Root/Remove/Keep"},
				"fields": {"Mode": 1}
			}),
		];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		let (nodes, primitives, missing, skipped) =
			apply_unavatar_remove_vertex_color(&mut scene, &components, &node_ids, &registry_paths, &paths, &normalized_paths);

		assert_eq!((nodes, primitives, missing, skipped), (0, 0, 0, 0));
		assert!(scene.meshes[0][0].colors_0.is_some());
	}

	#[test]
	fn modular_avatar_apply_reports_remove_vertex_color_and_catalog() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Renderer".to_string()),
					source_node_id: Some("node_renderer".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![vec![test_colored_primitive()]],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [{
						"shortType": "ModularAvatarRemoveVertexColor",
						"enabled": true,
						"target": {"nodeId": "node_renderer", "path": "Root/Renderer"},
						"fields": {"Mode": "Remove"}
					}]
				}
			}),
		};
		let mut report = ImportReport::default();

		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		assert!(scene.meshes[0][0].colors_0.is_none());
		assert!(report.messages.iter().any(|message| {
			message.contains("Modular Avatar components") && message.contains("resolver_supported=1") && message.contains("unsupported=0")
		}));
		assert!(report
			.messages
			.iter()
			.any(|message| { message.contains("remove_vertex_color_nodes=1") && message.contains("remove_vertex_color_primitives=1") }));
	}

	#[test]
	fn modular_avatar_bone_proxy_renames_duplicate_target_child() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Head".to_string()),
					source_node_id: Some("node_head".to_string()),
					resolved_node_id: None,
					children: vec![2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Proxy".to_string()),
					source_node_id: Some("node_existing_proxy".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Proxy".to_string()),
					source_node_id: Some("node_proxy".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
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
							"attachmentMode": "AsChildAtRoot",
							"matchScale": false
						}
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		assert_eq!(scene.nodes[1].children, vec![2, 3]);
		assert_eq!(scene.nodes[3].name.as_deref(), Some("Proxy (1)"));
		assert!(report.messages.iter().any(|m| m.contains("bone_proxy_applied=1")));
	}

	#[test]
	fn modular_avatar_bone_proxy_reports_missing_target() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Proxy".to_string()),
					source_node_id: Some("node_proxy".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
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
						"resolvedTarget": {"nodeId": "node_missing", "path": "Missing"},
						"fields": {
							"attachmentMode": "AsChildAtRoot",
							"matchScale": false
						}
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		assert_eq!(scene.nodes[0].children, vec![1]);
		assert!(report.messages.iter().any(|m| m.contains("bone_proxy_missing=1")));
	}

	#[test]
	fn modular_avatar_replace_object_moves_children_and_remaps_node_references() {
		let mut scene = UnaSceneSnapshot {
			skins: vec![UnaSkin {
				joint_nodes: vec![1, 3],
				inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array(), Mat4::IDENTITY.to_cols_array()],
				skeleton_node: Some(1),
			}],
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2, 5],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Original".to_string()),
					source_node_id: Some("node_original".to_string()),
					resolved_node_id: None,
					transform: Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)).to_cols_array(),
					children: vec![3],
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Replacement".to_string()),
					source_node_id: Some("node_replacement".to_string()),
					resolved_node_id: None,
					transform: Mat4::from_translation(Vec3::new(5.0, 0.0, 0.0)).to_cols_array(),
					children: vec![4],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("OriginalChild".to_string()),
					source_node_id: Some("node_original_child".to_string()),
					resolved_node_id: None,
					transform: Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)).to_cols_array(),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("ReplacementChild".to_string()),
					source_node_id: Some("node_replacement_child".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("ProbeUser".to_string()),
					probe_anchor_node: Some(1),
					..test_node(Vec::new())
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
					"components": [{
						"shortType": "ModularAvatarReplaceObject",
						"enabled": true,
						"target": {"nodeId": "node_replacement", "path": "Replacement"},
						"fields": {
							"targetObject": {"nodeId": "node_original", "path": "Original"}
						}
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);
		let after = scene_world_matrices(&scene);

		assert_eq!(scene.nodes[0].children, vec![2, 5]);
		assert_eq!(scene.nodes[2].children, vec![4, 3]);
		assert_eq!(scene.nodes[2].source_node_id.as_deref(), Some("node_replacement"));
		assert_eq!(
			scene.nodes[2].resolved_node_id.as_deref(),
			Some("ma:replace_object:node_original:node_replacement")
		);
		assert!(scene.nodes[1].children.is_empty());
		assert!(!scene.nodes[1].visible);
		assert!((after[2].transform_point3(Vec3::ZERO) - before[2].transform_point3(Vec3::ZERO)).length() < 1e-5);
		assert!((after[3].transform_point3(Vec3::ZERO) - before[3].transform_point3(Vec3::ZERO)).length() < 1e-5);
		assert_eq!(scene.skins[0].joint_nodes, vec![2, 3]);
		assert_eq!(scene.skins[0].skeleton_node, Some(2));
		assert_eq!(scene.nodes[5].probe_anchor_node, Some(2));
		assert!(report.messages.iter().any(|m| m.contains("replace_object_applied=1")));
	}

	#[test]
	fn modular_avatar_replace_object_reports_invalid_parent_target() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("OriginalParent".to_string()),
					source_node_id: Some("node_original".to_string()),
					resolved_node_id: None,
					children: vec![2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("ReplacementChild".to_string()),
					source_node_id: Some("node_replacement".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
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
						"shortType": "ModularAvatarReplaceObject",
						"enabled": true,
						"target": {"nodeId": "node_replacement", "path": "OriginalParent/ReplacementChild"},
						"fields": {
							"targetObject": {"nodeId": "node_original", "path": "OriginalParent"}
						}
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		assert_eq!(scene.nodes[0].children, vec![1]);
		assert_eq!(scene.nodes[1].children, vec![2]);
		assert!(scene.nodes[1].visible);
		assert!(report.messages.iter().any(|m| m.contains("replace_object_invalid=1")));
	}

	#[test]
	fn modular_avatar_replace_object_reports_missing_target_object() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Replacement".to_string()),
					source_node_id: Some("node_replacement".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
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
						"shortType": "ModularAvatarReplaceObject",
						"enabled": true,
						"target": {"nodeId": "node_replacement", "path": "Replacement"},
						"fields": {
							"targetObject": {"nodeId": "node_missing", "path": "Missing"}
						}
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		assert_eq!(scene.nodes[0].children, vec![1]);
		assert!(report.messages.iter().any(|m| m.contains("replace_object_missing=1")));
	}

	#[test]
	fn modular_avatar_bone_proxy_reparents_keep_world_pose() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					source_node_id: None,
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
	fn modular_avatar_merge_armature_reparents_auxiliary_bones() {
		let target_world = Mat4::from_translation(Vec3::new(0.0, 3.0, 0.0));
		let source_world = Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0));
		let aux_local = Mat4::from_translation(Vec3::new(0.0, 0.0, 5.0));
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					source_node_id: None,
					resolved_node_id: None,
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
					source_node_id: Some("node_target_head".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: target_world.to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Head".to_string()),
					source_node_id: Some("node_source_head".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: source_world.to_cols_array(),
					children: vec![3],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("HatRoot".to_string()),
					source_node_id: Some("node_hat_root".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: aux_local.to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			skins: vec![UnaSkin {
				joint_nodes: vec![3],
				inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array()],
				skeleton_node: Some(3),
			}],
			roots: vec![0],
			..Default::default()
		};
		let before = scene_world_matrices(&scene);
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [{
						"shortType": "ModularAvatarMergeArmature",
						"enabled": true,
						"target": {"nodeId": "node_source_head", "path": "Outfit/Armature/Head"},
						"boneMappings": [{
							"sourceBone": {"nodeId": "node_source_head", "path": "Outfit/Armature/Head"},
							"targetBone": {"nodeId": "node_target_head", "path": "Armature/Head"}
						}]
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);
		let after = scene_world_matrices(&scene);

		assert_eq!(scene.nodes[1].children, vec![3]);
		assert_eq!(scene.nodes[2].children, Vec::<usize>::new());
		assert_eq!(after[3].transform_point3(Vec3::ZERO), before[3].transform_point3(Vec3::ZERO));
		assert_eq!(scene.skins[0].joint_nodes, vec![3]);
		assert_eq!(scene.skins[0].skeleton_node, Some(3));
		assert!(report.messages.iter().any(|m| {
			m.contains("merge_armature_mappings=1")
				&& m.contains("mesh_retargeter_joints=0")
				&& m.contains("merge_armature_auxiliary_bones=1")
		}));
	}

	#[test]
	fn same_name_humanoid_armature_fallback_retargets_skin_bindposes() {
		let main_hips_world = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0));
		let outfit_hips_world = Mat4::from_translation(Vec3::new(3.0, 1.0, 0.0));
		let outfit_aux_local = Mat4::from_translation(Vec3::new(0.0, 2.0, 0.0));
		let old_bind = Mat4::from_translation(Vec3::new(-3.0, -1.0, 0.0));
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Armature".to_string()),
					source_node_id: None,
					resolved_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![1, 2],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Hips".to_string()),
					source_node_id: Some("main_hips".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: main_hips_world.to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("OutfitArmature".to_string()),
					source_node_id: None,
					resolved_node_id: None,
					visible: true,
					transform: Mat4::IDENTITY.to_cols_array(),
					children: vec![3],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("Hips".to_string()),
					source_node_id: Some("outfit_hips".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: outfit_hips_world.to_cols_array(),
					children: vec![4],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("ShirtRoot".to_string()),
					source_node_id: Some("outfit_shirt_root".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: outfit_aux_local.to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			skins: vec![UnaSkin {
				joint_nodes: vec![3],
				inverse_bind_matrices: vec![old_bind.to_cols_array()],
				skeleton_node: Some(3),
			}],
			roots: vec![0],
			..Default::default()
		};
		let before = scene_world_matrices(&scene);
		let humanoid = HumanoidProfile {
			bone_node_indices: BTreeMap::from([("hips".to_string(), 1)]),
		};

		let (mappings, retargeted, auxiliary_reparented) = retarget_same_name_humanoid_armature_skins(&mut scene, &humanoid);
		let after = scene_world_matrices(&scene);

		assert_eq!(mappings, 1);
		assert_eq!(retargeted, 1);
		assert_eq!(auxiliary_reparented, 1);
		assert_eq!(scene.nodes[1].children, vec![4]);
		assert_eq!(scene.nodes[3].children, Vec::<usize>::new());
		assert_eq!(after[4].transform_point3(Vec3::ZERO), before[4].transform_point3(Vec3::ZERO));
		assert_eq!(scene.skins[0].joint_nodes, vec![1]);
		assert_eq!(scene.skins[0].skeleton_node, Some(1));
		let expected = main_hips_world.inverse() * outfit_hips_world * old_bind;
		let actual = Mat4::from_cols_array(&scene.skins[0].inverse_bind_matrices[0]);
		for (a, e) in actual.to_cols_array().iter().zip(expected.to_cols_array()) {
			assert!((a - e).abs() < 0.0001, "actual={actual:?} expected={expected:?}");
		}
	}

	#[test]
	fn modular_avatar_mesh_settings_sets_skin_skeleton_node() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					source_node_id: None,
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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
					resolved_node_id: None,
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

	#[test]
	fn wardrobe_dynamics_enable_updates_runtime_group() {
		let mut doc = UnaDocument {
			scene: Some(UnaSceneSnapshot::default()),
			unavatar: Some(UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"wardrobe": {
						"baseSet": "base",
						"sets": [{
							"id": "base",
							"assetGroups": ["avatar:base"],
							"operations": []
						}, {
							"id": "no_hair_physics",
							"assetGroups": ["outfit:hair", "physics:hair", "outfit:hair"],
							"operations": [
								{"type": "dynamicsEnable", "target": {"dynamicsId": "physbone:hair"}, "enabled": false},
								{"type": "dynamicsEnable", "target": {"dynamicsId": "physbone:missing"}, "enabled": false}
							]
						}]
					}
				}),
			}),
			spring_bones: Some(UnaSpringBoneSettings {
				groups: vec![UnaSpringBoneGroup {
					source_kind: UnaDynamicsSourceKind::VrmSpringBone,
					enabled: true,
					source_id: "physbone:hair".to_string(),
					bone_node_indices: vec![0, 1],
					..Default::default()
				}],
				colliders: Vec::new(),
			}),
			..Default::default()
		};

		let applied = apply_unavatar_wardrobe_set(&mut doc, "no_hair_physics").expect("apply wardrobe");
		assert_eq!(
			applied.active_asset_groups,
			vec!["outfit:hair".to_string(), "physics:hair".to_string()]
		);
		assert_eq!(applied.dynamics_applied, 1);
		assert_eq!(applied.dynamics_missing, 1);
		assert_eq!(applied.missing_dynamics_ids, vec!["physbone:missing"]);
		assert!(!doc.spring_bones.as_ref().unwrap().groups[0].enabled);
		assert_eq!(doc.runtime_model().active_wardrobe_set(), Some("no_hair_physics"));
		assert_eq!(
			doc.runtime_model().active_asset_groups(),
			&["outfit:hair".to_string(), "physics:hair".to_string()]
		);

		let applied = apply_unavatar_wardrobe_set(&mut doc, "base").expect("apply base wardrobe");
		assert_eq!(applied.active_asset_groups, vec!["avatar:base".to_string()]);
		assert_eq!(applied.dynamics_applied, 0);
		assert_eq!(applied.dynamics_missing, 0);
		assert!(doc.spring_bones.as_ref().unwrap().groups[0].enabled);
		assert_eq!(doc.runtime_model().active_wardrobe_set(), Some("base"));
		assert_eq!(doc.runtime_model().active_asset_groups(), &["avatar:base".to_string()]);
	}
}
