//! glTF 2.0 インポート（静的メッシュ + スキニング。Morph・スパースアクセサは読み飛ばし／レポート記録）。
//!
//! 設計正本: `docs/development-plan.md` Commit 1.3〜1.4

#![forbid(unsafe_code)]
#![recursion_limit = "256"]

use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::hash::Hasher;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use exr::prelude::{f16, pixel_vec::PixelVec, read, ReadChannels, ReadLayers};
use glam::{Mat4, Quat, Vec3};
use serde_json::Value;
use un_avatar_core::{
	apply_runtime_material_color, apply_runtime_material_scalar, modular_avatar_component_support_kind,
	unavatar_modular_avatar_components_slice, unavatar_modular_avatar_value, Approximation, ReportStatus, UnaAlphaMode, UnaBounds,
	UnaCullMode, UnaDocument, UnaDynamicsCollider, UnaDynamicsColliderShape, UnaDynamicsConstraintRef, UnaDynamicsContact,
	UnaDynamicsContactKind, UnaDynamicsImmobileType, UnaDynamicsIntegrationType, UnaDynamicsInteraction, UnaDynamicsLimit,
	UnaDynamicsSettings, UnaDynamicsSourceKind, UnaDynamicsWritebackMode, UnaExpressionCatalog, UnaExpressionPreset, UnaExpressionWeights,
	UnaImagePixelFormat, UnaImageRgba, UnaImageSourceMetadata, UnaLilToonLikeBlendMode, UnaLilToonLikeMaterial,
	UnaLilToonLikeSourceProfile, UnaMaterialPbr, UnaMeshBuffers, UnaMeshPrimitiveKey, UnaMorphTargetBind, UnaMorphTargetDeltas,
	UnaMtoonMaterial, UnaMtoonOutlineWidthMode, UnaNodeConstraint, UnaNodeConstraintKind, UnaNodeConstraintSource, UnaRuntimeAction,
	UnaRuntimeActionCondition, UnaRuntimeActionEffect, UnaRuntimeActionSet, UnaRuntimeActionTrigger, UnaRuntimeDynamicsMut,
	UnaRuntimeMaterialSlotTarget, UnaRuntimeMaterialTarget, UnaRuntimeNodeTarget, UnaSceneAssetGroupOwnership, UnaSceneNode,
	UnaSceneSnapshot, UnaShadingModel, UnaSkin, UnaSpringBoneGroup, UnaSpringBoneSettings, UnaTextureFilterMode, UnaTextureSampler,
	UnaTextureWrapMode, UnaUnavatarExtension,
};
use un_avatar_io::{
	AvatarImporter, Capability, FormatCapabilities, FormatDescriptor, FormatDirection, FormatId, ImportContext, ImportError, ImportInput,
	ImportOptions, ImportProbe, ImportProbeResult, ImportReport, ImportResult, IoRegistry, PluginStability,
};
use un_avatar_types::HumanoidProfile;

/// glTF スキン 1 本あたりの joint 上限（レンダラのボーンパレット上限と揃える）。
const MAX_SKIN_JOINTS: usize = 512;
const UN_AVATAR_EXTENSION_NAME: &str = "UN_avatar";
const MAX_UNANIMATOR_ACTIONS: usize = 1024;
const MAX_UNANIMATOR_EFFECTS_PER_ACTION: usize = 16;
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

fn placeholder_deferred_image() -> UnaImageRgba {
	UnaImageRgba {
		width: 0,
		height: 0,
		pixel_format: UnaImagePixelFormat::R8G8B8A8,
		pixels: Vec::new(),
	}
}

fn is_deferred_image_placeholder(image: &UnaImageRgba) -> bool {
	image.width == 0 && image.height == 0 && image.pixels.is_empty()
}

fn image_format_from_mime_type(mime_type: Option<&str>) -> Option<image::ImageFormat> {
	match mime_type {
		Some("image/png") => Some(image::ImageFormat::Png),
		Some("image/jpeg") | Some("image/jpg") => Some(image::ImageFormat::Jpeg),
		Some("image/webp") => Some(image::ImageFormat::WebP),
		Some("image/x-exr") | Some("image/exr") => Some(image::ImageFormat::OpenExr),
		Some("image/vnd.radiance") | Some("image/hdr") => Some(image::ImageFormat::Hdr),
		_ => None,
	}
}

fn encoded_image_dimensions(bytes: &[u8], mime_type: Option<&str>) -> (Option<u32>, Option<u32>) {
	let result = if let Some(format) = image_format_from_mime_type(mime_type) {
		image::ImageReader::with_format(Cursor::new(bytes), format).into_dimensions()
	} else {
		match image::ImageReader::new(Cursor::new(bytes)).with_guessed_format() {
			Ok(reader) => reader.into_dimensions(),
			Err(_) => return (None, None),
		}
	};
	result.map(|(width, height)| (Some(width), Some(height))).unwrap_or((None, None))
}

fn retain_encoded_bytes_for_deferred_images(image_sources: &mut [Option<UnaImageSourceMetadata>], images: &[UnaImageRgba]) -> usize {
	let mut retained = 0usize;
	for (index, source) in image_sources.iter_mut().enumerate() {
		let keep_encoded = images.get(index).is_some_and(is_deferred_image_placeholder);
		let Some(source) = source else {
			continue;
		};
		if keep_encoded {
			retained += usize::from(source.encoded_bytes.is_some());
		} else {
			source.encoded_bytes = None;
		}
	}
	retained
}

fn path_backed_deferred_image_source_count(image_sources: &[Option<UnaImageSourceMetadata>], images: &[UnaImageRgba]) -> usize {
	image_sources
		.iter()
		.enumerate()
		.filter(|(index, source)| {
			images.get(*index).is_some_and(is_deferred_image_placeholder)
				&& source.as_ref().is_some_and(|source| {
					source.source_file_path.is_some()
						&& source.byte_offset.is_some()
						&& source.byte_length > 0
						&& source.encoded_bytes.is_none()
				})
		})
		.count()
}

fn collect_scene_images_from_imported_data(
	images_data: Vec<Option<gltf::image::Data>>,
	report: &mut ImportReport,
) -> Result<Vec<UnaImageRgba>, String> {
	let mut out = Vec::with_capacity(images_data.len());
	let mut deferred = 0usize;
	for (index, d) in images_data.into_iter().enumerate() {
		if let Some(d) = d {
			let (image, approximation) = from_gltf_image(d)?;
			if let Some(detail) = approximation {
				report.approximations.push(Approximation {
					feature: format!("image[{index}].pixel_format"),
					detail: Some(detail),
				});
			}
			out.push(image);
		} else {
			deferred += 1;
			out.push(placeholder_deferred_image());
		}
	}
	if deferred > 0 {
		report.push_info(format!("glTF import profile: deferred_image_decode_count={deferred}"));
	}
	Ok(out)
}

#[derive(Clone, Copy, Debug, Default)]
struct GltfSliceImportProfile {
	parse_ms: u128,
	buffers_ms: u128,
	image_decode_ms: u128,
	image_count: usize,
	decoded_image_count: usize,
	image_decode_workers: usize,
}

type GltfSliceImport = (
	gltf::Document,
	Vec<gltf::buffer::Data>,
	Vec<Option<gltf::image::Data>>,
	GltfSliceImportProfile,
);

fn import_gltf_slice_parallel_images(slice: &[u8], decode_image_indices: Option<&BTreeSet<usize>>) -> Result<GltfSliceImport, ImportError> {
	let parse_started = Instant::now();
	let gltf = gltf::Gltf::from_slice(slice).map_err(|e| ImportError::Message(e.to_string()))?;
	let parse_ms = parse_started.elapsed().as_millis();
	let document = gltf.document;
	let buffers_started = Instant::now();
	let buffers = gltf::import_buffers(&document, None, gltf.blob).map_err(|e| ImportError::Message(e.to_string()))?;
	let buffers_ms = buffers_started.elapsed().as_millis();
	let document_images = document.images().collect::<Vec<_>>();
	let image_count = document_images.len();
	if image_count == 0 {
		return Ok((
			document,
			buffers,
			Vec::new(),
			GltfSliceImportProfile {
				parse_ms,
				buffers_ms,
				..Default::default()
			},
		));
	}
	let decode_count = decode_image_indices.map_or(image_count, BTreeSet::len).min(image_count);
	if decode_count == 0 {
		return Ok((
			document,
			buffers,
			vec![None; image_count],
			GltfSliceImportProfile {
				parse_ms,
				buffers_ms,
				image_count,
				..Default::default()
			},
		));
	}

	let worker_count = std::thread::available_parallelism()
		.map(|n| n.get())
		.unwrap_or(1)
		.clamp(1, 8)
		.min(decode_count);
	let decode_indices = decode_image_indices
		.map(|indices| indices.iter().copied().filter(|index| *index < image_count).collect::<Vec<_>>())
		.unwrap_or_else(|| (0..image_count).collect());
	let chunk_size = decode_indices.len().div_ceil(worker_count);
	let image_decode_started = Instant::now();
	let decoded_chunks = std::thread::scope(|scope| {
		let mut handles = Vec::with_capacity(worker_count);
		for chunk in decode_indices.chunks(chunk_size) {
			let indices = chunk.to_vec();
			let document_images = &document_images;
			let buffers = &buffers;
			handles.push(scope.spawn(move || {
				let mut decoded = Vec::with_capacity(indices.len());
				for index in indices {
					let Some(image) = document_images.get(index) else {
						continue;
					};
					let data = gltf::image::Data::from_source(image.source(), None, buffers).map_err(|e| e.to_string())?;
					decoded.push((index, data));
				}
				Ok::<Vec<(usize, gltf::image::Data)>, String>(decoded)
			}));
		}

		let mut decoded_chunks = Vec::with_capacity(handles.len());
		for handle in handles {
			decoded_chunks.push(handle.join().map_err(|_| "glTF image decode worker panicked".to_owned())??);
		}
		Ok::<Vec<Vec<(usize, gltf::image::Data)>>, String>(decoded_chunks)
	})
	.map_err(ImportError::Message)?;

	let mut images = vec![None; image_count];
	for decoded in decoded_chunks {
		for (index, data) in decoded {
			images[index] = Some(data);
		}
	}
	Ok((
		document,
		buffers,
		images,
		GltfSliceImportProfile {
			parse_ms,
			buffers_ms,
			image_decode_ms: image_decode_started.elapsed().as_millis(),
			image_count,
			decoded_image_count: decode_count,
			image_decode_workers: worker_count,
		},
	))
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
					let (width, height) = encoded_image_dimensions(bytes, Some(mime_type));
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
						width,
						height,
						byte_offset: Some(start as u64),
						byte_length: bytes.len() as u64,
						source_hash: source_hash64(bytes),
						source_file_path: None,
						encoded_bytes: None,
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
					width: None,
					height: None,
					byte_offset: None,
					byte_length: 0,
					source_hash: source_hash64(uri.as_bytes()),
					source_file_path: None,
					encoded_bytes: None,
				}),
			}
		})
		.collect()
}

fn collect_glb_image_source_metadata(
	root: &Value,
	bin: &[u8],
	retain_encoded_indices: Option<&BTreeSet<usize>>,
	source_file_path: Option<&Path>,
	byte_offset_base: u64,
) -> Vec<Option<UnaImageSourceMetadata>> {
	collect_glb_image_source_metadata_inner(root, bin, retain_encoded_indices, source_file_path, byte_offset_base, None)
}

#[derive(Default)]
struct GlbImageSourceMetadataProfile {
	dimensions_ns: AtomicU64,
	hash_ns: AtomicU64,
	hash_bytes: AtomicU64,
}

impl GlbImageSourceMetadataProfile {
	fn record_dimensions(&self, elapsed: Duration) {
		self.dimensions_ns.fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
	}

	fn record_hash(&self, elapsed: Duration, bytes: usize) {
		self.hash_ns.fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
		self.hash_bytes.fetch_add(bytes as u64, Ordering::Relaxed);
	}

	fn dimensions_ms(&self) -> f64 {
		self.dimensions_ns.load(Ordering::Relaxed) as f64 / 1_000_000.0
	}

	fn hash_ms(&self) -> f64 {
		self.hash_ns.load(Ordering::Relaxed) as f64 / 1_000_000.0
	}

	fn hash_mb(&self) -> f64 {
		self.hash_bytes.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0)
	}
}

fn collect_glb_image_source_metadata_profiled(
	root: &Value,
	bin: &[u8],
	retain_encoded_indices: Option<&BTreeSet<usize>>,
	source_file_path: Option<&Path>,
	byte_offset_base: u64,
) -> (Vec<Option<UnaImageSourceMetadata>>, GlbImageSourceMetadataProfile) {
	let profile = GlbImageSourceMetadataProfile::default();
	let metadata = collect_glb_image_source_metadata_inner(
		root,
		bin,
		retain_encoded_indices,
		source_file_path,
		byte_offset_base,
		Some(&profile),
	);
	(metadata, profile)
}

fn collect_glb_image_source_metadata_inner(
	root: &Value,
	bin: &[u8],
	retain_encoded_indices: Option<&BTreeSet<usize>>,
	source_file_path: Option<&Path>,
	byte_offset_base: u64,
	profile: Option<&GlbImageSourceMetadataProfile>,
) -> Vec<Option<UnaImageSourceMetadata>> {
	let Some(images) = root.get("images").and_then(Value::as_array) else {
		return Vec::new();
	};
	let buffer_views = root.get("bufferViews").and_then(Value::as_array);
	let samplers = image_samplers_from_root_json(root);
	let image_count = images.len();
	if image_count == 0 {
		return Vec::new();
	}
	let worker_count = std::thread::available_parallelism()
		.map(|n| n.get())
		.unwrap_or(1)
		.clamp(1, 8)
		.min(image_count);
	let chunk_size = image_count.div_ceil(worker_count);
	let chunks = std::thread::scope(|scope| {
		let mut handles = Vec::with_capacity(worker_count);
		for start in (0..image_count).step_by(chunk_size) {
			let end = (start + chunk_size).min(image_count);
			let samplers = &samplers;
			handles.push(scope.spawn(move || {
				let mut out = Vec::with_capacity(end - start);
				for (image_index, image) in images.iter().enumerate().take(end).skip(start) {
					let retain_encoded = retain_encoded_indices.is_some_and(|indices| indices.contains(&image_index));
					out.push((
						image_index,
						glb_image_source_metadata_from_json_image(GlbJsonImageSourceMetadataInput {
							image_index,
							image,
							buffer_views,
							bin,
							samplers,
							retain_encoded,
							source_file_path,
							byte_offset_base,
							profile,
						}),
					));
				}
				out
			}));
		}
		handles
			.into_iter()
			.map(|handle| handle.join().expect("GLB image source metadata worker panicked"))
			.collect::<Vec<_>>()
	});
	let mut out = vec![None; image_count];
	for chunk in chunks {
		for (image_index, metadata) in chunk {
			out[image_index] = metadata;
		}
	}
	out
}

struct GlbJsonImageSourceMetadataInput<'a> {
	image_index: usize,
	image: &'a Value,
	buffer_views: Option<&'a Vec<Value>>,
	bin: &'a [u8],
	samplers: &'a [Option<UnaTextureSampler>],
	retain_encoded: bool,
	source_file_path: Option<&'a Path>,
	byte_offset_base: u64,
	profile: Option<&'a GlbImageSourceMetadataProfile>,
}

fn glb_image_source_metadata_from_json_image(input: GlbJsonImageSourceMetadataInput<'_>) -> Option<UnaImageSourceMetadata> {
	let GlbJsonImageSourceMetadataInput {
		image_index,
		image,
		buffer_views,
		bin,
		samplers,
		retain_encoded,
		source_file_path,
		byte_offset_base,
		profile,
	} = input;
	let sampler = samplers.get(image_index).copied().flatten();
	let name = image.get("name").and_then(Value::as_str).map(str::to_string);
	let mime_type = image.get("mimeType").and_then(Value::as_str).map(str::to_string);
	let image_metadata = unavatar_image_metadata_from_image_json(image);
	if let Some(uri) = image.get("uri").and_then(Value::as_str) {
		return Some(UnaImageSourceMetadata {
			name,
			mime_type,
			uri: Some(uri.to_string()),
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
			width: None,
			height: None,
			byte_offset: None,
			byte_length: 0,
			source_hash: source_hash64(uri.as_bytes()),
			source_file_path: None,
			encoded_bytes: None,
		});
	}
	let view_index = image.get("bufferView").and_then(Value::as_u64)? as usize;
	let view = buffer_views?.get(view_index)?;
	let offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
	let length = view.get("byteLength").and_then(Value::as_u64)? as usize;
	let bytes = bin.get(offset..offset.checked_add(length)?)?;
	let dimensions_started = Instant::now();
	let (width, height) = encoded_image_dimensions(bytes, mime_type.as_deref());
	if let Some(profile) = profile {
		profile.record_dimensions(dimensions_started.elapsed());
	}
	let encoded_bytes = (retain_encoded && source_file_path.is_none()).then(|| bytes.to_vec());
	let hash_started = Instant::now();
	let source_hash = source_hash64(bytes);
	if let Some(profile) = profile {
		profile.record_hash(hash_started.elapsed(), bytes.len());
	}
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
		width,
		height,
		byte_offset: Some(byte_offset_base.saturating_add(offset as u64)),
		byte_length: bytes.len() as u64,
		source_hash,
		source_file_path: source_file_path.map(Path::to_path_buf),
		encoded_bytes,
	})
}

#[cfg(test)]
fn collect_glb_image_source_metadata_serial(
	root: &Value,
	bin: &[u8],
	retain_encoded_indices: Option<&BTreeSet<usize>>,
) -> Vec<Option<UnaImageSourceMetadata>> {
	let Some(images) = root.get("images").and_then(Value::as_array) else {
		return Vec::new();
	};
	let buffer_views = root.get("bufferViews").and_then(Value::as_array);
	let samplers = image_samplers_from_root_json(root);
	images
		.iter()
		.enumerate()
		.map(|(image_index, image)| {
			let retain_encoded = retain_encoded_indices.is_some_and(|indices| indices.contains(&image_index));
			glb_image_source_metadata_from_json_image(GlbJsonImageSourceMetadataInput {
				image_index,
				image,
				buffer_views,
				bin,
				samplers: &samplers,
				retain_encoded,
				source_file_path: None,
				byte_offset_base: 0,
				profile: None,
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
		wrap_s: wrap_from_sampler_json(value, "wrapS", "unityWrapModeU"),
		wrap_t: wrap_from_sampler_json(value, "wrapT", "unityWrapModeV"),
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

fn wrap_from_sampler_json(value: &Value, gltf_key: &str, unity_key: &str) -> UnaTextureWrapMode {
	match value.get(unity_key).and_then(Value::as_str) {
		Some("MirrorOnce") | Some("mirror_once") => UnaTextureWrapMode::MirrorOnce,
		Some("Mirror") | Some("mirror") => UnaTextureWrapMode::MirroredRepeat,
		Some("Clamp") | Some("clamp") => UnaTextureWrapMode::ClampToEdge,
		Some("Repeat") | Some("repeat") => UnaTextureWrapMode::Repeat,
		_ => wrap_from_gltf_constant(value.get(gltf_key).and_then(Value::as_u64)),
	}
}

fn append_unavatar_texture_assets(
	scene: &mut UnaSceneSnapshot,
	root: &Value,
	bin: &[u8],
	report: &mut ImportReport,
) -> BTreeMap<String, usize> {
	let started = Instant::now();
	let mut map = BTreeMap::new();
	let Some(assets) = unavatar_texture_assets(root) else {
		return map;
	};
	let mut source_bytes = 0u64;
	let mut decoded_pixels = 0u64;
	for asset in assets {
		let id = asset.get("id").and_then(Value::as_str).unwrap_or("");
		if id.is_empty() {
			continue;
		}
		let Some(bytes) = texture_asset_bytes(root, bin, asset) else {
			report.lost_features.push(un_avatar_core::LostFeature {
				feature: format!("UN_avatar.textureAssets[{id}]"),
				detail: Some("missing or invalid bufferView".to_string()),
			});
			continue;
		};
		append_unavatar_texture_asset(
			scene,
			report,
			&mut map,
			&mut source_bytes,
			&mut decoded_pixels,
			asset,
			bytes,
			None,
			None,
			true,
		);
	}
	report.push_info(format!(
		".unavatar textureAssets: decoded={} source_bytes={} decoded_pixels={} decode_ms={}",
		map.len(),
		source_bytes,
		decoded_pixels,
		started.elapsed().as_millis()
	));
	map
}

fn append_unavatar_texture_assets_from_file(
	scene: &mut UnaSceneSnapshot,
	root: &Value,
	source_file_path: &Path,
	bin_offset: u64,
	report: &mut ImportReport,
) -> BTreeMap<String, usize> {
	let started = Instant::now();
	let mut map = BTreeMap::new();
	let Some(assets) = unavatar_texture_assets(root) else {
		return map;
	};
	let mut file = match File::open(source_file_path) {
		Ok(file) => file,
		Err(error) => {
			report.lost_features.push(un_avatar_core::LostFeature {
				feature: "UN_avatar.textureAssets".to_string(),
				detail: Some(format!("source file open: {error}")),
			});
			return map;
		}
	};
	let mut source_bytes = 0u64;
	let mut decoded_pixels = 0u64;
	for asset in assets {
		let id = asset.get("id").and_then(Value::as_str).unwrap_or("");
		if id.is_empty() {
			continue;
		}
		let Some(range) = texture_asset_bin_range(root, asset) else {
			report.lost_features.push(un_avatar_core::LostFeature {
				feature: format!("UN_avatar.textureAssets[{id}]"),
				detail: Some("missing or invalid bufferView".to_string()),
			});
			continue;
		};
		let byte_offset = bin_offset + range.start as u64;
		let mut bytes = vec![0; range.len()];
		if let Err(error) = file.seek(SeekFrom::Start(byte_offset)).and_then(|_| file.read_exact(&mut bytes)) {
			report.lost_features.push(un_avatar_core::LostFeature {
				feature: format!("UN_avatar.textureAssets[{id}]"),
				detail: Some(format!("source bytes read: {error}")),
			});
			continue;
		}
		append_unavatar_texture_asset(
			scene,
			report,
			&mut map,
			&mut source_bytes,
			&mut decoded_pixels,
			asset,
			&bytes,
			Some(source_file_path),
			Some(byte_offset),
			false,
		);
	}
	report.push_info(format!(
		".unavatar textureAssets: decoded={} source_bytes={} decoded_pixels={} decode_ms={} file_backed=true",
		map.len(),
		source_bytes,
		decoded_pixels,
		started.elapsed().as_millis()
	));
	map
}

#[allow(clippy::too_many_arguments)]
fn append_unavatar_texture_asset(
	scene: &mut UnaSceneSnapshot,
	report: &mut ImportReport,
	map: &mut BTreeMap<String, usize>,
	source_bytes: &mut u64,
	decoded_pixels: &mut u64,
	asset: &Value,
	bytes: &[u8],
	source_file_path: Option<&Path>,
	byte_offset: Option<u64>,
	keep_encoded_bytes: bool,
) {
	let id = asset.get("id").and_then(Value::as_str).unwrap_or("");
	let mime_type = asset.get("mimeType").and_then(Value::as_str).unwrap_or("");
	let source_pixel_format = asset.get("sourcePixelFormat").and_then(Value::as_str);
	let channels = asset.get("channels").and_then(Value::as_str);
	let decoded = match decode_unavatar_texture_asset(bytes, mime_type, source_pixel_format, channels) {
		Ok(image) => image,
		Err(error) => {
			report.lost_features.push(un_avatar_core::LostFeature {
				feature: format!("UN_avatar.textureAssets[{id}]"),
				detail: Some(error),
			});
			return;
		}
	};
	*source_bytes += bytes.len() as u64;
	*decoded_pixels += u64::from(decoded.width) * u64::from(decoded.height);
	let decoded_width = decoded.width;
	let decoded_height = decoded.height;
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
		width: Some(decoded_width),
		height: Some(decoded_height),
		byte_offset,
		byte_length: bytes.len() as u64,
		source_hash: source_hash64(bytes),
		source_file_path: source_file_path.map(Path::to_path_buf),
		encoded_bytes: keep_encoded_bytes.then(|| bytes.to_vec()),
	}));
	map.insert(id.to_string(), image_index);
}

fn unavatar_texture_assets(root: &Value) -> Option<&Vec<Value>> {
	root.get("extensions")
		.and_then(Value::as_object)
		.and_then(|extensions| extensions.get(UN_AVATAR_EXTENSION_NAME))
		.and_then(|ext| ext.get("textureAssets"))
		.and_then(Value::as_array)
}

fn texture_asset_bin_range(root: &Value, asset: &Value) -> Option<Range<usize>> {
	let view_index = asset.get("bufferView").and_then(Value::as_u64)? as usize;
	let view = root.get("bufferViews").and_then(Value::as_array)?.get(view_index)?;
	let offset = view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
	let length = view.get("byteLength").and_then(Value::as_u64)? as usize;
	Some(offset..offset.checked_add(length)?)
}

fn texture_asset_bytes<'a>(root: &Value, bin: &'a [u8], asset: &Value) -> Option<&'a [u8]> {
	let range = texture_asset_bin_range(root, asset)?;
	bin.get(range)
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
		"image/png" | "image/jpeg" => {
			let format = if mime_type == "image/png" {
				image::ImageFormat::Png
			} else {
				image::ImageFormat::Jpeg
			};
			let decoded = image::load_from_memory_with_format(bytes, format).map_err(|e| format!("{mime_type} decode: {e}"))?;
			let rgba = decoded.to_rgba8();
			let width = rgba.width();
			let height = rgba.height();
			Ok(UnaImageRgba {
				width,
				height,
				pixel_format: UnaImagePixelFormat::R8G8B8A8,
				pixels: rgba.into_raw(),
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

fn source_hash64(bytes: &[u8]) -> u64 {
	let mut hasher = DefaultHasher::new();
	hasher.write(bytes);
	hasher.finish()
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
	if !root_has_webp_image(&root) {
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

fn root_has_webp_image(root: &Value) -> bool {
	root.get("images").and_then(Value::as_array).is_some_and(|images| {
		images
			.iter()
			.any(|image| image.get("mimeType").and_then(Value::as_str) == Some("image/webp"))
	})
}

struct GltfBufferViewBytes {
	bytes: Vec<u8>,
	target: Option<Value>,
}

fn read_glb_json_and_bin(bytes: &[u8]) -> Result<(Value, Vec<u8>), ImportError> {
	let (json, bin_range) = read_glb_json_and_bin_range(bytes)?;
	Ok((json, bytes[bin_range].to_vec()))
}

fn read_glb_json_and_bin_range(bytes: &[u8]) -> Result<(Value, Range<usize>), ImportError> {
	if bytes.len() < 12 || read_glb_u32(bytes, 0)? != GLB_MAGIC || read_glb_u32(bytes, 4)? != GLB_VERSION_2 {
		return Err(ImportError::Message("GLB 2.0 expected".to_string()));
	}
	let mut offset = 12usize;
	let mut json = None;
	let mut bin_range = None;
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
			BIN_CHUNK_TYPE => bin_range = Some(offset..offset + length),
			_ => {}
		}
		offset += length;
	}
	Ok((
		json.ok_or_else(|| ImportError::Message("GLB JSON chunk is missing".to_string()))?,
		bin_range.ok_or_else(|| ImportError::Message("GLB BIN chunk is missing".to_string()))?,
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
	while !bytes.len().is_multiple_of(4) {
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

fn scene_path_for_index(paths: &BTreeMap<String, usize>, index: usize) -> Option<String> {
	paths
		.iter()
		.find_map(|(path, candidate)| (*candidate == index && !path.is_empty()).then(|| path.clone()))
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

struct WardrobeLookupContext {
	node_ids: BTreeMap<String, usize>,
	registry_paths: BTreeMap<String, String>,
	paths: BTreeMap<String, usize>,
	normalized_paths: BTreeMap<String, Vec<usize>>,
	paths_by_index: Vec<Option<String>>,
	parent_by_index: Vec<Option<usize>>,
	registry_source_normalized_paths_by_index: Vec<Option<String>>,
}

impl WardrobeLookupContext {
	fn new(scene: &UnaSceneSnapshot, unavatar: Option<&UnaUnavatarExtension>) -> Self {
		let node_ids = scene_node_ids(scene);
		let registry_paths = unavatar_node_registry_paths(unavatar);
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
		let registry_source_normalized_paths_by_index = scene
			.nodes
			.iter()
			.map(|node| {
				let source_node_id = node.source_node_id.as_deref()?;
				let source_path = registry_paths.get(source_node_id)?;
				Some(normalize_unavatar_path(source_path))
			})
			.collect();
		Self {
			node_ids,
			registry_paths,
			paths,
			normalized_paths,
			paths_by_index,
			parent_by_index,
			registry_source_normalized_paths_by_index,
		}
	}
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

fn operation_target_registry_path<'a>(registry_paths: &'a BTreeMap<String, String>, op: &'a Value) -> &'a str {
	operation_target_node_id(op)
		.and_then(|node_id| registry_paths.get(node_id).map(String::as_str))
		.filter(|path| !path.is_empty())
		.unwrap_or_else(|| operation_target_path(op))
}

fn unavatar_node_ref_display_path(
	scene: &UnaSceneSnapshot,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	node_ref: &Value,
	index: usize,
) -> String {
	let registry_path = operation_target_registry_path(registry_paths, node_ref);
	if !registry_path.is_empty() {
		return registry_path.to_string();
	}
	if let Some(scene_path) = scene_path_for_index(paths, index) {
		return scene_path;
	}
	scene
		.nodes
		.get(index)
		.and_then(|node| node.name.as_deref())
		.filter(|name| !name.is_empty())
		.map(str::to_string)
		.unwrap_or_else(|| format!("#{index}"))
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

fn normalized_path_is_same_or_descendant_normalized(path: &str, ancestor: &str) -> bool {
	!ancestor.is_empty() && (path == ancestor || path.strip_prefix(ancestor).is_some_and(|rest| rest.starts_with('/')))
}

fn lookup_operation_subtree_targets_all_with_lookup(scene: &UnaSceneSnapshot, lookup: &WardrobeLookupContext, op: &Value) -> Vec<usize> {
	let mut out = BTreeSet::new();
	for root in lookup_operation_targets_all(
		&lookup.node_ids,
		&lookup.registry_paths,
		&lookup.paths,
		&lookup.normalized_paths,
		op,
	) {
		collect_current_subtree(scene, root, &mut out);
	}
	let target_path = operation_target_registry_path(&lookup.registry_paths, op);
	if !target_path.is_empty() {
		let target_path = normalize_unavatar_path(target_path);
		for (idx, source_path) in lookup.registry_source_normalized_paths_by_index.iter().enumerate() {
			let Some(source_path) = source_path.as_deref() else {
				continue;
			};
			if normalized_path_is_same_or_descendant_normalized(source_path, &target_path) {
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

fn unavatar_dynamics_metadata_source_kind(value: &Value) -> UnaDynamicsSourceKind {
	let source = value
		.get("source")
		.or_else(|| value.get("sourceKind"))
		.or_else(|| value.get("source_kind"))
		.and_then(Value::as_str)
		.unwrap_or("");
	if source.to_ascii_lowercase().starts_with("vrc_") {
		UnaDynamicsSourceKind::VrcPhysBone
	} else {
		unavatar_dynamics_source_kind(value)
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

fn unavatar_modular_avatar_components(unavatar: &UnaUnavatarExtension) -> &[Value] {
	unavatar_modular_avatar_components_slice(&unavatar.source)
}

fn modular_avatar_pb_blocker_ignores(
	unavatar: &UnaUnavatarExtension,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	parents: &[Option<usize>],
) -> BTreeMap<usize, BTreeSet<usize>> {
	let mut ignores_by_root = BTreeMap::<usize, BTreeSet<usize>>::new();
	for component in unavatar_modular_avatar_components(unavatar) {
		if component.get("shortType").and_then(Value::as_str) != Some("ModularAvatarPBBlocker") {
			continue;
		}
		if component.get("enabled").and_then(Value::as_bool) == Some(false) {
			continue;
		}
		let Some(target_ref) = component.get("target").or_else(|| component.get("resolvedTarget")) else {
			continue;
		};
		let Some(tip) = modular_avatar_reference_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
			continue;
		};
		let mut node = tip;
		while let Some(parent) = parents.get(node).copied().flatten() {
			ignores_by_root.entry(parent).or_default().insert(tip);
			node = parent;
		}
	}
	ignores_by_root
}

fn modular_avatar_global_colliders(
	unavatar: &UnaUnavatarExtension,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Vec<UnaDynamicsCollider> {
	let mut colliders = Vec::new();
	for component in unavatar_modular_avatar_components(unavatar) {
		if component.get("shortType").and_then(Value::as_str) != Some("ModularAvatarGlobalCollider") {
			continue;
		}
		if component.get("enabled").and_then(Value::as_bool) == Some(false) {
			continue;
		}
		let fields = component.get("fields").unwrap_or(component);
		let root_ref = fields
			.get("m_rootTransform")
			.or_else(|| fields.get("rootTransform"))
			.or_else(|| fields.get("RootTransform"))
			.or_else(|| component.get("target"));
		let Some(root_ref) = root_ref else {
			continue;
		};
		let Some(node) = modular_avatar_reference_index(root_ref, node_ids, registry_paths, paths, normalized_paths) else {
			continue;
		};
		let radius = json_f32(
			fields
				.get("m_radius")
				.or_else(|| fields.get("radius"))
				.or_else(|| fields.get("Radius")),
		)
		.unwrap_or(0.0);
		if !radius.is_finite() || radius <= 0.0 {
			continue;
		}
		colliders.push(UnaDynamicsCollider {
			source_kind: UnaDynamicsSourceKind::VrcPhysBone,
			source_id: String::new(),
			collider_path: String::new(),
			node,
			shape: UnaDynamicsColliderShape::Capsule,
			radius,
			height: json_f32(
				fields
					.get("m_height")
					.or_else(|| fields.get("height"))
					.or_else(|| fields.get("Height")),
			)
			.unwrap_or(0.0)
			.max(0.0),
			position: unity_vec3_to_unavatar_runtime(
				json_vec3(
					fields
						.get("m_position")
						.or_else(|| fields.get("position"))
						.or_else(|| fields.get("Position")),
				)
				.unwrap_or([0.0; 3]),
			),
			rotation: unity_quat_to_unavatar_runtime(
				json_vec4(
					fields
						.get("m_rotation")
						.or_else(|| fields.get("rotation"))
						.or_else(|| fields.get("Rotation")),
				)
				.unwrap_or([0.0, 0.0, 0.0, 1.0]),
			),
			inside_bounds: false,
		});
	}
	colliders
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
	let shape_value = value
		.get("shapeType")
		.or_else(|| value.get("shape_type"))
		.or_else(|| value.get("shape"));
	if matches!(shape_value.and_then(Value::as_u64), Some(0)) {
		return UnaDynamicsColliderShape::Sphere;
	}
	if matches!(shape_value.and_then(Value::as_u64), Some(1)) {
		return UnaDynamicsColliderShape::Capsule;
	}
	let shape = shape_value.and_then(Value::as_str).unwrap_or("");
	if shape == "0" {
		return UnaDynamicsColliderShape::Sphere;
	}
	if shape == "1" {
		return UnaDynamicsColliderShape::Capsule;
	}
	if shape.eq_ignore_ascii_case("sphere") {
		UnaDynamicsColliderShape::Sphere
	} else if shape.eq_ignore_ascii_case("capsule") {
		UnaDynamicsColliderShape::Capsule
	} else if shape.eq_ignore_ascii_case("plane") {
		UnaDynamicsColliderShape::Plane
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
	source_id: &str,
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
	unavatar_dynamics_collider_array(colliders, source_kind, source_id, node_ids, registry_paths, paths, normalized_paths)
}

fn unavatar_dynamics_global_colliders(
	unavatar: &UnaUnavatarExtension,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Vec<UnaDynamicsCollider> {
	let Some(colliders) = unavatar.source.get("colliders").and_then(Value::as_array) else {
		return Vec::new();
	};
	unavatar_dynamics_collider_array(
		colliders,
		UnaDynamicsSourceKind::Unknown,
		"",
		node_ids,
		registry_paths,
		paths,
		normalized_paths,
	)
}

fn unavatar_dynamics_collider_array(
	colliders: &[Value],
	source_kind: UnaDynamicsSourceKind,
	source_id: &str,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Vec<UnaDynamicsCollider> {
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
			let collider_path = collider
				.get("component")
				.map(|component| operation_target_registry_path(registry_paths, component))
				.unwrap_or_else(|| operation_target_registry_path(registry_paths, root))
				.to_string();
			let node = unavatar_node_ref_index(root, node_ids, registry_paths, paths, normalized_paths)?;
			let shape = unavatar_dynamics_collider_shape(collider);
			let radius = json_f32(collider.get("radius")).unwrap_or(0.0);
			if shape != UnaDynamicsColliderShape::Plane && (!radius.is_finite() || radius <= 0.0) {
				return None;
			}
			Some(UnaDynamicsCollider {
				source_kind,
				source_id: source_id.to_string(),
				collider_path,
				node,
				shape,
				radius: radius.max(0.0),
				height: json_f32(collider.get("height")).unwrap_or(0.0).max(0.0),
				position: unity_vec3_to_unavatar_runtime(
					json_vec3(collider.get("position").or_else(|| collider.get("offset"))).unwrap_or([0.0; 3]),
				),
				rotation: unity_quat_to_unavatar_runtime(json_vec4(collider.get("rotation")).unwrap_or([0.0, 0.0, 0.0, 1.0])),
				inside_bounds,
			})
		})
		.collect()
}

fn unavatar_contact_kind(value: &Value) -> UnaDynamicsContactKind {
	let kind = value
		.get("kind")
		.or_else(|| value.get("contactKind"))
		.or_else(|| value.get("contact_kind"))
		.or_else(|| value.get("source"))
		.and_then(Value::as_str)
		.unwrap_or("");
	if kind.eq_ignore_ascii_case("sender") || kind.eq_ignore_ascii_case("vrc_contact_sender") {
		UnaDynamicsContactKind::Sender
	} else if kind.eq_ignore_ascii_case("receiver") || kind.eq_ignore_ascii_case("vrc_contact_receiver") {
		UnaDynamicsContactKind::Receiver
	} else {
		UnaDynamicsContactKind::Unknown
	}
}

fn unavatar_string_array(value: Option<&Value>) -> Vec<String> {
	match value {
		Some(Value::Array(values)) => values
			.iter()
			.filter_map(Value::as_str)
			.filter(|tag| !tag.is_empty())
			.map(ToOwned::to_owned)
			.collect(),
		Some(Value::String(value)) if !value.is_empty() => vec![value.clone()],
		_ => Vec::new(),
	}
}

fn unavatar_dynamics_contacts(
	unavatar: &UnaUnavatarExtension,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Vec<UnaDynamicsContact> {
	let Some(contacts) = unavatar
		.source
		.get("contacts")
		.or_else(|| unavatar.source.get("contactMetadata"))
		.or_else(|| unavatar.source.get("contact_metadata"))
		.and_then(Value::as_array)
	else {
		return Vec::new();
	};
	contacts
		.iter()
		.filter_map(|contact| {
			let node_ref = contact
				.get("node")
				.or_else(|| contact.get("root"))
				.or_else(|| contact.get("component"))?;
			let node = unavatar_node_ref_index(node_ref, node_ids, registry_paths, paths, normalized_paths)?;
			Some(UnaDynamicsContact {
				source_kind: unavatar_dynamics_metadata_source_kind(contact),
				source_id: contact
					.get("id")
					.or_else(|| contact.get("sourceId"))
					.or_else(|| contact.get("source_id"))
					.and_then(Value::as_str)
					.unwrap_or("")
					.to_string(),
				node,
				kind: unavatar_contact_kind(contact),
				parameter: contact
					.get("parameter")
					.or_else(|| contact.get("parameterName"))
					.or_else(|| contact.get("parameter_name"))
					.and_then(Value::as_str)
					.unwrap_or("")
					.to_string(),
				collision_tags: unavatar_string_array(
					contact
						.get("collisionTags")
						.or_else(|| contact.get("collision_tags"))
						.or_else(|| contact.get("tags")),
				),
				shape: unavatar_dynamics_collider_shape(contact),
				radius: json_f32(contact.get("radius")).unwrap_or(0.0).max(0.0),
				height: json_f32(contact.get("height")).unwrap_or(0.0).max(0.0),
				position: unity_vec3_to_unavatar_runtime(json_vec3(contact.get("position")).unwrap_or([0.0; 3])),
				rotation: unity_quat_to_unavatar_runtime(json_vec4(contact.get("rotation")).unwrap_or([0.0, 0.0, 0.0, 1.0])),
			})
		})
		.collect()
}

fn unavatar_dynamics_constraint_refs(
	unavatar: &UnaUnavatarExtension,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Vec<UnaDynamicsConstraintRef> {
	let Some(constraints) = unavatar
		.source
		.get("constraintRefs")
		.or_else(|| unavatar.source.get("constraint_refs"))
		.or_else(|| unavatar.source.get("constraints"))
		.and_then(Value::as_array)
	else {
		return Vec::new();
	};
	constraints
		.iter()
		.filter_map(|constraint| {
			let target_ref = constraint
				.get("targetNode")
				.or_else(|| constraint.get("target_node"))
				.or_else(|| constraint.get("target"))?;
			let target_node = unavatar_node_ref_index(target_ref, node_ids, registry_paths, paths, normalized_paths)?;
			let source_values = constraint
				.get("sourceNodes")
				.or_else(|| constraint.get("source_nodes"))
				.or_else(|| constraint.get("sources"))
				.and_then(Value::as_array)
				.map(Vec::as_slice)
				.unwrap_or(&[]);
			let source_nodes = source_values
				.iter()
				.filter_map(|source| unavatar_node_ref_index(source, node_ids, registry_paths, paths, normalized_paths))
				.collect::<Vec<_>>();
			Some(UnaDynamicsConstraintRef {
				source_kind: unavatar_dynamics_metadata_source_kind(constraint),
				source_id: constraint
					.get("id")
					.or_else(|| constraint.get("sourceId"))
					.or_else(|| constraint.get("source_id"))
					.and_then(Value::as_str)
					.unwrap_or("")
					.to_string(),
				target_node,
				source_nodes,
				constraint_type: constraint
					.get("type")
					.or_else(|| constraint.get("constraintType"))
					.or_else(|| constraint.get("constraint_type"))
					.and_then(Value::as_str)
					.unwrap_or("")
					.to_string(),
				weight: json_f32(constraint.get("weight")).unwrap_or(1.0).clamp(0.0, 1.0),
			})
		})
		.collect()
}

fn json_bool_or(value: Option<&Value>, fallback: bool) -> bool {
	value.and_then(Value::as_bool).unwrap_or(fallback)
}

fn unavatar_node_constraint_source(
	value: &Value,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Option<UnaNodeConstraintSource> {
	let node_ref = value
		.get("node")
		.or_else(|| value.get("sourceNode"))
		.or_else(|| value.get("source_node"))
		.unwrap_or(value);
	let source_node = unavatar_node_ref_index(node_ref, node_ids, registry_paths, paths, normalized_paths)?;
	Some(UnaNodeConstraintSource {
		source_node,
		weight: json_f32(value.get("weight")).unwrap_or(1.0).max(0.0),
		translation_offset: json_vec3(value.get("translationOffset").or_else(|| value.get("translation_offset"))).unwrap_or([0.0; 3]),
		rotation_offset: json_vec3(value.get("rotationOffset").or_else(|| value.get("rotation_offset"))).unwrap_or([0.0; 3]),
	})
}

fn unavatar_node_constraints(
	unavatar: &UnaUnavatarExtension,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	report: &mut ImportReport,
) -> Vec<UnaNodeConstraint> {
	let Some(constraints) = unavatar
		.source
		.get("nodeConstraints")
		.or_else(|| unavatar.source.get("node_constraints"))
		.and_then(Value::as_array)
	else {
		return Vec::new();
	};
	let mut out = Vec::new();
	let mut missing = 0usize;
	let mut unsupported = 0usize;
	for constraint in constraints {
		let kind_text = constraint
			.get("kind")
			.or_else(|| constraint.get("type"))
			.and_then(Value::as_str)
			.unwrap_or("");
		if kind_text != "parent" {
			unsupported += 1;
			continue;
		}
		let Some(target_ref) = constraint
			.get("target")
			.or_else(|| constraint.get("targetNode"))
			.or_else(|| constraint.get("target_node"))
		else {
			missing += 1;
			continue;
		};
		let Some(target_node) = unavatar_node_ref_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
			missing += 1;
			continue;
		};
		let mut sources = constraint
			.get("sources")
			.or_else(|| constraint.get("sourceNodes"))
			.or_else(|| constraint.get("source_nodes"))
			.and_then(Value::as_array)
			.map(|values| {
				values
					.iter()
					.filter_map(|source| unavatar_node_constraint_source(source, node_ids, registry_paths, paths, normalized_paths))
					.collect::<Vec<_>>()
			})
			.unwrap_or_default();
		if sources.is_empty() {
			if let Some(source_ref) = constraint
				.get("source")
				.or_else(|| constraint.get("sourceNode"))
				.or_else(|| constraint.get("source_node"))
			{
				if let Some(source) = unavatar_node_constraint_source(source_ref, node_ids, registry_paths, paths, normalized_paths) {
					sources.push(source);
				}
			}
		}
		let Some(primary_source) = sources.first().map(|source| source.source_node) else {
			missing += 1;
			continue;
		};
		out.push(UnaNodeConstraint {
			target_node,
			source_node: primary_source,
			weight: json_f32(constraint.get("weight")).unwrap_or(1.0).clamp(0.0, 1.0),
			kind: UnaNodeConstraintKind::Parent {
				translate_x: json_bool_or(constraint.get("translateX").or_else(|| constraint.get("translate_x")), true),
				translate_y: json_bool_or(constraint.get("translateY").or_else(|| constraint.get("translate_y")), true),
				translate_z: json_bool_or(constraint.get("translateZ").or_else(|| constraint.get("translate_z")), true),
				rotate_x: json_bool_or(constraint.get("rotateX").or_else(|| constraint.get("rotate_x")), true),
				rotate_y: json_bool_or(constraint.get("rotateY").or_else(|| constraint.get("rotate_y")), true),
				rotate_z: json_bool_or(constraint.get("rotateZ").or_else(|| constraint.get("rotate_z")), true),
				translation_at_rest: json_vec3(
					constraint
						.get("translationAtRest")
						.or_else(|| constraint.get("translation_at_rest")),
				)
				.unwrap_or([0.0; 3]),
				rotation_at_rest: json_vec3(constraint.get("rotationAtRest").or_else(|| constraint.get("rotation_at_rest")))
					.unwrap_or([0.0; 3]),
			},
			sources,
		});
	}
	if !out.is_empty() || missing > 0 || unsupported > 0 {
		report.push_info(format!(
			".unavatar node_constraints: parent={}, missing={}, unsupported={}",
			out.len(),
			missing,
			unsupported
		));
	}
	out
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

fn append_unavatar_dynamics_endpoint_tail_children(
	scene: &mut UnaSceneSnapshot,
	root_idx: usize,
	item: &Value,
	chains: &[Vec<usize>],
) -> usize {
	if root_idx >= scene.nodes.len() {
		return 0;
	}
	let Some(endpoint) = unavatar_dynamics_endpoint_position(item) else {
		return 0;
	};
	let world = scene_world_matrices(scene);
	if root_idx >= world.len() {
		return 0;
	}
	let endpoint_world = world[root_idx].transform_point3(Vec3::from(endpoint));
	let mut added = 0usize;
	for chain in chains {
		let Some(&leaf_idx) = chain.last() else {
			continue;
		};
		if leaf_idx >= scene.nodes.len() || leaf_idx >= world.len() {
			continue;
		}
		let endpoint_local = world[leaf_idx].inverse().transform_point3(endpoint_world);
		if endpoint_local.length_squared() <= 1e-12 {
			continue;
		}
		let endpoint_idx = scene.nodes.len();
		let leaf_name = scene.nodes.get(leaf_idx).and_then(|node| node.name.as_deref()).unwrap_or("Leaf");
		scene.nodes.push(UnaSceneNode {
			name: Some(format!("{leaf_name} Endpoint")),
			source_node_id: None,
			resolved_node_id: None,
			visible: true,
			transform: Mat4::from_translation(endpoint_local).to_cols_array(),
			children: Vec::new(),
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		});
		scene.nodes[leaf_idx].children.push(endpoint_idx);
		added += 1;
	}
	added
}

fn collect_scene_child_chains(scene: &UnaSceneSnapshot, root_idx: usize, ignored_nodes: &BTreeSet<usize>) -> Vec<Vec<usize>> {
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
		}
		if child_count == 0 {
			chains.push(chain);
		}
	}
	chains
}

fn unavatar_dynamics_gravity(value: &Value) -> (f32, [f32; 3]) {
	let source_params = unavatar_dynamics_source_params(value);
	let gravity =
		json_vec3(unavatar_dynamics_source_value(value, source_params, "gravityVector", "gravity_vector").or_else(|| value.get("gravity")))
			.unwrap_or([0.0, -1.0, 0.0]);
	let gravity_vec = Vec3::from(gravity);
	let vector_power = gravity_vec.length();
	let explicit_power =
		unavatar_dynamics_source_value(value, source_params, "gravityPower", "gravity_power").and_then(|value| json_f32(Some(value)));
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

fn unavatar_dynamics_immobile_type(value: &Value, source_params: Option<&Value>) -> UnaDynamicsImmobileType {
	let raw_value = unavatar_dynamics_source_value(value, source_params, "immobileType", "immobile_type");
	if let Some(number) = raw_value.and_then(Value::as_i64) {
		return match number {
			1 => UnaDynamicsImmobileType::World,
			_ => UnaDynamicsImmobileType::AllMotion,
		};
	}
	let raw = raw_value.and_then(Value::as_str).unwrap_or_default();
	let normalized = raw
		.chars()
		.filter(|ch| ch.is_ascii_alphanumeric())
		.flat_map(|ch| ch.to_lowercase())
		.collect::<String>();
	match normalized.as_str() {
		"1" | "world" | "worldmotion" | "worldexperimental" => UnaDynamicsImmobileType::World,
		_ => UnaDynamicsImmobileType::AllMotion,
	}
}

fn unavatar_dynamics_source_text(value: &Value, source_params: Option<&Value>, camel_key: &str, snake_key: &str) -> String {
	unavatar_dynamics_source_value(value, source_params, camel_key, snake_key)
		.and_then(Value::as_str)
		.unwrap_or_default()
		.chars()
		.filter(|ch| ch.is_ascii_alphanumeric())
		.flat_map(|ch| ch.to_lowercase())
		.collect()
}

fn unavatar_dynamics_writeback_mode(value: &Value) -> (UnaDynamicsWritebackMode, Option<String>) {
	let source_params = unavatar_dynamics_source_params(value);
	let Some(value) = unavatar_dynamics_source_value(value, source_params, "writebackMode", "writeback_mode").and_then(Value::as_str)
	else {
		if unavatar_dynamics_implies_translation_writeback(value, source_params) {
			return (UnaDynamicsWritebackMode::RotationTranslation, None);
		}
		return (UnaDynamicsWritebackMode::RotationOnly, None);
	};
	match value.trim().to_ascii_lowercase().as_str() {
		"rotation_only" | "rotationonly" | "rotation-only" => (UnaDynamicsWritebackMode::RotationOnly, None),
		"rotation_translation" | "rotationtranslation" | "rotation-translation" => (UnaDynamicsWritebackMode::RotationTranslation, None),
		_ => (UnaDynamicsWritebackMode::RotationOnly, Some(value.to_string())),
	}
}

fn unavatar_dynamics_implies_translation_writeback(value: &Value, source_params: Option<&Value>) -> bool {
	unavatar_dynamics_source_float_nonzero(value, source_params, "maxStretch", "max_stretch")
		|| unavatar_dynamics_source_float_nonzero(value, source_params, "maxSquish", "max_squish")
		|| unavatar_dynamics_source_float_nonzero(value, source_params, "stretchMotion", "stretch_motion")
		|| unavatar_dynamics_source_curve_has_keys(value, source_params, "maxStretchCurve", "max_stretch_curve")
		|| unavatar_dynamics_source_curve_has_keys(value, source_params, "maxSquishCurve", "max_squish_curve")
		|| unavatar_dynamics_source_curve_has_keys(value, source_params, "stretchMotionCurve", "stretch_motion_curve")
}

fn unavatar_dynamics_source_float_nonzero(value: &Value, source_params: Option<&Value>, camel_key: &str, snake_key: &str) -> bool {
	unavatar_dynamics_source_value(value, source_params, camel_key, snake_key)
		.and_then(|value| json_f32(Some(value)))
		.is_some_and(|value| value.is_finite() && value.abs() > 0.0)
}

fn unavatar_dynamics_source_curve_has_keys(value: &Value, source_params: Option<&Value>, camel_key: &str, snake_key: &str) -> bool {
	let Some(curve) = unavatar_dynamics_source_value(value, source_params, camel_key, snake_key) else {
		return false;
	};
	curve
		.get("keyCount")
		.or_else(|| curve.get("key_count"))
		.and_then(Value::as_u64)
		.is_some_and(|count| count > 0)
		|| curve
			.get("keys")
			.or_else(|| curve.get("Keys"))
			.and_then(Value::as_array)
			.is_some_and(|keys| !keys.is_empty())
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
	let limit_rotation = unavatar_dynamics_source_value(value, source_params, "limitRotation", "limit_rotation")
		.and_then(|value| json_vec3(Some(value)))
		.unwrap_or([0.0, 0.0, 0.0]);
	let max_stretch = unavatar_dynamics_source_value(value, source_params, "maxStretch", "max_stretch")
		.and_then(|value| json_f32(Some(value)))
		.unwrap_or(0.0);
	let max_squish = unavatar_dynamics_source_value(value, source_params, "maxSquish", "max_squish")
		.and_then(|value| json_f32(Some(value)))
		.unwrap_or(0.0);
	let stretch_motion = unavatar_dynamics_source_value(value, source_params, "stretchMotion", "stretch_motion")
		.and_then(|value| json_f32(Some(value)))
		.filter(|value| value.is_finite());
	if limit_type.is_empty()
		&& max_angle_x == 0.0
		&& max_angle_z == 0.0
		&& max_stretch == 0.0
		&& max_squish == 0.0
		&& stretch_motion.is_none()
		&& limit_rotation.iter().all(|value| *value == 0.0)
	{
		None
	} else {
		Some(UnaDynamicsLimit {
			limit_type,
			limit_rotation,
			max_angle_x,
			max_angle_z,
			max_stretch,
			max_squish,
			stretch_motion,
			max_stretch_samples: Vec::new(),
			max_squish_samples: Vec::new(),
			stretch_motion_samples: Vec::new(),
		})
	}
}

fn unavatar_dynamics_interaction(value: &Value) -> Option<UnaDynamicsInteraction> {
	let source_params = unavatar_dynamics_source_params(value);
	let allow_grabbing = unavatar_dynamics_source_value(value, source_params, "allowGrabbing", "allow_grabbing").and_then(Value::as_bool);
	let allow_posing = unavatar_dynamics_source_value(value, source_params, "allowPosing", "allow_posing").and_then(Value::as_bool);
	let parameter = unavatar_dynamics_source_value(value, source_params, "parameter", "parameter")
		.and_then(Value::as_str)
		.unwrap_or("")
		.to_string();
	if allow_grabbing.is_none() && allow_posing.is_none() && parameter.is_empty() {
		None
	} else {
		Some(UnaDynamicsInteraction {
			allow_grabbing,
			allow_posing,
			parameter,
		})
	}
}

fn unavatar_dynamics_curve_samples(value: &Value, base_value: f32, joint_count: usize, camel_key: &str, snake_key: &str) -> Vec<f32> {
	if joint_count == 0 {
		return Vec::new();
	}
	let source_params = unavatar_dynamics_source_params(value);
	let curve = unavatar_dynamics_source_value(value, source_params, camel_key, snake_key);
	let Some(_) = animation_curve_evaluate(curve, 1.0) else {
		return Vec::new();
	};
	let base_value = base_value.max(0.0);
	(0..joint_count)
		.map(|index| {
			let input = (index + 1) as f32 / joint_count as f32;
			animation_curve_evaluate(curve, input)
				.map(|scale| base_value * scale)
				.filter(|value| value.is_finite())
				.unwrap_or(base_value)
				.max(0.0)
		})
		.collect()
}

fn unavatar_dynamics_radius_samples(value: &Value, hit_radius: f32, joint_count: usize) -> Vec<f32> {
	unavatar_dynamics_curve_samples(value, hit_radius, joint_count, "radiusCurve", "radius_curve")
}

fn unavatar_dynamics_settings(
	scene: &mut UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
	report: &mut ImportReport,
) -> Option<UnaSpringBoneSettings> {
	let dynamics = unavatar
		.source
		.get("dynamics")
		.and_then(Value::as_array)
		.map(Vec::as_slice)
		.unwrap_or(&[]);
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(Some(unavatar));
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let parents = scene_parent_indices(scene);
	let pb_blocker_ignores = modular_avatar_pb_blocker_ignores(unavatar, &node_ids, &registry_paths, &paths, &normalized_paths, &parents);
	let mut groups = Vec::new();
	let mut missing_roots = 0usize;
	let mut short_chains = 0usize;
	let mut ignored_transform_count = 0usize;
	let mut pb_blocker_ignore_count = 0usize;
	let mut multi_child_ignore_count = 0usize;
	let mut endpoint_child_count = 0usize;
	let mut endpoint_tail_synthesis_failed_count = 0usize;
	let mut endpoint_tail_synthesis_failed_samples = Vec::new();
	let mut colliders = unavatar_dynamics_global_colliders(unavatar, &node_ids, &registry_paths, &paths, &normalized_paths);
	let ma_global_colliders = modular_avatar_global_colliders(unavatar, &node_ids, &registry_paths, &paths, &normalized_paths);
	let ma_global_collider_count = ma_global_colliders.len();
	colliders.extend(ma_global_colliders);
	let contacts = unavatar_dynamics_contacts(unavatar, &node_ids, &registry_paths, &paths, &normalized_paths);
	let constraint_refs = unavatar_dynamics_constraint_refs(unavatar, &node_ids, &registry_paths, &paths, &normalized_paths);

	for item in dynamics {
		let authored_enabled = item.get("enabled").and_then(Value::as_bool).unwrap_or(true);
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
		let source_params = unavatar_dynamics_source_params(item);
		let integration_type = unavatar_dynamics_source_text(item, source_params, "integrationType", "integration_type");
		let pull = unavatar_dynamics_source_value(item, source_params, "pull", "pull")
			.and_then(|value| json_f32(Some(value)))
			.unwrap_or_else(|| json_f32(item.get("stiffness").or_else(|| item.get("spring"))).unwrap_or(1.0));
		let raw_spring = unavatar_dynamics_source_value(item, source_params, "spring", "spring")
			.and_then(|value| json_f32(Some(value)))
			.unwrap_or(0.0);
		let raw_stiffness = unavatar_dynamics_source_value(item, source_params, "stiffness", "stiffness")
			.and_then(|value| json_f32(Some(value)))
			.unwrap_or(pull);
		let raw_momentum = unavatar_dynamics_source_value(item, source_params, "momentum", "momentum")
			.and_then(|value| json_f32(Some(value)))
			.filter(|value| value.is_finite() && *value > 0.0)
			.unwrap_or(raw_spring);
		let is_advanced_physbone = source_kind == UnaDynamicsSourceKind::VrcPhysBone && integration_type.eq_ignore_ascii_case("advanced");
		let integration_type = if source_kind == UnaDynamicsSourceKind::VrcPhysBone {
			if is_advanced_physbone {
				UnaDynamicsIntegrationType::VrcAdvanced
			} else {
				UnaDynamicsIntegrationType::VrcSimplified
			}
		} else {
			UnaDynamicsIntegrationType::Standard
		};
		let spring = if is_advanced_physbone { raw_momentum } else { raw_spring };
		let stiffness = if source_kind == UnaDynamicsSourceKind::VrmSpringBone
			|| (source_kind == UnaDynamicsSourceKind::VrcPhysBone && !is_advanced_physbone)
		{
			0.0
		} else {
			raw_stiffness
		};
		let gravity_falloff = unavatar_dynamics_source_value(item, source_params, "gravityFalloff", "gravity_falloff")
			.and_then(|value| json_f32(Some(value)))
			.unwrap_or(0.0);
		let immobile = unavatar_dynamics_source_value(item, source_params, "immobile", "immobile")
			.and_then(|value| json_f32(Some(value)))
			.unwrap_or(0.0);
		let immobile_type = unavatar_dynamics_immobile_type(item, source_params);
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
		let (writeback_mode, unknown_writeback_mode) = unavatar_dynamics_writeback_mode(item);
		if let Some(unknown_writeback_mode) = unknown_writeback_mode {
			report.push_warning(format!(
				".unavatar dynamics: unknown writebackMode {unknown_writeback_mode:?} for {source_id:?}; defaulting to rotation_only"
			));
		}
		let ignored_nodes = unavatar_dynamics_node_index_set(
			unavatar_dynamics_source_value(item, source_params, "ignoreTransforms", "ignore_transforms")
				.or_else(|| unavatar_dynamics_source_value(item, source_params, "ignoredTransforms", "ignored_transforms")),
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
			&source_id,
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
			let mut root_ignored_nodes = ignored_nodes.clone();
			if let Some(blocked_nodes) = pb_blocker_ignores.get(&root_idx) {
				pb_blocker_ignore_count += blocked_nodes.len();
				root_ignored_nodes.extend(blocked_nodes.iter().copied());
			}
			let endpoint_requested = unavatar_dynamics_endpoint_position(item).is_some();
			let has_non_ignored_child = scene.nodes[root_idx]
				.children
				.iter()
				.any(|child| !root_ignored_nodes.contains(child));
			if ensure_unavatar_dynamics_endpoint_child(scene, root_idx, item, &root_ignored_nodes) {
				endpoint_child_count += 1;
			} else if endpoint_requested && has_non_ignored_child {
				let chains = collect_scene_child_chains(scene, root_idx, &root_ignored_nodes);
				let appended = append_unavatar_dynamics_endpoint_tail_children(scene, root_idx, item, &chains);
				if appended > 0 {
					endpoint_child_count += appended;
				} else {
					endpoint_tail_synthesis_failed_count += 1;
					if endpoint_tail_synthesis_failed_samples.len() < 8 {
						let label = if source_id.is_empty() {
							comment.as_str()
						} else {
							source_id.as_str()
						};
						let path = unavatar_node_ref_display_path(scene, &registry_paths, &paths, root, root_idx);
						endpoint_tail_synthesis_failed_samples.push(format!("{label}@{path}"));
					}
				}
			}
			for chain in collect_scene_child_chains(scene, root_idx, &root_ignored_nodes) {
				let prepended_parent_anchor = false;
				if chain.len() < 2 {
					short_chains += 1;
					continue;
				}
				let joint_count = chain.len() - 1;
				let hit_radius_samples = unavatar_dynamics_radius_samples(item, hit_radius, joint_count);
				let stiffness_samples = unavatar_dynamics_curve_samples(item, stiffness, joint_count, "stiffnessCurve", "stiffness_curve");
				let pull_samples = unavatar_dynamics_curve_samples(item, pull, joint_count, "pullCurve", "pull_curve");
				let spring_samples = if is_advanced_physbone {
					unavatar_dynamics_curve_samples(item, spring, joint_count, "momentumCurve", "momentum_curve")
				} else {
					unavatar_dynamics_curve_samples(item, spring, joint_count, "springCurve", "spring_curve")
				};
				let gravity_power_samples =
					unavatar_dynamics_curve_samples(item, gravity_power, joint_count, "gravityCurve", "gravity_curve");
				let gravity_falloff_samples =
					unavatar_dynamics_curve_samples(item, gravity_falloff, joint_count, "gravityFalloffCurve", "gravity_falloff_curve");
				let immobile_samples = unavatar_dynamics_curve_samples(item, immobile, joint_count, "immobileCurve", "immobile_curve");
				let max_angle_x_samples = unavatar_dynamics_curve_samples(
					item,
					limit.as_ref().map(|limit| limit.max_angle_x).unwrap_or(0.0),
					joint_count,
					"maxAngleXCurve",
					"max_angle_x_curve",
				);
				let max_angle_z_samples = unavatar_dynamics_curve_samples(
					item,
					limit.as_ref().map(|limit| limit.max_angle_z).unwrap_or(0.0),
					joint_count,
					"maxAngleZCurve",
					"max_angle_z_curve",
				);
				let max_stretch_samples = unavatar_dynamics_curve_samples(
					item,
					limit.as_ref().map(|limit| limit.max_stretch).unwrap_or(0.0),
					joint_count,
					"maxStretchCurve",
					"max_stretch_curve",
				);
				let max_squish_samples = unavatar_dynamics_curve_samples(
					item,
					limit.as_ref().map(|limit| limit.max_squish).unwrap_or(0.0),
					joint_count,
					"maxSquishCurve",
					"max_squish_curve",
				);
				let stretch_motion_samples = unavatar_dynamics_curve_samples(
					item,
					limit.as_ref().and_then(|limit| limit.stretch_motion).unwrap_or(1.0),
					joint_count,
					"stretchMotionCurve",
					"stretch_motion_curve",
				);
				let mut chain_limit = limit.clone();
				if !max_stretch_samples.is_empty() || !max_squish_samples.is_empty() || !stretch_motion_samples.is_empty() {
					let limit = chain_limit.get_or_insert_with(UnaDynamicsLimit::default);
					limit.max_stretch_samples = max_stretch_samples;
					limit.max_squish_samples = max_squish_samples;
					limit.stretch_motion_samples = stretch_motion_samples;
				}
				groups.push(UnaSpringBoneGroup {
					source_kind,
					enabled: authored_enabled,
					source_id: source_id.clone(),
					comment: comment.clone(),
					category: category.clone(),
					stiffness,
					pull,
					spring,
					integration_type,
					gravity_power,
					gravity_falloff,
					immobile,
					immobile_type,
					gravity_dir,
					drag_force,
					center_node: None,
					hit_radius,
					hit_radius_samples,
					stiffness_samples,
					pull_samples,
					spring_samples,
					gravity_power_samples,
					gravity_falloff_samples,
					immobile_samples,
					max_angle_x_samples,
					max_angle_z_samples,
					writeback_mode,
					limit: chain_limit,
					interaction: interaction.clone(),
					interaction_chain_start_index: usize::from(prepended_parent_anchor && interaction.is_some()),
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
	if pb_blocker_ignore_count > 0 {
		report.push_info(format!(
			".unavatar dynamics: modular_avatar_pb_blocker_ignores={pb_blocker_ignore_count}"
		));
	}
	if ma_global_collider_count > 0 {
		report.push_info(format!(
			".unavatar dynamics: modular_avatar_global_colliders={ma_global_collider_count}"
		));
	}
	if endpoint_child_count > 0 {
		report.push_info(format!(".unavatar dynamics: synthesized_endpoint_children={endpoint_child_count}"));
	}
	if endpoint_tail_synthesis_failed_count > 0 {
		let samples = if endpoint_tail_synthesis_failed_samples.is_empty() {
			String::new()
		} else {
			format!(" samples=[{}]", endpoint_tail_synthesis_failed_samples.join(", "))
		};
		report.push_warning(format!(
			".unavatar dynamics: could not synthesize endpoint tail for {endpoint_tail_synthesis_failed_count} non-leaf dynamics root(s){samples}"
		));
	}
	if groups.is_empty() && colliders.is_empty() && contacts.is_empty() && constraint_refs.is_empty() {
		None
	} else {
		report.push_info(format!(
			".unavatar dynamics: lowered_groups={} lowered_colliders={} contacts={} constraint_refs={}",
			groups.len(),
			colliders.len(),
			contacts.len(),
			constraint_refs.len()
		));
		Some(UnaSpringBoneSettings {
			groups,
			colliders,
			contacts,
			constraint_refs,
		})
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

fn blend_shape_weight(scene: &UnaSceneSnapshot, node_idx: usize, name: &str) -> Option<f32> {
	let mesh_idx = scene.nodes.get(node_idx).and_then(|node| node.mesh)?;
	let primitives = scene.meshes.get(mesh_idx)?;
	for primitive in primitives {
		let Some(target_idx) = primitive.morph_target_names.iter().position(|candidate| candidate == name) else {
			continue;
		};
		if target_idx >= primitive.morph_targets.len() {
			continue;
		}
		return Some(primitive.default_morph_weights.get(target_idx).copied().unwrap_or(0.0));
	}
	None
}

fn ensure_unique_mesh_for_node(scene: &mut UnaSceneSnapshot, node_idx: usize) -> Option<usize> {
	let mesh_idx = scene.nodes.get(node_idx).and_then(|node| node.mesh)?;
	let users = scene.nodes.iter().filter(|node| node.mesh == Some(mesh_idx)).count();
	if users <= 1 {
		return Some(mesh_idx);
	}
	let mesh = scene.meshes.get(mesh_idx)?.clone();
	scene.meshes.push(mesh);
	let cloned_idx = scene.meshes.len() - 1;
	if let Some(node) = scene.nodes.get_mut(node_idx) {
		node.mesh = Some(cloned_idx);
	}
	Some(cloned_idx)
}

fn expression_catalog_from_morph_target_names(
	scene: &UnaSceneSnapshot,
	allowed_names: Option<&BTreeSet<String>>,
	allowed_normalized_names: Option<&BTreeSet<String>>,
) -> Option<UnaExpressionCatalog> {
	let mut binds_by_name: BTreeMap<String, Vec<UnaMorphTargetBind>> = BTreeMap::new();
	for (mesh_index, primitives) in scene.meshes.iter().enumerate() {
		for (primitive_index, primitive) in primitives.iter().enumerate() {
			for (morph_target_index, name) in primitive.morph_target_names.iter().enumerate() {
				if name.is_empty() || morph_target_index >= primitive.morph_targets.len() {
					continue;
				}
				let exact_allowed = allowed_names.is_some_and(|allowed_names| allowed_names.contains(name));
				let normalized_allowed =
					allowed_normalized_names.is_some_and(|allowed_names| allowed_names.contains(&normalize_expression_match_key(name)));
				if (allowed_names.is_some() || allowed_normalized_names.is_some()) && !exact_allowed && !normalized_allowed {
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

const ARKIT_PERFECT_SYNC_EXPRESSION_NAMES: &[&str] = &[
	"browInnerUp",
	"browDownLeft",
	"browDownRight",
	"browOuterUpLeft",
	"browOuterUpRight",
	"eyeLookUpLeft",
	"eyeLookUpRight",
	"eyeLookDownLeft",
	"eyeLookDownRight",
	"eyeLookInLeft",
	"eyeLookInRight",
	"eyeLookOutLeft",
	"eyeLookOutRight",
	"eyeBlinkLeft",
	"eyeBlinkRight",
	"eyeSquintLeft",
	"eyeSquintRight",
	"eyeWideLeft",
	"eyeWideRight",
	"cheekPuff",
	"cheekSquintLeft",
	"cheekSquintRight",
	"noseSneerLeft",
	"noseSneerRight",
	"jawOpen",
	"jawForward",
	"jawLeft",
	"jawRight",
	"mouthFunnel",
	"mouthPucker",
	"mouthLeft",
	"mouthRight",
	"mouthRollUpper",
	"mouthRollLower",
	"mouthShrugUpper",
	"mouthShrugLower",
	"mouthClose",
	"mouthSmileLeft",
	"mouthSmileRight",
	"mouthFrownLeft",
	"mouthFrownRight",
	"mouthDimpleLeft",
	"mouthDimpleRight",
	"mouthUpperUpLeft",
	"mouthUpperUpRight",
	"mouthLowerDownLeft",
	"mouthLowerDownRight",
	"mouthPressLeft",
	"mouthPressRight",
	"mouthStretchLeft",
	"mouthStretchRight",
	"tongueOut",
];

fn normalize_expression_match_key(name: &str) -> String {
	name.chars()
		.filter(|c| c.is_ascii_alphanumeric())
		.map(|c| c.to_ascii_lowercase())
		.collect()
}

fn arkit_perfect_sync_expression_name_set() -> BTreeSet<String> {
	ARKIT_PERFECT_SYNC_EXPRESSION_NAMES
		.iter()
		.map(|name| normalize_expression_match_key(name))
		.collect()
}

fn expression_weight_names_from_runtime_actions(actions: Option<&UnaRuntimeActionSet>) -> BTreeSet<String> {
	let mut names = BTreeSet::new();
	let Some(actions) = actions else {
		return names;
	};
	for action in &actions.actions {
		for effect in &action.effects {
			if let UnaRuntimeActionEffect::ExpressionWeight { name, .. } = effect {
				if !name.is_empty() {
					names.insert(name.clone());
				}
			}
		}
	}
	names
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WardrobeApplyReport {
	pub active_asset_groups: Vec<String>,
	pub scoped_active_asset_group_count: usize,
	pub scoped_missing_active_asset_groups: Vec<String>,
	pub scoped_resident_mesh_primitive_count: usize,
	pub scoped_resident_material_count: usize,
	pub scoped_resident_image_count: usize,
	pub scoped_resident_dynamics_count: usize,
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

fn refresh_wardrobe_apply_report_scoped_assets(document: &UnaDocument, report: &mut WardrobeApplyReport) {
	let selection = document.scoped_asset_selection();
	report.scoped_active_asset_group_count = selection.owned_active_groups.len();
	report.scoped_missing_active_asset_groups = selection.missing_active_asset_groups;
	report.scoped_resident_mesh_primitive_count = selection.mesh_primitives.len();
	report.scoped_resident_material_count = selection.materials.len();
	report.scoped_resident_image_count = selection.images.len();
	report.scoped_resident_dynamics_count = selection.dynamics_source_ids.len();
}

fn wardrobe_profile_enabled() -> bool {
	std::env::var_os("UN_AVATAR_PROFILE_WARDROBE").is_some()
}

fn log_wardrobe_profile_step(step: &str, started: Instant) {
	if wardrobe_profile_enabled() {
		eprintln!(
			"un-avatar-renderer: wardrobe profile step={step} elapsed={:.1}ms",
			started.elapsed().as_secs_f64() * 1000.0
		);
	}
}

fn apply_unavatar_wardrobe_operations(
	scene: &mut UnaSceneSnapshot,
	dynamics: Option<&mut UnaRuntimeDynamicsMut<'_>>,
	operations: &[Value],
	unavatar: Option<&UnaUnavatarExtension>,
) -> WardrobeApplyReport {
	let lookup = WardrobeLookupContext::new(scene, unavatar);
	apply_unavatar_wardrobe_operations_with_lookup(scene, dynamics, operations, &lookup)
}

fn apply_unavatar_wardrobe_operations_with_lookup(
	scene: &mut UnaSceneSnapshot,
	dynamics: Option<&mut UnaRuntimeDynamicsMut<'_>>,
	operations: &[Value],
	lookup: &WardrobeLookupContext,
) -> WardrobeApplyReport {
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
				let indices = lookup_operation_subtree_targets_all_with_lookup(scene, lookup, op);
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
				let indices = lookup_operation_targets_all(
					&lookup.node_ids,
					&lookup.registry_paths,
					&lookup.paths,
					&lookup.normalized_paths,
					op,
				);
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
				if let Some(idx) = lookup_operation_target(
					&lookup.node_ids,
					&lookup.registry_paths,
					&lookup.paths,
					&lookup.normalized_paths,
					op,
				) {
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
	let mut groups = set
		.get("assetGroups")
		.or_else(|| set.get("asset_groups"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.filter_map(|group| {
			if seen.insert(group.to_string()) {
				Some(group.to_string())
			} else {
				None
			}
		})
		.collect::<Vec<_>>();
	if groups.is_empty() && unavatar_base_wardrobe_set(unavatar).is_some_and(|(base_id, _)| base_id == set_id) {
		groups.push(String::new());
	}
	groups
}

fn merged_wardrobe_asset_groups(base: &[String], selected: &[String]) -> Vec<String> {
	let mut merged = Vec::new();
	let mut seen = BTreeSet::new();
	for group in base.iter().chain(selected.iter()) {
		if seen.insert(group.clone()) {
			merged.push(group.clone());
		}
	}
	merged
}

fn initial_wardrobe_asset_groups(unavatar: &UnaUnavatarExtension, initial_wardrobe_set: Option<&str>) -> Vec<String> {
	let base_id = unavatar_base_wardrobe_set(unavatar).map(|(id, _)| id.to_string());
	let selected_id = initial_wardrobe_set.or(base_id.as_deref());
	let base_asset_groups = if selected_id != base_id.as_deref() {
		base_id
			.as_deref()
			.map(|base_set_id| unavatar_wardrobe_set_asset_groups(unavatar, base_set_id))
			.unwrap_or_default()
	} else {
		Vec::new()
	};
	let selected_asset_groups = selected_id
		.map(|set_id| unavatar_wardrobe_set_asset_groups(unavatar, set_id))
		.unwrap_or_default();
	if selected_id == base_id.as_deref() {
		selected_asset_groups
	} else {
		merged_wardrobe_asset_groups(&base_asset_groups, &selected_asset_groups)
	}
}

fn texture_source_index_from_root(root: &Value, texture_index: usize) -> Option<usize> {
	root.get("textures")
		.and_then(Value::as_array)
		.and_then(|textures| textures.get(texture_index))
		.and_then(|texture| texture.get("source"))
		.and_then(Value::as_u64)
		.map(|value| value as usize)
}

fn gltf_texture_info_image_index(root: &Value, texture_info: Option<&Value>) -> Option<usize> {
	let texture_index = texture_info
		.and_then(|texture_info| texture_info.get("index"))
		.and_then(Value::as_u64)? as usize;
	texture_source_index_from_root(root, texture_index)
}

fn collect_direct_image_texture_indices_from_material_json(value: &Value, out: &mut BTreeSet<usize>) {
	match value {
		Value::Object(object) => {
			for (key, value) in object {
				let key_lower = key.to_ascii_lowercase();
				if (key_lower.ends_with("textureindex") || key_lower.ends_with("texture_index")) && value.as_u64().is_some() {
					out.insert(value.as_u64().unwrap() as usize);
					continue;
				}
				collect_direct_image_texture_indices_from_material_json(value, out);
			}
		}
		Value::Array(values) => {
			for value in values {
				collect_direct_image_texture_indices_from_material_json(value, out);
			}
		}
		_ => {}
	}
}

fn material_image_indices_from_root(root: &Value, material_index: usize) -> BTreeSet<usize> {
	let mut indices = BTreeSet::new();
	let Some(material) = root
		.get("materials")
		.and_then(Value::as_array)
		.and_then(|materials| materials.get(material_index))
	else {
		return indices;
	};
	let pbr = material.get("pbrMetallicRoughness");
	for texture_info in [
		pbr.and_then(|pbr| pbr.get("baseColorTexture")),
		pbr.and_then(|pbr| pbr.get("metallicRoughnessTexture")),
		material.get("normalTexture"),
		material.get("occlusionTexture"),
		material.get("emissiveTexture"),
	] {
		if let Some(image_index) = gltf_texture_info_image_index(root, texture_info) {
			indices.insert(image_index);
		}
	}
	if let Some(extras) = material.get("extras") {
		collect_direct_image_texture_indices_from_material_json(extras, &mut indices);
	}
	indices
}

fn mesh_primitive_material_index_from_root(root: &Value, mesh_index: usize, primitive_index: usize) -> Option<usize> {
	root.get("meshes")
		.and_then(Value::as_array)
		.and_then(|meshes| meshes.get(mesh_index))
		.and_then(|mesh| mesh.get("primitives"))
		.and_then(Value::as_array)
		.and_then(|primitives| primitives.get(primitive_index))
		.and_then(|primitive| primitive.get("material"))
		.and_then(Value::as_u64)
		.map(|value| value as usize)
}

fn initial_resident_image_indices(root: Option<&Value>, initial_wardrobe_set: Option<&str>) -> Option<BTreeSet<usize>> {
	let unavatar = root.and_then(unavatar_extension_from_root)?;
	let ownership = unavatar_asset_group_ownership(&unavatar);
	if ownership.is_empty() {
		return None;
	}
	let root = root?;
	let image_count = root.get("images").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
	let active_groups = initial_wardrobe_asset_groups(&unavatar, initial_wardrobe_set);
	let active_groups = active_groups.into_iter().collect::<BTreeSet<_>>();
	let mut owned_images = BTreeSet::new();
	let mut resident_images = BTreeSet::new();
	let mut active_materials = BTreeSet::new();
	for group in ownership {
		let is_active = active_groups.contains(&group.group_id);
		if is_active {
			active_materials.extend(group.materials.iter().copied());
			for primitive in &group.mesh_primitives {
				if let Some(material_index) = mesh_primitive_material_index_from_root(root, primitive.mesh_index, primitive.primitive_index)
				{
					active_materials.insert(material_index);
				}
			}
		}
		for image in group.images {
			owned_images.insert(image);
			if is_active {
				resident_images.insert(image);
			}
		}
	}
	for material_index in &active_materials {
		resident_images.extend(material_image_indices_from_root(root, *material_index));
	}
	if active_materials.is_empty() {
		for image in 0..image_count {
			if !owned_images.contains(&image) {
				resident_images.insert(image);
			}
		}
	}
	Some(resident_images)
}

fn apply_unavatar_asset_group_ownership(scene: &mut UnaSceneSnapshot, unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	let ownership = unavatar_asset_group_ownership(unavatar);
	report_unavatar_asset_group_ownership_ambiguities(unavatar, report);
	if ownership.is_empty() {
		return;
	}
	scene.asset_group_ownership = ownership;
	let counts = scene.asset_group_ownership_counts();
	report.push_info(format!(
		".unavatar asset groups: ownership_groups={}, mesh_primitives={}, materials={}, images={}, dynamics={}",
		counts.groups, counts.mesh_primitives, counts.materials, counts.images, counts.dynamics
	));
}

fn unavatar_asset_group_ownership(unavatar: &UnaUnavatarExtension) -> Vec<UnaSceneAssetGroupOwnership> {
	let ownership_arrays = [
		unavatar
			.source
			.get("assetGroupOwnership")
			.or_else(|| unavatar.source.get("asset_group_ownership")),
		unavatar.source.get("wardrobe").and_then(|wardrobe| {
			wardrobe
				.get("assetGroupOwnership")
				.or_else(|| wardrobe.get("asset_group_ownership"))
		}),
	];
	ownership_arrays
		.into_iter()
		.flatten()
		.filter_map(Value::as_array)
		.flat_map(|entries| entries.iter().filter_map(unavatar_asset_group_ownership_entry))
		.collect()
}

fn unavatar_asset_group_ownership_entry(value: &Value) -> Option<UnaSceneAssetGroupOwnership> {
	let group_id = value
		.get("groupId")
		.or_else(|| value.get("group_id"))
		.or_else(|| value.get("id"))
		.and_then(Value::as_str)?
		.to_string();
	let mesh_primitives = value
		.get("meshPrimitives")
		.or_else(|| value.get("mesh_primitives"))
		.and_then(Value::as_array)
		.map(|items| items.iter().filter_map(unavatar_mesh_primitive_key).collect())
		.unwrap_or_default();
	let materials = unavatar_usize_array(value.get("materials"));
	let images = unavatar_usize_array(value.get("images").or_else(|| value.get("textures")));
	let dynamics_source_ids = value
		.get("dynamicsSourceIds")
		.or_else(|| value.get("dynamics_source_ids"))
		.or_else(|| value.get("dynamics"))
		.and_then(Value::as_array)
		.map(|items| {
			items
				.iter()
				.filter_map(Value::as_str)
				.filter(|value| !value.is_empty())
				.map(str::to_string)
				.collect()
		})
		.unwrap_or_default();
	Some(UnaSceneAssetGroupOwnership {
		group_id,
		mesh_primitives,
		materials,
		images,
		dynamics_source_ids,
	})
}

fn unavatar_asset_group_ownership_ambiguity_items(unavatar: &UnaUnavatarExtension) -> Vec<&Value> {
	let mut out = Vec::new();
	let sources = [
		unavatar
			.source
			.get("assetGroupOwnershipAmbiguities")
			.or_else(|| unavatar.source.get("asset_group_ownership_ambiguities")),
		unavatar.source.get("wardrobe").and_then(|wardrobe| {
			wardrobe
				.get("assetGroupOwnershipAmbiguities")
				.or_else(|| wardrobe.get("asset_group_ownership_ambiguities"))
		}),
	];
	for source in sources.into_iter().flatten() {
		if let Some(items) = source.get("items").and_then(Value::as_array) {
			out.extend(items.iter());
		} else if let Some(items) = source.as_array() {
			out.extend(items.iter());
		}
	}
	out
}

fn report_unavatar_asset_group_ownership_ambiguities(unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	let mut ambiguity_items = unavatar_asset_group_ownership_ambiguity_items(unavatar)
		.into_iter()
		.map(|item| {
			let source_path = item
				.get("sourcePath")
				.or_else(|| item.get("source_path"))
				.or_else(|| item.get("path"))
				.and_then(Value::as_str)
				.unwrap_or("sourcePath=<unknown>");
			let normalized_path = item
				.get("normalizedPath")
				.or_else(|| item.get("normalized_path"))
				.and_then(Value::as_str)
				.unwrap_or("normalizedPath=<unknown>");
			let candidates = item
				.get("candidateGroups")
				.or_else(|| item.get("candidate_groups"))
				.and_then(Value::as_array)
				.map(|groups| {
					groups
						.iter()
						.filter_map(Value::as_str)
						.filter(|value| !value.is_empty())
						.map(|value| value.to_string())
						.collect::<Vec<_>>()
				})
				.filter(|value| !value.is_empty())
				.unwrap_or_default();
			let candidates = if candidates.is_empty() {
				"candidateGroups=<none>".to_string()
			} else {
				candidates.join(",")
			};
			format!("sourcePath={source_path}, normalizedPath={normalized_path}, candidateGroups=[{candidates}]")
		})
		.filter(|entry| !entry.is_empty())
		.collect::<Vec<_>>();
	if ambiguity_items.is_empty() {
		return;
	}

	let sample_count = ambiguity_items.len().min(4);
	for item in ambiguity_items.drain(0..sample_count) {
		report.push_warning(format!(
			"wardrobe assetGroupOwnershipAmbiguities detected: {item}. Resolve by adding wardrobe.sets[*].assetGroupOwnershipHints(path, groupId)."
		));
	}
	if !ambiguity_items.is_empty() {
		report.push_warning(format!(
			"... and {} additional wardrobe assetGroupOwnershipAmbiguity item(s) omitted; resolve all with wardrobe.sets[*].assetGroupOwnershipHints(path, groupId).",
			ambiguity_items.len()
		));
	}
}

fn unavatar_mesh_primitive_key(value: &Value) -> Option<UnaMeshPrimitiveKey> {
	let mesh_index = value
		.get("meshIndex")
		.or_else(|| value.get("mesh_index"))
		.or_else(|| value.get("mesh"))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok())?;
	let primitive_index = value
		.get("primitiveIndex")
		.or_else(|| value.get("primitive_index"))
		.or_else(|| value.get("primitive"))
		.and_then(Value::as_u64)
		.and_then(|value| usize::try_from(value).ok())?;
	Some(UnaMeshPrimitiveKey {
		mesh_index,
		primitive_index,
	})
}

fn unavatar_usize_array(value: Option<&Value>) -> Vec<usize> {
	value
		.and_then(Value::as_array)
		.map(|items| {
			items
				.iter()
				.filter_map(Value::as_u64)
				.filter_map(|value| usize::try_from(value).ok())
				.collect()
		})
		.unwrap_or_default()
}

fn unavatar_base_wardrobe_set_id(wardrobe: &serde_json::Map<String, Value>) -> Option<&str> {
	wardrobe.get("baseSet").and_then(Value::as_str)
}

fn unavatar_wardrobe_set_is_base(set: &Value, explicit_base_set: Option<&str>) -> bool {
	if let Some(base_set) = explicit_base_set {
		return set.get("id").and_then(Value::as_str) == Some(base_set);
	}
	set.get("default").and_then(Value::as_bool).unwrap_or(false) || set.get("id").and_then(Value::as_str) == Some("")
}

fn unavatar_base_wardrobe_set(unavatar: &UnaUnavatarExtension) -> Option<(&str, &[Value])> {
	let wardrobe = unavatar.source.get("wardrobe").and_then(|v| v.as_object())?;
	let explicit_base_set = unavatar_base_wardrobe_set_id(wardrobe);
	let sets = wardrobe.get("sets").and_then(|v| v.as_array())?;
	let base = sets.iter().find(|set| unavatar_wardrobe_set_is_base(set, explicit_base_set))?;
	let id = base.get("id").and_then(Value::as_str).or(explicit_base_set).unwrap_or("");
	let operations = base.get("operations").and_then(|v| v.as_array()).map(Vec::as_slice)?;
	Some((id, operations))
}

fn unavatar_runtime_action_set(
	unavatar: &UnaUnavatarExtension,
	scene: Option<&UnaSceneSnapshot>,
	enabled_animator_action_ids: &[String],
	animator_action_values: &BTreeMap<String, f32>,
) -> Option<UnaRuntimeActionSet> {
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
					conditions: Vec::new(),
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
	for (component_index, component) in unavatar_modular_avatar_components(unavatar).iter().enumerate() {
		let Some(action) = unavatar_modular_avatar_component_runtime_action(component, component_index, scene, unavatar) else {
			continue;
		};
		if actions.iter().any(|existing| existing.id == action.id) {
			continue;
		}
		actions.push(action);
	}
	if let Some(animator_actions) = unavatar_animator_runtime_actions(unavatar, scene, enabled_animator_action_ids, animator_action_values)
	{
		for action in animator_actions {
			if actions.iter().any(|existing| existing.id == action.id) {
				continue;
			}
			actions.push(action);
		}
	}
	(!actions.is_empty()).then_some(UnaRuntimeActionSet { actions })
}

fn unavatar_animator_runtime_actions(
	unavatar: &UnaUnavatarExtension,
	scene: Option<&UnaSceneSnapshot>,
	profile_enabled_action_ids: &[String],
	profile_action_values: &BTreeMap<String, f32>,
) -> Option<Vec<UnaRuntimeAction>> {
	let animator = unavatar.source.get("animator")?;
	let enabled_action_ids = unavatar_animator_enabled_action_ids(animator, profile_enabled_action_ids);
	if enabled_action_ids.is_empty() {
		return None;
	}
	let controllers = animator.get("controllers").and_then(Value::as_array)?;
	let mut actions = Vec::new();
	for (controller_index, controller) in controllers.iter().enumerate() {
		let motion_base_path = controller
			.get("motionBasePath")
			.or_else(|| controller.get("motion_base_path"))
			.and_then(Value::as_str)
			.unwrap_or("");
		let layers = controller.get("layers").and_then(Value::as_array);
		let Some(layers) = layers else {
			continue;
		};
		for (layer_index, layer) in layers.iter().enumerate() {
			let layer_name = layer.get("name").and_then(Value::as_str).unwrap_or("");
			let states = layer.get("states").and_then(Value::as_array);
			let any_state_transitions = layer.get("anyStateTransitions").and_then(Value::as_array);
			let (Some(states), Some(any_state_transitions)) = (states, any_state_transitions) else {
				continue;
			};
			let mut transitions_by_destination: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
			for transition in any_state_transitions {
				let Some(destination) = transition
					.get("destinationState")
					.and_then(Value::as_str)
					.filter(|value| !value.is_empty())
				else {
					continue;
				};
				transitions_by_destination.entry(destination).or_default().push(transition);
			}
			for state in states {
				if actions.len() >= MAX_UNANIMATOR_ACTIONS {
					break;
				}
				let Some(state_name) = state.get("name").and_then(Value::as_str).filter(|value| !value.is_empty()) else {
					continue;
				};
				let Some(transitions) = transitions_by_destination.get(state_name) else {
					continue;
				};
				let effects = unavatar_animator_state_effects(state, scene, motion_base_path);
				if effects.is_empty() {
					continue;
				}
				for (transition_index, transition) in transitions.iter().enumerate() {
					if actions.len() >= MAX_UNANIMATOR_ACTIONS {
						break;
					}
					let Some((parameter_name, parameter_value, conditions)) = unavatar_animator_transition_parameter_trigger(transition)
					else {
						continue;
					};
					let state_path = state.get("path").and_then(Value::as_str).unwrap_or(state_name);
					let label = if layer_name.is_empty() {
						state_path.to_string()
					} else {
						format!("{layer_name} / {state_path}")
					};
					let command = format!(
						"animator:{controller_index}:{layer_index}:{}:{transition_index}",
						stable_identifier(state_path)
					);
					if !enabled_action_ids.contains(command.as_str()) {
						continue;
					}
					let parameter_value = profile_action_values
						.get(command.as_str())
						.copied()
						.filter(|value| value.is_finite())
						.unwrap_or(parameter_value);
					actions.push(UnaRuntimeAction {
						id: command.clone(),
						label: unavatar_animator_action_label(&label),
						triggers: vec![
							UnaRuntimeActionTrigger::SupervisorCommand { command },
							UnaRuntimeActionTrigger::ParameterValue {
								name: parameter_name,
								value: parameter_value,
							},
						],
						conditions,
						effects: effects.iter().take(MAX_UNANIMATOR_EFFECTS_PER_ACTION).cloned().collect(),
					});
				}
			}
		}
	}
	(!actions.is_empty()).then_some(actions)
}

fn unavatar_animator_enabled_action_ids<'a>(animator: &'a Value, profile_enabled_action_ids: &'a [String]) -> BTreeSet<&'a str> {
	let mut ids = animator
		.get("enabledActionIds")
		.or_else(|| animator.get("enabled_action_ids"))
		.and_then(Value::as_array)
		.into_iter()
		.flatten()
		.filter_map(Value::as_str)
		.filter(|value| !value.is_empty())
		.collect::<BTreeSet<_>>();
	ids.extend(
		profile_enabled_action_ids
			.iter()
			.map(String::as_str)
			.filter(|value| !value.is_empty()),
	);
	ids
}

fn unavatar_animator_transition_parameter_trigger(transition: &Value) -> Option<(String, f32, Vec<UnaRuntimeActionCondition>)> {
	let conditions = transition.get("conditions").and_then(Value::as_array)?;
	let mut out = Vec::new();
	let mut trigger = None;
	for condition in conditions {
		let Some(name) = condition.get("parameter").and_then(Value::as_str).filter(|value| !value.is_empty()) else {
			continue;
		};
		let mode = condition.get("mode").and_then(Value::as_str).unwrap_or("");
		let threshold = condition
			.get("threshold")
			.and_then(Value::as_f64)
			.map(|value| value as f32)
			.unwrap_or(0.0);
		let (value, inverted) = match mode {
			"If" => (1.0, false),
			"IfNot" => (0.0, false),
			"Equals" => (threshold, false),
			"NotEqual" => (threshold, true),
			"Greater" => (threshold, false),
			"Less" => (threshold, true),
			_ => (threshold, false),
		};
		if trigger.is_none() {
			trigger = Some((name.to_string(), value));
		}
		out.push(UnaRuntimeActionCondition {
			parameter_name: Some(name.to_string()),
			parameter_value: Some(value),
			inverted,
			..Default::default()
		});
	}
	let (name, value) = trigger?;
	Some((name, value, out))
}

fn unavatar_animator_state_effects(state: &Value, scene: Option<&UnaSceneSnapshot>, motion_base_path: &str) -> Vec<UnaRuntimeActionEffect> {
	let mut effects = Vec::new();
	if let Some(motion) = state.get("motion") {
		unavatar_animator_motion_effects(motion, scene, motion_base_path, &mut effects);
	}
	effects
}

fn unavatar_animator_motion_effects(
	motion: &Value,
	scene: Option<&UnaSceneSnapshot>,
	motion_base_path: &str,
	effects: &mut Vec<UnaRuntimeActionEffect>,
) {
	if effects.len() >= MAX_UNANIMATOR_EFFECTS_PER_ACTION {
		return;
	}
	if let Some(bindings) = motion.get("curveBindings").and_then(Value::as_array) {
		for binding in bindings {
			if effects.len() >= MAX_UNANIMATOR_EFFECTS_PER_ACTION {
				break;
			}
			if let Some(effect) = unavatar_animator_curve_binding_effect(binding, scene, motion_base_path) {
				effects.push(effect);
			}
		}
	}
	if let Some(children) = motion.get("children").and_then(Value::as_array) {
		for child in children {
			if effects.len() >= MAX_UNANIMATOR_EFFECTS_PER_ACTION {
				break;
			}
			unavatar_animator_motion_effects(child, scene, motion_base_path, effects);
		}
	}
}

fn unavatar_animator_curve_binding_effect(
	binding: &Value,
	scene: Option<&UnaSceneSnapshot>,
	motion_base_path: &str,
) -> Option<UnaRuntimeActionEffect> {
	let property = binding.get("propertyName").and_then(Value::as_str)?;
	let value = unavatar_animator_binding_value(binding)?;
	match property {
		"m_IsActive" | "m_Enabled" => {
			let target = unavatar_animator_binding_node_target(binding, scene, motion_base_path)?;
			Some(UnaRuntimeActionEffect::NodeVisibility {
				target,
				visible: value > 0.5,
			})
		}
		_ if property.starts_with("blendShape.") => {
			let name = property.trim_start_matches("blendShape.").trim();
			if name.is_empty() {
				return None;
			}
			let weight = if value > 1.0 { value / 100.0 } else { value };
			Some(UnaRuntimeActionEffect::ExpressionWeight {
				name: name.to_string(),
				weight,
			})
		}
		_ => None,
	}
}

fn unavatar_animator_binding_value(binding: &Value) -> Option<f32> {
	binding
		.get("constantValue")
		.or_else(|| binding.get("lastValue"))
		.or_else(|| binding.get("firstValue"))
		.and_then(Value::as_f64)
		.map(|value| value as f32)
}

fn unavatar_animator_binding_node_target(
	binding: &Value,
	scene: Option<&UnaSceneSnapshot>,
	motion_base_path: &str,
) -> Option<UnaRuntimeNodeTarget> {
	let path = binding
		.get("path")
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(|value| unavatar_animator_resolve_binding_path(motion_base_path, value));
	if let (Some(scene), Some(path)) = (scene, path.as_deref()) {
		if let Some((_, node)) = scene.nodes.iter().enumerate().find(|(index, _)| {
			scene_node_path_for_index(scene, *index)
				.as_deref()
				.is_some_and(|scene_path| scene_path == path || scene_path.ends_with(&format!("/{path}")))
		}) {
			return Some(UnaRuntimeNodeTarget {
				node_index: None,
				source_node_id: node.source_node_id.clone(),
				resolved_node_id: node.resolved_node_id.clone(),
				path: Some(path.to_string()),
			});
		}
	}
	path.map(|path| UnaRuntimeNodeTarget {
		node_index: None,
		source_node_id: None,
		resolved_node_id: None,
		path: Some(path),
	})
}

fn unavatar_animator_resolve_binding_path(motion_base_path: &str, binding_path: &str) -> String {
	let binding_path = binding_path.trim_matches('/');
	if binding_path.is_empty() {
		return motion_base_path.trim_matches('/').to_string();
	}
	let motion_base_path = motion_base_path.trim_matches('/');
	if motion_base_path.is_empty() || binding_path.starts_with(motion_base_path) {
		binding_path.to_string()
	} else {
		format!("{motion_base_path}/{binding_path}")
	}
}

fn unavatar_animator_action_label(label: &str) -> String {
	label
		.replace(" / ", "/")
		.split('/')
		.filter(|segment| !segment.trim().is_empty())
		.map(str::trim)
		.collect::<Vec<_>>()
		.join(" / ")
}

fn stable_identifier(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	for ch in value.chars() {
		if ch.is_ascii_alphanumeric() {
			out.push(ch.to_ascii_lowercase());
		} else if !out.ends_with('_') {
			out.push('_');
		}
	}
	out.trim_matches('_').to_string()
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
		conditions: Vec::new(),
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
		"ModularAvatarObjectToggle" => unavatar_object_toggle_runtime_action(component, component_index, scene, unavatar),
		_ => None,
	}
}

fn unavatar_object_toggle_runtime_action(
	component: &Value,
	component_index: usize,
	scene: Option<&UnaSceneSnapshot>,
	unavatar: &UnaUnavatarExtension,
) -> Option<UnaRuntimeAction> {
	let objects = unavatar_modular_avatar_component_array(component, &["Objects", "objects", "m_objects"])?;
	let mut effects = Vec::new();
	for object in objects {
		let Some(target) = unavatar_object_toggle_node_target(object, scene, unavatar) else {
			continue;
		};
		let Some(visible) = object
			.get("Active")
			.or_else(|| object.get("active"))
			.or_else(|| object.get("visible"))
			.or_else(|| object.get("enabled"))
			.and_then(Value::as_bool)
		else {
			continue;
		};
		effects.push(UnaRuntimeActionEffect::NodeVisibility { target, visible });
	}
	if effects.is_empty() {
		return None;
	}
	let component_id = unavatar_modular_avatar_component_id(component, component_index);
	let label = unavatar_modular_avatar_component_label(component, "Object Toggle");
	let command = format!("ma:object_toggle:{component_id}");
	Some(UnaRuntimeAction {
		id: command.clone(),
		label,
		triggers: unavatar_modular_avatar_component_triggers(component, command),
		conditions: unavatar_modular_avatar_component_conditions(component, scene, unavatar),
		effects,
	})
}

fn unavatar_object_toggle_node_target(
	object: &Value,
	scene: Option<&UnaSceneSnapshot>,
	unavatar: &UnaUnavatarExtension,
) -> Option<UnaRuntimeNodeTarget> {
	let target_ref = object
		.get("Object")
		.or_else(|| object.get("object"))
		.or_else(|| object.get("target"))
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
	if let Some(scene) = scene {
		let node_ids = scene_node_ids(scene);
		let registry_paths = unavatar_node_registry_paths(Some(unavatar));
		let paths = scene_node_paths(scene);
		let normalized_paths = scene_node_normalized_paths(scene);
		let node_index = modular_avatar_reference_index(target_ref, &node_ids, &registry_paths, &paths, &normalized_paths)?;
		let node = scene.nodes.get(node_index)?;
		return Some(UnaRuntimeNodeTarget {
			node_index: None,
			source_node_id: node.source_node_id.clone(),
			resolved_node_id: node.resolved_node_id.clone(),
			path: scene_node_path_for_index(scene, node_index),
		});
	}
	Some(UnaRuntimeNodeTarget {
		node_index: None,
		source_node_id,
		resolved_node_id: None,
		path,
	})
}

fn unavatar_material_setter_runtime_action(
	component: &Value,
	component_index: usize,
	scene: Option<&UnaSceneSnapshot>,
	unavatar: &UnaUnavatarExtension,
) -> Option<UnaRuntimeAction> {
	let objects = unavatar_modular_avatar_component_array(
		component,
		&[
			"Objects",
			"objects",
			"m_objects",
			"materialSwitchObjects",
			"material_switch_objects",
		],
	)?;
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
	let component_id = unavatar_modular_avatar_component_id(component, component_index);
	let label = unavatar_modular_avatar_component_label(component, "Material Setter");
	let command = format!("ma:material_setter:{component_id}");
	Some(UnaRuntimeAction {
		id: command.clone(),
		label,
		triggers: unavatar_modular_avatar_component_triggers(component, command),
		conditions: unavatar_modular_avatar_component_conditions(component, scene, unavatar),
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
	let swaps = unavatar_modular_avatar_component_array(component, &["Swaps", "swaps", "m_swaps"])?;
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
	let component_id = unavatar_modular_avatar_component_id(component, component_index);
	let label = unavatar_modular_avatar_component_label(component, "Material Swap");
	let command = format!("ma:material_swap:{component_id}");
	Some(UnaRuntimeAction {
		id: command.clone(),
		label,
		triggers: unavatar_modular_avatar_component_triggers(component, command),
		conditions: unavatar_modular_avatar_component_conditions(component, Some(scene), unavatar),
		effects,
	})
}

fn unavatar_modular_avatar_component_id(component: &Value, component_index: usize) -> String {
	component
		.get("id")
		.or_else(|| component.get("componentId"))
		.or_else(|| component.get("component_id"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string)
		.unwrap_or_else(|| component_index.to_string())
}

fn unavatar_modular_avatar_component_array<'a>(component: &'a Value, names: &[&str]) -> Option<&'a Vec<Value>> {
	component
		.get("fields")
		.and_then(|fields| names.iter().find_map(|name| fields.get(*name)))
		.or_else(|| names.iter().find_map(|name| component.get(*name)))
		.and_then(Value::as_array)
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

fn unavatar_modular_avatar_component_conditions(
	component: &Value,
	scene: Option<&UnaSceneSnapshot>,
	unavatar: &UnaUnavatarExtension,
) -> Vec<UnaRuntimeActionCondition> {
	let source_component_id = component
		.get("id")
		.or_else(|| component.get("componentId"))
		.or_else(|| component.get("component_id"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string);
	let source_node = unavatar_modular_avatar_component_source_node(component, scene, unavatar);
	let active_parent_nodes = scene
		.zip(source_node.as_ref())
		.and_then(|(scene, source_node)| resolve_unavatar_runtime_node_target(scene, source_node).map(|index| (scene, index)))
		.map(|(scene, index)| {
			let parents = scene_parent_indices(scene);
			let mut out = Vec::new();
			let mut cursor = parents.get(index).copied().flatten();
			while let Some(parent) = cursor {
				if let Some(node) = scene.nodes.get(parent) {
					out.push(UnaRuntimeNodeTarget {
						node_index: None,
						source_node_id: node.source_node_id.clone(),
						resolved_node_id: node.resolved_node_id.clone(),
						path: scene_node_path_for_index(scene, parent),
					});
				}
				cursor = parents.get(parent).copied().flatten();
			}
			out
		})
		.unwrap_or_default();
	let (parameter_name, parameter_value) = unavatar_modular_avatar_component_parameter_value(component)
		.map(|(name, value)| (Some(name), Some(value)))
		.unwrap_or((None, None));
	let condition = UnaRuntimeActionCondition {
		source_component_id,
		source_node,
		parameter_name,
		parameter_value,
		sub_parameter_names: unavatar_modular_avatar_component_sub_parameter_names(component),
		inverted: unavatar_modular_avatar_component_inverted(component),
		active_parent_nodes,
	};
	(condition.inverted
		|| condition.source_component_id.is_some()
		|| condition.source_node.is_some()
		|| condition.parameter_name.is_some()
		|| !condition.sub_parameter_names.is_empty()
		|| !condition.active_parent_nodes.is_empty())
	.then_some(condition)
	.into_iter()
	.collect()
}

fn unavatar_modular_avatar_component_source_node(
	component: &Value,
	scene: Option<&UnaSceneSnapshot>,
	unavatar: &UnaUnavatarExtension,
) -> Option<UnaRuntimeNodeTarget> {
	let target_ref = component
		.get("target")
		.or_else(|| component.get("resolvedTarget"))
		.or_else(|| component.get("gameObject"))
		.or_else(|| component.get("GameObject"))?;
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
	if let Some(scene) = scene {
		let node_ids = scene_node_ids(scene);
		let registry_paths = unavatar_node_registry_paths(Some(unavatar));
		let paths = scene_node_paths(scene);
		let normalized_paths = scene_node_normalized_paths(scene);
		if let Some(node_index) = modular_avatar_reference_index(target_ref, &node_ids, &registry_paths, &paths, &normalized_paths) {
			if let Some(node) = scene.nodes.get(node_index) {
				return Some(UnaRuntimeNodeTarget {
					node_index: None,
					source_node_id: node.source_node_id.clone(),
					resolved_node_id: node.resolved_node_id.clone(),
					path: scene_node_path_for_index(scene, node_index),
				});
			}
		}
	}
	(source_node_id.is_some() || path.is_some()).then_some(UnaRuntimeNodeTarget {
		node_index: None,
		source_node_id,
		resolved_node_id: None,
		path,
	})
}

fn unavatar_modular_avatar_component_label(component: &Value, fallback: &str) -> String {
	unavatar_named_value(component)
		.or_else(|| component.get("fields").and_then(unavatar_named_value))
		.or_else(|| {
			component.get("fields").and_then(|fields| {
				fields
					.get("menuItem")
					.or_else(|| fields.get("menu_item"))
					.and_then(unavatar_menu_item_label)
			})
		})
		.or_else(|| {
			component
				.get("menuItem")
				.or_else(|| component.get("menu_item"))
				.and_then(unavatar_menu_item_label)
		})
		.unwrap_or(fallback)
		.to_string()
}

fn unavatar_named_value(value: &Value) -> Option<&str> {
	value
		.get("name")
		.or_else(|| value.get("Name"))
		.or_else(|| value.get("displayName"))
		.or_else(|| value.get("display_name"))
		.or_else(|| value.get("DisplayName"))
		.or_else(|| value.get("label"))
		.or_else(|| value.get("Label"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
}

fn unavatar_menu_item_label(menu_item: &Value) -> Option<&str> {
	unavatar_named_value(menu_item).or_else(|| {
		menu_item
			.get("control")
			.or_else(|| menu_item.get("Control"))
			.and_then(unavatar_named_value)
	})
}

fn unavatar_modular_avatar_component_expression_menu_path(component: &Value) -> Option<String> {
	unavatar_explicit_expression_menu_path(component)
		.or_else(|| {
			component.get("fields").and_then(|fields| {
				unavatar_explicit_expression_menu_path(fields)
					.or_else(|| fields.get("menuItem").and_then(unavatar_menu_item_expression_menu_path))
					.or_else(|| fields.get("menu_item").and_then(unavatar_menu_item_expression_menu_path))
			})
		})
		.or_else(|| {
			component
				.get("menuItem")
				.or_else(|| component.get("menu_item"))
				.and_then(unavatar_menu_item_expression_menu_path)
		})
}

fn add_expression_catalog_bind(
	catalog: &mut UnaExpressionCatalog,
	preset_name: &str,
	mesh_index: usize,
	primitive_index: usize,
	morph_target_index: usize,
	weight_scale: f32,
) -> bool {
	if weight_scale.abs() <= f32::EPSILON {
		return false;
	}
	let Some(preset) = catalog.presets.iter_mut().find(|preset| preset.name == preset_name) else {
		catalog.presets.push(UnaExpressionPreset {
			name: preset_name.to_string(),
			binds: vec![UnaMorphTargetBind {
				mesh_index,
				primitive_index,
				morph_target_index,
				weight_scale,
			}],
		});
		return true;
	};
	if preset.binds.iter().any(|bind| {
		bind.mesh_index == mesh_index && bind.primitive_index == primitive_index && bind.morph_target_index == morph_target_index
	}) {
		return false;
	}
	preset.binds.push(UnaMorphTargetBind {
		mesh_index,
		primitive_index,
		morph_target_index,
		weight_scale,
	});
	true
}

fn unavatar_explicit_expression_menu_path(value: &Value) -> Option<String> {
	value
		.get("expressionMenuPath")
		.or_else(|| value.get("expression_menu_path"))
		.or_else(|| value.get("ExpressionMenuPath"))
		.or_else(|| value.get("menuPath"))
		.or_else(|| value.get("menu_path"))
		.or_else(|| value.get("MenuPath"))
		.and_then(Value::as_str)
		.filter(|path| !path.is_empty())
		.map(str::to_string)
}

fn unavatar_menu_item_expression_menu_path(menu_item: &Value) -> Option<String> {
	unavatar_explicit_expression_menu_path(menu_item)
		.or_else(|| {
			menu_item
				.get("path")
				.or_else(|| menu_item.get("Path"))
				.and_then(Value::as_str)
				.filter(|path| !path.is_empty())
				.map(str::to_string)
		})
		.or_else(|| {
			menu_item
				.get("control")
				.or_else(|| menu_item.get("Control"))
				.and_then(unavatar_explicit_expression_menu_path)
		})
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

fn unavatar_modular_avatar_component_sub_parameter_names(component: &Value) -> Vec<String> {
	unavatar_explicit_sub_parameter_names(component)
		.or_else(|| {
			component.get("fields").and_then(|fields| {
				unavatar_explicit_sub_parameter_names(fields)
					.or_else(|| fields.get("control").and_then(unavatar_explicit_sub_parameter_names))
					.or_else(|| fields.get("Control").and_then(unavatar_explicit_sub_parameter_names))
					.or_else(|| fields.get("menuItem").and_then(unavatar_menu_item_sub_parameter_names))
					.or_else(|| fields.get("menu_item").and_then(unavatar_menu_item_sub_parameter_names))
			})
		})
		.or_else(|| {
			component
				.get("control")
				.or_else(|| component.get("Control"))
				.and_then(unavatar_explicit_sub_parameter_names)
		})
		.or_else(|| {
			component
				.get("menuItem")
				.or_else(|| component.get("menu_item"))
				.and_then(unavatar_menu_item_sub_parameter_names)
		})
		.unwrap_or_default()
}

fn unavatar_menu_item_sub_parameter_names(menu_item: &Value) -> Option<Vec<String>> {
	unavatar_explicit_sub_parameter_names(menu_item).or_else(|| {
		menu_item
			.get("control")
			.or_else(|| menu_item.get("Control"))
			.and_then(unavatar_explicit_sub_parameter_names)
	})
}

fn unavatar_explicit_sub_parameter_names(value: &Value) -> Option<Vec<String>> {
	let parameters = value
		.get("subParameters")
		.or_else(|| value.get("sub_parameters"))
		.or_else(|| value.get("SubParameters"))?
		.as_array()?;
	let names = parameters
		.iter()
		.filter_map(|parameter| {
			parameter
				.as_str()
				.or_else(|| parameter.get("name").and_then(Value::as_str))
				.or_else(|| parameter.get("Name").and_then(Value::as_str))
		})
		.filter(|value| !value.is_empty())
		.map(str::to_string)
		.collect::<Vec<_>>();
	(!names.is_empty()).then_some(names)
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
		.or_else(|| value.get("Parameter"))
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
	let explicit_base_set = unavatar_base_wardrobe_set_id(wardrobe);
	let Some(sets) = wardrobe.get("sets").and_then(|v| v.as_array()) else {
		return Vec::new();
	};
	let Some(base) = sets.iter().find(|set| unavatar_wardrobe_set_is_base(set, explicit_base_set)) else {
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

fn normalized_path_is_strict_descendant_of_any(path: &str, normalized_ancestors: &[String]) -> bool {
	let path = normalize_unavatar_path(path);
	normalized_ancestors
		.iter()
		.any(|ancestor| !ancestor.is_empty() && ancestor != &path && path.strip_prefix(ancestor).is_some_and(|rest| rest.starts_with('/')))
}

fn base_operation_is_inherited_hidden_under_base_resolved(
	op: &Value,
	base_hidden_normalized_paths: &[String],
	resolved: &[usize],
	paths_by_index: &[Option<String>],
) -> bool {
	let path = operation_target_path(op);
	if path.is_empty() {
		return false;
	}
	if resolved.is_empty() {
		return normalized_path_is_strict_descendant_of_any(path, base_hidden_normalized_paths);
	}
	resolved.iter().all(|idx| {
		paths_by_index
			.get(*idx)
			.and_then(|p| p.as_deref())
			.is_some_and(|resolved_path| normalized_path_is_strict_descendant_of_any(resolved_path, base_hidden_normalized_paths))
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

fn scene_find_child_path(scene: &UnaSceneSnapshot, start: usize, sub_path: &str) -> Option<usize> {
	let mut current = start;
	for segment in sub_path.split('/').map(str::trim).filter(|segment| !segment.is_empty()) {
		let node = scene.nodes.get(current)?;
		current = node
			.children
			.iter()
			.copied()
			.find(|child| scene.nodes.get(*child).and_then(|node| node.name.as_deref()) == Some(segment))?;
	}
	Some(current)
}

fn modular_avatar_bone_proxy_target_index(
	scene: &UnaSceneSnapshot,
	component: &Value,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	humanoid_profile: Option<&HumanoidProfile>,
) -> Option<usize> {
	if let Some(resolved_ref) = component.get("resolvedTarget") {
		if let Some(idx) = unavatar_node_ref_index(resolved_ref, node_ids, registry_paths, paths, normalized_paths) {
			return Some(idx);
		}
	}

	let bone_reference = modular_avatar_component_string(component, &["boneReference", "BoneReference", "m_boneReference"])?;
	let sub_path = modular_avatar_component_string(component, &["subPath", "SubPath", "m_subPath"]).unwrap_or_default();
	let sub_path = sub_path.trim();
	let avatar_root = scene.roots.first().copied()?;
	if sub_path == "$$AVATAR" {
		return Some(avatar_root);
	}
	if bone_reference == "LastBone" {
		return (!sub_path.is_empty())
			.then(|| scene_find_child_path(scene, avatar_root, sub_path))
			.flatten();
	}
	let humanoid = humanoid_profile?;
	let bone = humanoid.bone_node_indices.get(&bone_reference.to_ascii_lowercase()).copied()?;
	if sub_path.is_empty() {
		Some(bone)
	} else {
		scene_find_child_path(scene, bone, sub_path)
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
		for source in &mut constraint.sources {
			if source.source_node == old_node {
				source.source_node = new_node;
			}
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

#[derive(Clone, Debug)]
struct MergeArmatureComponentMapping {
	target_node: usize,
	mappings: Vec<(usize, usize)>,
}

fn collect_merge_armature_bone_mappings(
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> (Vec<MergeArmatureComponentMapping>, usize, usize) {
	let mut merge_components = Vec::new();
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
		let Some(target_ref) = component.get("target") else {
			missing += 1;
			continue;
		};
		let Some(target_node) = unavatar_node_ref_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
			missing += 1;
			continue;
		};
		let Some(bone_mappings) = component.get("boneMappings").and_then(|v| v.as_array()) else {
			missing += 1;
			continue;
		};
		let mut mappings = Vec::new();
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
				mappings.push((source, target));
			}
		}
		if !mappings.is_empty() {
			merge_components.push(MergeArmatureComponentMapping { target_node, mappings });
		}
	}
	(merge_components, missing, skipped)
}

fn order_merge_armature_components(components: &[MergeArmatureComponentMapping], parents: &[Option<usize>]) -> (Vec<usize>, usize) {
	let total = components.len();
	if total == 0 {
		return (Vec::new(), 0);
	}
	let mut predecessors = vec![Vec::new(); total];
	for i in 0..total {
		for j in 0..total {
			if i == j {
				continue;
			}
			if scene_is_descendant_of(parents, components[j].target_node, components[i].target_node) && !predecessors[j].contains(&i) {
				predecessors[j].push(i);
			}
		}
	}
	let mut ordered = Vec::with_capacity(total);
	let mut used = vec![false; total];
	loop {
		let mut progressed = false;
		for index in 0..total {
			if used[index] {
				continue;
			}
			if predecessors[index].iter().all(|dependency| used[*dependency]) {
				used[index] = true;
				ordered.push(index);
				progressed = true;
			}
		}
		if !progressed {
			break;
		}
		if ordered.len() == total {
			break;
		}
	}
	let cycle_count = if ordered.len() == total {
		0
	} else {
		let mut count = 0usize;
		for (index, used) in used.iter().enumerate() {
			if !*used {
				ordered.push(index);
				count += 1;
			}
		}
		count
	};
	(ordered, cycle_count)
}

fn count_merge_armature_cycle_nodes(mappings: &[(usize, usize)]) -> usize {
	if mappings.len() < 2 {
		return 0;
	}
	let mut outgoing: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
	let mut indegree: BTreeMap<usize, usize> = BTreeMap::new();
	for &(source, target) in mappings {
		if source == target {
			continue;
		}
		outgoing.entry(source).or_default().insert(target);
		indegree.entry(source).or_insert(0);
		indegree.entry(target).or_insert(0);
	}
	for targets in outgoing.values() {
		for &target in targets {
			if let Some(entry) = indegree.get_mut(&target) {
				*entry = entry.saturating_add(1);
			}
		}
	}
	let mut ready = Vec::new();
	for (&node, &degree) in &indegree {
		if degree == 0 {
			ready.push(node);
		}
	}
	let mut processed = BTreeSet::new();
	while let Some(node) = ready.pop() {
		processed.insert(node);
		if let Some(targets) = outgoing.get(&node) {
			for &target in targets {
				if let Some(entry) = indegree.get_mut(&target) {
					*entry = entry.saturating_sub(1);
					if *entry == 0 {
						ready.push(target);
					}
				}
			}
		}
	}
	indegree.len().saturating_sub(processed.len())
}

fn retarget_merge_armature_skins(scene: &mut UnaSceneSnapshot, mappings: &[(usize, usize)]) -> usize {
	if mappings.is_empty() {
		return 0;
	}
	let world = scene_world_matrices(scene);
	let mut resolved = BTreeMap::new();
	for &(source_node, target_node) in mappings {
		if source_node != target_node {
			resolved.insert(source_node, target_node);
		}
	}
	let mut retargeted = 0usize;
	for skin in &mut scene.skins {
		for joint_idx in 0..skin.joint_nodes.len() {
			let source_node = skin.joint_nodes[joint_idx];
			let Some(&target_node) = resolved.get(&source_node) else {
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
			if let Some(&target_node) = resolved.get(&skeleton_node) {
				skin.skeleton_node = Some(target_node);
			}
		}
	}
	retargeted
}

fn retarget_merge_armature_dynamics(settings: &mut UnaDynamicsSettings, mappings: &[(usize, usize)]) -> usize {
	if mappings.is_empty() {
		return 0;
	}
	let mut resolved = BTreeMap::new();
	for &(source_node, target_node) in mappings {
		if source_node != target_node {
			resolved.insert(source_node, target_node);
		}
	}
	if resolved.is_empty() {
		return 0;
	}
	let mut retargeted = 0usize;
	for group in &mut settings.groups {
		if let Some(center_node) = group.center_node {
			if let Some(&target_node) = resolved.get(&center_node) {
				group.center_node = Some(target_node);
				retargeted += 1;
			}
		}
		for node in &mut group.bone_node_indices {
			if let Some(&target_node) = resolved.get(node) {
				*node = target_node;
				retargeted += 1;
			}
		}
		group.interaction_chain_start_index = group.interaction_chain_start_index.min(group.bone_node_indices.len());
	}
	for collider in &mut settings.colliders {
		if let Some(&target_node) = resolved.get(&collider.node) {
			collider.node = target_node;
			retargeted += 1;
		}
	}
	for contact in &mut settings.contacts {
		if let Some(&target_node) = resolved.get(&contact.node) {
			contact.node = target_node;
			retargeted += 1;
		}
	}
	for constraint in &mut settings.constraint_refs {
		if let Some(&target_node) = resolved.get(&constraint.target_node) {
			constraint.target_node = target_node;
			retargeted += 1;
		}
		for source_node in &mut constraint.source_nodes {
			if let Some(&target_node) = resolved.get(source_node) {
				*source_node = target_node;
				retargeted += 1;
			}
		}
	}
	retargeted
}

fn retarget_merge_armature_node_constraint_sources(scene: &mut UnaSceneSnapshot, mappings: &[(usize, usize)]) -> usize {
	if mappings.is_empty() {
		return 0;
	}
	let mut resolved = BTreeMap::new();
	for &(source_node, target_node) in mappings {
		if source_node != target_node {
			resolved.insert(source_node, target_node);
		}
	}
	if resolved.is_empty() {
		return 0;
	}
	let mut retargeted = 0usize;
	for constraint in &mut scene.node_constraints {
		if let Some(&target_node) = resolved.get(&constraint.source_node) {
			constraint.source_node = target_node;
			retargeted += 1;
		}
		for source in &mut constraint.sources {
			if let Some(&target_node) = resolved.get(&source.source_node) {
				source.source_node = target_node;
				retargeted += 1;
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

fn subtree_contains_mapped_node(scene: &UnaSceneSnapshot, node: usize, mappings: &[(usize, usize)]) -> bool {
	if mappings.iter().any(|(source, _)| *source == node) {
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

fn subtree_contains_marked_node(scene: &UnaSceneSnapshot, node: usize, marked_nodes: &BTreeSet<usize>) -> bool {
	if marked_nodes.contains(&node) {
		return true;
	}
	let Some(scene_node) = scene.nodes.get(node) else {
		return false;
	};
	scene_node
		.children
		.iter()
		.any(|&child| child < scene.nodes.len() && subtree_contains_marked_node(scene, child, marked_nodes))
}

fn collect_modular_avatar_reference_indices(
	value: &Value,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	indices: &mut BTreeSet<usize>,
) {
	if let Some(index) = modular_avatar_reference_index(value, node_ids, registry_paths, paths, normalized_paths) {
		indices.insert(index);
	}

	match value {
		Value::Array(values) => {
			for nested in values {
				collect_modular_avatar_reference_indices(nested, node_ids, registry_paths, paths, normalized_paths, indices);
			}
		}
		Value::Object(entries) => {
			for nested in entries.values() {
				collect_modular_avatar_reference_indices(nested, node_ids, registry_paths, paths, normalized_paths, indices);
			}
		}
		_ => {}
	}
}

fn collect_merge_armature_retain_nodes(
	scene: &UnaSceneSnapshot,
	components: &[Value],
	_unavatar: &UnaUnavatarExtension,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> BTreeSet<usize> {
	let mut retained_nodes = BTreeSet::new();
	for skin in &scene.skins {
		if let Some(skeleton_node) = skin.skeleton_node {
			retained_nodes.insert(skeleton_node);
		}
	}
	for constraint in &scene.node_constraints {
		retained_nodes.insert(constraint.target_node);
	}
	for component in components {
		collect_modular_avatar_reference_indices(component, node_ids, registry_paths, paths, normalized_paths, &mut retained_nodes);
	}
	retained_nodes.retain(|index| *index < scene.nodes.len());
	retained_nodes
}

fn reparent_merge_armature_auxiliary_bones(
	scene: &mut UnaSceneSnapshot,
	mappings: &[(usize, usize)],
	retained_nodes: &BTreeSet<usize>,
) -> usize {
	if mappings.is_empty() {
		return 0;
	}
	let initial_world = scene_world_matrices(scene);
	let mut reparent_ops = Vec::new();
	for &(source_node, target_node) in mappings {
		let Some(source) = scene.nodes.get(source_node) else {
			continue;
		};
		for &child in &source.children {
			if child >= scene.nodes.len()
				|| subtree_contains_mapped_node(scene, child, mappings)
				|| subtree_contains_marked_node(scene, child, retained_nodes)
			{
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
	let mapping_pairs = mappings.iter().map(|(&source, &target)| (source, target)).collect::<Vec<_>>();
	let auxiliary_reparented = reparent_merge_armature_auxiliary_bones(scene, &mapping_pairs, &BTreeSet::new());
	let retargeted = retarget_merge_armature_skins(scene, &mapping_pairs);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MeshSettingsInheritMode {
	Set,
	Inherit,
	DontSet,
	SetOrInherit,
}

impl MeshSettingsInheritMode {
	fn from_str(value: Option<&str>) -> Self {
		match value {
			Some("Set") => Self::Set,
			Some("DontSet") => Self::DontSet,
			Some("SetOrInherit") => Self::SetOrInherit,
			_ => Self::Inherit,
		}
	}

	fn not_final(self) -> bool {
		matches!(self, Self::Inherit | Self::SetOrInherit)
	}

	fn sets(self) -> bool {
		matches!(self, Self::Set | Self::SetOrInherit)
	}
}

#[derive(Clone, Copy, Debug)]
struct MeshSettingsResolvedComponent {
	probe_mode: MeshSettingsInheritMode,
	probe_anchor: Option<usize>,
	bounds_mode: MeshSettingsInheritMode,
	root_bone: Option<usize>,
	local_bounds: Option<UnaBounds>,
}

#[derive(Clone, Copy, Debug)]
struct MeshSettingsMerged {
	probe_mode: MeshSettingsInheritMode,
	probe_anchor: Option<usize>,
	bounds_mode: MeshSettingsInheritMode,
	root_bone: Option<usize>,
	local_bounds: Option<UnaBounds>,
}

fn mesh_settings_should_use_src_value(current_mode: &mut MeshSettingsInheritMode, src_mode: MeshSettingsInheritMode) -> bool {
	match (*current_mode, src_mode) {
		(MeshSettingsInheritMode::Set | MeshSettingsInheritMode::DontSet, _) => false,
		(_, MeshSettingsInheritMode::Inherit) => false,
		(_, MeshSettingsInheritMode::DontSet) => {
			*current_mode = src_mode;
			true
		}
		(_, MeshSettingsInheritMode::Set | MeshSettingsInheritMode::SetOrInherit) => {
			*current_mode = src_mode;
			true
		}
	}
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

fn merge_mesh_settings_for_node(
	parents: &[Option<usize>],
	settings_by_node: &BTreeMap<usize, MeshSettingsResolvedComponent>,
	node_index: usize,
) -> MeshSettingsMerged {
	let mut merged = MeshSettingsMerged {
		probe_mode: MeshSettingsInheritMode::Inherit,
		probe_anchor: None,
		bounds_mode: MeshSettingsInheritMode::Inherit,
		root_bone: None,
		local_bounds: None,
	};
	let mut cursor = Some(node_index);
	while let Some(idx) = cursor {
		if let Some(settings) = settings_by_node.get(&idx) {
			if mesh_settings_should_use_src_value(&mut merged.probe_mode, settings.probe_mode) {
				merged.probe_anchor = settings.probe_anchor;
			}
			if mesh_settings_should_use_src_value(&mut merged.bounds_mode, settings.bounds_mode) {
				merged.root_bone = settings.root_bone;
				merged.local_bounds = settings.local_bounds;
			}
		}
		if !merged.probe_mode.not_final() && !merged.bounds_mode.not_final() {
			break;
		}
		cursor = parents.get(idx).copied().flatten();
	}
	merged
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
	let mut settings_by_node = BTreeMap::new();
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
		let probe_mode =
			MeshSettingsInheritMode::from_str(fields.and_then(|fields| fields.get("InheritProbeAnchor")).and_then(|v| v.as_str()));
		let probe_anchor = if probe_mode.sets() {
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
		let bounds_mode = MeshSettingsInheritMode::from_str(fields.and_then(|fields| fields.get("InheritBounds")).and_then(|v| v.as_str()));
		let root_bone = if bounds_mode.sets() {
			fields
				.and_then(|fields| fields.get("RootBone"))
				.and_then(|reference| modular_avatar_reference_index(reference, node_ids, registry_paths, paths, normalized_paths))
		} else {
			None
		};
		let local_bounds = if bounds_mode.sets() {
			fields.and_then(|fields| fields.get("Bounds")).and_then(mesh_settings_bounds)
		} else {
			None
		};
		settings_by_node.insert(
			target_root,
			MeshSettingsResolvedComponent {
				probe_mode,
				probe_anchor,
				bounds_mode,
				root_bone,
				local_bounds,
			},
		);
	}
	if settings_by_node.is_empty() {
		return (root_bone_applied, probe_anchor_applied, bounds_applied, missing);
	}
	let parents = scene_parent_indices(scene);
	for node_index in 0..scene.nodes.len() {
		let Some(skin_idx) = scene.nodes.get(node_index).and_then(|node| node.skin) else {
			continue;
		};
		let merged = merge_mesh_settings_for_node(&parents, &settings_by_node, node_index);
		if merged.probe_mode.sets() {
			if let Some(node) = scene.nodes.get_mut(node_index) {
				node.probe_anchor_node = merged.probe_anchor;
			}
			if merged.probe_anchor.is_some() {
				probe_anchor_applied += 1;
			}
		}
		if merged.bounds_mode.sets() {
			if let Some(skin) = scene.skins.get_mut(skin_idx) {
				skin.skeleton_node = merged.root_bone.or(Some(node_index));
				root_bone_applied += 1;
			}
			if let Some(node) = scene.nodes.get_mut(node_index) {
				node.local_bounds = merged.local_bounds;
			}
			if merged.local_bounds.is_some() {
				bounds_applied += 1;
			}
		}
	}
	(root_bone_applied, probe_anchor_applied, bounds_applied, missing)
}

fn apply_unavatar_scale_adjusters(
	scene: &mut UnaSceneSnapshot,
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> (usize, usize, usize, usize) {
	let mut proxies_created = 0usize;
	let mut skin_joints_remapped = 0usize;
	let mut missing = 0usize;
	let mut skipped = 0usize;
	let mut mappings = BTreeMap::<usize, usize>::new();
	for component in components {
		if component.get("shortType").and_then(|v| v.as_str()) != Some("ModularAvatarScaleAdjuster") {
			continue;
		}
		if component.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
			skipped += 1;
			continue;
		}
		let Some(target_ref) = component.get("target") else {
			missing += 1;
			continue;
		};
		let Some(target) = unavatar_node_ref_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
			missing += 1;
			continue;
		};
		if mappings.contains_key(&target) {
			continue;
		}
		let scale = json_vec3(modular_avatar_component_value(component, &["Scale", "scale", "m_Scale"])).unwrap_or([1.0, 1.0, 1.0]);
		let proxy = scene.nodes.len();
		let target_id = scene
			.nodes
			.get(target)
			.and_then(|node| node.source_node_id.as_deref().or(node.resolved_node_id.as_deref()))
			.unwrap_or("node");
		scene.nodes.push(UnaSceneNode {
			name: Some("ScaleProxy".to_string()),
			source_node_id: None,
			resolved_node_id: Some(format!("{target_id}#ma-scale-proxy")),
			visible: true,
			transform: Mat4::from_scale(Vec3::from(scale)).to_cols_array(),
			children: Vec::new(),
			mesh: None,
			skin: None,
			probe_anchor_node: None,
			local_bounds: None,
		});
		if let Some(node) = scene.nodes.get_mut(target) {
			node.children.push(proxy);
		}
		mappings.insert(target, proxy);
		proxies_created += 1;
	}
	if mappings.is_empty() {
		return (proxies_created, skin_joints_remapped, missing, skipped);
	}
	for skin in &mut scene.skins {
		for joint in &mut skin.joint_nodes {
			if let Some(proxy) = mappings.get(joint).copied() {
				*joint = proxy;
				skin_joints_remapped += 1;
			}
		}
	}
	(proxies_created, skin_joints_remapped, missing, skipped)
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
	let Some(mode) = modular_avatar_component_value(component, &["Mode", "mode", "m_Mode", "removeMode", "remove_mode"]) else {
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

#[derive(Clone, Debug)]
enum ModularAvatarVertexFilter {
	BlendShape {
		shapes: Vec<String>,
		threshold: f32,
	},
	Axis {
		center: [f32; 3],
		axis: [f32; 3],
	},
	Bone {
		bone_node: usize,
		threshold: f32,
	},
	Mask {
		material_index: usize,
		image_index: usize,
		mode: ModularAvatarMaskDeleteMode,
	},
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModularAvatarMaskDeleteMode {
	DeleteBlack,
	DeleteWhite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModularAvatarVertexFilterCombine {
	Single,
	Union,
	Intersection,
}

#[derive(Clone, Debug)]
struct ModularAvatarVertexFilterDeleteGroup {
	target: usize,
	combine: ModularAvatarVertexFilterCombine,
	filters: Vec<ModularAvatarVertexFilter>,
}

struct ModularAvatarVertexFilterContext<'a> {
	images: &'a [UnaImageRgba],
	image_sources: &'a [Option<UnaImageSourceMetadata>],
	texture_asset_map: &'a BTreeMap<String, usize>,
}

struct ModularAvatarAxisBakeContext<'a> {
	world_matrices: &'a [Mat4],
	target_world_inv: Mat4,
	skin: Option<&'a UnaSkin>,
}

fn modular_avatar_component_value<'a>(component: &'a Value, names: &[&str]) -> Option<&'a Value> {
	component
		.get("fields")
		.and_then(|fields| names.iter().find_map(|name| fields.get(*name)))
		.or_else(|| names.iter().find_map(|name| component.get(*name)))
}

fn modular_avatar_component_string(component: &Value, names: &[&str]) -> Option<String> {
	modular_avatar_component_value(component, names)
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string)
}

fn modular_avatar_component_f32(component: &Value, names: &[&str]) -> Option<f32> {
	modular_avatar_component_value(component, names).and_then(json_number_f32)
}

fn modular_avatar_blendshape_sync_binding_reference(binding: &Value) -> Option<&Value> {
	binding
		.get("referenceMesh")
		.or_else(|| binding.get("ReferenceMesh"))
		.or_else(|| binding.get("reference_mesh"))
}

fn modular_avatar_blendshape_sync_binding_shape(binding: &Value, names: &[&str]) -> Option<String> {
	names
		.iter()
		.find_map(|name| binding.get(*name))
		.and_then(Value::as_str)
		.map(str::trim)
		.filter(|value| !value.is_empty())
		.map(str::to_string)
}

#[derive(Clone, Copy, Debug)]
struct AnimationCurveKey {
	time: f32,
	value: f32,
	in_tangent: Option<f32>,
	out_tangent: Option<f32>,
}

fn animation_curve_key(key: &Value) -> Option<AnimationCurveKey> {
	Some(AnimationCurveKey {
		time: key.get("time").and_then(json_number_f32)?,
		value: key.get("value").and_then(json_number_f32)?,
		in_tangent: key.get("inTangent").or_else(|| key.get("in_tangent")).and_then(json_number_f32),
		out_tangent: key.get("outTangent").or_else(|| key.get("out_tangent")).and_then(json_number_f32),
	})
}

fn animation_curve_segment_evaluate(a: AnimationCurveKey, b: AnimationCurveKey, input: f32) -> f32 {
	let span = b.time - a.time;
	if span.abs() <= f32::EPSILON {
		return b.value;
	}
	let u = ((input - a.time) / span).clamp(0.0, 1.0);
	let linear_slope = (b.value - a.value) / span;
	let m0 = a.out_tangent.filter(|value| value.is_finite()).unwrap_or(linear_slope) * span;
	let m1 = b.in_tangent.filter(|value| value.is_finite()).unwrap_or(linear_slope) * span;
	let u2 = u * u;
	let u3 = u2 * u;
	let h00 = 2.0 * u3 - 3.0 * u2 + 1.0;
	let h10 = u3 - 2.0 * u2 + u;
	let h01 = -2.0 * u3 + 3.0 * u2;
	let h11 = u3 - u2;
	h00 * a.value + h10 * m0 + h01 * b.value + h11 * m1
}

fn animation_curve_evaluate(curve: Option<&Value>, input: f32) -> Option<f32> {
	let keys = curve
		.and_then(|curve| curve.get("keys").or_else(|| curve.get("Keys")))
		.and_then(Value::as_array)?;
	if keys.len() < 2 {
		return None;
	}
	let mut keys = keys
		.iter()
		.filter_map(animation_curve_key)
		.filter(|key| key.time.is_finite() && key.value.is_finite())
		.collect::<Vec<_>>();
	if keys.len() < 2 {
		return None;
	}
	keys.sort_by(|a, b| a.time.total_cmp(&b.time));
	if input <= keys[0].time {
		return Some(keys[0].value);
	}
	for pair in keys.windows(2) {
		let a = pair[0];
		let b = pair[1];
		if input <= b.time {
			return Some(animation_curve_segment_evaluate(a, b, input));
		}
	}
	keys.last().map(|key| key.value)
}

fn modular_avatar_remap_curve_evaluate(curve: Option<&Value>, input: f32) -> f32 {
	animation_curve_evaluate(curve, input).unwrap_or(input)
}

fn modular_avatar_remap_curve_linear_origin_scale(curve: Option<&Value>) -> Option<f32> {
	let Some(keys) = curve
		.and_then(|curve| curve.get("keys").or_else(|| curve.get("Keys")))
		.and_then(Value::as_array)
	else {
		return Some(1.0);
	};
	if keys.len() < 2 {
		return Some(1.0);
	}
	let mut keys = keys
		.iter()
		.filter_map(animation_curve_key)
		.filter(|key| key.time.is_finite() && key.value.is_finite())
		.collect::<Vec<_>>();
	if keys.len() < 2 {
		return Some(1.0);
	}
	keys.sort_by(|a, b| a.time.total_cmp(&b.time));
	let last = keys.last().copied()?;
	if keys[0].time.abs() > 0.0001 || keys[0].value.abs() > 0.0001 || last.time.abs() <= 0.0001 {
		return None;
	}
	let scale = last.value / last.time;
	if !scale.is_finite() {
		return None;
	}
	for key in &keys {
		if (key.value - key.time * scale).abs() > 0.0001 {
			return None;
		}
		if key
			.in_tangent
			.filter(|value| value.is_finite())
			.is_some_and(|tangent| (tangent - scale).abs() > 0.0001)
		{
			return None;
		}
		if key
			.out_tangent
			.filter(|value| value.is_finite())
			.is_some_and(|tangent| (tangent - scale).abs() > 0.0001)
		{
			return None;
		}
	}
	Some(scale)
}

fn scene_node_morph_bind(scene: &UnaSceneSnapshot, node_idx: usize, shape_name: &str) -> Option<(usize, usize, usize)> {
	let mesh_idx = scene.nodes.get(node_idx).and_then(|node| node.mesh)?;
	let primitives = scene.meshes.get(mesh_idx)?;
	for (primitive_index, primitive) in primitives.iter().enumerate() {
		let Some(morph_target_index) = primitive.morph_target_names.iter().position(|candidate| candidate == shape_name) else {
			continue;
		};
		if morph_target_index < primitive.morph_targets.len() {
			return Some((mesh_idx, primitive_index, morph_target_index));
		}
	}
	None
}

fn apply_unavatar_blendshape_sync_expression_binds(
	catalog: &mut UnaExpressionCatalog,
	scene: &UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
	report: &mut ImportReport,
) {
	let components = unavatar_modular_avatar_components(unavatar);
	if components.is_empty() {
		return;
	}
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(Some(unavatar));
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let mut added = 0usize;
	let mut skipped_non_linear = 0usize;
	let mut missing = 0usize;
	for component in components {
		if component.get("shortType").and_then(Value::as_str) != Some("ModularAvatarBlendshapeSync") {
			continue;
		}
		if component.get("enabled").and_then(Value::as_bool) == Some(false) {
			continue;
		}
		let Some(target_ref) = component.get("target").or_else(|| component.get("resolvedTarget")) else {
			missing += 1;
			continue;
		};
		let Some(target) = modular_avatar_reference_index(target_ref, &node_ids, &registry_paths, &paths, &normalized_paths) else {
			missing += 1;
			continue;
		};
		let Some(bindings) = unavatar_modular_avatar_component_array(component, &["Bindings", "bindings", "m_bindings"]) else {
			missing += 1;
			continue;
		};
		for binding in bindings {
			if !binding.is_object() {
				missing += 1;
				continue;
			}
			let Some(reference) = modular_avatar_blendshape_sync_binding_reference(binding) else {
				missing += 1;
				continue;
			};
			let Some(source) = modular_avatar_reference_index(reference, &node_ids, &registry_paths, &paths, &normalized_paths) else {
				missing += 1;
				continue;
			};
			let Some(source_shape) = modular_avatar_blendshape_sync_binding_shape(binding, &["blendshape", "Blendshape", "blendShape"])
			else {
				missing += 1;
				continue;
			};
			let target_shape =
				modular_avatar_blendshape_sync_binding_shape(binding, &["localBlendshape", "LocalBlendshape", "localBlendShape"])
					.unwrap_or_else(|| source_shape.clone());
			if scene_node_morph_bind(scene, source, &source_shape).is_none() {
				missing += 1;
				continue;
			}
			let Some((mesh_index, primitive_index, morph_target_index)) = scene_node_morph_bind(scene, target, &target_shape) else {
				missing += 1;
				continue;
			};
			let Some(weight_scale) = modular_avatar_remap_curve_linear_origin_scale(
				binding
					.get("remapCurve")
					.or_else(|| binding.get("RemapCurve"))
					.or_else(|| binding.get("remap_curve")),
			) else {
				skipped_non_linear += 1;
				continue;
			};
			if add_expression_catalog_bind(
				catalog,
				&source_shape,
				mesh_index,
				primitive_index,
				morph_target_index,
				weight_scale,
			) {
				added += 1;
			}
		}
	}
	if added > 0 || skipped_non_linear > 0 || missing > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: blendshape_sync_expression_binds={added}, blendshape_sync_expression_non_linear={skipped_non_linear}, blendshape_sync_expression_missing={missing}"
		));
	}
}

fn apply_unavatar_blendshape_syncs(
	scene: &mut UnaSceneSnapshot,
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> (usize, usize, usize, usize) {
	let mut applied = 0usize;
	let mut missing = 0usize;
	let mut skipped = 0usize;
	let mut unsupported = 0usize;
	for component in components {
		if component.get("shortType").and_then(Value::as_str) != Some("ModularAvatarBlendshapeSync") {
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
		let Some(bindings) = unavatar_modular_avatar_component_array(component, &["Bindings", "bindings", "m_bindings"]) else {
			missing += 1;
			continue;
		};
		for binding in bindings {
			if !binding.is_object() {
				unsupported += 1;
				continue;
			}
			let Some(reference) = modular_avatar_blendshape_sync_binding_reference(binding) else {
				missing += 1;
				continue;
			};
			let Some(source) = modular_avatar_reference_index(reference, node_ids, registry_paths, paths, normalized_paths) else {
				missing += 1;
				continue;
			};
			let Some(source_shape) = modular_avatar_blendshape_sync_binding_shape(binding, &["blendshape", "Blendshape", "blendShape"])
			else {
				missing += 1;
				continue;
			};
			let target_shape =
				modular_avatar_blendshape_sync_binding_shape(binding, &["localBlendshape", "LocalBlendshape", "localBlendShape"])
					.unwrap_or_else(|| source_shape.clone());
			let Some(source_weight) = blend_shape_weight(scene, source, &source_shape) else {
				missing += 1;
				continue;
			};
			let remapped = modular_avatar_remap_curve_evaluate(
				binding
					.get("remapCurve")
					.or_else(|| binding.get("RemapCurve"))
					.or_else(|| binding.get("remap_curve")),
				source_weight,
			);
			if ensure_unique_mesh_for_node(scene, target).is_some()
				&& apply_blend_shape_weight(scene, target, &target_shape, (remapped * 100.0).clamp(0.0, 100.0))
			{
				applied += 1;
			} else {
				missing += 1;
			}
		}
	}
	(applied, missing, skipped, unsupported)
}

fn modular_avatar_shape_change_type(shape: &Value) -> Option<&str> {
	shape
		.get("ChangeType")
		.or_else(|| shape.get("changeType"))
		.or_else(|| shape.get("change_type"))
		.or_else(|| shape.get("m_changeType"))
		.and_then(Value::as_str)
}

fn modular_avatar_shape_name(shape: &Value) -> Option<String> {
	shape
		.get("ShapeName")
		.or_else(|| shape.get("shapeName"))
		.or_else(|| shape.get("shape_name"))
		.or_else(|| shape.get("m_shapeName"))
		.and_then(Value::as_str)
		.filter(|value| !value.is_empty())
		.map(str::to_string)
}

fn modular_avatar_shape_value(shape: &Value) -> f32 {
	json_f32(shape.get("Value").or_else(|| shape.get("value")).or_else(|| shape.get("m_value"))).unwrap_or(0.0)
}

fn modular_avatar_shape_object_ref(shape: &Value) -> Option<&Value> {
	shape
		.get("Object")
		.or_else(|| shape.get("object"))
		.or_else(|| shape.get("m_object"))
		.or_else(|| shape.get("target"))
		.or_else(|| shape.get("resolvedTarget"))
}

fn modular_avatar_component_target_ref(component: &Value) -> Option<&Value> {
	component.get("target").or_else(|| component.get("resolvedTarget"))
}

fn modular_avatar_shape_string_payload(shape: &Value) -> Option<(&str, &str, f32)> {
	let value = shape.as_str()?.trim();
	if value.is_empty() {
		return None;
	}
	let (head, raw_value) = value.rsplit_once(' ')?;
	let value = raw_value.parse::<f32>().ok()?;
	let (target_and_shape, change_type) = head.rsplit_once(' ')?;
	Some((target_and_shape.trim(), change_type.trim(), value))
}

fn modular_avatar_shape_string_target_and_name(
	target_and_shape: &str,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Option<(usize, String)> {
	let mut best = None;
	for (split, _) in target_and_shape.match_indices(' ') {
		let target_path = target_and_shape[..split].trim();
		let shape_name = target_and_shape[split + 1..].trim();
		if target_path.is_empty() || shape_name.is_empty() {
			continue;
		}
		let target_ref = serde_json::json!({ "path": target_path });
		if let Some(target) = modular_avatar_reference_index(&target_ref, node_ids, registry_paths, paths, normalized_paths) {
			best = Some((target, shape_name.to_string()));
		}
	}
	best
}

fn apply_unavatar_shape_changer_sets(
	scene: &mut UnaSceneSnapshot,
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	visible_components_only: bool,
) -> (usize, usize, usize) {
	let mut applied = 0usize;
	let mut missing = 0usize;
	let mut skipped = 0usize;
	for component in components {
		if component.get("shortType").and_then(Value::as_str) != Some("ModularAvatarShapeChanger") {
			continue;
		}
		if component.get("enabled").and_then(Value::as_bool) == Some(false) {
			skipped += 1;
			continue;
		}
		if visible_components_only {
			let Some(target_ref) = modular_avatar_component_target_ref(component) else {
				missing += 1;
				continue;
			};
			let Some(component_target) = modular_avatar_reference_index(target_ref, node_ids, registry_paths, paths, normalized_paths)
			else {
				missing += 1;
				continue;
			};
			if !scene.effective_node_visible(component_target) {
				skipped += 1;
				continue;
			}
		}
		let Some(shapes) = unavatar_modular_avatar_component_array(component, &["Shapes", "shapes", "m_shapes"]) else {
			continue;
		};
		for shape in shapes {
			let (target, shape_name, value) =
				if let Some((target_and_shape, change_type, value)) = modular_avatar_shape_string_payload(shape) {
					if !matches!(change_type, "Set" | "set" | "1") {
						continue;
					}
					let Some((target, shape_name)) =
						modular_avatar_shape_string_target_and_name(target_and_shape, node_ids, registry_paths, paths, normalized_paths)
					else {
						missing += 1;
						continue;
					};
					(target, shape_name, value)
				} else {
					if !matches!(modular_avatar_shape_change_type(shape), Some("Set" | "set" | "1")) {
						continue;
					}
					let Some(shape_name) = modular_avatar_shape_name(shape) else {
						missing += 1;
						continue;
					};
					let Some(target_ref) = modular_avatar_shape_object_ref(shape) else {
						missing += 1;
						continue;
					};
					let Some(target) = modular_avatar_reference_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
						missing += 1;
						continue;
					};
					(target, shape_name, modular_avatar_shape_value(shape))
				};
			if ensure_unique_mesh_for_node(scene, target).is_some() && apply_blend_shape_weight(scene, target, &shape_name, value) {
				applied += 1;
			} else {
				missing += 1;
			}
		}
	}
	(applied, missing, skipped)
}

fn modular_avatar_mesh_cutter_object_ref(component: &Value) -> Option<&Value> {
	modular_avatar_component_value(
		component,
		&["Object", "object", "m_object", "target", "resolvedTarget", "targetObject"],
	)
}

fn modular_avatar_mesh_cutter_combine(component: &Value) -> ModularAvatarVertexFilterCombine {
	match modular_avatar_component_string(component, &["MultiMode", "multiMode", "multi_mode", "m_multiMode"]).as_deref() {
		Some("VertexUnion" | "vertex_union" | "Union" | "union" | "0") => ModularAvatarVertexFilterCombine::Union,
		Some("VertexIntersection" | "vertex_intersection" | "Intersection" | "intersection" | "1") => {
			ModularAvatarVertexFilterCombine::Intersection
		}
		_ => ModularAvatarVertexFilterCombine::Intersection,
	}
}

fn modular_avatar_vertex_filter_by_shape(component: &Value) -> Option<ModularAvatarVertexFilter> {
	let shapes = modular_avatar_component_value(component, &["Shapes", "shapes", "m_shapes"])
		.and_then(Value::as_array)
		.map(|values| {
			values
				.iter()
				.filter_map(Value::as_str)
				.filter(|value| !value.is_empty())
				.map(str::to_string)
				.collect::<Vec<_>>()
		})
		.unwrap_or_default();
	if shapes.is_empty() {
		return None;
	}
	Some(ModularAvatarVertexFilter::BlendShape {
		shapes,
		threshold: modular_avatar_component_f32(component, &["Threshold", "threshold", "m_threshold"]).unwrap_or(0.001),
	})
}

fn modular_avatar_vertex_filter_by_axis(component: &Value) -> Option<ModularAvatarVertexFilter> {
	Some(ModularAvatarVertexFilter::Axis {
		center: json_vec3(modular_avatar_component_value(component, &["Center", "center", "m_center"])).unwrap_or([0.0; 3]),
		axis: json_vec3(modular_avatar_component_value(component, &["Axis", "axis", "m_axis"])).unwrap_or([-1.0, 0.0, 0.0]),
	})
}

fn modular_avatar_vertex_filter_by_bone(
	component: &Value,
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> Option<ModularAvatarVertexFilter> {
	let bone_ref = modular_avatar_component_value(component, &["Bone", "bone", "m_bone"])?;
	Some(ModularAvatarVertexFilter::Bone {
		bone_node: modular_avatar_reference_index(bone_ref, node_ids, registry_paths, paths, normalized_paths)?,
		threshold: modular_avatar_component_f32(component, &["Threshold", "threshold", "m_threshold"]).unwrap_or(0.01),
	})
}

fn modular_avatar_mask_delete_mode(component: &Value) -> ModularAvatarMaskDeleteMode {
	let value = modular_avatar_component_value(component, &["DeleteMode", "deleteMode", "delete_mode", "m_deleteMode"]);
	match value {
		Some(Value::String(mode)) => match mode.as_str() {
			"DeleteWhite" | "delete_white" | "White" | "white" | "1" => ModularAvatarMaskDeleteMode::DeleteWhite,
			_ => ModularAvatarMaskDeleteMode::DeleteBlack,
		},
		Some(Value::Number(number)) if number.as_u64() == Some(1) => ModularAvatarMaskDeleteMode::DeleteWhite,
		_ => ModularAvatarMaskDeleteMode::DeleteBlack,
	}
}

fn modular_avatar_vertex_filter_by_mask(
	component: &Value,
	texture_asset_map: &BTreeMap<String, usize>,
) -> Option<ModularAvatarVertexFilter> {
	let texture_asset_id =
		modular_avatar_component_string(component, &["maskTextureAssetId", "MaskTextureAssetId", "mask_texture_asset_id"])?;
	let image_index = texture_asset_map.get(&texture_asset_id).copied()?;
	Some(ModularAvatarVertexFilter::Mask {
		material_index: modular_avatar_component_value(component, &["MaterialIndex", "materialIndex", "material_index", "m_materialIndex"])
			.and_then(|value| json_usize(Some(value)))
			.unwrap_or(0),
		image_index,
		mode: modular_avatar_mask_delete_mode(component),
	})
}

fn collect_modular_avatar_vertex_filter_delete_groups(
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	context: &ModularAvatarVertexFilterContext<'_>,
) -> (Vec<ModularAvatarVertexFilterDeleteGroup>, usize, usize, usize) {
	let mut groups = Vec::new();
	let mut missing = 0usize;
	let mut skipped = 0usize;
	let mut unsupported = 0usize;
	for component in components {
		let short_type = component.get("shortType").and_then(Value::as_str).unwrap_or("");
		if !matches!(short_type, "ModularAvatarMeshCutter" | "ModularAvatarShapeChanger") {
			continue;
		}
		if component.get("enabled").and_then(Value::as_bool) == Some(false) {
			skipped += 1;
			continue;
		}
		if short_type == "ModularAvatarShapeChanger" {
			let threshold = modular_avatar_component_f32(component, &["Threshold", "threshold", "m_threshold"]).unwrap_or(0.01);
			let Some(shapes) = unavatar_modular_avatar_component_array(component, &["Shapes", "shapes", "m_shapes"]) else {
				continue;
			};
			for shape in shapes {
				if let Some((_target_and_shape, change_type, _value)) = modular_avatar_shape_string_payload(shape) {
					if !matches!(change_type, "Delete" | "delete" | "0") {
						continue;
					}
					unsupported += 1;
					continue;
				}
				if !matches!(modular_avatar_shape_change_type(shape), None | Some("Delete" | "delete" | "0")) {
					continue;
				}
				let Some(shape_name) = modular_avatar_shape_name(shape) else {
					missing += 1;
					continue;
				};
				let Some(target_ref) = modular_avatar_shape_object_ref(shape) else {
					missing += 1;
					continue;
				};
				let Some(target) = modular_avatar_reference_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
					missing += 1;
					continue;
				};
				groups.push(ModularAvatarVertexFilterDeleteGroup {
					target,
					combine: ModularAvatarVertexFilterCombine::Single,
					filters: vec![ModularAvatarVertexFilter::BlendShape {
						shapes: vec![shape_name],
						threshold,
					}],
				});
			}
			continue;
		}
		let Some(target_ref) = modular_avatar_mesh_cutter_object_ref(component) else {
			missing += 1;
			continue;
		};
		let Some(target) = modular_avatar_reference_index(target_ref, node_ids, registry_paths, paths, normalized_paths) else {
			missing += 1;
			continue;
		};
		let Some(filters) = unavatar_modular_avatar_component_array(component, &["filters", "Filters", "vertexFilters", "vertex_filters"])
		else {
			continue;
		};
		let mut vertex_filters = Vec::new();
		let mut has_unsupported = false;
		for filter in filters {
			match filter.get("shortType").and_then(Value::as_str) {
				Some("VertexFilterByShapeComponent") => {
					if let Some(filter) = modular_avatar_vertex_filter_by_shape(filter) {
						vertex_filters.push(filter);
					}
				}
				Some("VertexFilterByAxisComponent") => {
					if let Some(filter) = modular_avatar_vertex_filter_by_axis(filter) {
						vertex_filters.push(filter);
					}
				}
				Some("VertexFilterByBoneComponent") => {
					if let Some(filter) = modular_avatar_vertex_filter_by_bone(filter, node_ids, registry_paths, paths, normalized_paths) {
						vertex_filters.push(filter);
					} else {
						has_unsupported = true;
					}
				}
				Some("VertexFilterByMaskComponent") => {
					if let Some(filter) = modular_avatar_vertex_filter_by_mask(filter, context.texture_asset_map) {
						vertex_filters.push(filter);
					} else {
						has_unsupported = true;
					}
				}
				Some(_) => has_unsupported = true,
				None => has_unsupported = true,
			}
		}
		if has_unsupported {
			unsupported += 1;
			continue;
		}
		if vertex_filters.is_empty() {
			continue;
		}
		groups.push(ModularAvatarVertexFilterDeleteGroup {
			target,
			combine: if vertex_filters.len() == 1 {
				ModularAvatarVertexFilterCombine::Single
			} else {
				modular_avatar_mesh_cutter_combine(component)
			},
			filters: vertex_filters,
		});
	}
	(groups, missing, skipped, unsupported)
}

fn modular_avatar_blend_shape_filter_mask(primitive: &UnaMeshBuffers, shapes: &[String], threshold: f32) -> Vec<bool> {
	let mut mask = vec![false; primitive.positions.len()];
	let threshold_sq = threshold * threshold;
	for shape in shapes {
		let Some(shape_index) = primitive.morph_target_names.iter().position(|name| name == shape) else {
			continue;
		};
		let Some(target) = primitive.morph_targets.get(shape_index) else {
			continue;
		};
		for (index, delta) in target.position_deltas.iter().enumerate().take(mask.len()) {
			let len_sq = delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2];
			if len_sq > threshold_sq {
				mask[index] = true;
			}
		}
	}
	mask
}

fn modular_avatar_axis_filter_mask(primitive: &UnaMeshBuffers, center: [f32; 3], axis: [f32; 3]) -> Vec<bool> {
	primitive
		.positions
		.iter()
		.map(|position| {
			let offset = [position[0] - center[0], position[1] - center[1], position[2] - center[2]];
			axis[0] * offset[0] + axis[1] * offset[1] + axis[2] * offset[2] > 0.0
		})
		.collect()
}

fn modular_avatar_skinned_rest_positions_for_axis(
	primitive: &UnaMeshBuffers,
	context: &ModularAvatarAxisBakeContext<'_>,
) -> Option<Vec<[f32; 3]>> {
	let skin = context.skin?;
	let (Some(joints), Some(weights)) = (&primitive.joints, &primitive.weights) else {
		return None;
	};
	if joints.len() != primitive.positions.len() || weights.len() != primitive.positions.len() {
		return None;
	}
	let mut positions = Vec::with_capacity(primitive.positions.len());
	for (position, (vertex_joints, vertex_weights)) in primitive.positions.iter().zip(joints.iter().zip(weights)) {
		let source = Vec3::from(*position).extend(1.0);
		let mut total_weight = 0.0f32;
		let mut baked = Vec3::ZERO;
		for slot in 0..4 {
			let weight = vertex_weights[slot];
			if weight <= 0.0 {
				continue;
			}
			let joint_index = usize::from(vertex_joints[slot]);
			let joint_node = skin.joint_nodes.get(joint_index).copied()?;
			let joint_world = context.world_matrices.get(joint_node).copied().unwrap_or(Mat4::IDENTITY);
			let inverse_bind = skin
				.inverse_bind_matrices
				.get(joint_index)
				.map(Mat4::from_cols_array)
				.unwrap_or(Mat4::IDENTITY);
			baked += (context.target_world_inv * joint_world * inverse_bind * source).truncate() * weight;
			total_weight += weight;
		}
		if total_weight > 0.0 {
			baked /= total_weight;
		} else {
			baked = Vec3::from(*position);
		}
		positions.push(baked.to_array());
	}
	Some(positions)
}

fn modular_avatar_axis_filter_mask_with_bake(
	primitive: &UnaMeshBuffers,
	center: [f32; 3],
	axis: [f32; 3],
	axis_context: Option<&ModularAvatarAxisBakeContext<'_>>,
) -> Vec<bool> {
	let Some(positions) = axis_context.and_then(|context| modular_avatar_skinned_rest_positions_for_axis(primitive, context)) else {
		return modular_avatar_axis_filter_mask(primitive, center, axis);
	};
	positions
		.iter()
		.map(|position| {
			let offset = [position[0] - center[0], position[1] - center[1], position[2] - center[2]];
			axis[0] * offset[0] + axis[1] * offset[1] + axis[2] * offset[2] > 0.0
		})
		.collect()
}

fn modular_avatar_bone_filter_mask(
	primitive: &UnaMeshBuffers,
	skin_joint_nodes: Option<&[usize]>,
	bone_node: usize,
	threshold: f32,
) -> Vec<bool> {
	let mut mask = vec![false; primitive.positions.len()];
	let (Some(joints), Some(weights), Some(skin_joint_nodes)) = (&primitive.joints, &primitive.weights, skin_joint_nodes) else {
		return mask;
	};
	if joints.len() != mask.len() || weights.len() != mask.len() {
		return mask;
	}
	for (index, (vertex_joints, vertex_weights)) in joints.iter().zip(weights).enumerate() {
		let mut total_weight = 0.0f32;
		let mut target_weight = 0.0f32;
		for slot in 0..4 {
			let weight = vertex_weights[slot];
			if weight <= 0.0 {
				continue;
			}
			total_weight += weight;
			let joint_index = usize::from(vertex_joints[slot]);
			if skin_joint_nodes.get(joint_index).copied() == Some(bone_node) {
				target_weight += weight;
			}
		}
		if target_weight > 0.0 && total_weight > 0.0 && target_weight / total_weight >= threshold {
			mask[index] = true;
		}
	}
	mask
}

fn modular_avatar_wrap_uv(value: f32, mode: UnaTextureWrapMode) -> f32 {
	match mode {
		UnaTextureWrapMode::ClampToEdge => value.clamp(0.0, 1.0),
		UnaTextureWrapMode::Repeat => value.rem_euclid(1.0),
		UnaTextureWrapMode::MirrorOnce => {
			let mirrored = if value < 0.0 { -value - f32::EPSILON } else { value };
			mirrored.clamp(0.0, 1.0)
		}
		UnaTextureWrapMode::MirroredRepeat => {
			let repeated = value.rem_euclid(2.0);
			if repeated <= 1.0 {
				repeated
			} else {
				2.0 - repeated
			}
		}
	}
}

fn modular_avatar_mask_pixel_selected(px: &[u8], mode: ModularAvatarMaskDeleteMode) -> bool {
	match mode {
		ModularAvatarMaskDeleteMode::DeleteBlack => px == [0, 0, 0, 255],
		ModularAvatarMaskDeleteMode::DeleteWhite => px == [255, 255, 255, 255],
	}
}

fn modular_avatar_mask_filter_mask(
	primitive: &UnaMeshBuffers,
	primitive_index: usize,
	primitive_count: usize,
	image: &UnaImageRgba,
	source: Option<&UnaImageSourceMetadata>,
	material_index: usize,
	mode: ModularAvatarMaskDeleteMode,
) -> Vec<bool> {
	let mut mask = vec![false; primitive.positions.len()];
	let selected_primitive = material_index.min(primitive_count.saturating_sub(1));
	if primitive_index != selected_primitive || image.width == 0 || image.height == 0 {
		return mask;
	}
	let Some(uvs) = primitive.tex_coords_0.as_ref() else {
		return mask;
	};
	if uvs.len() != mask.len() {
		return mask;
	}
	let sampler = source.and_then(|source| source.sampler).unwrap_or_default();
	let pixels = image.rgba8_compat_pixels();
	let width = image.width as usize;
	let height = image.height as usize;
	for (index, uv) in uvs.iter().enumerate() {
		let u = modular_avatar_wrap_uv(uv[0], sampler.wrap_s);
		let v = modular_avatar_wrap_uv(uv[1], sampler.wrap_t);
		let x = ((u * image.width as f32).floor() as usize).min(width - 1);
		let y = ((v * image.height as f32).floor() as usize).min(height - 1);
		let offset = (y * width + x) * 4;
		if pixels
			.get(offset..offset + 4)
			.is_some_and(|px| modular_avatar_mask_pixel_selected(px, mode))
		{
			mask[index] = true;
		}
	}
	mask
}

fn modular_avatar_single_vertex_filter_mask(
	primitive: &UnaMeshBuffers,
	skin_joint_nodes: Option<&[usize]>,
	axis_context: Option<&ModularAvatarAxisBakeContext<'_>>,
	primitive_index: usize,
	primitive_count: usize,
	context: &ModularAvatarVertexFilterContext<'_>,
	filter: &ModularAvatarVertexFilter,
) -> Vec<bool> {
	match filter {
		ModularAvatarVertexFilter::BlendShape { shapes, threshold } => {
			modular_avatar_blend_shape_filter_mask(primitive, shapes, *threshold)
		}
		ModularAvatarVertexFilter::Axis { center, axis } => {
			modular_avatar_axis_filter_mask_with_bake(primitive, *center, *axis, axis_context)
		}
		ModularAvatarVertexFilter::Bone { bone_node, threshold } => {
			modular_avatar_bone_filter_mask(primitive, skin_joint_nodes, *bone_node, *threshold)
		}
		ModularAvatarVertexFilter::Mask {
			material_index,
			image_index,
			mode,
		} => context.images.get(*image_index).map_or_else(
			|| vec![false; primitive.positions.len()],
			|image| {
				modular_avatar_mask_filter_mask(
					primitive,
					primitive_index,
					primitive_count,
					image,
					context.image_sources.get(*image_index).and_then(Option::as_ref),
					*material_index,
					*mode,
				)
			},
		),
	}
}

fn modular_avatar_vertex_filter_mask(
	primitive: &UnaMeshBuffers,
	skin_joint_nodes: Option<&[usize]>,
	axis_context: Option<&ModularAvatarAxisBakeContext<'_>>,
	primitive_index: usize,
	primitive_count: usize,
	context: &ModularAvatarVertexFilterContext<'_>,
	group: &ModularAvatarVertexFilterDeleteGroup,
) -> Vec<bool> {
	let mut filters = group.filters.iter();
	let Some(first) = filters.next() else {
		return vec![false; primitive.positions.len()];
	};
	let mut mask = modular_avatar_single_vertex_filter_mask(
		primitive,
		skin_joint_nodes,
		axis_context,
		primitive_index,
		primitive_count,
		context,
		first,
	);
	match group.combine {
		ModularAvatarVertexFilterCombine::Single | ModularAvatarVertexFilterCombine::Union => {
			for filter in filters {
				let next = modular_avatar_single_vertex_filter_mask(
					primitive,
					skin_joint_nodes,
					axis_context,
					primitive_index,
					primitive_count,
					context,
					filter,
				);
				for (target, value) in mask.iter_mut().zip(next) {
					*target = *target || value;
				}
			}
		}
		ModularAvatarVertexFilterCombine::Intersection => {
			for filter in filters {
				let next = modular_avatar_single_vertex_filter_mask(
					primitive,
					skin_joint_nodes,
					axis_context,
					primitive_index,
					primitive_count,
					context,
					filter,
				);
				for (target, value) in mask.iter_mut().zip(next) {
					*target = *target && value;
				}
			}
		}
	}
	mask
}

fn filter_mesh_primitive_triangles_by_vertex_mask(primitive: &mut UnaMeshBuffers, vertex_mask: &[bool]) -> usize {
	let source_indices = primitive.indices.clone().unwrap_or_else(|| {
		(0..primitive.positions.len())
			.filter_map(|index| u32::try_from(index).ok())
			.collect()
	});
	let mut filtered = Vec::with_capacity(source_indices.len());
	let mut removed = 0usize;
	for triangle in source_indices.chunks_exact(3) {
		let v0 = usize::try_from(triangle[0]).ok();
		let v1 = usize::try_from(triangle[1]).ok();
		let v2 = usize::try_from(triangle[2]).ok();
		let delete = [v0, v1, v2]
			.into_iter()
			.flatten()
			.any(|index| vertex_mask.get(index).copied().unwrap_or(false));
		if delete {
			removed += 1;
		} else {
			filtered.extend_from_slice(triangle);
		}
	}
	filtered.extend_from_slice(source_indices.chunks_exact(3).remainder());
	if removed > 0 {
		primitive.indices = Some(filtered);
	}
	removed
}

#[cfg(test)]
fn apply_unavatar_vertex_filters(
	scene: &mut UnaSceneSnapshot,
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
) -> (usize, usize, usize, usize, usize, usize) {
	apply_unavatar_vertex_filters_with_texture_assets(
		scene,
		components,
		node_ids,
		registry_paths,
		paths,
		normalized_paths,
		&BTreeMap::new(),
	)
}

fn apply_unavatar_vertex_filters_with_texture_assets(
	scene: &mut UnaSceneSnapshot,
	components: &[Value],
	node_ids: &BTreeMap<String, usize>,
	registry_paths: &BTreeMap<String, String>,
	paths: &BTreeMap<String, usize>,
	normalized_paths: &BTreeMap<String, Vec<usize>>,
	texture_asset_map: &BTreeMap<String, usize>,
) -> (usize, usize, usize, usize, usize, usize) {
	let images = std::mem::take(&mut scene.images);
	let image_sources = std::mem::take(&mut scene.image_sources);
	let result = {
		let context = ModularAvatarVertexFilterContext {
			images: &images,
			image_sources: &image_sources,
			texture_asset_map,
		};
		let (groups, missing, skipped, unsupported) =
			collect_modular_avatar_vertex_filter_delete_groups(components, node_ids, registry_paths, paths, normalized_paths, &context);
		if groups.is_empty() {
			(0, 0, 0, missing, skipped, unsupported)
		} else {
			let world_matrices = scene_world_matrices(scene);
			let mesh_user_counts =
				scene
					.nodes
					.iter()
					.filter_map(|node| node.mesh)
					.fold(BTreeMap::<usize, usize>::new(), |mut counts, mesh| {
						*counts.entry(mesh).or_default() += 1;
						counts
					});
			let mut mutated_nodes = 0usize;
			let mut mutated_primitives = 0usize;
			let mut removed_triangles = 0usize;
			for group in groups {
				let Some(mesh_idx) = scene.nodes.get(group.target).and_then(|node| node.mesh) else {
					continue;
				};
				let skin_joint_nodes = scene
					.nodes
					.get(group.target)
					.and_then(|node| node.skin)
					.and_then(|skin_index| scene.skins.get(skin_index))
					.map(|skin| skin.joint_nodes.clone());
				let target_world_inv = world_matrices
					.get(group.target)
					.copied()
					.map(inverse_finite_or_identity)
					.unwrap_or(Mat4::IDENTITY);
				let target_skin = scene
					.nodes
					.get(group.target)
					.and_then(|node| node.skin)
					.and_then(|skin_index| scene.skins.get(skin_index))
					.cloned();
				let target_mesh_idx = if mesh_user_counts.get(&mesh_idx).copied().unwrap_or(0) > 1 {
					let Some(mesh) = scene.meshes.get(mesh_idx).cloned() else {
						continue;
					};
					scene.meshes.push(mesh);
					let cloned_idx = scene.meshes.len() - 1;
					if let Some(node) = scene.nodes.get_mut(group.target) {
						node.mesh = Some(cloned_idx);
					}
					cloned_idx
				} else {
					mesh_idx
				};
				let Some(mesh) = scene.meshes.get_mut(target_mesh_idx) else {
					continue;
				};
				let skin_joint_nodes = skin_joint_nodes.as_deref();
				let axis_context = ModularAvatarAxisBakeContext {
					world_matrices: &world_matrices,
					target_world_inv,
					skin: target_skin.as_ref(),
				};
				let mut node_mutated = false;
				let primitive_count = mesh.len();
				for (primitive_index, primitive) in mesh.iter_mut().enumerate() {
					let vertex_mask = modular_avatar_vertex_filter_mask(
						primitive,
						skin_joint_nodes,
						Some(&axis_context),
						primitive_index,
						primitive_count,
						&context,
						&group,
					);
					if !vertex_mask.iter().any(|value| *value) {
						continue;
					}
					let removed = filter_mesh_primitive_triangles_by_vertex_mask(primitive, &vertex_mask);
					if removed > 0 {
						mutated_primitives += 1;
						removed_triangles += removed;
						node_mutated = true;
					}
				}
				if node_mutated {
					mutated_nodes += 1;
				}
			}
			(mutated_nodes, mutated_primitives, removed_triangles, missing, skipped, unsupported)
		}
	};
	scene.images = images;
	scene.image_sources = image_sources;
	result
}

fn report_unavatar_modular_avatar_component_catalog(components: &[Value], report: &mut ImportReport) {
	if components.is_empty() {
		return;
	}
	let mut resolver_supported = 0usize;
	let mut approximate_supported = 0usize;
	let mut runtime_action_supported = 0usize;
	let mut metadata_supported = 0usize;
	let mut unsupported = 0usize;
	let mut disabled = 0usize;
	let mut support_kind_mismatches = 0usize;
	let mut unsupported_types = BTreeMap::<String, usize>::new();
	let mut unsupported_active_types = BTreeMap::<String, usize>::new();
	let mut approximate_active_types = BTreeMap::<String, usize>::new();
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
		let local_support_kind = modular_avatar_component_support_kind(short_type);
		if component
			.get("supportKind")
			.and_then(Value::as_str)
			.filter(|exported| !exported.is_empty() && *exported != local_support_kind)
			.is_some()
		{
			support_kind_mismatches += 1;
		}
		match local_support_kind {
			"resolver" => resolver_supported += 1,
			"approximate" => {
				approximate_supported += 1;
				if !component_disabled {
					*approximate_active_types.entry(short_type.to_string()).or_default() += 1;
				}
			}
			"metadata" => metadata_supported += 1,
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
	for (short_type, count) in &approximate_active_types {
		report.approximations.push(Approximation {
			feature: format!("ModularAvatar.{short_type}"),
			detail: Some(modular_avatar_approximation_detail(short_type, *count)),
		});
	}
	if support_kind_mismatches > 0 {
		report.push_warning(format!(
			".unavatar Modular Avatar component supportKind mismatch: count={support_kind_mismatches}; importer classification was used"
		));
	}
	let unsupported_types = unsupported_types
		.into_iter()
		.map(|(ty, count)| format!("{ty}:{count}"))
		.collect::<Vec<_>>()
		.join(",");
	report.push_info(format!(
		".unavatar Modular Avatar components: total={}, resolver_supported={}, approximate_supported={}, runtime_action_supported={}, metadata_supported={}, unsupported={}, disabled={}, support_kind_mismatches={}, unsupported_types={}",
		components.len(),
		resolver_supported,
		approximate_supported,
		runtime_action_supported,
		metadata_supported,
		unsupported,
		disabled,
		support_kind_mismatches,
		unsupported_types
	));
}

fn modular_avatar_approximation_detail(short_type: &str, count: usize) -> String {
	let scope = match short_type {
		"ModularAvatarBlendshapeSync" => "static default-weight propagation and linear runtime expression bindings",
		"ModularAvatarMergeArmature" => "resolver-side bone/skin merge subset",
		"ModularAvatarMeshCutter" => "enabled static vertex-filter deletion only; ReactiveObject dynamic gating is not evaluated",
		"ModularAvatarMeshSettings" => "renderer metadata subset",
		"ModularAvatarScaleAdjuster" => "resolver-side transform/scale subset",
		"ModularAvatarShapeChanger" => "enabled static set/delete payloads only; ReactiveObject dynamic gating is not evaluated",
		_ => "preserved/applied subset only",
	};
	format!("{count} active approximate component(s); {scope}")
}

fn unavatar_modular_avatar_component_inverted(component: &Value) -> bool {
	component
		.get("Inverted")
		.or_else(|| component.get("inverted"))
		.or_else(|| component.get("m_inverted"))
		.or_else(|| component.get("fields").and_then(|fields| fields.get("Inverted")))
		.or_else(|| component.get("fields").and_then(|fields| fields.get("inverted")))
		.or_else(|| component.get("fields").and_then(|fields| fields.get("m_inverted")))
		.and_then(Value::as_bool)
		.unwrap_or(false)
}

#[cfg(test)]
fn apply_unavatar_modular_avatar(scene: &mut UnaSceneSnapshot, unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	apply_unavatar_modular_avatar_with_context(scene, unavatar, &BTreeMap::new(), None, report);
}

#[cfg(test)]
fn apply_unavatar_modular_avatar_with_humanoid(
	scene: &mut UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
	humanoid_profile: &HumanoidProfile,
	report: &mut ImportReport,
) {
	apply_unavatar_modular_avatar_with_context(scene, unavatar, &BTreeMap::new(), Some(humanoid_profile), report);
}

fn apply_unavatar_modular_avatar_with_texture_assets(
	scene: &mut UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
	texture_asset_map: &BTreeMap<String, usize>,
	humanoid_profile: Option<&HumanoidProfile>,
	report: &mut ImportReport,
) {
	apply_unavatar_modular_avatar_with_context(scene, unavatar, texture_asset_map, humanoid_profile, report);
}

fn apply_unavatar_modular_avatar_with_context(
	scene: &mut UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
	texture_asset_map: &BTreeMap<String, usize>,
	humanoid_profile: Option<&HumanoidProfile>,
	report: &mut ImportReport,
) {
	let Some(modular_avatar) = unavatar_modular_avatar_value(&unavatar.source).and_then(|v| v.as_object()) else {
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
	let step_started = Instant::now();
	let (remove_vcol_nodes, remove_vcol_primitives, remove_vcol_missing, remove_vcol_skipped) =
		apply_unavatar_remove_vertex_color(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths);
	record_modular_avatar_profile_step(report, "remove_vertex_color", step_started);
	if remove_vcol_nodes > 0 || remove_vcol_primitives > 0 || remove_vcol_missing > 0 || remove_vcol_skipped > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: remove_vertex_color_nodes={remove_vcol_nodes}, remove_vertex_color_primitives={remove_vcol_primitives}, remove_vertex_color_missing={remove_vcol_missing}, remove_vertex_color_skipped={remove_vcol_skipped}"
		));
	}
	let step_started = Instant::now();
	let (shape_changer_set_applied, shape_changer_set_missing, shape_changer_set_skipped) =
		apply_unavatar_shape_changer_sets(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths, false);
	record_modular_avatar_profile_step(report, "shape_changer_sets", step_started);
	if shape_changer_set_applied > 0 || shape_changer_set_missing > 0 || shape_changer_set_skipped > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: shape_changer_set_applied={shape_changer_set_applied}, shape_changer_set_missing={shape_changer_set_missing}, shape_changer_set_skipped={shape_changer_set_skipped}"
		));
	}
	let step_started = Instant::now();
	let (
		vertex_filter_nodes,
		vertex_filter_primitives,
		vertex_filter_triangles,
		vertex_filter_missing,
		vertex_filter_skipped,
		vertex_filter_unsupported,
	) = apply_unavatar_vertex_filters_with_texture_assets(
		scene,
		components,
		&node_ids,
		&registry_paths,
		&paths,
		&normalized_paths,
		texture_asset_map,
	);
	record_modular_avatar_profile_step(report, "vertex_filters", step_started);
	if vertex_filter_nodes > 0
		|| vertex_filter_primitives > 0
		|| vertex_filter_triangles > 0
		|| vertex_filter_missing > 0
		|| vertex_filter_skipped > 0
		|| vertex_filter_unsupported > 0
	{
		report.push_info(format!(
			".unavatar Modular Avatar: vertex_filter_nodes={vertex_filter_nodes}, vertex_filter_primitives={vertex_filter_primitives}, vertex_filter_triangles={vertex_filter_triangles}, vertex_filter_missing={vertex_filter_missing}, vertex_filter_skipped={vertex_filter_skipped}, vertex_filter_unsupported={vertex_filter_unsupported}"
		));
	}
	let step_started = Instant::now();
	let (blendshape_sync_applied, blendshape_sync_missing, blendshape_sync_skipped, blendshape_sync_unsupported) =
		apply_unavatar_blendshape_syncs(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths);
	record_modular_avatar_profile_step(report, "blendshape_syncs", step_started);
	if blendshape_sync_applied > 0 || blendshape_sync_missing > 0 || blendshape_sync_skipped > 0 || blendshape_sync_unsupported > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: blendshape_sync_applied={blendshape_sync_applied}, blendshape_sync_missing={blendshape_sync_missing}, blendshape_sync_skipped={blendshape_sync_skipped}, blendshape_sync_unsupported={blendshape_sync_unsupported}"
		));
	}
	let step_started = Instant::now();
	let (mesh_settings_root_bones, mesh_settings_probe_anchors, mesh_settings_bounds, mesh_settings_missing) =
		apply_unavatar_mesh_settings(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths);
	record_modular_avatar_profile_step(report, "mesh_settings", step_started);
	if mesh_settings_root_bones > 0 || mesh_settings_probe_anchors > 0 || mesh_settings_bounds > 0 || mesh_settings_missing > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: mesh_settings_root_bones={}, mesh_settings_probe_anchors={}, mesh_settings_bounds={}, mesh_settings_missing={}",
			mesh_settings_root_bones, mesh_settings_probe_anchors, mesh_settings_bounds, mesh_settings_missing
		));
	}

	let step_started = Instant::now();
	let (scale_adjuster_proxies, scale_adjuster_skin_joints, scale_adjuster_missing, scale_adjuster_skipped) =
		apply_unavatar_scale_adjusters(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths);
	record_modular_avatar_profile_step(report, "scale_adjusters", step_started);
	if scale_adjuster_proxies > 0 || scale_adjuster_skin_joints > 0 || scale_adjuster_missing > 0 || scale_adjuster_skipped > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: scale_adjuster_proxies={}, scale_adjuster_skin_joints={}, scale_adjuster_missing={}, scale_adjuster_skipped={}",
			scale_adjuster_proxies, scale_adjuster_skin_joints, scale_adjuster_missing, scale_adjuster_skipped
		));
	}

	let step_started = Instant::now();
	let (replace_object_applied, replace_object_missing, replace_object_skipped, replace_object_invalid) =
		apply_unavatar_replace_objects(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths);
	record_modular_avatar_profile_step(report, "replace_objects", step_started);
	if replace_object_applied > 0 || replace_object_missing > 0 || replace_object_skipped > 0 || replace_object_invalid > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: replace_object_applied={replace_object_applied}, replace_object_missing={replace_object_missing}, replace_object_skipped={replace_object_skipped}, replace_object_invalid={replace_object_invalid}"
		));
	}

	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let step_started = Instant::now();
	let (merge_mappings, merge_missing, merge_skipped) =
		collect_merge_armature_bone_mappings(components, &node_ids, &registry_paths, &paths, &normalized_paths);
	let merge_retain_nodes =
		collect_merge_armature_retain_nodes(scene, components, unavatar, &node_ids, &registry_paths, &paths, &normalized_paths);
	let merge_mapping_pairs = merge_mappings
		.iter()
		.flat_map(|component| component.mappings.iter().copied())
		.collect::<Vec<_>>();
	let merge_cycle_nodes = count_merge_armature_cycle_nodes(&merge_mapping_pairs);
	let merge_constraint_sources = retarget_merge_armature_node_constraint_sources(scene, &merge_mapping_pairs);
	let parents = scene_parent_indices(scene);
	let (ordered_merge_indices, merge_component_cycles) = order_merge_armature_components(&merge_mappings, &parents);
	let mut merge_auxiliary_reparented = 0usize;
	let mut merge_retargeted = 0usize;
	for merge_index in ordered_merge_indices {
		let component_mappings = &merge_mappings[merge_index].mappings;
		merge_auxiliary_reparented += reparent_merge_armature_auxiliary_bones(scene, component_mappings, &merge_retain_nodes);
		merge_retargeted += retarget_merge_armature_skins(scene, component_mappings);
	}
	record_modular_avatar_profile_step(report, "merge_armature", step_started);
	if merge_retargeted > 0 || merge_auxiliary_reparented > 0 || merge_constraint_sources > 0 || merge_missing > 0 || merge_skipped > 0 {
		report.push_info(format!(
			".unavatar Modular Avatar: merge_armature_mappings={}, mesh_retargeter_joints={}, merge_armature_auxiliary_bones={}, merge_armature_constraint_sources={}, merge_armature_missing={}, merge_armature_skipped={}, merge_armature_cycles={}, merge_armature_component_cycles={}",
			merge_mapping_pairs.len(),
			merge_retargeted,
			merge_auxiliary_reparented,
			merge_constraint_sources,
			merge_missing,
			merge_skipped,
			merge_cycle_nodes,
			merge_component_cycles
		));
	}
	if merge_cycle_nodes > 0 {
		report.push_warning(format!(
			".unavatar Modular Avatar: merge_armature_cycles={} (cyclic bone mapping detected; resolver may approximate)",
			merge_cycle_nodes
		));
	}
	if merge_component_cycles > 0 {
		report.push_warning(format!(
			".unavatar Modular Avatar: merge_armature_component_cycles={} (nested MergeArmature target hierarchy cycle)",
			merge_component_cycles
		));
	}

	let node_ids = scene_node_ids(scene);
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	let step_started = Instant::now();
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
		let Some(child) = unavatar_node_ref_index(target_ref, &node_ids, &registry_paths, &paths, &normalized_paths) else {
			bone_proxy_missing += 1;
			continue;
		};
		let Some(new_parent) = modular_avatar_bone_proxy_target_index(
			scene,
			component,
			&node_ids,
			&registry_paths,
			&paths,
			&normalized_paths,
			humanoid_profile,
		) else {
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
	record_modular_avatar_profile_step(report, "bone_proxy", step_started);

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
	let base_id = unavatar_base_wardrobe_set(&unavatar).map(|(id, _)| id.to_string());
	let base_asset_groups = if base_id.as_deref() != Some(set_id) {
		base_id
			.as_deref()
			.map(|base_set_id| unavatar_wardrobe_set_asset_groups(&unavatar, base_set_id))
			.unwrap_or_default()
	} else {
		Vec::new()
	};
	let selected_asset_groups = unavatar_wardrobe_set_asset_groups(&unavatar, set_id);
	let active_asset_groups = if base_id.as_deref() == Some(set_id) {
		selected_asset_groups
	} else {
		merged_wardrobe_asset_groups(&base_asset_groups, &selected_asset_groups)
	};
	let mut report = {
		let Some(mut runtime) = document.runtime_scene_and_dynamics_mut() else {
			return Err("document has no scene".to_string());
		};
		let lookup = WardrobeLookupContext::new(runtime.scene, Some(&unavatar));
		let step_started = Instant::now();
		reset_runtime_dynamics_enabled(Some(&mut runtime.dynamics));
		log_wardrobe_profile_step("reset_runtime_dynamics_enabled", step_started);
		let mut report = if base_id.as_deref() == Some(set_id) {
			let step_started = Instant::now();
			match filtered_unavatar_base_wardrobe_operations_with_lookup(runtime.scene, &unavatar, &lookup) {
				Some((base_operations, _skipped, reset_operations)) => {
					log_wardrobe_profile_step("filtered_base_operations", step_started);
					let step_started = Instant::now();
					reset_scene_visibility(runtime.scene);
					log_wardrobe_profile_step("reset_scene_visibility", step_started);
					let step_started = Instant::now();
					let _ = apply_unavatar_wardrobe_operations_with_lookup(
						runtime.scene,
						Some(&mut runtime.dynamics),
						&reset_operations,
						&lookup,
					);
					log_wardrobe_profile_step("apply_base_reset_operations", step_started);
					let step_started = Instant::now();
					let report = apply_unavatar_wardrobe_operations_with_lookup(
						runtime.scene,
						Some(&mut runtime.dynamics),
						&base_operations,
						&lookup,
					);
					log_wardrobe_profile_step("apply_base_operations", step_started);
					report
				}
				None => {
					log_wardrobe_profile_step("filtered_base_operations_none", step_started);
					WardrobeApplyReport::default()
				}
			}
		} else {
			if base_id.as_deref() != Some(set_id) {
				let step_started = Instant::now();
				if let Some((base_operations, _skipped, reset_operations)) =
					filtered_unavatar_base_wardrobe_operations_with_lookup(runtime.scene, &unavatar, &lookup)
				{
					log_wardrobe_profile_step("filtered_base_operations", step_started);
					let step_started = Instant::now();
					reset_scene_visibility(runtime.scene);
					log_wardrobe_profile_step("reset_scene_visibility", step_started);
					let step_started = Instant::now();
					let _ = apply_unavatar_wardrobe_operations_with_lookup(
						runtime.scene,
						Some(&mut runtime.dynamics),
						&reset_operations,
						&lookup,
					);
					log_wardrobe_profile_step("apply_base_reset_operations", step_started);
					let step_started = Instant::now();
					let _ = apply_unavatar_wardrobe_operations_with_lookup(
						runtime.scene,
						Some(&mut runtime.dynamics),
						&base_operations,
						&lookup,
					);
					log_wardrobe_profile_step("apply_base_operations", step_started);
				} else {
					log_wardrobe_profile_step("filtered_base_operations_none", step_started);
				}
			}
			let step_started = Instant::now();
			let report = apply_unavatar_wardrobe_operations_with_lookup(runtime.scene, Some(&mut runtime.dynamics), operations, &lookup);
			log_wardrobe_profile_step("apply_selected_operations", step_started);
			report
		};
		let step_started = Instant::now();
		let (shape_changer_set_applied, shape_changer_set_missing, _shape_changer_set_skipped) =
			apply_visible_unavatar_shape_changer_sets_after_wardrobe(runtime.scene, &unavatar);
		log_wardrobe_profile_step("apply_visible_shape_changer_sets", step_started);
		report.blendshape_applied += shape_changer_set_applied;
		report.blendshape_missing += shape_changer_set_missing;
		report
	};
	document.runtime_model_mut().set_active_wardrobe_set(Some(set_id.to_string()));
	document.runtime_model_mut().set_active_asset_groups(active_asset_groups);
	report.active_asset_groups = document.runtime_model().active_asset_groups().to_vec();
	let step_started = Instant::now();
	refresh_wardrobe_apply_report_scoped_assets(document, &mut report);
	log_wardrobe_profile_step("refresh_scoped_assets", step_started);
	Ok(report)
}

fn apply_unavatar_base_wardrobe(scene: &mut UnaSceneSnapshot, unavatar: &UnaUnavatarExtension, report: &mut ImportReport) {
	let lookup = WardrobeLookupContext::new(scene, Some(unavatar));
	let Some((filtered_operations, skipped, reset_operations)) =
		filtered_unavatar_base_wardrobe_operations_with_lookup(scene, unavatar, &lookup)
	else {
		return;
	};
	reset_scene_visibility(scene);
	let _ = apply_unavatar_wardrobe_operations_with_lookup(scene, None, &reset_operations, &lookup);
	let applied = apply_unavatar_wardrobe_operations_with_lookup(scene, None, &filtered_operations, &lookup);
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
	let (shape_changer_set_applied, shape_changer_set_missing, shape_changer_set_skipped) =
		apply_visible_unavatar_shape_changer_sets_after_wardrobe(scene, unavatar);
	if shape_changer_set_applied > 0 || shape_changer_set_missing > 0 || shape_changer_set_skipped > 0 {
		report.push_info(format!(
			".unavatar wardrobe Modular Avatar: visible_shape_changer_set_applied={shape_changer_set_applied}, visible_shape_changer_set_missing={shape_changer_set_missing}, visible_shape_changer_set_skipped={shape_changer_set_skipped}"
		));
	}
}

fn apply_visible_unavatar_shape_changer_sets_after_wardrobe(
	scene: &mut UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
) -> (usize, usize, usize) {
	let Some(modular_avatar) = unavatar_modular_avatar_value(&unavatar.source).and_then(|v| v.as_object()) else {
		return (0, 0, 0);
	};
	let Some(components) = modular_avatar.get("components").and_then(|v| v.as_array()) else {
		return (0, 0, 0);
	};
	let node_ids = scene_node_ids(scene);
	let registry_paths = unavatar_node_registry_paths(Some(unavatar));
	let paths = scene_node_paths(scene);
	let normalized_paths = scene_node_normalized_paths(scene);
	apply_unavatar_shape_changer_sets(scene, components, &node_ids, &registry_paths, &paths, &normalized_paths, true)
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

fn filtered_unavatar_base_wardrobe_operations_with_lookup(
	scene: &UnaSceneSnapshot,
	unavatar: &UnaUnavatarExtension,
	lookup: &WardrobeLookupContext,
) -> Option<(Vec<Value>, usize, Vec<Value>)> {
	let (_, operations) = unavatar_base_wardrobe_set(unavatar)?;
	let mut base_hidden_indices = BTreeSet::new();
	let mut base_hidden_paths = Vec::new();
	for op in operations
		.iter()
		.filter(|op| op.get("visible").and_then(|v| v.as_bool()) == Some(false))
	{
		let resolved = lookup_operation_subtree_targets_all_with_lookup(scene, lookup, op);
		if resolved.is_empty() {
			let path = operation_target_path(op);
			if !path.is_empty() {
				base_hidden_paths.push(path.to_string());
			}
		} else {
			for idx in resolved {
				base_hidden_indices.insert(idx);
				if let Some(path) = lookup.paths_by_index.get(idx).and_then(|p| p.clone()) {
					if !path.is_empty() {
						base_hidden_paths.push(path);
					}
				}
			}
		}
	}
	let base_hidden_normalized_paths = base_hidden_paths
		.iter()
		.map(|path| normalize_unavatar_path(path))
		.filter(|path| !path.is_empty())
		.collect::<Vec<_>>();
	let mut filtered_operations = Vec::with_capacity(operations.len());
	let mut reset_operations = Vec::new();
	for op in operations {
		let mut skip_inherited_hidden = false;
		let ty = op.get("type").or_else(|| op.get("op")).and_then(|v| v.as_str()).unwrap_or("");
		let is_hidden_visibility = matches!(
			ty,
			"subtreeEnabled" | "subtreeVisibility" | "nodeEnabled" | "nodeVisibility" | "rendererEnabled" | "rendererVisibility"
		) && op.get("visible").and_then(|v| v.as_bool()) == Some(false);
		let mut resolved_visibility_targets = None;
		if is_hidden_visibility {
			let resolved = lookup_operation_targets_all(
				&lookup.node_ids,
				&lookup.registry_paths,
				&lookup.paths,
				&lookup.normalized_paths,
				op,
			);
			if !resolved.is_empty()
				&& resolved.iter().all(|idx| {
					let mut parent = lookup.parent_by_index.get(*idx).copied().flatten();
					while let Some(parent_idx) = parent {
						if base_hidden_indices.contains(&parent_idx) {
							return true;
						}
						parent = lookup.parent_by_index.get(parent_idx).copied().flatten();
					}
					false
				}) {
				skip_inherited_hidden = true;
			}
			resolved_visibility_targets = Some(resolved);
		}
		if !skip_inherited_hidden && is_hidden_visibility {
			let resolved = resolved_visibility_targets.unwrap_or_else(|| {
				lookup_operation_targets_all(
					&lookup.node_ids,
					&lookup.registry_paths,
					&lookup.paths,
					&lookup.normalized_paths,
					op,
				)
			});
			skip_inherited_hidden = base_operation_is_inherited_hidden_under_base_resolved(
				op,
				&base_hidden_normalized_paths,
				&resolved,
				&lookup.paths_by_index,
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
	let mut alpha_cache = vec![None; images.len()];
	for material in materials {
		if !unavatar_material_is_liltoon(material) {
			continue;
		}
		let Some(image_index) = material.base_color_texture_index else {
			continue;
		};
		let Some(image) = images.get(image_index) else {
			continue;
		};
		if image.width == 0 || image.height == 0 {
			continue;
		}
		let (has_transparent_alpha, has_translucent_alpha) = if let Some(cached) = alpha_cache.get(image_index).copied().flatten() {
			cached
		} else {
			let has_transparent_alpha = image_alpha_has_transparency(image);
			let has_translucent_alpha = has_transparent_alpha && image_alpha_has_translucency(image);
			let cached = (has_transparent_alpha, has_translucent_alpha);
			if let Some(slot) = alpha_cache.get_mut(image_index) {
				*slot = Some(cached);
			}
			cached
		};
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
				} else if material.alpha_cutoff <= 0.01 && has_translucent_alpha {
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

	let source_shader_has_outline_pass = source_shader.to_ascii_lowercase().contains("outline");
	out.outline.enabled_factor = if source_shader_has_outline_pass {
		1.0
	} else {
		unavatar_material_float_param(extras, "_UseOutline").unwrap_or(0.0).clamp(0.0, 1.0)
	};
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
	let mtoon_outline_width = mtoon.and_then(|m| json_f32(m.get("outlineWidthFactor").or_else(|| m.get("outline_width_factor"))));
	let source_outline_width = unavatar_material_float_param(extras, "_OutlineWidth");
	let outline_width = match (mtoon_outline_width, source_outline_width) {
		(Some(value), Some(source_value)) if source_shader_has_outline_pass && value <= 0.0 && source_value > 0.0 => {
			Some(source_value * 0.01)
		}
		(Some(value), _) => Some(value * liltoon_outline_width_scale),
		(None, Some(source_value)) => Some(source_value * 0.01),
		(None, None) => None,
	};
	if let Some(value) = outline_width {
		out.outline.width_factor = value;
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PrimitiveVertexPayloadKey {
	positions: Option<usize>,
	normals: Option<usize>,
	tangents: Option<usize>,
	tex_coords_0: Option<usize>,
	tex_coords_1: Option<usize>,
	tex_coords_2: Option<usize>,
	tex_coords_3: Option<usize>,
	colors_0: Option<usize>,
	joints_0: Option<usize>,
	weights_0: Option<usize>,
	morph_targets: Vec<(Option<usize>, Option<usize>)>,
	mesh_weights: Vec<u32>,
	mesh_target_names: Vec<String>,
}

#[derive(Clone, Debug)]
struct PrimitiveVertexPayload {
	positions: Vec<[f32; 3]>,
	normals: Option<Vec<[f32; 3]>>,
	tangents: Option<Vec<[f32; 4]>>,
	tex_coords_0: Option<Vec<[f32; 2]>>,
	tex_coords_1: Option<Vec<[f32; 2]>>,
	tex_coords_2: Option<Vec<[f32; 2]>>,
	tex_coords_3: Option<Vec<[f32; 2]>>,
	colors_0: Option<Vec<[f32; 4]>>,
	joints: Option<Vec<[u16; 4]>>,
	weights: Option<Vec<[f32; 4]>>,
	morph_targets: Vec<UnaMorphTargetDeltas>,
	morph_target_names: Vec<String>,
	default_morph_weights: Vec<f32>,
}

#[derive(Clone, Copy, Debug, Default)]
struct PrimitiveReadProfile {
	cache_clone: Duration,
	cache_take: Duration,
	positions: Duration,
	joints_weights: Duration,
	attributes: Duration,
	indices: Duration,
	morphs: Duration,
	defaults: Duration,
	cache_insert: Duration,
}

impl PrimitiveReadProfile {
	fn add(&mut self, other: Self) {
		self.cache_clone += other.cache_clone;
		self.cache_take += other.cache_take;
		self.positions += other.positions;
		self.joints_weights += other.joints_weights;
		self.attributes += other.attributes;
		self.indices += other.indices;
		self.morphs += other.morphs;
		self.defaults += other.defaults;
		self.cache_insert += other.cache_insert;
	}
}

#[derive(Clone, Copy, Debug)]
struct PrimitiveVertexPayloadCacheConfig {
	disabled: bool,
	min_uses: usize,
}

impl PrimitiveVertexPayloadCacheConfig {
	fn from_env() -> Self {
		Self {
			disabled: std::env::var_os("UN_AVATAR_DISABLE_IMPORT_VERTEX_PAYLOAD_CACHE").is_some(),
			min_uses: std::env::var("UN_AVATAR_IMPORT_VERTEX_PAYLOAD_CACHE_MIN_USES")
				.ok()
				.and_then(|value| value.parse::<usize>().ok())
				.unwrap_or(2)
				.max(2),
		}
	}
}

fn accessor_index(accessor: Option<gltf::Accessor<'_>>) -> Option<usize> {
	accessor.map(|accessor| accessor.index())
}

fn primitive_vertex_payload_key(
	prim: &gltf::Primitive<'_>,
	mesh_weights: Option<&[f32]>,
	mesh_target_names: &[String],
) -> PrimitiveVertexPayloadKey {
	PrimitiveVertexPayloadKey {
		positions: accessor_index(prim.get(&gltf::Semantic::Positions)),
		normals: accessor_index(prim.get(&gltf::Semantic::Normals)),
		tangents: accessor_index(prim.get(&gltf::Semantic::Tangents)),
		tex_coords_0: accessor_index(prim.get(&gltf::Semantic::TexCoords(0))),
		tex_coords_1: accessor_index(prim.get(&gltf::Semantic::TexCoords(1))),
		tex_coords_2: accessor_index(prim.get(&gltf::Semantic::TexCoords(2))),
		tex_coords_3: accessor_index(prim.get(&gltf::Semantic::TexCoords(3))),
		colors_0: accessor_index(prim.get(&gltf::Semantic::Colors(0))),
		joints_0: accessor_index(prim.get(&gltf::Semantic::Joints(0))),
		weights_0: accessor_index(prim.get(&gltf::Semantic::Weights(0))),
		morph_targets: prim
			.morph_targets()
			.map(|target| (accessor_index(target.positions()), accessor_index(target.normals())))
			.collect(),
		mesh_weights: mesh_weights.unwrap_or_default().iter().map(|weight| weight.to_bits()).collect(),
		mesh_target_names: mesh_target_names.to_vec(),
	}
}

fn mesh_buffers_from_vertex_payload(
	payload: PrimitiveVertexPayload,
	indices: Option<Vec<u32>>,
	material_index: Option<usize>,
	vertex_payload_id: Option<u64>,
) -> UnaMeshBuffers {
	UnaMeshBuffers {
		name: None,
		vertex_payload_id,
		positions: payload.positions,
		normals: payload.normals,
		tangents: payload.tangents,
		tex_coords_0: payload.tex_coords_0,
		tex_coords_1: payload.tex_coords_1,
		tex_coords_2: payload.tex_coords_2,
		tex_coords_3: payload.tex_coords_3,
		colors_0: payload.colors_0,
		joints: payload.joints,
		weights: payload.weights,
		indices,
		material_index,
		morph_targets: payload.morph_targets,
		morph_target_names: payload.morph_target_names,
		default_morph_weights: payload.default_morph_weights,
	}
}

struct PrimitiveReadInput<'a> {
	prim: gltf::Primitive<'a>,
	buffers: &'a [gltf::buffer::Data],
	mesh_weights: Option<&'a [f32]>,
	mesh_target_names: &'a [String],
	payload_key: PrimitiveVertexPayloadKey,
	vertex_payload_id: Option<u64>,
	vertex_payload_cache_last_use: bool,
	vertex_payload_cache: &'a mut BTreeMap<PrimitiveVertexPayloadKey, PrimitiveVertexPayload>,
	vertex_payload_key_counts: &'a BTreeMap<PrimitiveVertexPayloadKey, usize>,
	cache_config: PrimitiveVertexPayloadCacheConfig,
	report: &'a mut ImportReport,
}

fn read_primitive(input: PrimitiveReadInput<'_>) -> Result<Option<(UnaMeshBuffers, bool, bool, PrimitiveReadProfile)>, ImportError> {
	let PrimitiveReadInput {
		prim,
		buffers,
		mesh_weights,
		mesh_target_names,
		payload_key,
		vertex_payload_id,
		vertex_payload_cache_last_use,
		vertex_payload_cache,
		vertex_payload_key_counts,
		cache_config,
		report,
	} = input;
	if prim.mode() != gltf::mesh::Mode::Triangles {
		report.approximations.push(Approximation {
			feature: "primitive.mode".into(),
			detail: Some(format!("{:?} はスキップ（Triangles のみ）", prim.mode())),
		});
		return Ok(None);
	}

	let cache_reusable =
		!cache_config.disabled && vertex_payload_key_counts.get(&payload_key).copied().unwrap_or(0) >= cache_config.min_uses;
	let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d.as_ref()));
	if cache_reusable {
		let cache_clone_started = Instant::now();
		let payload = if vertex_payload_cache_last_use {
			vertex_payload_cache.remove(&payload_key).map(|payload| (payload, true))
		} else {
			vertex_payload_cache.get(&payload_key).cloned().map(|payload| (payload, false))
		};
		if let Some((payload, cache_take)) = payload {
			let cache_elapsed = cache_clone_started.elapsed();
			let indices_started = Instant::now();
			let indices = reader.read_indices().map(|idx| idx.into_u32().collect());
			let indices_elapsed = indices_started.elapsed();
			let material_index = prim.material().index();
			let profile = if cache_take {
				PrimitiveReadProfile {
					cache_take: cache_elapsed,
					indices: indices_elapsed,
					..Default::default()
				}
			} else {
				PrimitiveReadProfile {
					cache_clone: cache_elapsed,
					indices: indices_elapsed,
					..Default::default()
				}
			};
			return Ok(Some((
				mesh_buffers_from_vertex_payload(payload, indices, material_index, vertex_payload_id),
				true,
				cache_reusable,
				profile,
			)));
		}
	}
	let Some(iter_pos) = reader.read_positions() else {
		return Err(ImportError::Message("POSITION アクセサがありません".into()));
	};
	let positions_started = Instant::now();
	let positions: Vec<[f32; 3]> = iter_pos.collect();
	let positions_elapsed = positions_started.elapsed();

	let joints_weights_started = Instant::now();
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
	let joints_weights_elapsed = joints_weights_started.elapsed();

	let attributes_started = Instant::now();
	let normals = reader.read_normals().map(|it| it.collect());
	let tangents = reader.read_tangents().map(|it| it.collect());
	let tex_coords_0 = reader.read_tex_coords(0).map(|tc| tc.into_f32().collect());
	let tex_coords_1 = reader.read_tex_coords(1).map(|tc| tc.into_f32().collect());
	let tex_coords_2 = reader.read_tex_coords(2).map(|tc| tc.into_f32().collect());
	let tex_coords_3 = reader.read_tex_coords(3).map(|tc| tc.into_f32().collect());
	let colors_0 = reader.read_colors(0).map(|colors| colors.into_rgba_f32().collect());
	let attributes_elapsed = attributes_started.elapsed();
	let indices_started = Instant::now();
	let indices = reader.read_indices().map(|idx| idx.into_u32().collect());
	let indices_elapsed = indices_started.elapsed();
	let material_index = prim.material().index();
	let (joints, weights) = joints_weights;

	let morphs_started = Instant::now();
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
	let morphs_elapsed = morphs_started.elapsed();

	let defaults_started = Instant::now();
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
	let defaults_elapsed = defaults_started.elapsed();

	let payload = PrimitiveVertexPayload {
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
		morph_targets,
		morph_target_names,
		default_morph_weights,
	};
	let cache_insert_started = Instant::now();
	if cache_reusable && !vertex_payload_cache_last_use {
		vertex_payload_cache.insert(payload_key, payload.clone());
	}
	let cache_insert_elapsed = cache_insert_started.elapsed();

	Ok(Some((
		mesh_buffers_from_vertex_payload(payload, indices, material_index, vertex_payload_id),
		false,
		cache_reusable,
		PrimitiveReadProfile {
			positions: positions_elapsed,
			joints_weights: joints_weights_elapsed,
			attributes: attributes_elapsed,
			indices: indices_elapsed,
			morphs: morphs_elapsed,
			defaults: defaults_elapsed,
			cache_insert: cache_insert_elapsed,
			..Default::default()
		},
	)))
}

/// glTF [`Document`] から [`UnaSceneSnapshot`] を構築（メッシュ・材質・スキン・ノード階層）。
pub fn scene_snapshot_from_gltf(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	image_data: Vec<gltf::image::Data>,
	report: &mut ImportReport,
) -> Result<UnaSceneSnapshot, ImportError> {
	scene_snapshot_from_gltf_inner(document, buffers, image_data.into_iter().map(Some).collect(), None, report, false)
}

pub fn scene_snapshot_from_gltf_profiled(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	image_data: Vec<gltf::image::Data>,
	report: &mut ImportReport,
) -> Result<UnaSceneSnapshot, ImportError> {
	scene_snapshot_from_gltf_inner(document, buffers, image_data.into_iter().map(Some).collect(), None, report, true)
}

fn log_scene_snapshot_profile_step(step: &str, started: Instant) {
	eprintln!(
		"un-avatar-renderer: gltf scene profile step={step} elapsed={:.1}ms",
		started.elapsed().as_secs_f64() * 1000.0
	);
}

fn record_scene_snapshot_profile_step(report: &mut ImportReport, profile: bool, step: &str, started: Instant) {
	let elapsed_ms = started.elapsed().as_millis();
	report.push_info(format!("glTF scene profile: {step}_ms={elapsed_ms}"));
	if profile {
		log_scene_snapshot_profile_step(step, started);
	}
}

fn record_gltf_import_profile_step(report: &mut ImportReport, step: &str, started: Instant) {
	report.push_info(format!("glTF import profile: {step}_ms={}", started.elapsed().as_millis()));
}

fn initial_image_decode_indices_for_import(
	root_json: Option<&Value>,
	initial_wardrobe_set: Option<&str>,
	defer_initial_image_decode: bool,
	import_profile_messages: &mut Vec<String>,
) -> Option<BTreeSet<usize>> {
	let indices = initial_resident_image_indices(root_json, initial_wardrobe_set);
	if defer_initial_image_decode {
		if let Some(indices) = &indices {
			import_profile_messages.push(format!(
				"glTF import profile: deferred_initial_image_decode_count={}",
				indices.len()
			));
			return Some(BTreeSet::new());
		}
	}
	if let Some(indices) = &indices {
		import_profile_messages.push(format!("glTF import profile: selective_image_decode_count={}", indices.len()));
	}
	indices
}

fn deferred_image_indices_for_decode_selection(
	root_json: Option<&Value>,
	decode_image_indices: Option<&BTreeSet<usize>>,
) -> Option<BTreeSet<usize>> {
	let decode_image_indices = decode_image_indices?;
	let image_count = root_json
		.and_then(|root| root.get("images"))
		.and_then(Value::as_array)
		.map(Vec::len)
		.unwrap_or(0);
	Some((0..image_count).filter(|index| !decode_image_indices.contains(index)).collect())
}

fn record_modular_avatar_profile_step(report: &mut ImportReport, step: &str, started: Instant) {
	report.push_info(format!(
		"glTF import profile: modular_avatar.{step}_ms={}",
		started.elapsed().as_millis()
	));
}

fn scene_snapshot_from_gltf_inner(
	document: &gltf::Document,
	buffers: &[gltf::buffer::Data],
	image_data: Vec<Option<gltf::image::Data>>,
	precomputed_image_sources: Option<Vec<Option<UnaImageSourceMetadata>>>,
	report: &mut ImportReport,
	profile: bool,
) -> Result<UnaSceneSnapshot, ImportError> {
	let step_started = Instant::now();
	let mut materials = build_materials(document);
	if materials.is_empty() {
		materials.push(UnaMaterialPbr::default());
	}
	record_scene_snapshot_profile_step(report, profile, "build_materials", step_started);

	let step_started = Instant::now();
	let mut image_sources = if let Some(image_sources) = precomputed_image_sources {
		record_scene_snapshot_profile_step(report, profile, "reuse_image_source_metadata", step_started);
		image_sources
	} else {
		let image_sources = collect_image_source_metadata(document, buffers);
		record_scene_snapshot_profile_step(report, profile, "collect_image_source_metadata", step_started);
		image_sources
	};
	let step_started = Instant::now();
	let images = collect_scene_images_from_imported_data(image_data, report).map_err(ImportError::Message)?;
	record_scene_snapshot_profile_step(report, profile, "collect_images", step_started);
	let retained_encoded_sources = retain_encoded_bytes_for_deferred_images(&mut image_sources, &images);
	if retained_encoded_sources > 0 {
		report.push_info(format!(
			"glTF import profile: retained_deferred_encoded_image_count={retained_encoded_sources}"
		));
	}
	let path_backed_deferred_sources = path_backed_deferred_image_source_count(&image_sources, &images);
	if path_backed_deferred_sources > 0 {
		report.push_info(format!(
			"glTF import profile: file_backed_deferred_encoded_image_count={path_backed_deferred_sources}"
		));
	}
	let step_started = Instant::now();
	refine_liltoon_alpha_from_images(&mut materials, &images);
	record_scene_snapshot_profile_step(report, profile, "refine_liltoon_alpha_from_images", step_started);

	let step_started = Instant::now();
	let skins = build_skins(document, buffers)?;
	record_scene_snapshot_profile_step(report, profile, "build_skins", step_started);

	let step_started = Instant::now();
	let mut meshes: Vec<Vec<UnaMeshBuffers>> = document.meshes().map(|mesh| Vec::with_capacity(mesh.primitives().len())).collect();
	let mut vertex_payload_key_counts = BTreeMap::<PrimitiveVertexPayloadKey, usize>::new();
	for mesh in document.meshes() {
		let mw = mesh.weights();
		let target_names = mesh_target_names(mesh.clone());
		for prim in mesh.primitives() {
			if prim.mode() == gltf::mesh::Mode::Triangles {
				*vertex_payload_key_counts
					.entry(primitive_vertex_payload_key(&prim, mw, &target_names))
					.or_default() += 1;
			}
		}
	}
	let mut vertex_payload_key_ids = BTreeMap::<PrimitiveVertexPayloadKey, u64>::new();
	let vertex_payload_cache_config = PrimitiveVertexPayloadCacheConfig::from_env();
	let mut next_vertex_payload_id = 1u64;
	for (key, count) in &vertex_payload_key_counts {
		if !vertex_payload_cache_config.disabled && *count >= vertex_payload_cache_config.min_uses {
			vertex_payload_key_ids.insert(key.clone(), next_vertex_payload_id);
			next_vertex_payload_id = next_vertex_payload_id.saturating_add(1);
		}
	}
	let mut vertex_payload_cache = BTreeMap::new();
	let mut vertex_payload_remaining_counts = vertex_payload_key_counts.clone();
	let mut mesh_primitive_count = 0usize;
	let mut mesh_cacheable_primitive_count = 0usize;
	let mut mesh_vertex_payload_cache_hits = 0usize;
	let mut mesh_vertex_count = 0usize;
	let mut mesh_index_count = 0usize;
	let mut mesh_morph_target_count = 0usize;
	let mut mesh_read_profile = PrimitiveReadProfile::default();
	for mesh in document.meshes() {
		let mid = mesh.index();
		let mw = mesh.weights();
		let target_names = mesh_target_names(mesh.clone());
		for prim in mesh.primitives() {
			let primitive_started = Instant::now();
			let primitive_index = prim.index();
			let payload_key = primitive_vertex_payload_key(&prim, mw, &target_names);
			let vertex_payload_id = vertex_payload_key_ids.get(&payload_key).copied();
			let vertex_payload_cache_last_use = if vertex_payload_id.is_some() {
				let remaining = vertex_payload_remaining_counts.entry(payload_key.clone()).or_default();
				*remaining = remaining.saturating_sub(1);
				*remaining == 0
			} else {
				false
			};
			if let Some((buf, cache_hit, cacheable, primitive_profile)) = read_primitive(PrimitiveReadInput {
				prim,
				buffers,
				mesh_weights: mw,
				mesh_target_names: &target_names,
				payload_key,
				vertex_payload_id,
				vertex_payload_cache_last_use,
				vertex_payload_cache: &mut vertex_payload_cache,
				vertex_payload_key_counts: &vertex_payload_key_counts,
				cache_config: vertex_payload_cache_config,
				report,
			})? {
				mesh_read_profile.add(primitive_profile);
				mesh_primitive_count += 1;
				mesh_cacheable_primitive_count += usize::from(cacheable);
				mesh_vertex_payload_cache_hits += usize::from(cache_hit);
				let vertex_count = buf.positions.len();
				let index_count = buf.indices.as_ref().map(Vec::len).unwrap_or(0);
				let morph_count = buf.morph_targets.len();
				mesh_vertex_count += vertex_count;
				mesh_index_count += index_count;
				mesh_morph_target_count += morph_count;
				if profile {
					eprintln!(
						"un-avatar-renderer: gltf scene primitive profile mesh={mid} primitive={primitive_index} vertices={vertex_count} indices={index_count} morphs={morph_count} elapsed={:.1}ms",
						primitive_started.elapsed().as_secs_f64() * 1000.0
					);
				}
				if mid < meshes.len() {
					meshes[mid].push(buf);
				}
			}
		}
	}
	record_scene_snapshot_profile_step(report, profile, "read_meshes", step_started);
	report.push_info(format!(
		"glTF scene profile: read_meshes.primitives={mesh_primitive_count} cacheable={mesh_cacheable_primitive_count} vertex_payload_cache_hits={mesh_vertex_payload_cache_hits} vertices={mesh_vertex_count} indices={mesh_index_count} morph_targets={mesh_morph_target_count}"
	));
	report.push_info(format!(
		"glTF scene profile: read_meshes.stage_ms cache_clone={} cache_take={} positions={} joints_weights={} attributes={} indices={} morphs={} defaults={} cache_insert={}",
		mesh_read_profile.cache_clone.as_millis(),
		mesh_read_profile.cache_take.as_millis(),
		mesh_read_profile.positions.as_millis(),
		mesh_read_profile.joints_weights.as_millis(),
		mesh_read_profile.attributes.as_millis(),
		mesh_read_profile.indices.as_millis(),
		mesh_read_profile.morphs.as_millis(),
		mesh_read_profile.defaults.as_millis(),
		mesh_read_profile.cache_insert.as_millis()
	));

	let step_started = Instant::now();
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
	record_scene_snapshot_profile_step(report, profile, "read_nodes", step_started);

	let step_started = Instant::now();
	let roots: Vec<usize> = document
		.default_scene()
		.or_else(|| document.scenes().next())
		.map(|s| s.nodes().map(|n| n.index()).collect())
		.unwrap_or_default();
	record_scene_snapshot_profile_step(report, profile, "read_roots", step_started);

	let scene = UnaSceneSnapshot {
		meshes,
		materials,
		images,
		image_sources,
		skins,
		nodes,
		roots,
		node_constraints: Vec::new(),
		asset_group_ownership: Vec::new(),
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

	fn import(&self, ctx: &mut ImportContext, input: ImportInput, _options: ImportOptions) -> Result<ImportResult, ImportError> {
		let mut root_json: Option<Value> = None;
		let mut original_image_sources: Option<Vec<Option<UnaImageSourceMetadata>>> = None;
		let mut original_glb_bin: Option<Vec<u8>> = None;
		let mut original_glb_file_path: Option<std::path::PathBuf> = None;
		let mut original_glb_bin_range: Option<Range<usize>> = None;
		let import_started = Instant::now();
		let mut import_profile_messages = Vec::new();
		let (path_hint, document, buffers, image_data) = match input {
			ImportInput::Path(path) => {
				let extension = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
				if matches!(extension.as_deref(), Some("unavatar" | "glb")) {
					let mut precomputed_decode_image_indices = None;
					let read_started = Instant::now();
					let bytes = std::fs::read(&path).map_err(|e| ImportError::Message(format!("{}: {e}", path.display())))?;
					import_profile_messages.push(format!(
						"glTF import profile: file_read_bytes={} file_read_ms={}",
						bytes.len(),
						read_started.elapsed().as_millis()
					));
					if bytes.starts_with(b"glTF") {
						let glb_started = Instant::now();
						let (root, bin_range) = read_glb_json_and_bin_range(&bytes)?;
						let bin = &bytes[bin_range.clone()];
						import_profile_messages.push(format!(
							"glTF import profile: glb_json_bin_borrow_ms={} bin_bytes={}",
							glb_started.elapsed().as_millis(),
							bin.len()
						));
						let decode_image_indices = initial_image_decode_indices_for_import(
							Some(&root),
							ctx.initial_wardrobe_set.as_deref(),
							ctx.defer_initial_image_decode,
							&mut import_profile_messages,
						);
						let retain_encoded_indices =
							deferred_image_indices_for_decode_selection(Some(&root), decode_image_indices.as_ref());
						precomputed_decode_image_indices = decode_image_indices;
						let source_started = Instant::now();
						if ctx.profile {
							let (image_sources, source_profile) = collect_glb_image_source_metadata_profiled(
								&root,
								bin,
								retain_encoded_indices.as_ref(),
								Some(&path),
								bin_range.start as u64,
							);
							original_image_sources = Some(image_sources);
							import_profile_messages.push(format!(
								"glTF import profile: image_source_metadata.detail dimensions_ms={:.1} hash_ms={:.1} hash_mb={:.1}",
								source_profile.dimensions_ms(),
								source_profile.hash_ms(),
								source_profile.hash_mb()
							));
						} else {
							original_image_sources = Some(collect_glb_image_source_metadata(
								&root,
								bin,
								retain_encoded_indices.as_ref(),
								Some(&path),
								bin_range.start as u64,
							));
						}
						import_profile_messages.push(format!(
							"glTF import profile: image_source_metadata_ms={}",
							source_started.elapsed().as_millis()
						));
						original_glb_bin_range = Some(bin_range);
						root_json = Some(root);
					} else if extension.as_deref() == Some("unavatar") {
						let json_started = Instant::now();
						root_json = Some(gltf_root_json_from_bytes(&bytes)?);
						import_profile_messages.push(format!("glTF import profile: root_json_ms={}", json_started.elapsed().as_millis()));
					}
					let normalize_started = Instant::now();
					let import_bytes = if bytes.starts_with(b"glTF") && root_json.as_ref().is_some_and(|root| !root_has_webp_image(root)) {
						Cow::Borrowed(bytes.as_slice())
					} else {
						normalize_webp_glb_for_gltf_import(&bytes)?
					};
					let normalized_owned = matches!(&import_bytes, Cow::Owned(_));
					import_profile_messages.push(format!(
						"glTF import profile: webp_normalize_ms={} rebuilt_glb={}",
						normalize_started.elapsed().as_millis(),
						normalized_owned
					));
					let decode_image_indices = precomputed_decode_image_indices.or_else(|| {
						initial_image_decode_indices_for_import(
							root_json.as_ref(),
							ctx.initial_wardrobe_set.as_deref(),
							ctx.defer_initial_image_decode,
							&mut import_profile_messages,
						)
					});
					let import_slice_started = Instant::now();
					let imported = import_gltf_slice_parallel_images(import_bytes.as_ref(), decode_image_indices.as_ref())?;
					import_profile_messages.push(format!(
						"glTF import profile: gltf_import_slice_ms={}",
						import_slice_started.elapsed().as_millis()
					));
					import_profile_messages.push(format!(
						"glTF import profile: gltf_import_slice.parse_ms={} buffers_ms={} image_decode_ms={} images={}/{} workers={}",
						imported.3.parse_ms,
						imported.3.buffers_ms,
						imported.3.image_decode_ms,
						imported.3.decoded_image_count,
						imported.3.image_count,
						imported.3.image_decode_workers
					));
					if bytes.starts_with(b"glTF") {
						original_glb_file_path = Some(path.clone());
					}
					(Some(path), imported.0, imported.1, imported.2)
				} else if path
					.extension()
					.and_then(|e| e.to_str())
					.is_some_and(|e| e.eq_ignore_ascii_case("gltf"))
				{
					let read_started = Instant::now();
					let bytes = std::fs::read(&path).map_err(|e| ImportError::Message(format!("{}: {e}", path.display())))?;
					import_profile_messages.push(format!(
						"glTF import profile: file_read_bytes={} file_read_ms={}",
						bytes.len(),
						read_started.elapsed().as_millis()
					));
					let json_started = Instant::now();
					root_json = Some(gltf_root_json_from_bytes(&bytes)?);
					import_profile_messages.push(format!("glTF import profile: root_json_ms={}", json_started.elapsed().as_millis()));
					let import_started = Instant::now();
					let imported = gltf::import(&path).map_err(|e| ImportError::Message(e.to_string()))?;
					import_profile_messages.push(format!(
						"glTF import profile: gltf_import_path_ms={}",
						import_started.elapsed().as_millis()
					));
					(Some(path), imported.0, imported.1, imported.2.into_iter().map(Some).collect())
				} else {
					let import_started = Instant::now();
					let imported = gltf::import(&path).map_err(|e| ImportError::Message(e.to_string()))?;
					import_profile_messages.push(format!(
						"glTF import profile: gltf_import_path_ms={}",
						import_started.elapsed().as_millis()
					));
					(Some(path), imported.0, imported.1, imported.2.into_iter().map(Some).collect())
				}
			}
			ImportInput::Bytes { bytes, path_hint } => {
				let mut precomputed_decode_image_indices = None;
				if bytes.as_ref().starts_with(b"glTF") {
					let glb_started = Instant::now();
					let (root, bin) = read_glb_json_and_bin(bytes.as_ref())?;
					import_profile_messages.push(format!(
						"glTF import profile: in_memory_bytes={} glb_json_bin_copy_ms={} bin_bytes={}",
						bytes.len(),
						glb_started.elapsed().as_millis(),
						bin.len()
					));
					let decode_image_indices = initial_image_decode_indices_for_import(
						Some(&root),
						ctx.initial_wardrobe_set.as_deref(),
						ctx.defer_initial_image_decode,
						&mut import_profile_messages,
					);
					let retain_encoded_indices = deferred_image_indices_for_decode_selection(Some(&root), decode_image_indices.as_ref());
					precomputed_decode_image_indices = decode_image_indices;
					let source_started = Instant::now();
					if ctx.profile {
						let (image_sources, source_profile) =
							collect_glb_image_source_metadata_profiled(&root, &bin, retain_encoded_indices.as_ref(), None, 0);
						original_image_sources = Some(image_sources);
						import_profile_messages.push(format!(
							"glTF import profile: image_source_metadata.detail dimensions_ms={:.1} hash_ms={:.1} hash_mb={:.1}",
							source_profile.dimensions_ms(),
							source_profile.hash_ms(),
							source_profile.hash_mb()
						));
					} else {
						original_image_sources = Some(collect_glb_image_source_metadata(
							&root,
							&bin,
							retain_encoded_indices.as_ref(),
							None,
							0,
						));
					}
					import_profile_messages.push(format!(
						"glTF import profile: image_source_metadata_ms={}",
						source_started.elapsed().as_millis()
					));
					original_glb_bin = Some(bin);
					root_json = Some(root);
				} else {
					let json_started = Instant::now();
					root_json = Some(gltf_root_json_from_bytes(bytes.as_ref())?);
					import_profile_messages.push(format!(
						"glTF import profile: in_memory_bytes={} root_json_ms={}",
						bytes.len(),
						json_started.elapsed().as_millis()
					));
				}
				let normalize_started = Instant::now();
				let import_bytes =
					if bytes.as_ref().starts_with(b"glTF") && root_json.as_ref().is_some_and(|root| !root_has_webp_image(root)) {
						Cow::Borrowed(bytes.as_ref())
					} else {
						normalize_webp_glb_for_gltf_import(bytes.as_ref())?
					};
				let normalized_owned = matches!(&import_bytes, Cow::Owned(_));
				import_profile_messages.push(format!(
					"glTF import profile: webp_normalize_ms={} rebuilt_glb={}",
					normalize_started.elapsed().as_millis(),
					normalized_owned
				));
				let decode_image_indices = precomputed_decode_image_indices.or_else(|| {
					initial_image_decode_indices_for_import(
						root_json.as_ref(),
						ctx.initial_wardrobe_set.as_deref(),
						ctx.defer_initial_image_decode,
						&mut import_profile_messages,
					)
				});
				let import_slice_started = Instant::now();
				let imported = import_gltf_slice_parallel_images(import_bytes.as_ref(), decode_image_indices.as_ref())?;
				import_profile_messages.push(format!(
					"glTF import profile: gltf_import_slice_ms={}",
					import_slice_started.elapsed().as_millis()
				));
				import_profile_messages.push(format!(
					"glTF import profile: gltf_import_slice.parse_ms={} buffers_ms={} image_decode_ms={} images={}/{} workers={}",
					imported.3.parse_ms,
					imported.3.buffers_ms,
					imported.3.image_decode_ms,
					imported.3.decoded_image_count,
					imported.3.image_count,
					imported.3.image_decode_workers
				));
				(path_hint, imported.0, imported.1, imported.2)
			}
		};

		let mut report = ImportReport {
			source_format: Some(self.descriptor().id.clone()),
			..Default::default()
		};
		for message in import_profile_messages {
			report.push_info(message);
		}
		report.push_info(format!(
			"glTF import profile: pre_scene_import_ms={}",
			import_started.elapsed().as_millis()
		));

		let scene_started = Instant::now();
		let mut scene = scene_snapshot_from_gltf_inner(&document, &buffers, image_data, original_image_sources.take(), &mut report, false)?;
		report.push_info(format!(
			"glTF import profile: scene_snapshot_ms={}",
			scene_started.elapsed().as_millis()
		));
		let mut texture_asset_map = BTreeMap::new();
		if let (Some(root), Some(source_file_path), Some(bin_range)) =
			(root_json.as_ref(), original_glb_file_path.as_ref(), original_glb_bin_range.as_ref())
		{
			let step_started = Instant::now();
			texture_asset_map =
				append_unavatar_texture_assets_from_file(&mut scene, root, source_file_path, bin_range.start as u64, &mut report);
			record_gltf_import_profile_step(&mut report, "append_texture_assets", step_started);
			let step_started = Instant::now();
			apply_unavatar_material_texture_asset_refs(&mut scene, root, &texture_asset_map);
			record_gltf_import_profile_step(&mut report, "apply_texture_asset_refs", step_started);
		} else if let (Some(root), Some(bin)) = (root_json.as_ref(), original_glb_bin.as_deref()) {
			let step_started = Instant::now();
			texture_asset_map = append_unavatar_texture_assets(&mut scene, root, bin, &mut report);
			record_gltf_import_profile_step(&mut report, "append_texture_assets", step_started);
			let step_started = Instant::now();
			apply_unavatar_material_texture_asset_refs(&mut scene, root, &texture_asset_map);
			record_gltf_import_profile_step(&mut report, "apply_texture_asset_refs", step_started);
		}
		let step_started = Instant::now();
		let unavatar = root_json.as_ref().and_then(unavatar_extension_from_root);
		record_gltf_import_profile_step(&mut report, "parse_unavatar_extension", step_started);
		let step_started = Instant::now();
		let humanoid_profile = unavatar
			.as_ref()
			.and_then(|unavatar| unavatar_humanoid_profile(&scene, unavatar, &mut report));
		record_gltf_import_profile_step(&mut report, "unavatar_humanoid_profile", step_started);
		let mut modular_avatar_merge_mapping_pairs = Vec::new();
		if let Some(unavatar) = &unavatar {
			let step_started = Instant::now();
			report_unavatar_path_diagnostics(&scene, unavatar, &mut report);
			record_gltf_import_profile_step(&mut report, "path_diagnostics", step_started);
			let step_started = Instant::now();
			apply_unavatar_asset_group_ownership(&mut scene, unavatar, &mut report);
			record_gltf_import_profile_step(&mut report, "asset_group_ownership", step_started);
			let step_started = Instant::now();
			let node_ids = scene_node_ids(&scene);
			let registry_paths = unavatar_node_registry_paths(Some(unavatar));
			let paths = scene_node_paths(&scene);
			let normalized_paths = scene_node_normalized_paths(&scene);
			scene.node_constraints =
				unavatar_node_constraints(unavatar, &node_ids, &registry_paths, &paths, &normalized_paths, &mut report);
			record_gltf_import_profile_step(&mut report, "node_constraints", step_started);
			let step_started = Instant::now();
			modular_avatar_merge_mapping_pairs = {
				let components = unavatar_modular_avatar_components(unavatar);
				let (merge_mappings, _, _) =
					collect_merge_armature_bone_mappings(components, &node_ids, &registry_paths, &paths, &normalized_paths);
				merge_mappings
					.iter()
					.flat_map(|component| component.mappings.iter().copied())
					.collect::<Vec<_>>()
			};
			apply_unavatar_modular_avatar_with_texture_assets(
				&mut scene,
				unavatar,
				&texture_asset_map,
				humanoid_profile.as_ref(),
				&mut report,
			);
			record_gltf_import_profile_step(&mut report, "modular_avatar", step_started);
			if let Some(humanoid_profile) = &humanoid_profile {
				let step_started = Instant::now();
				let (same_name_mappings, same_name_retargeted, same_name_auxiliary_reparented) =
					retarget_same_name_humanoid_armature_skins(&mut scene, humanoid_profile);
				if same_name_mappings > 0 || same_name_retargeted > 0 || same_name_auxiliary_reparented > 0 {
					report.push_info(format!(
						".unavatar humanoid armature fallback: same_name_mappings={}, skin_joints={}, auxiliary_bones={}",
						same_name_mappings, same_name_retargeted, same_name_auxiliary_reparented
					));
				}
				record_gltf_import_profile_step(&mut report, "retarget_same_name_humanoid_armature_skins", step_started);
			}
			let step_started = Instant::now();
			apply_unavatar_initial_variant_state(&mut scene, unavatar, &mut report);
			record_gltf_import_profile_step(&mut report, "initial_variant_state", step_started);
			let step_started = Instant::now();
			apply_unavatar_base_wardrobe(&mut scene, unavatar, &mut report);
			record_gltf_import_profile_step(&mut report, "base_wardrobe", step_started);
		}
		let step_started = Instant::now();
		let runtime_actions = unavatar.as_ref().and_then(|unavatar| {
			unavatar_runtime_action_set(
				unavatar,
				Some(&scene),
				&ctx.enabled_animator_action_ids,
				&ctx.animator_action_values,
			)
		});
		record_gltf_import_profile_step(&mut report, "runtime_actions", step_started);
		let step_started = Instant::now();
		let runtime_expression_names = expression_weight_names_from_runtime_actions(runtime_actions.as_ref());
		let arkit_perfect_sync_names = arkit_perfect_sync_expression_name_set();
		let mut expression_catalog = if unavatar.is_some() {
			expression_catalog_from_morph_target_names(
				&scene,
				(!runtime_expression_names.is_empty()).then_some(&runtime_expression_names),
				Some(&arkit_perfect_sync_names),
			)
		} else {
			None
		};
		record_gltf_import_profile_step(&mut report, "expression_catalog", step_started);
		if let (Some(unavatar), Some(catalog)) = (unavatar.as_ref(), expression_catalog.as_mut()) {
			let step_started = Instant::now();
			apply_unavatar_blendshape_sync_expression_binds(catalog, &scene, unavatar, &mut report);
			record_gltf_import_profile_step(&mut report, "blendshape_sync_expression_binds", step_started);
		}
		let step_started = Instant::now();
		let mut spring_bones = unavatar
			.as_ref()
			.and_then(|unavatar| unavatar_dynamics_settings(&mut scene, unavatar, &mut report));
		if let Some(settings) = spring_bones.as_mut() {
			let retargeted = retarget_merge_armature_dynamics(settings, &modular_avatar_merge_mapping_pairs);
			if retargeted > 0 {
				report.push_info(format!(".unavatar Modular Avatar: merge_armature_dynamics_nodes={retargeted}"));
			}
		}
		record_gltf_import_profile_step(&mut report, "dynamics_settings", step_started);
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

		let base_runtime_wardrobe = unavatar.as_ref().and_then(|unavatar| {
			unavatar_base_wardrobe_set(unavatar)
				.map(|(base_id, _)| (base_id.to_string(), unavatar_wardrobe_set_asset_groups(unavatar, base_id)))
		});
		let mut document = UnaDocument {
			scene: Some(scene),
			unavatar,
			humanoid_profile,
			expression_weights: expression_catalog.as_ref().map(|_| UnaExpressionWeights::default()),
			expression_catalog,
			runtime_actions,
			spring_bones,
			..Default::default()
		};
		if let Some((base_id, asset_groups)) = base_runtime_wardrobe {
			document.runtime_model_mut().set_active_wardrobe_set(Some(base_id));
			document.runtime_model_mut().set_active_asset_groups(asset_groups);
		}

		Ok(ImportResult { document, report })
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
	use image::ImageEncoder;
	use std::io::Write;
	use un_avatar_core::{una_dynamics_translation_writeback_candidate_count, una_dynamics_translation_writeback_target_count};
	use un_avatar_core::{UnaNodeConstraint, UnaNodeConstraintKind, UnaNodeConstraintSource};

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
				"colliders": [{
					"id": "global_head",
					"node": {"nodeId": "node_root", "path": "Root"},
					"shape": "sphere",
					"offset": [0.0, 0.25, 0.0],
					"radius": 0.12
				}],
				"contacts": [{
					"id": "contact_hand",
					"source": "vrc_contact_receiver",
					"node": {"nodeId": "node_tip", "path": "Root/Tip"},
					"kind": "receiver",
					"parameter": "ContactHand",
					"collisionTags": ["Hand", "Interact"],
					"shape": "sphere",
					"radius": 0.05,
					"position": [0.1, 0.2, 0.3],
					"rotation": [0.0, 0.5, 0.0, 0.8660254]
				}],
				"constraintRefs": [{
					"id": "constraint_parent",
					"source": "vrc_parent_constraint",
					"targetNode": {"nodeId": "node_tip", "path": "Root/Tip"},
					"sourceNodes": [{"nodeId": "node_root", "path": "Root"}],
					"type": "parent",
					"weight": 0.75
				}],
				"dynamics": [{
					"id": "hair_front",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"stiffness": 0.35,
					"drag": 0.2,
					"gravity": [0.0, -0.4, 0.0],
					"radius": 0.03,
					"sourceParams": {
						"integrationType": "Advanced",
						"pull": 0.25,
						"spring": 0.15,
						"momentum": 0.35,
						"stiffness": 0.45,
						"gravityPower": 0.8,
						"gravityVector": [0.0, -0.5, 0.0],
						"gravityFalloff": 0.6,
						"immobile": 0.35,
						"immobileType": 1,
						"pullCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.5}
							]
						},
						"springCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 2.0}
							]
						},
						"momentumCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.5}
							]
						},
						"stiffnessCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.25}
							]
						},
						"gravityCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.25}
							]
						},
						"gravityFalloffCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.5}
							]
						},
						"immobileCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.5}
							]
						},
						"allowCollision": true,
						"writebackMode": "rotation_translation",
						"allowGrabbing": true,
						"allowPosing": false,
						"parameter": "HairPB",
						"limitType": "Angle",
						"limitRotation": [10.0, 20.0, 30.0],
						"maxAngleX": 45.0,
						"maxAngleZ": 30.0,
						"maxAngleXCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.5}
							]
						},
						"maxAngleZCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.25}
							]
						},
						"maxStretch": 0.2,
						"maxSquish": 0.15,
						"stretchMotion": 0.5,
						"maxStretchCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.5}
							]
						},
						"maxSquishCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.25}
							]
						},
						"stretchMotionCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.5}
							]
						},
						"radiusCurve": {
							"keys": [
								{"time": 0.0, "value": 1.0},
								{"time": 1.0, "value": 0.5}
							]
						},
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
							"shapeType": 0,
							"radius": 0.2,
							"insideBounds": true
						}, {
							"root": {"nodeId": "node_root", "path": "Root"},
							"shapeType": "1",
							"radius": 0.06,
							"height": 0.4
						}, {
							"root": {"nodeId": "node_root", "path": "Root"},
							"shapeType": "Plane",
							"position": [0.0, 0.0, 0.1],
							"rotation": [0.0, 0.0, 0.0, 1.0]
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

		assert_eq!(settings.groups.len(), 3);
		assert_eq!(settings.groups[0].source_kind, UnaDynamicsSourceKind::VrcPhysBone);
		assert!(settings.groups[0].enabled);
		assert_eq!(settings.groups[0].source_id, "hair_front");
		assert_eq!(settings.groups[0].comment, "hair_front");
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 1]);
		assert_eq!(settings.groups[0].integration_type, UnaDynamicsIntegrationType::VrcAdvanced);
		assert_eq!(settings.groups[0].pull, 0.25);
		assert_eq!(settings.groups[0].spring, 0.35);
		assert_eq!(settings.groups[0].stiffness, 0.45);
		assert_eq!(settings.groups[0].gravity_falloff, 0.6);
		assert_eq!(settings.groups[0].immobile, 0.35);
		assert_eq!(settings.groups[0].immobile_type, UnaDynamicsImmobileType::World);
		assert_eq!(settings.groups[0].stiffness_samples, vec![0.1125]);
		assert_eq!(settings.groups[0].pull_samples, vec![0.125]);
		assert_eq!(settings.groups[0].spring_samples, vec![0.175]);
		assert_eq!(settings.groups[0].gravity_power_samples, vec![0.2]);
		assert_eq!(settings.groups[0].gravity_falloff_samples, vec![0.3]);
		assert_eq!(settings.groups[0].immobile_samples, vec![0.175]);
		assert_eq!(settings.groups[0].max_angle_x_samples, vec![22.5]);
		assert_eq!(settings.groups[0].max_angle_z_samples, vec![7.5]);
		assert_eq!(settings.groups[0].hit_radius, 0.03);
		assert_eq!(settings.groups[0].hit_radius_samples.len(), 1);
		assert!((settings.groups[0].hit_radius_samples[0] - 0.015).abs() < 1e-6);
		assert_eq!(settings.groups[0].writeback_mode, UnaDynamicsWritebackMode::RotationTranslation);
		assert!((settings.groups[0].gravity_power - 0.8).abs() < 1e-6);
		assert_eq!(settings.groups[0].gravity_dir, [0.0, -1.0, 0.0]);
		let limit = settings.groups[0].limit.as_ref().expect("limit");
		assert_eq!(limit.limit_type, "Angle");
		assert_eq!(limit.limit_rotation, [10.0, 20.0, 30.0]);
		assert_eq!(limit.max_angle_x, 45.0);
		assert_eq!(limit.max_angle_z, 30.0);
		assert_eq!(limit.max_stretch, 0.2);
		assert_eq!(limit.max_squish, 0.15);
		assert_eq!(limit.stretch_motion, Some(0.5));
		assert_eq!(limit.max_stretch_samples, vec![0.1]);
		assert!((limit.max_squish_samples[0] - 0.0375).abs() < 1e-6);
		assert_eq!(limit.stretch_motion_samples, vec![0.25]);
		let interaction = settings.groups[0].interaction.as_ref().expect("interaction");
		assert_eq!(interaction.allow_grabbing, Some(true));
		assert_eq!(interaction.allow_posing, Some(false));
		assert_eq!(interaction.parameter, "HairPB");
		assert_eq!(settings.colliders.len(), 5);
		assert_eq!(settings.colliders[0].source_kind, UnaDynamicsSourceKind::Unknown);
		assert_eq!(settings.colliders[0].node, 0);
		assert_eq!(settings.colliders[0].shape, UnaDynamicsColliderShape::Sphere);
		assert_eq!(settings.colliders[0].radius, 0.12);
		assert_eq!(settings.colliders[0].position, [-0.0, 0.25, 0.0]);
		assert_eq!(settings.colliders[1].source_kind, UnaDynamicsSourceKind::VrcPhysBone);
		assert_eq!(settings.colliders[1].node, 0);
		assert_eq!(settings.colliders[1].shape, UnaDynamicsColliderShape::Sphere);
		assert_eq!(settings.colliders[1].radius, 0.08);
		assert_eq!(settings.colliders[1].position, [-0.1, 0.2, 0.3]);
		assert_eq!(settings.colliders[1].rotation, [0.0, -0.5, -0.0, 0.8660254]);
		assert!(!settings.colliders[1].inside_bounds);
		assert_eq!(settings.colliders[2].radius, 0.2);
		assert!(settings.colliders[2].inside_bounds);
		assert_eq!(settings.colliders[3].shape, UnaDynamicsColliderShape::Capsule);
		assert_eq!(settings.colliders[3].radius, 0.06);
		assert_eq!(settings.colliders[3].height, 0.4);
		assert_eq!(settings.colliders[4].shape, UnaDynamicsColliderShape::Plane);
		assert_eq!(settings.colliders[4].position, [-0.0, 0.0, 0.1]);
		assert_eq!(settings.contacts.len(), 1);
		assert_eq!(settings.contacts[0].source_kind, UnaDynamicsSourceKind::VrcPhysBone);
		assert_eq!(settings.contacts[0].kind, UnaDynamicsContactKind::Receiver);
		assert_eq!(settings.contacts[0].node, 1);
		assert_eq!(settings.contacts[0].parameter, "ContactHand");
		assert_eq!(settings.contacts[0].collision_tags, vec!["Hand", "Interact"]);
		assert_eq!(settings.contacts[0].position, [-0.1, 0.2, 0.3]);
		assert_eq!(settings.contacts[0].rotation, [0.0, -0.5, -0.0, 0.8660254]);
		assert_eq!(settings.constraint_refs.len(), 1);
		assert_eq!(settings.constraint_refs[0].source_kind, UnaDynamicsSourceKind::VrcPhysBone);
		assert_eq!(settings.constraint_refs[0].target_node, 1);
		assert_eq!(settings.constraint_refs[0].source_nodes, vec![0]);
		assert_eq!(settings.constraint_refs[0].constraint_type, "parent");
		assert_eq!(settings.constraint_refs[0].weight, 0.75);
		assert!(settings.groups[1].enabled);
		assert_eq!(settings.groups[2].source_id, "disabled_tail");
		assert!(!settings.groups[2].enabled);
	}

	#[test]
	fn unavatar_node_constraints_lowers_parent_constraint_sources() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("node_root", vec![1, 2, 3]),
				test_scene_node("node_target", Vec::new()),
				test_scene_node("node_source_a", Vec::new()),
				test_scene_node("node_source_b", Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_target", "path": "node_root/node_target"},
					{"nodeId": "node_source_a", "path": "node_root/node_source_a"},
					{"nodeId": "node_source_b", "path": "node_root/node_source_b"}
				],
				"nodeConstraints": [{
					"kind": "parent",
					"target": {"nodeId": "node_target"},
					"weight": 0.75,
					"sources": [
						{"node": {"nodeId": "node_source_a"}, "weight": 0.25},
						{"node": {"nodeId": "node_source_b"}, "weight": 0.75}
					]
				}]
			}),
		};
		let node_ids = scene_node_ids(&scene);
		let registry_paths = unavatar_node_registry_paths(Some(&unavatar));
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);
		let mut report = ImportReport::default();

		let constraints = unavatar_node_constraints(&unavatar, &node_ids, &registry_paths, &paths, &normalized_paths, &mut report);

		assert_eq!(constraints.len(), 1);
		assert_eq!(constraints[0].target_node, 1);
		assert_eq!(constraints[0].source_node, 2);
		assert_eq!(constraints[0].sources.len(), 2);
		assert_eq!(constraints[0].sources[1].source_node, 3);
		assert!((constraints[0].weight - 0.75).abs() < f32::EPSILON);
		assert!(matches!(constraints[0].kind, UnaNodeConstraintKind::Parent { .. }));
	}

	#[test]
	fn unavatar_vrc_physbone_chain_prepends_parent_anchor_for_root_writeback() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("node_anchor", vec![1]),
				test_scene_node("node_root", vec![2]),
				test_scene_node("node_tip", Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_anchor", "path": "Anchor"},
					{"nodeId": "node_root", "path": "Anchor/Root"},
					{"nodeId": "node_tip", "path": "Anchor/Root/Tip"}
				],
				"dynamics": [{
					"id": "physbone:cloth",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Anchor/Root"}]
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(settings.groups.len(), 1);
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 1, 2]);
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
	fn unavatar_dynamics_writeback_target_counts_follow_lowered_chains() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("node_root", vec![1, 3]),
				test_scene_node("node_mid", vec![2]),
				test_scene_node("node_tip", Vec::new()),
				test_scene_node("node_leaf", Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"},
					{"nodeId": "node_mid", "path": "Root/Mid"},
					{"nodeId": "node_tip", "path": "Root/Mid/Tip"},
					{"nodeId": "node_leaf", "path": "Root/Leaf"}
				],
				"dynamics": [{
					"id": "branched_stretch",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"sourceParams": {
						"writebackMode": "rotation_translation",
						"maxStretch": 0.25
					}
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");
		let mut counts: Vec<(Vec<usize>, usize, usize)> = settings
			.groups
			.iter()
			.map(|group| {
				(
					group.bone_node_indices.clone(),
					una_dynamics_translation_writeback_candidate_count(&scene, group.writeback_mode, &group.bone_node_indices),
					una_dynamics_translation_writeback_target_count(&scene, group.writeback_mode, &group.bone_node_indices),
				)
			})
			.collect();
		counts.sort_by(|a, b| a.0.cmp(&b.0));

		assert_eq!(counts, vec![(vec![0, 1, 2], 2, 1), (vec![0, 3], 1, 1)]);
	}

	#[test]
	fn unavatar_dynamics_infers_writeback_mode_from_stretch_source_params() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("node_root", vec![1]),
				test_scene_node("node_mid", vec![2]),
				test_scene_node("node_tip", Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"},
					{"nodeId": "node_mid", "path": "Root/Mid"},
					{"nodeId": "node_tip", "path": "Root/Mid/Tip"}
				],
				"dynamics": [{
					"id": "legacy_stretch",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"sourceParams": {
						"maxStretch": 0.25
					}
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(settings.groups.len(), 1);
		assert_eq!(settings.groups[0].writeback_mode, UnaDynamicsWritebackMode::RotationTranslation);
		assert_eq!(
			una_dynamics_translation_writeback_target_count(
				&scene,
				settings.groups[0].writeback_mode,
				&settings.groups[0].bone_node_indices,
			),
			1
		);
	}

	#[test]
	fn unavatar_dynamics_infers_writeback_mode_from_stretch_curve_keys() {
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
					"id": "curve_stretch",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"sourceParams": {
						"maxStretchCurve": {
							"keyCount": 1,
							"keys": [{"time": 0.0, "value": 0.25}]
						}
					}
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(settings.groups[0].writeback_mode, UnaDynamicsWritebackMode::RotationTranslation);
	}

	#[test]
	fn unavatar_dynamics_explicit_rotation_only_overrides_stretch_source_params() {
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
					"id": "rotation_only_stretch",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"writebackMode": "rotation_only",
					"sourceParams": {
						"maxStretch": 0.25
					}
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(settings.groups[0].writeback_mode, UnaDynamicsWritebackMode::RotationOnly);
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

		let mut chains = settings
			.groups
			.iter()
			.map(|group| group.bone_node_indices.clone())
			.collect::<Vec<_>>();
		chains.sort();
		assert_eq!(chains, vec![vec![0, 3], vec![0, 4]]);
		assert!(report.messages.iter().any(|message| message.contains("ignored_transforms=1")));
		assert!(report.messages.iter().any(|message| message.contains("multi_child_ignore=1")));
	}

	#[test]
	fn unavatar_dynamics_does_not_prepend_parent_anchor_for_multi_child_vrc_physbone_root() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("node_parent", vec![1]),
				test_scene_node("node_root", vec![2, 4]),
				test_scene_node("node_left", vec![3]),
				test_scene_node("node_left_tip", Vec::new()),
				test_scene_node("node_right", vec![5]),
				test_scene_node("node_right_tip", Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_parent", "path": "Parent"},
					{"nodeId": "node_root", "path": "Parent/Root"},
					{"nodeId": "node_left", "path": "Parent/Root/Left"},
					{"nodeId": "node_left_tip", "path": "Parent/Root/Left/Tip"},
					{"nodeId": "node_right", "path": "Parent/Root/Right"},
					{"nodeId": "node_right_tip", "path": "Parent/Root/Right/Tip"}
				],
				"dynamics": [{
					"id": "multi_child",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Parent/Root"}],
					"multiChildType": "First"
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		let mut chains = settings
			.groups
			.iter()
			.map(|group| group.bone_node_indices.clone())
			.collect::<Vec<_>>();
		chains.sort();
		assert_eq!(chains, vec![vec![1, 2, 3], vec![1, 4, 5]]);
		assert!(chains.iter().all(|chain| !chain.starts_with(&[0, 1])));
	}

	#[test]
	fn unavatar_dynamics_does_not_prepend_parent_anchor_for_single_child_vrc_physbone_root() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("node_parent", vec![1]),
				test_scene_node("node_root", vec![2]),
				test_scene_node("node_mid", vec![3]),
				test_scene_node("node_tip", Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_parent", "path": "Parent"},
					{"nodeId": "node_root", "path": "Parent/Root"},
					{"nodeId": "node_mid", "path": "Parent/Root/Mid"},
					{"nodeId": "node_tip", "path": "Parent/Root/Mid/Tip"}
				],
				"dynamics": [{
					"id": "single_child",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Parent/Root"}],
					"multiChildType": "Ignore"
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		let chains = settings
			.groups
			.iter()
			.map(|group| group.bone_node_indices.clone())
			.collect::<Vec<_>>();
		assert_eq!(chains, vec![vec![1, 2, 3]]);
		assert!(chains.iter().all(|chain| !chain.starts_with(&[0, 1])));
	}

	#[test]
	fn unavatar_dynamics_applies_modular_avatar_pb_blockers_as_ignores() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("node_root", vec![1, 3]),
				test_scene_node("node_blocked", vec![2]),
				test_scene_node("node_blocked_tip", Vec::new()),
				test_scene_node("node_kept_tip", Vec::new()),
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"},
					{"nodeId": "node_blocked", "path": "Root/Blocked"},
					{"nodeId": "node_blocked_tip", "path": "Root/Blocked/Tip"},
					{"nodeId": "node_kept_tip", "path": "Root/KeptTip"}
				],
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarPBBlocker",
						"enabled": true,
						"target": {"nodeId": "node_blocked", "path": "Root/Blocked"}
					}]
				},
				"dynamics": [{
					"id": "blocked_branch",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}]
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(settings.groups.len(), 1);
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 3]);
		assert!(report
			.messages
			.iter()
			.any(|message| message.contains("modular_avatar_pb_blocker_ignores=1")));
	}

	#[test]
	fn unavatar_dynamics_lowers_modular_avatar_global_colliders() {
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
				"modularAvatar": {
					"components": [{
						"shortType": "ModularAvatarGlobalCollider",
						"enabled": true,
						"target": {"nodeId": "node_root", "path": "Root"},
						"fields": {
							"m_rootTransform": {
								"resolvedTarget": {"nodeId": "node_root", "path": "Root"}
							},
							"m_radius": 0.05,
							"m_height": 0.2,
							"m_position": [0.0, 0.1, 0.2],
							"m_rotation": [0.0, 0.5, 0.0, 0.8660254]
						}
					}]
				}
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(settings.colliders.len(), 1);
		assert_eq!(settings.colliders[0].source_kind, UnaDynamicsSourceKind::VrcPhysBone);
		assert_eq!(settings.colliders[0].node, 0);
		assert_eq!(settings.colliders[0].shape, UnaDynamicsColliderShape::Capsule);
		assert_eq!(settings.colliders[0].radius, 0.05);
		assert_eq!(settings.colliders[0].height, 0.2);
		assert_eq!(settings.colliders[0].position, [-0.0, 0.1, 0.2]);
		assert_eq!(settings.colliders[0].rotation, [0.0, -0.5, -0.0, 0.8660254]);
		assert!(report
			.messages
			.iter()
			.any(|message| message.contains("modular_avatar_global_colliders=1")));
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

	#[test]
	fn unavatar_dynamics_synthesizes_endpoint_tail_for_non_leaf_root() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![test_scene_node("node_root", vec![1]), test_scene_node("node_child", Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		scene.nodes[1].transform = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0)).to_cols_array();
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"},
					{"nodeId": "node_child", "path": "Root/Child"}
				],
				"dynamics": [{
					"id": "non_leaf_tail",
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

		assert_eq!(scene.nodes.len(), 3);
		assert_eq!(scene.nodes[1].children, vec![2]);
		let (_, _, endpoint_translation) = Mat4::from_cols_array(&scene.nodes[2].transform).to_scale_rotation_translation();
		let expected_endpoint = Vec3::new(-0.1, -0.8, 0.3);
		assert!(
			(endpoint_translation - expected_endpoint).length() < 1e-6,
			"endpoint_translation={endpoint_translation:?}"
		);
		assert_eq!(settings.groups.len(), 1);
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 1, 2]);
		assert!(report
			.messages
			.iter()
			.any(|message| message.contains("synthesized_endpoint_children=1")));
		assert!(!report.messages.iter().any(|message| message.contains("ignored endpointPosition")));
	}

	#[test]
	fn unavatar_dynamics_warns_when_non_leaf_endpoint_tail_is_degenerate() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![test_scene_node("node_root", vec![1]), test_scene_node("node_child", Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		scene.nodes[1].transform = Mat4::from_translation(Vec3::new(-0.1, 0.2, 0.3)).to_cols_array();
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"},
					{"nodeId": "node_child", "path": "Root/Child"}
				],
				"dynamics": [{
					"id": "degenerate_endpoint_tail",
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
		assert_eq!(settings.groups.len(), 1);
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 1]);
		assert!(report
			.messages
			.iter()
			.any(|message| message.contains("could not synthesize endpoint tail for 1 non-leaf dynamics root")));
		assert!(!report.messages.iter().any(|message| message.contains("ignored endpointPosition")));
	}

	#[test]
	fn unavatar_dynamics_synthesizes_endpoint_when_all_children_are_ignored() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![test_scene_node("node_root", vec![1]), test_scene_node("node_ignored", Vec::new())],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"nodes": [
					{"nodeId": "node_root", "path": "Root"},
					{"nodeId": "node_ignored", "path": "Root/Ignored"}
				],
				"dynamics": [{
					"id": "ignored_child_leaf_tail",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"sourceParams": {
						"ignoreTransforms": [{"nodeId": "node_ignored", "path": "Root/Ignored"}],
						"endpointPosition": [0.0, 0.25, 0.0]
					}
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(scene.nodes.len(), 3);
		assert_eq!(scene.nodes[0].children, vec![1, 2]);
		let (_, _, endpoint_translation) = Mat4::from_cols_array(&scene.nodes[2].transform).to_scale_rotation_translation();
		assert_eq!(endpoint_translation.to_array(), [-0.0, 0.25, 0.0]);
		assert_eq!(settings.groups.len(), 1);
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 2]);
		assert!(report
			.messages
			.iter()
			.any(|message| message.contains("synthesized_endpoint_children=1")));
		assert!(!report.messages.iter().any(|message| message.contains("ignored endpointPosition")));
	}

	#[test]
	fn unavatar_dynamics_warns_on_unknown_writeback_mode() {
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
					"id": "tail",
					"source": "vrc_physbone",
					"roots": [{"nodeId": "node_root", "path": "Root"}],
					"writebackMode": "stretch_everything"
				}]
			}),
		};
		let mut report = ImportReport::default();
		let settings = unavatar_dynamics_settings(&mut scene, &unavatar, &mut report).expect("dynamics");

		assert_eq!(settings.groups.len(), 1);
		assert_eq!(settings.groups[0].writeback_mode, UnaDynamicsWritebackMode::RotationOnly);
		assert!(report.messages.iter().any(|message| {
			message.contains("unknown writebackMode \"stretch_everything\"") && message.contains("defaulting to rotation_only")
		}));
	}

	fn glb_bytes_with_bin(json: &str, bin: &[u8]) -> Vec<u8> {
		let mut json_bytes = json.as_bytes().to_vec();
		while !json_bytes.len().is_multiple_of(4) {
			json_bytes.push(b' ');
		}
		let mut bin_bytes = bin.to_vec();
		while !bin_bytes.len().is_multiple_of(4) {
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
		import_marks_enabled_unsupported_modular_avatar_component_partial_success_of("ModularAvatarWorldFixedObject");
	}

	#[test]
	fn import_marks_world_scale_object_as_unsupported_modular_avatar_component() {
		import_marks_enabled_unsupported_modular_avatar_component_partial_success_of("ModularAvatarWorldScaleObject");
	}

	#[test]
	fn import_marks_move_independently_as_unsupported_modular_avatar_component() {
		import_marks_enabled_unsupported_modular_avatar_component_partial_success_of("MAMoveIndependently");
	}

	#[test]
	fn import_marks_other_unimplemented_modular_avatar_components_as_unsupported() {
		for short_type in [
			"ModularAvatarConvertConstraints",
			"ModularAvatarFloorAdjuster",
			"ModularAvatarMMDLayerControl",
			"ModularAvatarMergeBlendTree",
			"ModularAvatarPlatformFilter",
			"ModularAvatarRenameVRChatCollisionTags",
			"ModularAvatarVRChatSettings",
		] {
			import_marks_enabled_unsupported_modular_avatar_component_partial_success_of(short_type);
		}
	}

	fn import_marks_enabled_unsupported_modular_avatar_component_partial_success_of(short_type: &str) {
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
								"shortType": "{}",
								"enabled": true
							}}]
						}}
					}}
				}}
			}}"#,
			short_type,
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
		assert_eq!(got.report.lost_features[0].feature, format!("ModularAvatar.{short_type}"));
		assert!(got
			.report
			.diagnostics
			.iter()
			.any(|diagnostic| { diagnostic.severity == un_avatar_core::ReportSeverity::Warning && diagnostic.text.contains(short_type) }));
	}

	#[test]
	fn import_preserves_inverted_modular_avatar_action_condition_without_approximation() {
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
				"nodes": [{{"name": "Root", "mesh": 0, "extras": {{"UN_avatar_node": {{"nodeId": "node_root", "path": "Root"}}}}}}],
				"extensionsUsed": ["UN_avatar"],
				"extensions": {{
					"UN_avatar": {{
						"specVersion": "0.1-preview",
						"modularAvatar": {{
							"schemaVersion": "0.1-preview",
							"components": [{{
								"shortType": "ModularAvatarObjectToggle",
								"enabled": true,
								"fields": {{
									"m_inverted": true,
									"objects": [{{
										"object": {{"nodeId": "node_root", "path": "Root"}},
										"active": false
									}}]
								}}
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
					path_hint: Some(std::path::PathBuf::from("inverted-ma-action.glb")),
				},
				ImportOptions,
			)
			.unwrap();

		assert_eq!(got.report.status, ReportStatus::Success);
		assert_eq!(got.report.lost_features.len(), 0);
		assert_eq!(got.report.approximations.len(), 0);
		let actions = got.document.runtime_actions.as_ref().expect("runtime actions");
		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].id, "ma:object_toggle:0");
		assert_eq!(actions.actions[0].conditions.len(), 1);
		assert!(actions.actions[0].conditions[0].inverted);
		assert!(!got
			.report
			.diagnostics
			.iter()
			.any(|diagnostic| diagnostic.text.contains("inverted_ignored")));
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
				{ "magFilter": 9728, "minFilter": 9987, "wrapS": 33071, "wrapT": 33648, "unityWrapModeV": "MirrorOnce" }
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
				wrap_t: UnaTextureWrapMode::MirrorOnce,
			})
		);
		assert_eq!(samplers[1], Some(UnaTextureSampler::default()));
		let metadata = collect_glb_image_source_metadata(&root, &[], None, None, 0);
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
		let metadata = collect_glb_image_source_metadata(&root, &[0, 0, 0, 0, 1, 2, 3, 4], None, None, 0);
		assert_eq!(metadata.len(), 1);
		let source = metadata[0].as_ref().unwrap();
		assert_eq!(source.name.as_deref(), Some("main"));
		assert_eq!(source.mime_type.as_deref(), Some("image/png"));
		assert_eq!(source.byte_length, 3);
		assert_eq!(source.source_hash, source_hash64(&[1, 2, 3]));
		assert!(source.encoded_bytes.is_none());
	}

	#[test]
	fn glb_image_source_metadata_retains_encoded_bytes_only_for_requested_indices() {
		let root: Value = serde_json::from_str(
			r#"{
				"images": [
					{"name": "a", "bufferView": 0, "mimeType": "image/png"},
					{"name": "b", "bufferView": 1, "mimeType": "image/png"}
				],
				"bufferViews": [
					{"buffer": 0, "byteOffset": 0, "byteLength": 3},
					{"buffer": 0, "byteOffset": 3, "byteLength": 3}
				]
			}"#,
		)
		.unwrap();
		let retain = BTreeSet::from([1usize]);
		let metadata = collect_glb_image_source_metadata(&root, &[1, 2, 3, 4, 5, 6], Some(&retain), None, 0);

		assert!(metadata[0].as_ref().unwrap().encoded_bytes.is_none());
		assert_eq!(metadata[1].as_ref().unwrap().encoded_bytes.as_deref(), Some(&[4, 5, 6][..]));
	}

	#[test]
	fn glb_image_source_metadata_uses_file_range_for_path_backed_lazy_decode() {
		let root: Value = serde_json::from_str(
			r#"{
				"images": [{"name": "a", "bufferView": 0, "mimeType": "image/png"}],
				"bufferViews": [{"buffer": 0, "byteOffset": 2, "byteLength": 3}]
			}"#,
		)
		.unwrap();
		let retain = BTreeSet::from([0usize]);
		let metadata = collect_glb_image_source_metadata(&root, &[0, 0, 4, 5, 6], Some(&retain), Some(Path::new("avatar.glb")), 100);
		let source = metadata[0].as_ref().unwrap();
		assert_eq!(source.byte_offset, Some(102));
		assert_eq!(source.byte_length, 3);
		assert_eq!(source.source_file_path.as_deref(), Some(Path::new("avatar.glb")));
		assert!(source.encoded_bytes.is_none());
	}

	#[test]
	fn parallel_glb_image_source_metadata_matches_serial_order() {
		let root: Value = serde_json::from_str(
			r#"{
				"images": [
					{"name": "a", "bufferView": 0, "mimeType": "image/png"},
					{"name": "b", "bufferView": 1, "mimeType": "image/jpeg"},
					{"name": "c", "uri": "textures/c.png"},
					{"name": "d", "bufferView": 2, "mimeType": "image/png"}
				],
				"bufferViews": [
					{"buffer": 0, "byteOffset": 0, "byteLength": 4},
					{"buffer": 0, "byteOffset": 4, "byteLength": 4},
					{"buffer": 0, "byteOffset": 8, "byteLength": 4}
				]
			}"#,
		)
		.unwrap();
		let bin = (0u8..16).collect::<Vec<_>>();

		assert_eq!(
			collect_glb_image_source_metadata(&root, &bin, None, None, 0),
			collect_glb_image_source_metadata_serial(&root, &bin, None)
		);
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
	fn imports_unavatar_png_texture_asset() {
		let mut png = Vec::new();
		image::codecs::png::PngEncoder::new(&mut png)
			.write_image(&[0, 0, 0, 255, 255, 255, 255, 255], 2, 1, image::ColorType::Rgba8.into())
			.unwrap();
		let mut bin = vec![0; 12];
		let png_offset = bin.len();
		bin.extend_from_slice(&png);
		let json = format!(
			r#"{{
				"asset": {{"version": "2.0"}},
				"scene": 0,
				"scenes": [{{"nodes": [0]}}],
				"meshes": [{{
					"primitives": [{{
						"attributes": {{"POSITION": 0}}
					}}]
				}}],
				"accessors": [
					{{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3", "min": [0, 0, 0], "max": [0, 0, 0]}}
				],
				"bufferViews": [
					{{"buffer": 0, "byteOffset": 0, "byteLength": 12}},
					{{"buffer": 0, "byteOffset": {png_offset}, "byteLength": {png_len}}}
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
							"id": "mask-png",
							"name": "mask",
							"mimeType": "image/png",
							"colorSpace": "sRGB",
							"channels": "rgba",
							"sampler": {{"magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 33648, "unityWrapModeV": "MirrorOnce"}},
							"bufferView": 1
						}}]
					}}
				}}
			}}"#,
			png_offset = png_offset,
			png_len = png.len(),
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
					path_hint: Some(std::path::PathBuf::from("mask.unavatar")),
				},
				ImportOptions,
			)
			.unwrap();

		let scene = got.document.scene.as_ref().unwrap();
		assert_eq!(scene.images.len(), 1);
		assert_eq!(scene.images[0].width, 2);
		assert_eq!(scene.images[0].height, 1);
		assert_eq!(scene.images[0].pixel_format, UnaImagePixelFormat::R8G8B8A8);
		assert_eq!(scene.images[0].pixels, vec![0, 0, 0, 255, 255, 255, 255, 255]);
		assert_eq!(
			scene.image_sources[0].as_ref().unwrap().sampler,
			Some(UnaTextureSampler {
				mag_filter: UnaTextureFilterMode::Nearest,
				min_filter: UnaTextureFilterMode::Nearest,
				wrap_s: UnaTextureWrapMode::ClampToEdge,
				wrap_t: UnaTextureWrapMode::MirrorOnce,
			})
		);
	}

	#[test]
	fn imports_unavatar_texture_asset_from_path_without_retaining_encoded_bytes() {
		let mut png = Vec::new();
		image::codecs::png::PngEncoder::new(&mut png)
			.write_image(&[8, 16, 24, 255], 1, 1, image::ColorType::Rgba8.into())
			.unwrap();
		let mut bin = vec![0; 12];
		let png_offset = bin.len();
		bin.extend_from_slice(&png);
		let json = format!(
			r#"{{
				"asset": {{"version": "2.0"}},
				"scene": 0,
				"scenes": [{{"nodes": [0]}}],
				"meshes": [{{
					"primitives": [{{
						"attributes": {{"POSITION": 0}}
					}}]
				}}],
				"accessors": [
					{{"bufferView": 0, "componentType": 5126, "count": 1, "type": "VEC3", "min": [0, 0, 0], "max": [0, 0, 0]}}
				],
				"bufferViews": [
					{{"buffer": 0, "byteOffset": 0, "byteLength": 12}},
					{{"buffer": 0, "byteOffset": {png_offset}, "byteLength": {png_len}}}
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
							"id": "mask-png",
							"name": "mask",
							"mimeType": "image/png",
							"bufferView": 1
						}}]
					}}
				}}
			}}"#,
			png_offset = png_offset,
			png_len = png.len(),
			bin_len = bin.len()
		);
		let bytes = glb_bytes_with_bin(&json, &bin);
		let path = std::env::temp_dir().join(format!("un-avatar-texture-asset-path-{}.unavatar", std::process::id()));
		std::fs::write(&path, bytes).unwrap();

		let imp = GltfImporter;
		let mut ctx = ImportContext::dummy();
		let got = imp.import(&mut ctx, ImportInput::Path(path.clone()), ImportOptions).unwrap();
		let _ = std::fs::remove_file(&path);

		let scene = got.document.scene.as_ref().unwrap();
		assert_eq!(scene.images.len(), 1);
		assert_eq!(scene.images[0].pixels, vec![8, 16, 24, 255]);
		let source = scene.image_sources[0].as_ref().unwrap();
		assert_eq!(source.source_file_path.as_deref(), Some(path.as_path()));
		assert!(source.byte_offset.is_some());
		assert_eq!(source.byte_length, png.len() as u64);
		assert_eq!(source.source_hash, source_hash64(&png));
		assert!(source.encoded_bytes.is_none());
		assert!(got
			.report
			.messages
			.iter()
			.any(|message| message.contains(".unavatar textureAssets:") && message.contains("file_backed=true")));
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
						"shortType": "ModularAvatarWorldFixedObject",
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
		assert_eq!(got.report.lost_features[0].feature, "ModularAvatar.ModularAvatarWorldFixedObject");
		let _ = std::fs::remove_dir_all(&dir);
	}

	#[test]
	fn imports_unavatar_extension_from_gltf_bytes() {
		let mut root: Value = serde_json::from_str(include_str!("../tests/fixtures/triangle.gltf")).unwrap();
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
				"generator": "bytes-test"
			}
		});
		let json = serde_json::to_string(&root).unwrap();
		let bytes = glb_bytes_with_bin(&json, &triangle_bin_bytes());

		let imp = GltfImporter;
		let mut ctx = ImportContext::dummy();
		let got = imp
			.import(
				&mut ctx,
				ImportInput::Bytes {
					bytes: bytes.into(),
					path_hint: Some(std::path::PathBuf::from("triangle.glb")),
				},
				ImportOptions,
			)
			.unwrap();

		assert_eq!(got.document.unavatar.as_ref().unwrap().spec_version, "0.1-preview");
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
		assert!(got.document.expression_catalog.is_none());
		assert!(got.document.expression_weights.is_none());
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
	fn unavatar_runtime_actions_import_fx_animator_object_toggle() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("HatRoot".to_string()),
					source_node_id: Some("node_hat".to_string()),
					resolved_node_id: Some("resolved_hat".to_string()),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"animator": {
					"enabledActionIds": ["animator:0:0:hat_off:0"],
					"controllers": [{
						"name": "paryi_FX",
						"source": "rootAnimator",
						"layers": [{
							"name": "Cloth",
							"states": [{
								"name": "Hat OFF",
								"path": "Hat OFF",
								"motion": {
									"motionType": "AnimationClip",
									"name": "Hat_OFF",
									"curveBindings": [{
										"path": "HatRoot",
										"propertyName": "m_IsActive",
										"type": "UnityEngine.GameObject",
										"constantValue": 0
									}]
								}
							}],
							"anyStateTransitions": [{
								"destinationState": "Hat OFF",
								"conditions": [{
									"parameter": "Hat",
									"mode": "IfNot",
									"threshold": 0
								}]
							}]
						}]
					}]
				}
			}),
		};

		let actions = unavatar_runtime_action_set(&unavatar, Some(&scene), &[], &BTreeMap::new()).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].id, "animator:0:0:hat_off:0");
		assert_eq!(actions.actions[0].label, "Cloth / Hat OFF");
		assert_eq!(
			actions.actions[0].triggers,
			vec![
				UnaRuntimeActionTrigger::SupervisorCommand {
					command: "animator:0:0:hat_off:0".to_string()
				},
				UnaRuntimeActionTrigger::ParameterValue {
					name: "Hat".to_string(),
					value: 0.0
				}
			]
		);
		assert_eq!(actions.actions[0].conditions[0].parameter_name.as_deref(), Some("Hat"));
		assert_eq!(actions.actions[0].conditions[0].parameter_value, Some(0.0));
		assert_eq!(
			actions.actions[0].effects,
			vec![UnaRuntimeActionEffect::NodeVisibility {
				target: UnaRuntimeNodeTarget {
					node_index: None,
					source_node_id: Some("node_hat".to_string()),
					resolved_node_id: Some("resolved_hat".to_string()),
					path: Some("HatRoot".to_string()),
				},
				visible: false,
			}]
		);

		let mut action_values = BTreeMap::new();
		action_values.insert("animator:0:0:hat_off:0".to_string(), 0.45);
		let actions = unavatar_runtime_action_set(&unavatar, Some(&scene), &[], &action_values).expect("runtime actions");
		assert!(actions.actions[0].triggers.iter().any(|trigger| matches!(
			trigger,
			UnaRuntimeActionTrigger::ParameterValue { name, value } if name == "Hat" && (*value - 0.45).abs() < f32::EPSILON
		)));
	}

	#[test]
	fn unavatar_runtime_actions_skip_fx_animator_until_profile_enables_action() {
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"animator": {
					"controllers": [{
						"name": "paryi_FX",
						"layers": [{
							"name": "Cloth",
							"states": [{
								"name": "Hat OFF",
								"path": "Hat OFF",
								"motion": {
									"motionType": "AnimationClip",
									"curveBindings": [{
										"path": "HatRoot",
										"propertyName": "m_IsActive",
										"constantValue": 0
									}]
								}
							}],
							"anyStateTransitions": [{
								"destinationState": "Hat OFF",
								"conditions": [{
									"parameter": "Hat",
									"mode": "IfNot",
									"threshold": 0
								}]
							}]
						}]
					}]
				}
			}),
		};

		assert!(unavatar_runtime_action_set(&unavatar, None, &[], &BTreeMap::new()).is_none());
		assert!(unavatar_runtime_action_set(&unavatar, None, &["animator:0:0:hat_off:0".to_string()], &BTreeMap::new()).is_some());
	}

	#[test]
	fn unavatar_runtime_actions_do_not_truncate_profile_enabled_actions_at_legacy_ui_count() {
		let mut states = Vec::new();
		let mut transitions = Vec::new();
		let mut enabled_ids = Vec::new();
		for index in 0..120 {
			let state_name = format!("Action {index:03}");
			let state_id = stable_identifier(&state_name);
			enabled_ids.push(format!("animator:0:0:{state_id}:0"));
			states.push(serde_json::json!({
				"name": state_name,
				"path": state_name,
				"motion": {
					"motionType": "AnimationClip",
					"name": format!("Clip {index:03}"),
					"curveBindings": [{
						"path": "Root/Target",
						"propertyName": "m_IsActive",
						"type": "UnityEngine.GameObject",
						"constantValue": 1
					}]
				}
			}));
			transitions.push(serde_json::json!({
				"destinationState": state_name,
				"conditions": [{
					"parameter": format!("Action{index:03}"),
					"mode": "If",
					"threshold": 0
				}]
			}));
		}
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"animator": {
					"controllers": [{
						"name": "FX",
						"layers": [{
							"name": "Actions",
							"states": states,
							"anyStateTransitions": transitions
						}]
					}]
				}
			}),
		};

		let actions = unavatar_runtime_action_set(&unavatar, None, &enabled_ids, &BTreeMap::new()).expect("runtime actions");

		assert_eq!(actions.actions.len(), 120);
		assert_eq!(actions.actions[0].id, "animator:0:0:action_000:0");
		assert_eq!(actions.actions[119].id, "animator:0:0:action_119:0");
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
									"Parameter": {"Name": "JacketColor"},
									"Value": "1",
									"subParameters": [{"name": "JacketHue"}, {"Name": "JacketSat"}]
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

		let actions = unavatar_runtime_action_set(&unavatar, None, &[], &BTreeMap::new()).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].id, "ma:material_setter:mat-setter");
		assert_eq!(actions.actions[0].label, "Jacket Color");
		assert_eq!(actions.actions[0].conditions.len(), 1);
		assert_eq!(actions.actions[0].conditions[0].source_component_id.as_deref(), Some("mat-setter"));
		assert_eq!(actions.actions[0].conditions[0].parameter_name.as_deref(), Some("JacketColor"));
		assert_eq!(actions.actions[0].conditions[0].parameter_value, Some(1.0));
		assert_eq!(
			actions.actions[0].conditions[0].sub_parameter_names,
			vec!["JacketHue".to_string(), "JacketSat".to_string()]
		);
		assert!(!actions.actions[0].conditions[0].inverted);
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
	fn unavatar_runtime_actions_import_modular_avatar_object_toggle() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Hat".to_string()),
					source_node_id: Some("node_hat".to_string()),
					resolved_node_id: Some("resolved_hat".to_string()),
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
						"shortType": "ModularAvatarObjectToggle",
						"enabled": true,
						"id": "hat-toggle",
						"fields": {
							"menuItem": {
								"label": "Hat",
								"path": "Clothes/Hat",
								"Control": {
									"Parameter": {"Name": "Hat"},
									"Value": 1
								}
							},
							"Objects": [{
								"Object": {
									"nodeId": "node_hat",
									"path": "Outdated/Hat"
								},
								"Active": false
							}]
						}
					}]
				}
			}),
		};

		let actions = unavatar_runtime_action_set(&unavatar, Some(&scene), &[], &BTreeMap::new()).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].id, "ma:object_toggle:hat-toggle");
		assert_eq!(actions.actions[0].label, "Hat");
		assert_eq!(actions.actions[0].conditions.len(), 1);
		assert_eq!(actions.actions[0].conditions[0].source_component_id.as_deref(), Some("hat-toggle"));
		assert_eq!(actions.actions[0].conditions[0].parameter_name.as_deref(), Some("Hat"));
		assert_eq!(actions.actions[0].conditions[0].parameter_value, Some(1.0));
		assert_eq!(
			actions.actions[0].triggers,
			vec![
				UnaRuntimeActionTrigger::SupervisorCommand {
					command: "ma:object_toggle:hat-toggle".to_string()
				},
				UnaRuntimeActionTrigger::ExpressionMenu {
					path: "Clothes/Hat".to_string()
				},
				UnaRuntimeActionTrigger::ParameterValue {
					name: "Hat".to_string(),
					value: 1.0
				}
			]
		);
		assert_eq!(
			actions.actions[0].effects,
			vec![UnaRuntimeActionEffect::NodeVisibility {
				target: UnaRuntimeNodeTarget {
					node_index: None,
					source_node_id: Some("node_hat".to_string()),
					resolved_node_id: Some("resolved_hat".to_string()),
					path: Some("Root/Hat".to_string()),
				},
				visible: false,
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

		let actions = unavatar_runtime_action_set(&unavatar, Some(&scene), &[], &BTreeMap::new()).expect("runtime actions");

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

		let actions = unavatar_runtime_action_set(&unavatar, None, &[], &BTreeMap::new()).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].label, "Jacket Red");
	}

	#[test]
	fn unavatar_runtime_actions_use_modular_avatar_menu_item_label_field_and_path() {
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
								"label": "Jacket Crimson",
								"path": "Clothes/Jacket Crimson",
								"Control": {
									"name": "Ignored Control Label",
									"Parameter": {"Name": "JacketColor"},
									"Value": 2
								}
							},
							"objects": [{
								"object": {
									"nodeId": "node_jacket",
									"path": "Root/Jacket"
								},
								"MaterialIndex": 0,
								"Material": {
									"materialName": "Jacket Crimson"
								}
							}]
						}
					}]
				}
			}),
		};

		let actions = unavatar_runtime_action_set(&unavatar, None, &[], &BTreeMap::new()).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].label, "Jacket Crimson");
		assert_eq!(
			actions.actions[0].triggers,
			vec![
				UnaRuntimeActionTrigger::SupervisorCommand {
					command: "ma:material_setter:mat-setter".to_string()
				},
				UnaRuntimeActionTrigger::ExpressionMenu {
					path: "Clothes/Jacket Crimson".to_string()
				},
				UnaRuntimeActionTrigger::ParameterValue {
					name: "JacketColor".to_string(),
					value: 2.0
				}
			]
		);
	}

	#[test]
	fn unavatar_runtime_actions_use_modular_avatar_control_name_label_fallback() {
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
								"Control": {
									"name": "Jacket Red"
								}
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

		let actions = unavatar_runtime_action_set(&unavatar, None, &[], &BTreeMap::new()).expect("runtime actions");

		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].label, "Jacket Red");
		assert!(actions.actions[0]
			.triggers
			.iter()
			.all(|trigger| !matches!(trigger, UnaRuntimeActionTrigger::ExpressionMenu { .. })));
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

		assert!(unavatar_runtime_action_set(&unavatar, Some(&scene), &[], &BTreeMap::new()).is_none());
	}

	#[test]
	fn unavatar_runtime_actions_import_modular_avatar_material_swap_from_scene_slots() {
		let primitive_base = UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
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

		let actions = unavatar_runtime_action_set(&unavatar, Some(&scene), &[], &BTreeMap::new()).expect("runtime actions");

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
			vertex_payload_id: None,
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

		let actions = unavatar_runtime_action_set(&unavatar, Some(&scene), &[], &BTreeMap::new()).expect("runtime actions");

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
			vertex_payload_id: None,
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

		assert!(unavatar_runtime_action_set(&unavatar, Some(&scene), &[], &BTreeMap::new()).is_none());
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
			vertex_payload_id: None,
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
	fn hidden_liltoon_outline_shader_enables_outline_pass_even_when_toggle_is_zero() {
		let extras = serde_json::json!({
			"family": "liltoon",
			"sourceShader": "Hidden/lilToonOutline",
			"floatParams": {
				"_UseOutline": 0.0,
				"_OutlineWidth": 0.2
			},
			"colorParams": {
				"_OutlineColor": [0.3, 0.3, 0.3, 1.0]
			},
			"mtoon": {
				"outlineWidthFactor": 0.0,
				"outlineWidthFactorUnit": "meters"
			}
		});

		let liltoon_like = unavatar_liltoon_like_from_extras(&extras).expect("liltoon_like material");

		assert_eq!(liltoon_like.outline.enabled_factor, 1.0);
		assert!((liltoon_like.outline.width_factor - 0.002).abs() < 0.000001);
		assert_eq!(liltoon_like.outline.color_factor, [0.3, 0.3, 0.3, 1.0]);
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

	#[test]
	fn unavatar_expression_catalog_includes_arkit_perfect_sync_without_all_morphs() {
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![UnaMeshBuffers {
				name: None,
				vertex_payload_id: None,
				positions: vec![[0.0, 0.0, 0.0]],
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
				morph_targets: vec![
					UnaMorphTargetDeltas {
						position_deltas: vec![[0.0, 0.0, 0.0]],
						normal_deltas: None,
					},
					UnaMorphTargetDeltas {
						position_deltas: vec![[0.0, 0.0, 0.0]],
						normal_deltas: None,
					},
					UnaMorphTargetDeltas {
						position_deltas: vec![[0.0, 0.0, 0.0]],
						normal_deltas: None,
					},
				],
				morph_target_names: vec!["jawOpen".to_string(), "MenuToggleMorph".to_string(), "BodySetupMorph".to_string()],
				default_morph_weights: Vec::new(),
			}]],
			..Default::default()
		};
		let runtime_expression_names = BTreeSet::from(["MenuToggleMorph".to_string()]);
		let arkit_names = arkit_perfect_sync_expression_name_set();

		let catalog = expression_catalog_from_morph_target_names(&scene, Some(&runtime_expression_names), Some(&arkit_names))
			.expect("expression catalog");
		let names: Vec<_> = catalog.presets.iter().map(|preset| preset.name.as_str()).collect();

		assert_eq!(names, vec!["MenuToggleMorph", "jawOpen"]);
		assert_eq!(catalog.presets[0].binds[0].morph_target_index, 1);
		assert_eq!(catalog.presets[1].binds[0].morph_target_index, 0);
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
			vertex_payload_id: None,
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

	fn test_blend_shape_delete_primitive() -> UnaMeshBuffers {
		UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
			positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: Some(vec![0, 1, 2, 1, 3, 2]),
			material_index: None,
			morph_targets: vec![UnaMorphTargetDeltas {
				position_deltas: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.25, 0.0, 0.0]],
				normal_deltas: None,
			}],
			morph_target_names: vec!["DeleteMe".to_string()],
			default_morph_weights: Vec::new(),
		}
	}

	fn test_morph_primitive(shape_name: &str, default_weight: f32) -> UnaMeshBuffers {
		UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
			positions: vec![[0.0, 0.0, 0.0]],
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
			morph_targets: vec![UnaMorphTargetDeltas {
				position_deltas: vec![[0.0, 0.0, 0.0]],
				normal_deltas: None,
			}],
			morph_target_names: vec![shape_name.to_string()],
			default_morph_weights: vec![default_weight],
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
			serde_json::json!({"shortType": "ModularAvatarMenuItem", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarMeshCutter", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarSyncParameterSequence", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarVisibleHeadAccessory", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarConvertConstraints", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarFloorAdjuster", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarGlobalCollider", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarPBBlocker", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarPlatformFilter", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarRenameVRChatCollisionTags", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarVRChatSettings", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarScaleAdjuster", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarWorldFixedObject", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarWorldScaleObject", "enabled": true}),
			serde_json::json!({"shortType": "MAMoveIndependently", "enabled": true}),
			serde_json::json!({"shortType": "ModularAvatarUnknownDisabled", "enabled": false}),
		];
		let mut report = ImportReport::default();
		report_unavatar_modular_avatar_component_catalog(&components, &mut report);

		let message = report
			.messages
			.iter()
			.find(|message| message.contains("Modular Avatar components"))
			.unwrap();
		assert!(message.contains("total=18"));
		assert!(message.contains("resolver_supported=1"));
		assert!(message.contains("approximate_supported=2"));
		assert!(message.contains("runtime_action_supported=1"));
		assert!(message.contains("metadata_supported=5"));
		assert!(message.contains("unsupported=9"));
		assert!(message.contains("disabled=1"));
		assert!(message.contains("support_kind_mismatches=0"));
		assert!(message.contains("ModularAvatarConvertConstraints:1"));
		assert!(message.contains("ModularAvatarFloorAdjuster:1"));
		assert!(message.contains("ModularAvatarPlatformFilter:1"));
		assert!(message.contains("ModularAvatarRenameVRChatCollisionTags:1"));
		assert!(message.contains("ModularAvatarVRChatSettings:1"));
		assert!(message.contains("ModularAvatarWorldFixedObject:1"));
		assert!(message.contains("ModularAvatarWorldScaleObject:1"));
		assert!(message.contains("MAMoveIndependently:1"));
		assert!(message.contains("ModularAvatarUnknownDisabled:1"));
		let approximations = report
			.approximations
			.iter()
			.map(|approximation| (approximation.feature.as_str(), approximation.detail.as_deref().unwrap_or("")))
			.collect::<Vec<_>>();
		assert_eq!(approximations.len(), 2);
		assert!(approximations.iter().any(|(feature, detail)| {
			*feature == "ModularAvatar.ModularAvatarMeshCutter" && detail.contains("dynamic gating is not evaluated")
		}));
		assert!(approximations.iter().any(|(feature, detail)| {
			*feature == "ModularAvatar.ModularAvatarScaleAdjuster" && detail.contains("resolver-side transform/scale subset")
		}));
		let unsupported_features = report
			.lost_features
			.iter()
			.map(|feature| feature.feature.as_str())
			.collect::<Vec<_>>();
		assert!(unsupported_features.contains(&"ModularAvatar.ModularAvatarWorldFixedObject"));
		assert!(unsupported_features.contains(&"ModularAvatar.ModularAvatarWorldScaleObject"));
		assert!(unsupported_features.contains(&"ModularAvatar.ModularAvatarConvertConstraints"));
		assert!(unsupported_features.contains(&"ModularAvatar.ModularAvatarFloorAdjuster"));
		assert!(unsupported_features.contains(&"ModularAvatar.ModularAvatarPlatformFilter"));
		assert!(unsupported_features.contains(&"ModularAvatar.ModularAvatarRenameVRChatCollisionTags"));
		assert!(unsupported_features.contains(&"ModularAvatar.ModularAvatarVRChatSettings"));
		assert!(unsupported_features.contains(&"ModularAvatar.ModularAvatarWorldFixedObject"));
		assert!(unsupported_features.contains(&"ModularAvatar.ModularAvatarWorldScaleObject"));
		assert!(unsupported_features.contains(&"ModularAvatar.MAMoveIndependently"));
		assert_eq!(report.lost_features.len(), 8);
		for unsupported_type in [
			"ModularAvatarConvertConstraints",
			"ModularAvatarFloorAdjuster",
			"ModularAvatarPlatformFilter",
			"ModularAvatarRenameVRChatCollisionTags",
			"ModularAvatarVRChatSettings",
			"ModularAvatarWorldFixedObject",
			"ModularAvatarWorldScaleObject",
			"MAMoveIndependently",
		] {
			assert!(report.diagnostics.iter().any(|diagnostic| {
				diagnostic.severity == un_avatar_core::ReportSeverity::Warning && diagnostic.text.contains(unsupported_type)
			}));
		}
	}

	#[test]
	fn modular_avatar_component_catalog_warns_on_exported_support_kind_mismatch() {
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarMeshCutter",
			"supportKind": "unsupported",
			"enabled": true
		})];
		let mut report = ImportReport::default();
		report_unavatar_modular_avatar_component_catalog(&components, &mut report);

		let message = report
			.messages
			.iter()
			.find(|message| message.contains("Modular Avatar components"))
			.unwrap();
		assert!(message.contains("approximate_supported=1"));
		assert!(message.contains("support_kind_mismatches=1"));
		assert!(report.diagnostics.iter().any(|diagnostic| {
			diagnostic.severity == un_avatar_core::ReportSeverity::Warning
				&& diagnostic.text.contains("supportKind mismatch")
				&& diagnostic.text.contains("count=1")
		}));
		assert!(report
			.approximations
			.iter()
			.any(|approximation| approximation.feature == "ModularAvatar.ModularAvatarMeshCutter"));
		assert!(report.lost_features.is_empty());
	}

	#[test]
	fn modular_avatar_component_catalog_does_not_warn_for_inverted_runtime_actions() {
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarObjectToggle",
			"enabled": true,
			"fields": {"m_inverted": true}
		})];
		let mut report = ImportReport::default();
		report_unavatar_modular_avatar_component_catalog(&components, &mut report);

		let message = report
			.messages
			.iter()
			.find(|message| message.contains("Modular Avatar components"))
			.unwrap();
		assert!(message.contains("runtime_action_supported=1"));
		assert!(!report.messages.iter().any(|message| message.contains("inverted_ignored")));
		assert_eq!(report.lost_features.len(), 0);
		assert_eq!(report.approximations.len(), 0);
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
				"fields": {"m_Mode": 1}
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
	fn modular_avatar_vertex_filter_deletes_blend_shape_triangles_and_clones_shared_mesh() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("TargetRenderer".to_string()),
					source_node_id: Some("node_target".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("OutsideRenderer".to_string()),
					source_node_id: Some("node_outside".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![vec![test_blend_shape_delete_primitive()]],
			..Default::default()
		};
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarMeshCutter",
			"enabled": true,
			"fields": {
				"m_object": {"nodeId": "node_target", "path": "Root/TargetRenderer"},
				"m_multiMode": "VertexIntersection",
				"filters": [{
					"shortType": "VertexFilterByShapeComponent",
					"fields": {
						"m_shapes": ["DeleteMe"],
						"m_threshold": 0.01
					}
				}]
			}
		})];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		let (nodes, primitives, triangles, missing, skipped, unsupported) =
			apply_unavatar_vertex_filters(&mut scene, &components, &node_ids, &registry_paths, &paths, &normalized_paths);

		assert_eq!((nodes, primitives, triangles, missing, skipped, unsupported), (1, 1, 1, 0, 0, 0));
		assert_eq!(scene.nodes[1].mesh, Some(1));
		assert_eq!(scene.nodes[2].mesh, Some(0));
		assert_eq!(scene.meshes[0][0].indices.as_deref(), Some(&[0, 1, 2, 1, 3, 2][..]));
		assert_eq!(scene.meshes[1][0].indices.as_deref(), Some(&[0, 1, 2][..]));
	}

	#[test]
	fn modular_avatar_vertex_filter_deletes_axis_selected_triangles() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("TargetRenderer".to_string()),
					source_node_id: Some("node_target".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![vec![test_blend_shape_delete_primitive()]],
			..Default::default()
		};
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarMeshCutter",
			"enabled": true,
			"fields": {
				"m_object": {"nodeId": "node_target", "path": "Root/TargetRenderer"},
				"m_multiMode": "VertexIntersection",
				"filters": [{
					"shortType": "VertexFilterByAxisComponent",
					"fields": {
						"m_center": [0.0, 0.5, 0.0],
						"m_axis": [0.0, 1.0, 0.0]
					}
				}]
			}
		})];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		let (nodes, primitives, triangles, missing, skipped, unsupported) =
			apply_unavatar_vertex_filters(&mut scene, &components, &node_ids, &registry_paths, &paths, &normalized_paths);

		assert_eq!((nodes, primitives, triangles, missing, skipped, unsupported), (1, 1, 2, 0, 0, 0));
		assert_eq!(scene.nodes[1].mesh, Some(0));
		assert_eq!(scene.meshes[0][0].indices.as_deref(), Some(&[][..]));
	}

	#[test]
	fn modular_avatar_vertex_filter_axis_uses_skinned_rest_pose() {
		let mut primitive = UnaMeshBuffers {
			name: None,
			vertex_payload_id: None,
			positions: vec![[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: Some(vec![[0, 0, 0, 0]; 3]),
			weights: Some(vec![[1.0, 0.0, 0.0, 0.0]; 3]),
			indices: Some(vec![0, 1, 2]),
			material_index: None,
			morph_targets: Vec::new(),
			morph_target_names: Vec::new(),
			default_morph_weights: Vec::new(),
		};
		primitive.morph_targets.clear();
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("TargetRenderer".to_string()),
					source_node_id: Some("node_target".to_string()),
					mesh: Some(0),
					skin: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Bone".to_string()),
					source_node_id: Some("node_bone".to_string()),
					transform: Mat4::from_translation(Vec3::new(1.0, 0.0, 0.0)).to_cols_array(),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![vec![primitive]],
			skins: vec![UnaSkin {
				joint_nodes: vec![2],
				inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array()],
				skeleton_node: None,
			}],
			..Default::default()
		};
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarMeshCutter",
			"enabled": true,
			"fields": {
				"m_object": {"nodeId": "node_target", "path": "Root/TargetRenderer"},
				"filters": [{
					"shortType": "VertexFilterByAxisComponent",
					"fields": {
						"m_center": [0.5, 0.0, 0.0],
						"m_axis": [1.0, 0.0, 0.0]
					}
				}]
			}
		})];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		let (nodes, primitives, triangles, missing, skipped, unsupported) =
			apply_unavatar_vertex_filters(&mut scene, &components, &node_ids, &registry_paths, &paths, &normalized_paths);

		assert_eq!((nodes, primitives, triangles, missing, skipped, unsupported), (1, 1, 1, 0, 0, 0));
		assert_eq!(scene.meshes[0][0].indices.as_deref(), Some(&[][..]));
	}

	#[test]
	fn modular_avatar_vertex_filter_axis_default_matches_modular_avatar() {
		let component = serde_json::json!({
			"shortType": "VertexFilterByAxisComponent",
			"fields": {
				"m_center": [0.0, 0.0, 0.0]
			}
		});

		let Some(ModularAvatarVertexFilter::Axis { center, axis }) = modular_avatar_vertex_filter_by_axis(&component) else {
			panic!("expected axis filter");
		};

		assert_eq!(center, [0.0, 0.0, 0.0]);
		assert_eq!(axis, [-1.0, 0.0, 0.0]);
	}

	#[test]
	fn modular_avatar_vertex_filter_deletes_bone_weight_selected_triangles() {
		let mut primitive = test_blend_shape_delete_primitive();
		primitive.joints = Some(vec![[0, 0, 0, 0], [0, 0, 0, 0], [0, 0, 0, 0], [1, 0, 0, 0]]);
		primitive.weights = Some(vec![[1.0, 0.0, 0.0, 0.0]; 4]);
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2, 3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("TargetRenderer".to_string()),
					source_node_id: Some("node_target".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					skin: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Bone0".to_string()),
					source_node_id: Some("node_bone0".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Bone1".to_string()),
					source_node_id: Some("node_bone1".to_string()),
					resolved_node_id: None,
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![vec![primitive]],
			skins: vec![UnaSkin {
				joint_nodes: vec![2, 3],
				inverse_bind_matrices: Vec::new(),
				skeleton_node: None,
			}],
			..Default::default()
		};
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarMeshCutter",
			"enabled": true,
			"fields": {
				"m_object": {"nodeId": "node_target", "path": "Root/TargetRenderer"},
				"m_multiMode": "VertexIntersection",
				"filters": [{
					"shortType": "VertexFilterByBoneComponent",
					"fields": {
						"m_bone": {"nodeId": "node_bone1", "path": "Root/Bone1"},
						"m_threshold": 0.5
					}
				}]
			}
		})];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		let (nodes, primitives, triangles, missing, skipped, unsupported) =
			apply_unavatar_vertex_filters(&mut scene, &components, &node_ids, &registry_paths, &paths, &normalized_paths);

		assert_eq!((nodes, primitives, triangles, missing, skipped, unsupported), (1, 1, 1, 0, 0, 0));
		assert_eq!(scene.meshes[0][0].indices.as_deref(), Some(&[0, 1, 2][..]));
	}

	#[test]
	fn modular_avatar_vertex_filter_deletes_mask_selected_triangles() {
		let mut primitive = test_blend_shape_delete_primitive();
		primitive.tex_coords_0 = Some(vec![[0.0, 0.0], [0.75, 0.0], [0.0, 0.75], [0.75, 0.75]]);
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("TargetRenderer".to_string()),
					source_node_id: Some("node_target".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![vec![primitive]],
			images: vec![UnaImageRgba {
				width: 2,
				height: 2,
				pixel_format: UnaImagePixelFormat::R8G8B8A8,
				pixels: vec![0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255],
			}],
			image_sources: vec![Some(UnaImageSourceMetadata::default())],
			..Default::default()
		};
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarMeshCutter",
			"enabled": true,
			"fields": {
				"m_object": {"nodeId": "node_target", "path": "Root/TargetRenderer"},
				"m_multiMode": "VertexIntersection",
				"filters": [{
					"shortType": "VertexFilterByMaskComponent",
					"fields": {
						"maskTextureAssetId": "mask_0",
						"m_materialIndex": 0,
						"m_deleteMode": "DeleteBlack"
					}
				}]
			}
		})];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);
		let texture_asset_map = BTreeMap::from([("mask_0".to_string(), 0usize)]);

		let (nodes, primitives, triangles, missing, skipped, unsupported) = apply_unavatar_vertex_filters_with_texture_assets(
			&mut scene,
			&components,
			&node_ids,
			&registry_paths,
			&paths,
			&normalized_paths,
			&texture_asset_map,
		);

		assert_eq!((nodes, primitives, triangles, missing, skipped, unsupported), (1, 1, 1, 0, 0, 0));
		assert_eq!(scene.meshes[0][0].indices.as_deref(), Some(&[1, 3, 2][..]));
	}

	#[test]
	fn modular_avatar_mask_wraps_mirror_once_coordinates() {
		assert_eq!(modular_avatar_wrap_uv(-0.25, UnaTextureWrapMode::MirrorOnce), 0.25 - f32::EPSILON);
		assert_eq!(modular_avatar_wrap_uv(1.25, UnaTextureWrapMode::MirrorOnce), 1.0);
		assert_eq!(modular_avatar_wrap_uv(-1.25, UnaTextureWrapMode::MirrorOnce), 1.0);
	}

	#[test]
	fn modular_avatar_shape_changer_set_applies_default_morph_and_clones_shared_mesh() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("TargetRenderer".to_string()),
					source_node_id: Some("node_target".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("OutsideRenderer".to_string()),
					source_node_id: Some("node_outside".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![vec![test_morph_primitive("Smile", 0.0)]],
			..Default::default()
		};
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarShapeChanger",
			"enabled": true,
			"fields": {
				"m_shapes": [{
					"m_object": {"nodeId": "node_target", "path": "Root/TargetRenderer"},
					"m_shapeName": "Smile",
					"m_changeType": "Set",
					"m_value": 75.0
				}]
			}
		})];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		let result = apply_unavatar_shape_changer_sets(
			&mut scene,
			&components,
			&node_ids,
			&registry_paths,
			&paths,
			&normalized_paths,
			false,
		);

		assert_eq!(result, (1, 0, 0));
		assert_eq!(scene.nodes[1].mesh, Some(1));
		assert_eq!(scene.nodes[2].mesh, Some(0));
		assert_eq!(scene.meshes[1][0].default_morph_weights, vec![0.75]);
		assert_eq!(scene.meshes[0][0].default_morph_weights, vec![0.0]);
	}

	#[test]
	fn modular_avatar_shape_changer_set_applies_string_payloads() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Body_b".to_string()),
					source_node_id: Some("node_body".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("UV2_Shirts".to_string()),
					source_node_id: Some("node_shirts".to_string()),
					resolved_node_id: None,
					mesh: Some(1),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![
				vec![test_morph_primitive("Spine1_____腰_上部", 0.0)],
				vec![test_morph_primitive("Skirt_ON", 0.0)],
			],
			..Default::default()
		};
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarShapeChanger",
			"enabled": true,
			"fields": {
				"m_shapes": [
					"Body_b Spine1_____腰_上部 Set 100",
					"UV2_Shirts Skirt_ON Set 75"
				]
			}
		})];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		let result = apply_unavatar_shape_changer_sets(
			&mut scene,
			&components,
			&node_ids,
			&registry_paths,
			&paths,
			&normalized_paths,
			false,
		);

		assert_eq!(result, (2, 0, 0));
		assert_eq!(scene.meshes[0][0].default_morph_weights, vec![1.0]);
		assert_eq!(scene.meshes[1][0].default_morph_weights, vec![0.75]);
	}

	#[test]
	fn modular_avatar_shape_changer_set_feeds_static_blendshape_sync() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Body".to_string()),
					source_node_id: Some("node_body".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Jacket".to_string()),
					source_node_id: Some("node_jacket".to_string()),
					resolved_node_id: None,
					mesh: Some(1),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![
				vec![test_morph_primitive("Breast_Big", 0.0)],
				vec![test_morph_primitive("Jacket_Breast_Big", 0.0)],
			],
			..Default::default()
		};
		let components = vec![
			serde_json::json!({
				"shortType": "ModularAvatarShapeChanger",
				"enabled": true,
				"fields": {
					"m_shapes": [{
						"m_object": {"nodeId": "node_body", "path": "Root/Body"},
						"m_shapeName": "Breast_Big",
						"m_changeType": "Set",
						"m_value": 50.0
					}]
				}
			}),
			serde_json::json!({
				"shortType": "ModularAvatarBlendshapeSync",
				"enabled": true,
				"target": {"nodeId": "node_jacket", "path": "Root/Jacket"},
				"fields": {
					"Bindings": [{
						"referenceMesh": {"resolvedTarget": {"nodeId": "node_body", "path": "Root/Body"}},
						"blendshape": "Breast_Big",
						"localBlendshape": "Jacket_Breast_Big",
						"remapCurve": {
							"keyCount": 2,
							"keys": [
								{"time": 0.0, "value": 0.0},
								{"time": 1.0, "value": 1.0}
							]
						}
					}]
				}
			}),
		];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		assert_eq!(
			apply_unavatar_shape_changer_sets(
				&mut scene,
				&components,
				&node_ids,
				&registry_paths,
				&paths,
				&normalized_paths,
				false,
			),
			(1, 0, 0)
		);
		assert_eq!(
			apply_unavatar_blendshape_syncs(&mut scene, &components, &node_ids, &registry_paths, &paths, &normalized_paths),
			(1, 0, 0, 0)
		);

		assert_eq!(scene.meshes[0][0].default_morph_weights, vec![0.5]);
		assert_eq!(scene.meshes[1][0].default_morph_weights, vec![0.5]);
	}

	#[test]
	fn modular_avatar_blendshape_sync_propagates_static_default_weight() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2, 3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Body".to_string()),
					source_node_id: Some("node_body".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Jacket".to_string()),
					source_node_id: Some("node_jacket".to_string()),
					resolved_node_id: None,
					mesh: Some(1),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("JacketCopy".to_string()),
					source_node_id: Some("node_jacket_copy".to_string()),
					resolved_node_id: None,
					mesh: Some(1),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![
				vec![test_morph_primitive("Breast_Big", 0.25)],
				vec![test_morph_primitive("Jacket_Breast_Big", 0.0)],
			],
			..Default::default()
		};
		let components = vec![serde_json::json!({
			"shortType": "ModularAvatarBlendshapeSync",
			"enabled": true,
			"target": {"nodeId": "node_jacket", "path": "Root/Jacket"},
			"fields": {
				"Bindings": [{
					"referenceMesh": {"resolvedTarget": {"nodeId": "node_body", "path": "Root/Body"}},
					"blendshape": "Breast_Big",
					"localBlendshape": "Jacket_Breast_Big",
					"remapCurve": {
						"keyCount": 2,
						"keys": [
							{"time": 0.0, "value": 0.0},
							{"time": 1.0, "value": 0.5}
						]
					}
				}]
			}
		})];
		let node_ids = scene_node_ids(&scene);
		let registry_paths = BTreeMap::new();
		let paths = scene_node_paths(&scene);
		let normalized_paths = scene_node_normalized_paths(&scene);

		let result = apply_unavatar_blendshape_syncs(&mut scene, &components, &node_ids, &registry_paths, &paths, &normalized_paths);

		assert_eq!(result, (1, 0, 0, 0));
		assert_eq!(scene.nodes[2].mesh, Some(2));
		assert_eq!(scene.nodes[3].mesh, Some(1));
		assert_eq!(scene.meshes[2][0].default_morph_weights, vec![0.125]);
		assert_eq!(scene.meshes[1][0].default_morph_weights, vec![0.0]);
	}

	#[test]
	fn modular_avatar_blendshape_sync_remap_curve_uses_key_tangents() {
		let curve = serde_json::json!({
			"keyCount": 2,
			"keys": [
				{"time": 0.0, "value": 0.0, "outTangent": 0.0},
				{"time": 1.0, "value": 1.0, "inTangent": 0.0}
			]
		});

		let value = modular_avatar_remap_curve_evaluate(Some(&curve), 0.25);

		assert!((value - 0.15625).abs() < 0.00001, "value={value}");
	}

	#[test]
	fn modular_avatar_blendshape_sync_adds_runtime_expression_bind_for_linear_mapping() {
		let scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Body".to_string()),
					source_node_id: Some("node_body".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Jacket".to_string()),
					source_node_id: Some("node_jacket".to_string()),
					resolved_node_id: None,
					mesh: Some(1),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![
				vec![test_morph_primitive("Breast_Big", 0.0)],
				vec![test_morph_primitive("Jacket_Breast_Big", 0.0)],
			],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [{
						"shortType": "ModularAvatarBlendshapeSync",
						"enabled": true,
						"target": {"nodeId": "node_jacket", "path": "Root/Jacket"},
						"fields": {
							"Bindings": [{
								"referenceMesh": {"resolvedTarget": {"nodeId": "node_body", "path": "Root/Body"}},
								"blendshape": "Breast_Big",
								"localBlendshape": "Jacket_Breast_Big",
								"remapCurve": {
									"keyCount": 2,
									"keys": [
										{"time": 0.0, "value": 0.0, "outTangent": 0.5},
										{"time": 1.0, "value": 0.5, "inTangent": 0.5}
									]
								}
							}]
						}
					}]
				}
			}),
		};
		let mut catalog = expression_catalog_from_morph_target_names(&scene, None, None).expect("expression catalog");
		let mut report = ImportReport::default();

		apply_unavatar_blendshape_sync_expression_binds(&mut catalog, &scene, &unavatar, &mut report);

		let preset = catalog
			.presets
			.iter()
			.find(|preset| preset.name == "Breast_Big")
			.expect("source preset");
		assert!(preset
			.binds
			.iter()
			.any(|bind| bind.mesh_index == 1 && bind.primitive_index == 0 && bind.morph_target_index == 0 && bind.weight_scale == 0.5));
		assert!(report
			.messages
			.iter()
			.any(|message| message.contains("blendshape_sync_expression_binds=1")));
		let mut weights = UnaExpressionWeights::default();
		weights.preset_weights.insert("Breast_Big".to_string(), 0.8);
		let morphs = un_avatar_core::morph_weights_for_primitive(&scene.meshes[1][0], Some(&catalog), Some(&weights), 1, 0);
		assert_eq!(morphs, vec![0.4]);
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
	fn modular_avatar_apply_orders_shape_changer_set_before_blendshape_sync() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Body".to_string()),
					source_node_id: Some("node_body".to_string()),
					resolved_node_id: None,
					mesh: Some(0),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Jacket".to_string()),
					source_node_id: Some("node_jacket".to_string()),
					resolved_node_id: None,
					mesh: Some(1),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			meshes: vec![
				vec![test_morph_primitive("Breast_Big", 0.0)],
				vec![test_morph_primitive("Jacket_Breast_Big", 0.0)],
			],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [
						{
							"shortType": "ModularAvatarShapeChanger",
							"enabled": true,
							"fields": {
								"m_shapes": [{
									"m_object": {"nodeId": "node_body", "path": "Root/Body"},
									"m_shapeName": "Breast_Big",
									"m_changeType": "Set",
									"m_value": 50.0
								}]
							}
						},
						{
							"shortType": "ModularAvatarBlendshapeSync",
							"enabled": true,
							"target": {"nodeId": "node_jacket", "path": "Root/Jacket"},
							"fields": {
								"Bindings": [{
									"referenceMesh": {"resolvedTarget": {"nodeId": "node_body", "path": "Root/Body"}},
									"blendshape": "Breast_Big",
									"localBlendshape": "Jacket_Breast_Big",
									"remapCurve": {
										"keyCount": 2,
										"keys": [
											{"time": 0.0, "value": 0.0},
											{"time": 1.0, "value": 1.0}
										]
									}
								}]
							}
						}
					]
				}
			}),
		};
		let mut report = ImportReport::default();

		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		assert_eq!(scene.meshes[0][0].default_morph_weights, vec![0.5]);
		assert_eq!(scene.meshes[1][0].default_morph_weights, vec![0.5]);
		assert!(report
			.messages
			.iter()
			.any(|message| message.contains("shape_changer_set_applied=1")));
		assert!(report.messages.iter().any(|message| message.contains("blendshape_sync_applied=1")));
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
	fn modular_avatar_bone_proxy_resolves_humanoid_bone_sub_path() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 4],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Hips".to_string()),
					source_node_id: Some("node_hips".to_string()),
					children: vec![2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("TailRoot".to_string()),
					source_node_id: Some("node_tail_root".to_string()),
					children: vec![3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("TailTip".to_string()),
					source_node_id: Some("node_tail_tip".to_string()),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Proxy".to_string()),
					source_node_id: Some("node_proxy".to_string()),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let humanoid = HumanoidProfile {
			bone_node_indices: BTreeMap::from([("hips".to_string(), 1)]),
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
						"fields": {
							"boneReference": "Hips",
							"subPath": "TailRoot/TailTip",
							"attachmentMode": "AsChildAtRoot",
							"matchScale": false
						}
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar_with_humanoid(&mut scene, &unavatar, &humanoid, &mut report);

		assert_eq!(scene.nodes[3].children, vec![4]);
		assert!(report.messages.iter().any(|m| m.contains("bone_proxy_applied=1")));
	}

	#[test]
	fn modular_avatar_bone_proxy_resolves_avatar_root_and_root_relative_sub_path() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2, 3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Head".to_string()),
					source_node_id: Some("node_head".to_string()),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("ProxyA".to_string()),
					source_node_id: Some("node_proxy_a".to_string()),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("ProxyB".to_string()),
					source_node_id: Some("node_proxy_b".to_string()),
					..test_node(Vec::new())
				},
			],
			roots: vec![0],
			..Default::default()
		};
		let humanoid = HumanoidProfile::default();
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [
						{
							"shortType": "ModularAvatarBoneProxy",
							"enabled": true,
							"target": {"nodeId": "node_proxy_a", "path": "ProxyA"},
							"fields": {
								"boneReference": "LastBone",
								"subPath": "Head",
								"attachmentMode": "AsChildAtRoot"
							}
						},
						{
							"shortType": "ModularAvatarBoneProxy",
							"enabled": true,
							"target": {"nodeId": "node_proxy_b", "path": "ProxyB"},
							"fields": {
								"boneReference": "LastBone",
								"subPath": "$$AVATAR",
								"attachmentMode": "AsChildAtRoot"
							}
						}
					]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar_with_humanoid(&mut scene, &unavatar, &humanoid, &mut report);

		assert_eq!(scene.nodes[1].children, vec![2]);
		assert!(scene.nodes[0].children.contains(&3));
		assert!(report.messages.iter().any(|m| m.contains("bone_proxy_applied=2")));
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
	fn modular_avatar_merge_armature_retargets_dynamics_nodes() {
		let mut settings = UnaDynamicsSettings {
			groups: vec![UnaSpringBoneGroup {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				center_node: Some(2),
				bone_node_indices: vec![0, 2, 3],
				interaction_chain_start_index: 2,
				..Default::default()
			}],
			colliders: vec![UnaDynamicsCollider {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				node: 2,
				..Default::default()
			}],
			contacts: vec![UnaDynamicsContact {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				node: 2,
				..Default::default()
			}],
			constraint_refs: vec![UnaDynamicsConstraintRef {
				source_kind: UnaDynamicsSourceKind::VrcPhysBone,
				target_node: 2,
				source_nodes: vec![2, 3],
				..Default::default()
			}],
			..Default::default()
		};

		let retargeted = retarget_merge_armature_dynamics(&mut settings, &[(2, 1)]);

		assert_eq!(retargeted, 6);
		assert_eq!(settings.groups[0].center_node, Some(1));
		assert_eq!(settings.groups[0].bone_node_indices, vec![0, 1, 3]);
		assert_eq!(settings.groups[0].interaction_chain_start_index, 2);
		assert_eq!(settings.colliders[0].node, 1);
		assert_eq!(settings.contacts[0].node, 1);
		assert_eq!(settings.constraint_refs[0].target_node, 1);
		assert_eq!(settings.constraint_refs[0].source_nodes, vec![1, 3]);
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

		assert_eq!(scene.nodes[1].children, Vec::<usize>::new());
		assert_eq!(scene.nodes[2].children, vec![3]);
		assert_eq!(after[3].transform_point3(Vec3::ZERO), before[3].transform_point3(Vec3::ZERO));
		assert_eq!(scene.skins[0].joint_nodes, vec![3]);
		assert_eq!(scene.skins[0].skeleton_node, Some(3));
	}

	#[test]
	fn modular_avatar_merge_armature_reparents_constraint_source_auxiliary_bones() {
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
			node_constraints: vec![UnaNodeConstraint {
				target_node: 1,
				source_node: 3,
				weight: 1.0,
				kind: UnaNodeConstraintKind::Rotation,
				sources: Vec::new(),
			}],
			roots: vec![0],
			..Default::default()
		};
		let before = scene_world_matrices(&scene);
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!(
				{
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
				}
			),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);
		let after = scene_world_matrices(&scene);
		assert_eq!(scene.nodes[1].children, vec![3]);
		assert_eq!(scene.nodes[2].children, Vec::<usize>::new());
		assert_eq!(after[3].transform_point3(Vec3::ZERO), before[3].transform_point3(Vec3::ZERO));
	}

	#[test]
	fn modular_avatar_merge_armature_reparents_multi_source_constraint_auxiliary_bones() {
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
					name: Some("Chest".to_string()),
					source_node_id: Some("node_target_chest".to_string()),
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
					name: Some("Chest".to_string()),
					source_node_id: Some("node_source_chest".to_string()),
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
					name: Some("CapeSource".to_string()),
					source_node_id: Some("node_cape_source".to_string()),
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
			node_constraints: vec![UnaNodeConstraint {
				target_node: 1,
				source_node: 1,
				weight: 1.0,
				kind: UnaNodeConstraintKind::Parent {
					translate_x: true,
					translate_y: true,
					translate_z: true,
					rotate_x: true,
					rotate_y: true,
					rotate_z: true,
					translation_at_rest: [0.0; 3],
					rotation_at_rest: [0.0; 3],
				},
				sources: vec![UnaNodeConstraintSource {
					source_node: 3,
					weight: 1.0,
					translation_offset: [0.0; 3],
					rotation_offset: [0.0; 3],
				}],
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
						"target": {"nodeId": "node_source_chest", "path": "Outfit/Armature/Chest"},
						"boneMappings": [{
							"sourceBone": {"nodeId": "node_source_chest", "path": "Outfit/Armature/Chest"},
							"targetBone": {"nodeId": "node_target_chest", "path": "Armature/Chest"}
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
		assert_eq!(scene.node_constraints[0].sources[0].source_node, 3);
		assert_eq!(after[3].transform_point3(Vec3::ZERO), before[3].transform_point3(Vec3::ZERO));
	}

	#[test]
	fn remap_scene_node_references_updates_multi_source_constraints() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				test_scene_node("Root", vec![1, 2, 3]),
				test_scene_node("Target", Vec::new()),
				test_scene_node("OldSource", Vec::new()),
				test_scene_node("NewSource", Vec::new()),
			],
			node_constraints: vec![UnaNodeConstraint {
				target_node: 1,
				source_node: 2,
				weight: 1.0,
				kind: UnaNodeConstraintKind::Parent {
					translate_x: true,
					translate_y: true,
					translate_z: true,
					rotate_x: true,
					rotate_y: true,
					rotate_z: true,
					translation_at_rest: [0.0; 3],
					rotation_at_rest: [0.0; 3],
				},
				sources: vec![UnaNodeConstraintSource {
					source_node: 2,
					weight: 1.0,
					translation_offset: [0.0; 3],
					rotation_offset: [0.0; 3],
				}],
			}],
			roots: vec![0],
			..Default::default()
		};

		remap_scene_node_references(&mut scene, 2, 3);

		assert_eq!(scene.node_constraints[0].source_node, 3);
		assert_eq!(scene.node_constraints[0].sources[0].source_node, 3);
	}

	#[test]
	fn modular_avatar_merge_armature_reparents_auxiliary_bone_ancestors_with_dynamics_roots() {
		let target_world = Mat4::from_translation(Vec3::new(0.0, 3.0, 0.0));
		let source_world = Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0));
		let hat_root_local = Mat4::from_translation(Vec3::new(0.0, 0.0, 5.0));
		let ribbon_root_local = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0));
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
					transform: hat_root_local.to_cols_array(),
					children: vec![4],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("RibbonRoot".to_string()),
					source_node_id: Some("node_ribbon_root".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: ribbon_root_local.to_cols_array(),
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
					"components": [{
						"shortType": "ModularAvatarMergeArmature",
						"enabled": true,
						"target": {"nodeId": "node_source_head", "path": "Outfit/Armature/Head"},
						"boneMappings": [{
							"sourceBone": {"nodeId": "node_source_head", "path": "Outfit/Armature/Head"},
							"targetBone": {"nodeId": "node_target_head", "path": "Armature/Head"}
						}]
					}]
				},
				"dynamics": [{
					"id": "physbone:hat-ribbon",
					"source": "vrc_physbone",
					"enabled": true,
					"roots": [{"nodeId": "node_ribbon_root", "path": "Outfit/Armature/Head/HatRoot/RibbonRoot"}],
					"ignoreTransforms": [{"nodeId": "node_ribbon_root", "path": "Outfit/Armature/Head/HatRoot/RibbonRoot"}]
				}]
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);
		let after = scene_world_matrices(&scene);

		assert_eq!(scene.nodes[1].children, vec![3]);
		assert_eq!(scene.nodes[2].children, Vec::<usize>::new());
		assert_eq!(scene.nodes[3].children, vec![4]);
		assert_eq!(after[3].transform_point3(Vec3::ZERO), before[3].transform_point3(Vec3::ZERO));
		assert_eq!(after[4].transform_point3(Vec3::ZERO), before[4].transform_point3(Vec3::ZERO));
	}

	#[test]
	fn modular_avatar_merge_armature_orders_nested_components_by_target_hierarchy() {
		let parents = vec![None, Some(0), Some(1), None];
		let components = vec![
			MergeArmatureComponentMapping {
				target_node: 1,
				mappings: Vec::new(),
			},
			MergeArmatureComponentMapping {
				target_node: 2,
				mappings: Vec::new(),
			},
			MergeArmatureComponentMapping {
				target_node: 0,
				mappings: Vec::new(),
			},
		];
		let (ordered, cycles) = order_merge_armature_components(&components, &parents);
		assert_eq!(cycles, 0);
		assert_eq!(ordered, vec![2, 0, 1]);
	}

	#[test]
	fn modular_avatar_merge_armature_counts_cycle_nodes() {
		assert_eq!(count_merge_armature_cycle_nodes(&[(0usize, 1usize), (1, 2), (2, 0)]), 3);
		assert_eq!(count_merge_armature_cycle_nodes(&[(0usize, 1), (1, 0), (2usize, 3usize)]), 2);
		assert_eq!(count_merge_armature_cycle_nodes(&[(0usize, 1), (2, 3)]), 0);
		assert_eq!(count_merge_armature_cycle_nodes(&[(0usize, 0)]), 0);
	}

	#[test]
	fn modular_avatar_merge_armature_reports_cyclic_mapping_warning() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					source_node_id: None,
					resolved_node_id: None,
					visible: true,
					transform: Mat4::from_translation(Vec3::new(0.0, 0.0, 0.0)).to_cols_array(),
					children: vec![1, 2],
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("SourceBoneA".to_string()),
					source_node_id: Some("node_source_a".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: Mat4::from_translation(Vec3::new(2.0, 0.0, 0.0)).to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
				UnaSceneNode {
					name: Some("SourceBoneB".to_string()),
					source_node_id: Some("node_source_b".to_string()),
					resolved_node_id: None,
					visible: true,
					transform: Mat4::from_translation(Vec3::new(3.0, 0.0, 0.0)).to_cols_array(),
					children: Vec::new(),
					mesh: None,
					skin: None,
					probe_anchor_node: None,
					local_bounds: None,
				},
			],
			skins: vec![UnaSkin {
				joint_nodes: vec![1],
				inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array()],
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
						"target": {"nodeId": "node_source_a", "path": "SourceBoneA"},
						"boneMappings": [{
							"sourceBone": {"nodeId": "node_source_a", "path": "SourceBoneA"},
							"targetBone": {"nodeId": "node_source_b", "path": "SourceBoneB"}
						}]
					}, {
						"shortType": "ModularAvatarMergeArmature",
						"enabled": true,
						"target": {"nodeId": "node_source_b", "path": "SourceBoneB"},
						"boneMappings": [{
							"sourceBone": {"nodeId": "node_source_b", "path": "SourceBoneB"},
							"targetBone": {"nodeId": "node_source_a", "path": "SourceBoneA"}
						}]
					}]
				}
			}),
		};
		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);
		assert!(report
			.messages
			.iter()
			.any(|m| m.contains("merge_armature_cycles=2") && m.contains("merge_armature_component_cycles=0")));
		assert!(report.diagnostics.iter().any(|diagnostic| {
			diagnostic.severity == un_avatar_core::ReportSeverity::Warning && diagnostic.text.contains("merge_armature_cycles=2")
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
	fn modular_avatar_mesh_settings_child_dont_set_blocks_parent_inheritance() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Hips".to_string()),
					source_node_id: Some("node_hips".to_string()),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Outfit".to_string()),
					source_node_id: Some("node_outfit".to_string()),
					children: vec![3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Override".to_string()),
					source_node_id: Some("node_override".to_string()),
					children: vec![4],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Mesh".to_string()),
					source_node_id: Some("node_mesh".to_string()),
					mesh: Some(0),
					skin: Some(0),
					..test_node(Vec::new())
				},
			],
			skins: vec![UnaSkin {
				joint_nodes: vec![1],
				inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array()],
				skeleton_node: None,
			}],
			roots: vec![0],
			..Default::default()
		};
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"modularAvatar": {
					"schemaVersion": "0.1-preview",
					"components": [
						{
							"shortType": "ModularAvatarMeshSettings",
							"enabled": true,
							"target": {"nodeId": "node_outfit", "path": "Outfit"},
							"fields": {
								"InheritProbeAnchor": "SetOrInherit",
								"ProbeAnchor": {"resolvedTarget": {"nodeId": "node_hips", "path": "Hips"}},
								"InheritBounds": "SetOrInherit",
								"RootBone": {"resolvedTarget": {"nodeId": "node_hips", "path": "Hips"}},
								"Bounds": {"center": [0.0, 0.0, 0.0], "extents": [1.0, 1.0, 1.0]}
							}
						},
						{
							"shortType": "ModularAvatarMeshSettings",
							"enabled": true,
							"target": {"nodeId": "node_override", "path": "Outfit/Override"},
							"fields": {
								"InheritProbeAnchor": "DontSet",
								"InheritBounds": "DontSet"
							}
						}
					]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		assert_eq!(scene.skins[0].skeleton_node, None);
		assert_eq!(scene.nodes[4].probe_anchor_node, None);
		assert_eq!(scene.nodes[4].local_bounds, None);
		assert!(!report.messages.iter().any(|m| m.contains("mesh_settings_root_bones=")));
	}

	#[test]
	fn modular_avatar_scale_adjuster_creates_proxy_and_remaps_skin_joints() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![
				UnaSceneNode {
					name: Some("Root".to_string()),
					children: vec![1, 2, 3],
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("ScaleBone".to_string()),
					source_node_id: Some("node_scale_bone".to_string()),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("OtherBone".to_string()),
					source_node_id: Some("node_other_bone".to_string()),
					..test_node(Vec::new())
				},
				UnaSceneNode {
					name: Some("Mesh".to_string()),
					source_node_id: Some("node_mesh".to_string()),
					mesh: Some(0),
					skin: Some(0),
					..test_node(Vec::new())
				},
			],
			skins: vec![UnaSkin {
				joint_nodes: vec![1, 2],
				inverse_bind_matrices: vec![Mat4::IDENTITY.to_cols_array(), Mat4::IDENTITY.to_cols_array()],
				skeleton_node: None,
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
						"shortType": "ModularAvatarScaleAdjuster",
						"enabled": true,
						"target": {"nodeId": "node_scale_bone", "path": "ScaleBone"},
						"fields": {
							"m_Scale": [2.0, 3.0, 4.0]
						}
					}]
				}
			}),
		};

		let mut report = ImportReport::default();
		apply_unavatar_modular_avatar(&mut scene, &unavatar, &mut report);

		let proxy = scene.nodes.len() - 1;
		assert_eq!(scene.nodes[1].children, vec![proxy]);
		assert_eq!(scene.nodes[proxy].name.as_deref(), Some("ScaleProxy"));
		let (scale, _, _) = Mat4::from_cols_array(&scene.nodes[proxy].transform).to_scale_rotation_translation();
		assert_vec3_near(scale, Vec3::new(2.0, 3.0, 4.0));
		assert_eq!(scene.skins[0].joint_nodes, vec![proxy, 2]);
		assert!(report
			.messages
			.iter()
			.any(|m| { m.contains("scale_adjuster_proxies=1") && m.contains("scale_adjuster_skin_joints=1") }));
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
	fn unavatar_asset_group_ownership_imports_scene_metadata() {
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"assetGroupOwnership": [{
					"groupId": "outfit:coat",
					"meshPrimitives": [{"meshIndex": 1, "primitiveIndex": 2}],
					"materials": [3],
					"images": [4, 5],
					"dynamicsSourceIds": ["physbone:coat"]
				}],
				"wardrobe": {
					"asset_group_ownership": [{
						"id": "effect:glow",
						"mesh_primitives": [{"mesh": 6, "primitive": 7}],
						"materials": [8],
						"textures": [9],
						"dynamics": ["spring:glow"]
					}]
				}
			}),
		};
		let mut scene = UnaSceneSnapshot::default();
		let mut report = ImportReport::default();

		apply_unavatar_asset_group_ownership(&mut scene, &unavatar, &mut report);

		assert_eq!(scene.asset_group_ownership.len(), 2);
		assert_eq!(scene.asset_group_ownership[0].group_id, "outfit:coat");
		assert_eq!(
			scene.asset_group_ownership[0].mesh_primitives,
			vec![UnaMeshPrimitiveKey {
				mesh_index: 1,
				primitive_index: 2,
			}]
		);
		assert_eq!(scene.asset_group_ownership[0].materials, vec![3]);
		assert_eq!(scene.asset_group_ownership[0].images, vec![4, 5]);
		assert_eq!(
			scene.asset_group_ownership[0].dynamics_source_ids,
			vec!["physbone:coat".to_string()]
		);
		assert_eq!(
			scene.asset_group_ownership[1].mesh_primitives,
			vec![UnaMeshPrimitiveKey {
				mesh_index: 6,
				primitive_index: 7,
			}]
		);
		assert_eq!(scene.asset_group_ownership_counts().groups, 2);
		assert!(report.messages.iter().any(|message| message.contains("ownership_groups=2")));
	}

	#[test]
	fn unavatar_asset_group_ownership_reports_ambiguities() {
		let unavatar = UnaUnavatarExtension {
			spec_version: "0.1-preview".to_string(),
			source: serde_json::json!({
				"wardrobe": {
					"assetGroupOwnership": [],
					"assetGroupOwnershipAmbiguities": {
						"itemLimit": 10,
						"itemCount": 2,
						"items": [{
							"sourcePath": "Hair/Body",
							"normalizedPath": "hair/body",
							"candidateGroups": ["avatar:base", "physics:hair"]
						}, {
							"sourcePath": "Eye",
							"candidateGroups": ["avatar:base"]
						}]
					}
				}
			}),
		};
		let mut scene = UnaSceneSnapshot::default();
		let mut report = ImportReport::default();

		apply_unavatar_asset_group_ownership(&mut scene, &unavatar, &mut report);

		assert!(scene.asset_group_ownership.is_empty());
		assert!(report
			.messages
			.iter()
			.any(|message| message.contains("assetGroupOwnershipAmbiguities detected")));
		let has_warning = report.diagnostics.iter().any(|diagnostic| {
			diagnostic.severity == un_avatar_core::ReportSeverity::Warning
				&& diagnostic.text.contains("assetGroupOwnershipAmbiguities detected")
		});
		assert!(has_warning);
	}

	#[test]
	fn initial_resident_image_indices_use_base_and_selected_wardrobe_groups() {
		let root = serde_json::json!({
			"images": [{}, {}, {}, {}, {}, {}],
			"extensions": {
				"UN_avatar": {
					"wardrobe": {
						"baseSet": "base",
						"sets": [{
							"id": "base",
							"assetGroups": [""],
							"operations": []
						}, {
							"id": "coat",
							"assetGroups": ["outfit:coat"],
							"operations": []
						}, {
							"id": "hat",
							"assetGroups": ["outfit:hat"],
							"operations": []
						}],
						"assetGroupOwnership": [{
							"groupId": "",
							"images": [0]
						}, {
							"groupId": "outfit:coat",
							"images": [1, 2]
						}, {
							"groupId": "outfit:hat",
							"images": [3]
						}]
					}
				}
			}
		});

		let base = initial_resident_image_indices(Some(&root), None).expect("base selection");
		assert_eq!(base, [0, 4, 5].into_iter().collect());

		let coat = initial_resident_image_indices(Some(&root), Some("coat")).expect("coat selection");
		assert_eq!(coat, [0, 1, 2, 4, 5].into_iter().collect());
	}

	#[test]
	fn initial_resident_image_indices_use_empty_string_base_set_id() {
		let root = serde_json::json!({
			"images": [{}, {}, {}, {}, {}, {}],
			"extensions": {
				"UN_avatar": {
					"wardrobe": {
						"baseSet": "",
						"sets": [{
							"id": "",
							"assetGroups": [""],
							"operations": []
						}, {
							"id": "coat",
							"assetGroups": ["outfit:coat"],
							"operations": []
						}],
						"assetGroupOwnership": [{
							"groupId": "",
							"images": [0]
						}, {
							"groupId": "outfit:coat",
							"images": [1, 2]
						}]
					}
				}
			}
		});

		let base = initial_resident_image_indices(Some(&root), None).expect("base selection");
		assert_eq!(base, [0, 3, 4, 5].into_iter().collect());

		let coat = initial_resident_image_indices(Some(&root), Some("coat")).expect("coat selection");
		assert_eq!(coat, [0, 1, 2, 3, 4, 5].into_iter().collect());
	}

	#[test]
	fn initial_resident_image_indices_limit_unowned_images_to_active_material_refs() {
		let root = serde_json::json!({
			"images": [{}, {}, {}, {}, {}],
			"textures": [
				{"source": 0},
				{"source": 1},
				{"source": 2},
				{"source": 3},
				{"source": 4}
			],
			"materials": [{
				"pbrMetallicRoughness": {
					"baseColorTexture": {"index": 1}
				},
				"extras": {
					"UN_avatar_material": {
						"main2ndTextureIndex": 2
					}
				}
			}, {
				"pbrMetallicRoughness": {
					"baseColorTexture": {"index": 4}
				}
			}],
			"meshes": [{
				"primitives": [{"material": 0}]
			}, {
				"primitives": [{"material": 1}]
			}],
			"extensions": {
				"UN_avatar": {
					"wardrobe": {
						"baseSet": "base",
						"sets": [{
							"id": "base",
							"assetGroups": [""],
							"operations": []
						}, {
							"id": "coat",
							"assetGroups": ["outfit:coat"],
							"operations": []
						}],
						"assetGroupOwnership": [{
							"groupId": "",
							"images": [0],
							"meshPrimitives": [{"mesh": 0, "primitive": 0}]
						}, {
							"groupId": "outfit:coat",
							"images": [3],
							"materials": [1],
							"meshPrimitives": [{"mesh": 1, "primitive": 0}]
						}]
					}
				}
			}
		});

		let base = initial_resident_image_indices(Some(&root), None).expect("base selection");
		assert_eq!(base, [0, 1, 2].into_iter().collect());

		let coat = initial_resident_image_indices(Some(&root), Some("coat")).expect("coat selection");
		assert_eq!(coat, [0, 1, 2, 3, 4].into_iter().collect());
	}

	#[test]
	fn initial_resident_image_indices_disable_selective_decode_without_ownership() {
		let root = serde_json::json!({
			"images": [{}, {}],
			"extensions": {
				"UN_avatar": {
					"wardrobe": {
						"sets": [{
							"id": "base",
							"assetGroups": [""],
							"operations": []
						}]
					}
				}
			}
		});

		assert!(initial_resident_image_indices(Some(&root), None).is_none());
	}

	#[test]
	fn encoded_image_bytes_are_retained_only_for_deferred_placeholders() {
		let mut image_sources = vec![
			Some(UnaImageSourceMetadata {
				encoded_bytes: Some(vec![1, 2, 3]),
				..Default::default()
			}),
			Some(UnaImageSourceMetadata {
				encoded_bytes: Some(vec![4, 5, 6]),
				..Default::default()
			}),
			Some(UnaImageSourceMetadata {
				encoded_bytes: None,
				..Default::default()
			}),
		];
		let images = vec![
			UnaImageRgba {
				width: 1,
				height: 1,
				pixel_format: UnaImagePixelFormat::R8G8B8A8,
				pixels: vec![255, 255, 255, 255],
			},
			placeholder_deferred_image(),
			placeholder_deferred_image(),
		];

		let retained = retain_encoded_bytes_for_deferred_images(&mut image_sources, &images);

		assert_eq!(retained, 1);
		assert!(image_sources[0].as_ref().unwrap().encoded_bytes.is_none());
		assert_eq!(image_sources[1].as_ref().unwrap().encoded_bytes.as_deref(), Some(&[4, 5, 6][..]));
		assert!(image_sources[2].as_ref().unwrap().encoded_bytes.is_none());
	}

	#[test]
	fn wardrobe_asset_groups_preserve_empty_base_group() {
		let mut doc = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				asset_group_ownership: vec![
					UnaSceneAssetGroupOwnership {
						group_id: "".to_string(),
						mesh_primitives: vec![UnaMeshPrimitiveKey {
							mesh_index: 0,
							primitive_index: 0,
						}],
						materials: vec![1],
						images: vec![2],
						..Default::default()
					},
					UnaSceneAssetGroupOwnership {
						group_id: "outfit:coat".to_string(),
						mesh_primitives: vec![UnaMeshPrimitiveKey {
							mesh_index: 1,
							primitive_index: 0,
						}],
						..Default::default()
					},
				],
				..Default::default()
			}),
			unavatar: Some(UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"wardrobe": {
						"baseSet": "base",
						"sets": [{
							"id": "base",
							"assetGroups": [""],
							"operations": []
						}, {
							"id": "coat",
							"assetGroups": ["outfit:coat"],
							"operations": []
						}]
					}
				}),
			}),
			..Default::default()
		};

		let base = apply_unavatar_wardrobe_set(&mut doc, "base").expect("apply base wardrobe");
		assert_eq!(base.active_asset_groups, vec!["".to_string()]);
		assert_eq!(base.scoped_active_asset_group_count, 1);
		assert!(base.scoped_missing_active_asset_groups.is_empty());
		assert_eq!(base.scoped_resident_mesh_primitive_count, 1);
		assert_eq!(base.scoped_resident_material_count, 1);
		assert_eq!(base.scoped_resident_image_count, 1);

		let coat = apply_unavatar_wardrobe_set(&mut doc, "coat").expect("apply coat wardrobe");
		assert_eq!(coat.active_asset_groups, vec!["".to_string(), "outfit:coat".to_string()]);
		assert_eq!(coat.scoped_active_asset_group_count, 2);
		assert!(coat.scoped_missing_active_asset_groups.is_empty());
		assert_eq!(coat.scoped_resident_mesh_primitive_count, 2);
	}

	#[test]
	fn wardrobe_empty_string_base_set_id_is_explicit_base() {
		let mut doc = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				asset_group_ownership: vec![
					UnaSceneAssetGroupOwnership {
						group_id: "".to_string(),
						mesh_primitives: vec![UnaMeshPrimitiveKey {
							mesh_index: 0,
							primitive_index: 0,
						}],
						materials: vec![1],
						images: vec![2],
						..Default::default()
					},
					UnaSceneAssetGroupOwnership {
						group_id: "outfit:coat".to_string(),
						mesh_primitives: vec![UnaMeshPrimitiveKey {
							mesh_index: 1,
							primitive_index: 0,
						}],
						..Default::default()
					},
				],
				..Default::default()
			}),
			unavatar: Some(UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"wardrobe": {
						"baseSet": "",
						"sets": [{
							"id": "",
							"assetGroups": [""],
							"operations": []
						}, {
							"id": "coat",
							"assetGroups": ["outfit:coat"],
							"operations": []
						}]
					}
				}),
			}),
			..Default::default()
		};

		let base = unavatar_base_wardrobe_set(doc.unavatar.as_ref().unwrap()).expect("base wardrobe");
		assert_eq!(base.0, "");

		let base = apply_unavatar_wardrobe_set(&mut doc, "").expect("apply empty base wardrobe");
		assert_eq!(base.active_asset_groups, vec!["".to_string()]);
		assert_eq!(base.scoped_active_asset_group_count, 1);
		assert!(base.scoped_missing_active_asset_groups.is_empty());
		assert_eq!(base.scoped_resident_mesh_primitive_count, 1);
		assert_eq!(base.scoped_resident_material_count, 1);
		assert_eq!(base.scoped_resident_image_count, 1);

		let coat = apply_unavatar_wardrobe_set(&mut doc, "coat").expect("apply coat wardrobe");
		assert_eq!(coat.active_asset_groups, vec!["".to_string(), "outfit:coat".to_string()]);
		assert_eq!(coat.scoped_active_asset_group_count, 2);
		assert!(coat.scoped_missing_active_asset_groups.is_empty());
		assert_eq!(coat.scoped_resident_mesh_primitive_count, 2);
	}

	#[test]
	fn wardrobe_empty_base_asset_groups_normalize_to_empty_base_group() {
		let mut doc = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				asset_group_ownership: vec![UnaSceneAssetGroupOwnership {
					group_id: "outfit:coat".to_string(),
					mesh_primitives: vec![UnaMeshPrimitiveKey {
						mesh_index: 1,
						primitive_index: 0,
					}],
					..Default::default()
				}],
				..Default::default()
			}),
			unavatar: Some(UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"wardrobe": {
						"baseSet": "base",
						"sets": [{
							"id": "base",
							"assetGroups": [],
							"operations": []
						}, {
							"id": "coat",
							"assetGroups": ["outfit:coat"],
							"operations": []
						}]
					}
				}),
			}),
			..Default::default()
		};

		let base = apply_unavatar_wardrobe_set(&mut doc, "base").expect("apply base wardrobe");
		assert_eq!(base.active_asset_groups, vec!["".to_string()]);
		assert_eq!(base.scoped_active_asset_group_count, 1);
		assert!(base.scoped_missing_active_asset_groups.is_empty());

		let coat = apply_unavatar_wardrobe_set(&mut doc, "coat").expect("apply coat wardrobe");
		assert_eq!(coat.active_asset_groups, vec!["".to_string(), "outfit:coat".to_string()]);
		assert_eq!(coat.scoped_active_asset_group_count, 2);
		assert!(coat.scoped_missing_active_asset_groups.is_empty());
		assert_eq!(coat.scoped_resident_mesh_primitive_count, 1);
	}

	#[test]
	fn wardrobe_reapplies_visible_shape_changer_sets_after_base_blendshape_reset() {
		let mut doc = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				nodes: vec![
					UnaSceneNode {
						name: Some("Root".to_string()),
						children: vec![1, 2],
						..test_node(Vec::new())
					},
					UnaSceneNode {
						name: Some("Shirts".to_string()),
						source_node_id: Some("node_shirts".to_string()),
						resolved_node_id: None,
						mesh: Some(0),
						..test_node(Vec::new())
					},
					UnaSceneNode {
						name: Some("Skirt".to_string()),
						source_node_id: Some("node_skirt".to_string()),
						resolved_node_id: None,
						..test_node(Vec::new())
					},
				],
				roots: vec![0],
				meshes: vec![vec![test_morph_primitive("Skirt_ON", 0.0)]],
				..Default::default()
			}),
			unavatar: Some(UnaUnavatarExtension {
				spec_version: "0.1-preview".to_string(),
				source: serde_json::json!({
					"modularAvatar": {
						"components": [{
							"shortType": "ModularAvatarShapeChanger",
							"enabled": true,
							"target": {"nodeId": "node_skirt", "path": "Root/Skirt"},
							"fields": {
								"m_shapes": ["Root/Shirts Skirt_ON Set 100"]
							}
						}]
					},
					"wardrobe": {
						"baseSet": "base",
						"sets": [{
							"id": "base",
							"assetGroups": [""],
							"operations": [{
								"type": "nodeEnabled",
								"target": {"nodeId": "node_skirt", "path": "Root/Skirt"},
								"visible": false
							}, {
								"type": "blendShapeWeight",
								"target": {"nodeId": "node_shirts", "path": "Root/Shirts"},
								"name": "Skirt_ON",
								"value": 0
							}]
						}, {
							"id": "coat",
							"assetGroups": ["outfit:coat"],
							"operations": [{
								"type": "nodeEnabled",
								"target": {"nodeId": "node_skirt", "path": "Root/Skirt"},
								"visible": true
							}]
						}]
					}
				}),
			}),
			..Default::default()
		};

		let base = apply_unavatar_wardrobe_set(&mut doc, "base").expect("apply base wardrobe");
		assert_eq!(base.blendshape_applied, 1);
		assert_eq!(blend_shape_weight(doc.scene.as_ref().unwrap(), 1, "Skirt_ON"), Some(0.0));

		let coat = apply_unavatar_wardrobe_set(&mut doc, "coat").expect("apply coat wardrobe");
		assert_eq!(coat.blendshape_applied, 1);
		assert_eq!(blend_shape_weight(doc.scene.as_ref().unwrap(), 1, "Skirt_ON"), Some(1.0));
	}

	#[test]
	fn wardrobe_dynamics_enable_updates_runtime_group() {
		let mut doc = UnaDocument {
			scene: Some(UnaSceneSnapshot {
				asset_group_ownership: vec![
					UnaSceneAssetGroupOwnership {
						group_id: "avatar:base".to_string(),
						mesh_primitives: vec![UnaMeshPrimitiveKey {
							mesh_index: 0,
							primitive_index: 0,
						}],
						..Default::default()
					},
					UnaSceneAssetGroupOwnership {
						group_id: "outfit:hair".to_string(),
						mesh_primitives: vec![UnaMeshPrimitiveKey {
							mesh_index: 1,
							primitive_index: 0,
						}],
						materials: vec![2],
						images: vec![3],
						dynamics_source_ids: vec!["physbone:hair".to_string()],
					},
				],
				..Default::default()
			}),
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
				..Default::default()
			}),
			..Default::default()
		};

		let applied = apply_unavatar_wardrobe_set(&mut doc, "no_hair_physics").expect("apply wardrobe");
		assert_eq!(
			applied.active_asset_groups,
			vec!["avatar:base".to_string(), "outfit:hair".to_string(), "physics:hair".to_string()]
		);
		assert_eq!(applied.scoped_active_asset_group_count, 2);
		assert_eq!(applied.scoped_missing_active_asset_groups, vec!["physics:hair".to_string()]);
		assert_eq!(applied.scoped_resident_mesh_primitive_count, 2);
		assert_eq!(applied.scoped_resident_material_count, 1);
		assert_eq!(applied.scoped_resident_image_count, 1);
		assert_eq!(applied.scoped_resident_dynamics_count, 1);
		assert_eq!(applied.dynamics_applied, 1);
		assert_eq!(applied.dynamics_missing, 1);
		assert_eq!(applied.missing_dynamics_ids, vec!["physbone:missing"]);
		let dynamics = doc.runtime_model().dynamics();
		assert!(!dynamics.group_enabled(&dynamics.groups()[0]));
		assert!(doc.spring_bones.as_ref().unwrap().groups[0].enabled);
		assert_eq!(doc.runtime_model().active_wardrobe_set(), Some("no_hair_physics"));
		assert_eq!(
			doc.runtime_model().active_asset_groups(),
			&["avatar:base".to_string(), "outfit:hair".to_string(), "physics:hair".to_string()]
		);

		let applied = apply_unavatar_wardrobe_set(&mut doc, "base").expect("apply base wardrobe");
		assert_eq!(applied.active_asset_groups, vec!["avatar:base".to_string()]);
		assert_eq!(applied.scoped_active_asset_group_count, 1);
		assert!(applied.scoped_missing_active_asset_groups.is_empty());
		assert_eq!(applied.scoped_resident_mesh_primitive_count, 1);
		assert_eq!(applied.scoped_resident_material_count, 0);
		assert_eq!(applied.scoped_resident_image_count, 0);
		assert_eq!(applied.scoped_resident_dynamics_count, 0);
		assert_eq!(applied.dynamics_applied, 0);
		assert_eq!(applied.dynamics_missing, 0);
		let dynamics = doc.runtime_model().dynamics();
		assert!(dynamics.group_enabled(&dynamics.groups()[0]));
		assert_eq!(doc.runtime_model().active_wardrobe_set(), Some("base"));
		assert_eq!(doc.runtime_model().active_asset_groups(), &["avatar:base".to_string()]);
	}
}
