//! Texture processing, mip generation, compression, and texture cache helpers.

use std::{
	borrow::Cow,
	fs,
	io::{BufReader, BufWriter, Read, Write},
	path::{Path, PathBuf},
	sync::atomic::{AtomicBool, Ordering},
	thread,
	time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use image::imageops::FilterType;
use pic_scale::{ImageSize, ImageStore, ImageStoreMut, ResamplingFunction, Scaler, ThreadingPolicy};
use serde::Serialize;
use un_avatar_core::{UnaImagePixelFormat, UnaImageRgba, UnaImageSourceMetadata};

use crate::{
	BlockCompressionEncoder, TextureCompressionAdvancedOptions, TextureCompressionMode, TextureCompressionPreference, TextureMipmapFilter,
};

pub(crate) struct ProcessedTexture {
	pub(crate) width: u32,
	pub(crate) height: u32,
	pub(crate) mips: Vec<(u32, u32, Vec<u8>)>,
}

#[derive(Clone, Copy)]
pub(crate) struct TextureCacheEvent {
	pub(crate) hit: bool,
	pub(crate) miss: bool,
	pub(crate) write: bool,
	pub(crate) read_elapsed: Duration,
	pub(crate) read_bytes: u64,
}

impl TextureCacheEvent {
	pub(crate) const DISABLED: Self = Self {
		hit: false,
		miss: false,
		write: false,
		read_elapsed: Duration::ZERO,
		read_bytes: 0,
	};
}

#[derive(Clone)]
pub(crate) struct CompressedTextureCacheLookup {
	pub(crate) key: u64,
	pub(crate) kind: TextureUploadKind,
	pub(crate) path: PathBuf,
	pub(crate) processed_width: u32,
	pub(crate) processed_height: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TextureRole {
	Face,
	Eyes,
	Clothing,
	Normal,
	Occlusion,
	Emissive,
	#[default]
	GenericColor,
	Data,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextureUploadKind {
	Rgba,
	Bc1Srgb,
	Bc5Unorm,
	Bc7Unorm,
	Bc7Srgb,
}

impl TextureUploadKind {
	pub(crate) fn cache_tag(self) -> u8 {
		match self {
			Self::Rgba => 0,
			Self::Bc1Srgb => 1,
			Self::Bc5Unorm => 2,
			Self::Bc7Unorm => 3,
			Self::Bc7Srgb => 4,
		}
	}

	pub(crate) fn from_cache_tag(tag: u8) -> Option<Self> {
		match tag {
			0 => Some(Self::Rgba),
			1 => Some(Self::Bc1Srgb),
			2 => Some(Self::Bc5Unorm),
			3 => Some(Self::Bc7Unorm),
			4 => Some(Self::Bc7Srgb),
			_ => None,
		}
	}

	pub(crate) fn is_compressed(self) -> bool {
		!matches!(self, Self::Rgba)
	}

	pub(crate) fn block_bytes(self) -> u32 {
		match self {
			Self::Rgba => 4,
			Self::Bc1Srgb => 8,
			Self::Bc5Unorm | Self::Bc7Unorm | Self::Bc7Srgb => 16,
		}
	}

	fn compression_variant(self, rgba: &[u8]) -> Option<block_compression::CompressionVariant> {
		match self {
			Self::Rgba => None,
			Self::Bc1Srgb => Some(block_compression::CompressionVariant::BC1),
			Self::Bc5Unorm => Some(block_compression::CompressionVariant::BC5),
			Self::Bc7Unorm | Self::Bc7Srgb => {
				let settings = if rgba_has_translucent_alpha(rgba) {
					block_compression::BC7Settings::alpha_basic()
				} else {
					block_compression::BC7Settings::opaque_basic()
				};
				Some(block_compression::CompressionVariant::BC7(settings))
			}
		}
	}
}

#[derive(Clone)]
pub(crate) struct TextureUploadMip {
	// For compressed formats, these are block-aligned upload dimensions, not original image dimensions.
	// Returning logical dimensions here regressed non-4-aligned BCn textures into startup hangs.
	pub(crate) width: u32,
	pub(crate) height: u32,
	pub(crate) data: Vec<u8>,
}

#[derive(Clone)]
pub(crate) struct TextureUploadPayload {
	pub(crate) kind: TextureUploadKind,
	pub(crate) mips: Vec<TextureUploadMip>,
}

#[derive(Clone)]
pub(crate) struct SourceTextureUpload {
	pub(crate) format: wgpu::TextureFormat,
	pub(crate) width: u32,
	pub(crate) height: u32,
	pub(crate) bytes_per_row: u32,
	pub(crate) data: Vec<u8>,
}

pub(crate) struct GpuTextureCompressionContext {
	device: wgpu::Device,
	queue: wgpu::Queue,
	compressor: block_compression::GpuBlockCompressor,
}

struct PaddedRgba<'a> {
	width: u32,
	height: u32,
	data: Cow<'a, [u8]>,
}

type TextureUploadProgress<'a> = &'a mut dyn FnMut(u32, u32, u32, u32, TextureUploadKind);

impl TextureUploadPayload {
	pub(crate) fn byte_len(&self) -> u64 {
		self.mips.iter().map(|mip| mip.data.len() as u64).sum()
	}
}

pub(crate) fn source_texture_upload(image: &UnaImageRgba) -> Option<SourceTextureUpload> {
	let width = image.width.max(1);
	let height = image.height.max(1);
	let pixels = image.pixels.as_slice();
	match image.pixel_format {
		UnaImagePixelFormat::R16 => rgba16_unorm_upload(pixels, width, height, 1),
		UnaImagePixelFormat::R16G16 => rgba16_unorm_upload(pixels, width, height, 2),
		UnaImagePixelFormat::R16G16B16 => rgba16_unorm_upload(pixels, width, height, 3),
		UnaImagePixelFormat::R16G16B16A16 => rgba16_unorm_upload(pixels, width, height, 4),
		UnaImagePixelFormat::R16G16B16Float => rgba16_float_upload_from_half(pixels, width, height, 3),
		UnaImagePixelFormat::R16G16B16A16Float => rgba16_float_upload_from_half(pixels, width, height, 4),
		UnaImagePixelFormat::R32G32B32Float => rgba16_float_upload(pixels, width, height, 3),
		UnaImagePixelFormat::R32G32B32A32Float => rgba16_float_upload(pixels, width, height, 4),
		UnaImagePixelFormat::R8 | UnaImagePixelFormat::R8G8 | UnaImagePixelFormat::R8G8B8 | UnaImagePixelFormat::R8G8B8A8 => None,
	}
}

fn rgba16_unorm_upload(pixels: &[u8], width: u32, height: u32, channels: usize) -> Option<SourceTextureUpload> {
	let stride = channels.checked_mul(2)?;
	if stride == 0 || pixels.len() % stride != 0 {
		return None;
	}
	let expected_pixels = width.checked_mul(height)? as usize;
	if pixels.len() / stride != expected_pixels {
		return None;
	}
	let mut data = Vec::with_capacity(expected_pixels * 8);
	for pixel in pixels.chunks_exact(stride) {
		let channel = |index: usize| -> u16 {
			if index >= channels {
				return if index == 3 { u16::MAX } else { 0 };
			}
			let offset = index * 2;
			u16::from_ne_bytes([pixel[offset], pixel[offset + 1]])
		};
		let r = channel(0);
		let g = if channels == 1 { r } else { channel(1) };
		let b = if channels == 1 {
			r
		} else if channels == 2 {
			0
		} else {
			channel(2)
		};
		let a = if channels >= 4 { channel(3) } else { u16::MAX };
		for value in [r, g, b, a] {
			data.extend_from_slice(&value.to_ne_bytes());
		}
	}
	Some(SourceTextureUpload {
		format: wgpu::TextureFormat::Rgba16Unorm,
		width,
		height,
		bytes_per_row: width * 8,
		data,
	})
}

fn rgba16_float_upload_from_half(pixels: &[u8], width: u32, height: u32, channels: usize) -> Option<SourceTextureUpload> {
	let stride = channels.checked_mul(2)?;
	if stride == 0 || pixels.len() % stride != 0 {
		return None;
	}
	let expected_pixels = width.checked_mul(height)? as usize;
	if pixels.len() / stride != expected_pixels {
		return None;
	}
	let mut data = Vec::with_capacity(expected_pixels * 8);
	let one = half::f16::ONE.to_bits().to_le_bytes();
	for pixel in pixels.chunks_exact(stride) {
		for index in 0..4 {
			if index < channels {
				let offset = index * 2;
				data.extend_from_slice(&pixel[offset..offset + 2]);
			} else if index == 3 {
				data.extend_from_slice(&one);
			} else {
				data.extend_from_slice(&[0, 0]);
			}
		}
	}
	Some(SourceTextureUpload {
		format: wgpu::TextureFormat::Rgba16Float,
		width,
		height,
		bytes_per_row: width * 8,
		data,
	})
}

fn rgba16_float_upload(pixels: &[u8], width: u32, height: u32, channels: usize) -> Option<SourceTextureUpload> {
	let stride = channels.checked_mul(4)?;
	if stride == 0 || pixels.len() % stride != 0 {
		return None;
	}
	let expected_pixels = width.checked_mul(height)? as usize;
	if pixels.len() / stride != expected_pixels {
		return None;
	}
	let mut data = Vec::with_capacity(expected_pixels * 8);
	for pixel in pixels.chunks_exact(stride) {
		let channel = |index: usize| -> f32 {
			if index >= channels {
				return if index == 3 { 1.0 } else { 0.0 };
			}
			let offset = index * 4;
			f32::from_ne_bytes([pixel[offset], pixel[offset + 1], pixel[offset + 2], pixel[offset + 3]])
		};
		let r = channel(0);
		let g = channel(1);
		let b = if channels == 3 { channel(2) } else { channel(2) };
		let a = if channels >= 4 { channel(3) } else { 1.0 };
		for value in [r, g, b, a] {
			data.extend_from_slice(&half::f16::from_f32(value).to_bits().to_ne_bytes());
		}
	}
	Some(SourceTextureUpload {
		format: wgpu::TextureFormat::Rgba16Float,
		width,
		height,
		bytes_per_row: width * 8,
		data,
	})
}

pub(crate) fn create_vulkan_gpu_texture_compression_context() -> Result<GpuTextureCompressionContext, String> {
	let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
	instance_descriptor.backends = wgpu::Backends::VULKAN;
	let instance = wgpu::Instance::new(instance_descriptor);
	let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
		power_preference: wgpu::PowerPreference::HighPerformance,
		compatible_surface: None,
		force_fallback_adapter: false,
	}))
	.map_err(|e| format!("texture compression vulkan adapter: {e}"))?;
	let limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
	let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
		label: Some("un-avatar-texture-compression"),
		required_features: wgpu::Features::empty(),
		required_limits: limits,
		memory_hints: Default::default(),
		..Default::default()
	}))
	.map_err(|e| format!("texture compression vulkan device: {e}"))?;
	let compressor = block_compression::GpuBlockCompressor::new(device.clone(), queue.clone());
	Ok(GpuTextureCompressionContext { device, queue, compressor })
}

pub(crate) fn mip_level_count(width: u32, height: u32) -> u32 {
	let max_dim = width.max(height).max(1);
	u32::BITS - max_dim.leading_zeros()
}

fn resized_dimensions_to_max_dimension(width: u32, height: u32, max_dimension: Option<u32>) -> (u32, u32) {
	let width = width.max(1);
	let height = height.max(1);
	let Some(max_dimension) = max_dimension.map(|v| v.max(1)) else {
		return (width, height);
	};
	if width <= max_dimension && height <= max_dimension {
		return (width, height);
	}
	let long_edge = width.max(height) as u64;
	let new_width = (((width as u64) * (max_dimension as u64) + long_edge / 2) / long_edge).max(1) as u32;
	let new_height = (((height as u64) * (max_dimension as u64) + long_edge / 2) / long_edge).max(1) as u32;
	(new_width, new_height)
}

pub(crate) fn estimated_processed_mip_count(width: u32, height: u32, max_dimension: Option<u32>, role: TextureRole) -> u32 {
	let (width, height) = resized_dimensions_to_max_dimension(width, height, max_dimension);
	if texture_role_uses_mips(role) {
		mip_level_count(width, height)
	} else {
		1
	}
}

pub(crate) fn normalized_rgba_base(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
	let expected = (width as usize) * (height as usize) * 4;
	if rgba.len() == expected {
		rgba.to_vec()
	} else {
		let mut base = vec![0; expected];
		let copy_len = rgba.len().min(expected);
		base[..copy_len].copy_from_slice(&rgba[..copy_len]);
		base
	}
}

fn has_low_alpha_pixels(rgba: &[u8]) -> bool {
	rgba.chunks_exact(4).any(|px| px[3] < 250)
}

fn alpha_safe_rgba_base(rgba: &[u8], width: u32, height: u32, role: TextureRole) -> Vec<u8> {
	let mut base = normalized_rgba_base(rgba, width, height);
	if !texture_role_uses_alpha_weighted_rgb_mips(role) || !has_low_alpha_pixels(&base) {
		return base;
	}
	bleed_transparent_rgb(&mut base, width.max(1), height.max(1));
	base
}

fn bleed_transparent_rgb(rgba: &mut [u8], width: u32, height: u32) {
	let expected = (width as usize).saturating_mul(height as usize).saturating_mul(4);
	if rgba.len() != expected || width == 0 || height == 0 {
		return;
	}
	let mut scratch = rgba.to_vec();
	for _ in 0..8 {
		let changed = AtomicBool::new(false);
		scratch.copy_from_slice(rgba);
		fill_bleed_transparent_rgb(rgba, width, height, &mut scratch, &changed);
		if !changed.load(Ordering::Relaxed) {
			break;
		}
		rgba.copy_from_slice(&scratch);
	}
}

fn fill_bleed_transparent_rgb(src: &[u8], width: u32, height: u32, dst: &mut [u8], changed: &AtomicBool) {
	let row_bytes = (width as usize) * 4;
	let stripes = parallel_row_stripes(height, PARALLEL_TEXTURE_MIN_ROWS_PER_WORKER);
	if stripes.len() <= 1 {
		fill_bleed_transparent_rgb_rows(src, width, height, 0, dst, changed);
		return;
	}
	let mut remaining = dst;
	thread::scope(|scope| {
		for stripe in stripes {
			let chunk_len = (stripe.len as usize) * row_bytes;
			let (chunk, rest) = remaining.split_at_mut(chunk_len);
			remaining = rest;
			scope.spawn(move || fill_bleed_transparent_rgb_rows(src, width, height, stripe.start, chunk, changed));
		}
	});
}

fn fill_bleed_transparent_rgb_rows(src: &[u8], width: u32, height: u32, start_y: u32, dst: &mut [u8], changed: &AtomicBool) {
	let row_bytes = (width as usize) * 4;
	for (row_index, row) in dst.chunks_exact_mut(row_bytes).enumerate() {
		let y = start_y + row_index as u32;
		for x in 0..width {
			let dst_i = (x as usize) * 4;
			let src_i = ((y * width + x) as usize) * 4;
			if src[src_i + 3] >= 250 {
				continue;
			}
			let mut rgb = [0u32; 3];
			let mut count = 0u32;
			let y0 = y.saturating_sub(1);
			let y1 = (y + 1).min(height - 1);
			let x0 = x.saturating_sub(1);
			let x1 = (x + 1).min(width - 1);
			for ny in y0..=y1 {
				for nx in x0..=x1 {
					if nx == x && ny == y {
						continue;
					}
					let ni = ((ny * width + nx) as usize) * 4;
					if src[ni + 3] < 250 {
						continue;
					}
					rgb[0] += u32::from(src[ni]);
					rgb[1] += u32::from(src[ni + 1]);
					rgb[2] += u32::from(src[ni + 2]);
					count += 1;
				}
			}
			if count > 0 {
				row[dst_i] = (rgb[0] / count) as u8;
				row[dst_i + 1] = (rgb[1] / count) as u8;
				row[dst_i + 2] = (rgb[2] / count) as u8;
				changed.store(true, Ordering::Relaxed);
			}
		}
	}
}

#[derive(Clone, Copy)]
struct RowStripe {
	start: u32,
	len: u32,
}

fn parallel_row_worker_count(row_count: u32, min_rows_per_worker: u32) -> u32 {
	let logical = thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(1).max(1);
	let row_limited = (row_count / min_rows_per_worker.max(1)).max(1);
	logical.min(row_limited).min(row_count.max(1))
}

fn parallel_row_stripes(row_count: u32, min_rows_per_worker: u32) -> Vec<RowStripe> {
	let worker_count = parallel_row_worker_count(row_count, min_rows_per_worker);
	if worker_count <= 1 {
		return vec![RowStripe { start: 0, len: row_count }];
	}
	let rows_per_worker = row_count.div_ceil(worker_count);
	let mut stripes = Vec::with_capacity(worker_count as usize);
	let mut row = 0;
	while row < row_count {
		let rows = rows_per_worker.min(row_count - row);
		stripes.push(RowStripe { start: row, len: rows });
		row += rows;
	}
	stripes
}

fn downsample_rgba_2x2(src: &[u8], width: u32, height: u32) -> (u32, u32, Vec<u8>) {
	let dst_width = (width / 2).max(1);
	let dst_height = (height / 2).max(1);
	let mut dst = vec![0; (dst_width as usize) * (dst_height as usize) * 4];
	fill_downsample_rgba_2x2(src, width, height, dst_width, dst_height, &mut dst);
	(dst_width, dst_height, dst)
}

fn fill_downsample_rgba_2x2(src: &[u8], width: u32, height: u32, dst_width: u32, dst_height: u32, dst: &mut [u8]) {
	let row_bytes = (dst_width as usize) * 4;
	let stripes = parallel_row_stripes(dst_height, PARALLEL_TEXTURE_MIN_ROWS_PER_WORKER);
	if stripes.len() <= 1 {
		fill_downsample_rgba_2x2_rows(src, width, height, dst_width, 0, dst);
		return;
	}
	let mut remaining = dst;
	thread::scope(|scope| {
		for stripe in stripes {
			let chunk_len = (stripe.len as usize) * row_bytes;
			let (chunk, rest) = remaining.split_at_mut(chunk_len);
			remaining = rest;
			scope.spawn(move || fill_downsample_rgba_2x2_rows(src, width, height, dst_width, stripe.start, chunk));
		}
	});
}

fn fill_downsample_rgba_2x2_rows(src: &[u8], width: u32, height: u32, dst_width: u32, start_y: u32, dst: &mut [u8]) {
	let row_bytes = (dst_width as usize) * 4;
	for (row_index, row) in dst.chunks_exact_mut(row_bytes).enumerate() {
		let y = start_y + row_index as u32;
		for x in 0..dst_width {
			let mut acc = [0u32; 4];
			let mut count = 0u32;
			for oy in 0..2 {
				for ox in 0..2 {
					let sx = (x * 2 + ox).min(width - 1);
					let sy = (y * 2 + oy).min(height - 1);
					let si = ((sy * width + sx) as usize) * 4;
					acc[0] += src[si] as u32;
					acc[1] += src[si + 1] as u32;
					acc[2] += src[si + 2] as u32;
					acc[3] += src[si + 3] as u32;
					count += 1;
				}
			}
			let di = (x as usize) * 4;
			row[di] = (acc[0] / count) as u8;
			row[di + 1] = (acc[1] / count) as u8;
			row[di + 2] = (acc[2] / count) as u8;
			row[di + 3] = (acc[3] / count) as u8;
		}
	}
}

fn downsample_rgba_2x2_alpha_weighted_rgb(src: &[u8], width: u32, height: u32) -> (u32, u32, Vec<u8>) {
	let dst_width = (width / 2).max(1);
	let dst_height = (height / 2).max(1);
	let mut dst = vec![0; (dst_width as usize) * (dst_height as usize) * 4];
	fill_downsample_rgba_2x2_alpha_weighted_rgb(src, width, height, dst_width, dst_height, &mut dst);
	(dst_width, dst_height, dst)
}

fn fill_downsample_rgba_2x2_alpha_weighted_rgb(src: &[u8], width: u32, height: u32, dst_width: u32, dst_height: u32, dst: &mut [u8]) {
	let row_bytes = (dst_width as usize) * 4;
	let stripes = parallel_row_stripes(dst_height, PARALLEL_TEXTURE_MIN_ROWS_PER_WORKER);
	if stripes.len() <= 1 {
		fill_downsample_rgba_2x2_alpha_weighted_rgb_rows(src, width, height, dst_width, 0, dst);
		return;
	}
	let mut remaining = dst;
	thread::scope(|scope| {
		for stripe in stripes {
			let chunk_len = (stripe.len as usize) * row_bytes;
			let (chunk, rest) = remaining.split_at_mut(chunk_len);
			remaining = rest;
			scope.spawn(move || fill_downsample_rgba_2x2_alpha_weighted_rgb_rows(src, width, height, dst_width, stripe.start, chunk));
		}
	});
}

fn fill_downsample_rgba_2x2_alpha_weighted_rgb_rows(src: &[u8], width: u32, height: u32, dst_width: u32, start_y: u32, dst: &mut [u8]) {
	let row_bytes = (dst_width as usize) * 4;
	for (row_index, row) in dst.chunks_exact_mut(row_bytes).enumerate() {
		let y = start_y + row_index as u32;
		for x in 0..dst_width {
			let mut rgb_weighted = [0u32; 3];
			let mut alpha_sum = 0u32;
			let mut alpha_acc = 0u32;
			let mut count = 0u32;
			let mut fallback_rgb = [0u8; 3];
			let mut fallback_alpha = 0u8;
			for oy in 0..2 {
				for ox in 0..2 {
					let sx = (x * 2 + ox).min(width - 1);
					let sy = (y * 2 + oy).min(height - 1);
					let si = ((sy * width + sx) as usize) * 4;
					let a = src[si + 3];
					if a > fallback_alpha {
						fallback_alpha = a;
						fallback_rgb = [src[si], src[si + 1], src[si + 2]];
					}
					rgb_weighted[0] += u32::from(src[si]) * u32::from(a);
					rgb_weighted[1] += u32::from(src[si + 1]) * u32::from(a);
					rgb_weighted[2] += u32::from(src[si + 2]) * u32::from(a);
					alpha_sum += u32::from(a);
					alpha_acc += u32::from(a);
					count += 1;
				}
			}
			let di = (x as usize) * 4;
			if alpha_sum > 0 {
				row[di] = (rgb_weighted[0] / alpha_sum) as u8;
				row[di + 1] = (rgb_weighted[1] / alpha_sum) as u8;
				row[di + 2] = (rgb_weighted[2] / alpha_sum) as u8;
			} else {
				row[di] = fallback_rgb[0];
				row[di + 1] = fallback_rgb[1];
				row[di + 2] = fallback_rgb[2];
			}
			row[di + 3] = (alpha_acc / count) as u8;
		}
	}
}

fn texture_role_uses_alpha_weighted_rgb_mips(role: TextureRole) -> bool {
	matches!(
		role,
		TextureRole::Face | TextureRole::Eyes | TextureRole::Clothing | TextureRole::Emissive | TextureRole::GenericColor
	)
}

fn texture_role_needs_alpha_weighted_rgb_mips(role: TextureRole, rgba: &[u8]) -> bool {
	texture_role_uses_alpha_weighted_rgb_mips(role) && rgba_has_translucent_alpha(rgba)
}

fn resize_rgba_pic_scale(
	rgba: &[u8],
	width: u32,
	height: u32,
	dst_width: u32,
	dst_height: u32,
	filter: ResamplingFunction,
	alpha_weighted_rgb: bool,
) -> (u32, u32, Vec<u8>) {
	let width = width.max(1) as usize;
	let height = height.max(1) as usize;
	let dst_width = dst_width.max(1) as usize;
	let dst_height = dst_height.max(1) as usize;
	let src_store = ImageStore::<u8, 4>::from_slice(rgba, width, height).expect("normalized rgba source dimensions");
	let mut dst_store = ImageStoreMut::<u8, 4>::alloc(dst_width, dst_height);
	let scaler = Scaler::new(filter).set_threading_policy(ThreadingPolicy::Adaptive);
	let plan = scaler
		.plan_rgba_resampling(
			ImageSize::new(width, height),
			ImageSize::new(dst_width, dst_height),
			alpha_weighted_rgb,
		)
		.expect("pic-scale rgba resampling plan");
	plan.resample(&src_store, &mut dst_store).expect("pic-scale rgba resampling");
	(dst_width as u32, dst_height as u32, dst_store.as_bytes().to_vec())
}

fn downsample_rgba_mip(
	rgba: &[u8],
	width: u32,
	height: u32,
	mipmap_filter: TextureMipmapFilter,
	alpha_weighted_rgb: bool,
) -> (u32, u32, Vec<u8>) {
	let dst_width = (width / 2).max(1);
	let dst_height = (height / 2).max(1);
	match mipmap_filter {
		TextureMipmapFilter::Box2x2 if alpha_weighted_rgb => downsample_rgba_2x2_alpha_weighted_rgb(rgba, width, height),
		TextureMipmapFilter::Box2x2 => downsample_rgba_2x2(rgba, width, height),
		TextureMipmapFilter::Bilinear => resize_rgba_pic_scale(
			rgba,
			width,
			height,
			dst_width,
			dst_height,
			ResamplingFunction::Bilinear,
			alpha_weighted_rgb,
		),
		TextureMipmapFilter::Bicubic => resize_rgba_pic_scale(
			rgba,
			width,
			height,
			dst_width,
			dst_height,
			ResamplingFunction::Bicubic,
			alpha_weighted_rgb,
		),
		TextureMipmapFilter::CatmullRom => resize_rgba_pic_scale(
			rgba,
			width,
			height,
			dst_width,
			dst_height,
			ResamplingFunction::CatmullRom,
			alpha_weighted_rgb,
		),
		TextureMipmapFilter::Lanczos3 => resize_rgba_pic_scale(
			rgba,
			width,
			height,
			dst_width,
			dst_height,
			ResamplingFunction::Lanczos3,
			alpha_weighted_rgb,
		),
		TextureMipmapFilter::Mitchell => resize_rgba_pic_scale(
			rgba,
			width,
			height,
			dst_width,
			dst_height,
			ResamplingFunction::MitchellNetravalli,
			alpha_weighted_rgb,
		),
	}
}

#[cfg(test)]
fn build_rgba_mips_with_mode(
	rgba: &[u8],
	width: u32,
	height: u32,
	mipmap_filter: TextureMipmapFilter,
	alpha_weighted_rgb: bool,
) -> Vec<(u32, u32, Vec<u8>)> {
	let mip_data = normalized_rgba_base(rgba, width.max(1), height.max(1));
	build_rgba_mips_from_base(mip_data, width, height, mipmap_filter, alpha_weighted_rgb)
}

fn build_rgba_mips_from_base(
	mip_data: Vec<u8>,
	width: u32,
	height: u32,
	mipmap_filter: TextureMipmapFilter,
	alpha_weighted_rgb: bool,
) -> Vec<(u32, u32, Vec<u8>)> {
	let mut mips = Vec::with_capacity(mip_level_count(width, height) as usize);
	let mut mip_width = width.max(1);
	let mut mip_height = height.max(1);
	let mut mip_data = mip_data;
	loop {
		let next_mip = if mip_width == 1 && mip_height == 1 {
			None
		} else {
			Some(downsample_rgba_mip(
				&mip_data,
				mip_width,
				mip_height,
				mipmap_filter,
				alpha_weighted_rgb,
			))
		};
		mips.push((mip_width, mip_height, mip_data));
		match next_mip {
			Some((next_width, next_height, next_data)) => {
				mip_width = next_width;
				mip_height = next_height;
				mip_data = next_data;
			}
			None => break,
		}
	}
	mips
}

fn renormalize_normal_mips(mips: &mut [(u32, u32, Vec<u8>)]) {
	for (_, _, mip) in mips.iter_mut().skip(1) {
		for pixel in mip.chunks_exact_mut(4) {
			let x = f32::from(pixel[0]) / 255.0 * 2.0 - 1.0;
			let y = f32::from(pixel[1]) / 255.0 * 2.0 - 1.0;
			let z = f32::from(pixel[2]) / 255.0 * 2.0 - 1.0;
			let len = (x * x + y * y + z * z).sqrt();
			let (nx, ny, nz) = if len > 0.01 { (x / len, y / len, z / len) } else { (0.0, 0.0, 1.0) };
			pixel[0] = ((nx * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
			pixel[1] = ((ny * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
			pixel[2] = ((nz * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
		}
	}
}

pub(crate) fn texture_role_uses_mips(role: TextureRole) -> bool {
	!matches!(role, TextureRole::Clothing)
}

#[cfg(test)]
fn build_rgba_mips(rgba: &[u8], width: u32, height: u32) -> Vec<(u32, u32, Vec<u8>)> {
	build_rgba_mips_with_mode(rgba, width, height, TextureMipmapFilter::Box2x2, false)
}

fn resize_rgba_to_max_dimension(rgba: &[u8], width: u32, height: u32, max_dimension: u32) -> (u32, u32, Vec<u8>) {
	let width = width.max(1);
	let height = height.max(1);
	let max_dimension = max_dimension.max(1);
	if width <= max_dimension && height <= max_dimension {
		return (width, height, normalized_rgba_base(rgba, width, height));
	}
	let long_edge = width.max(height) as u64;
	let new_width = (((width as u64) * (max_dimension as u64) + long_edge / 2) / long_edge).max(1) as u32;
	let new_height = (((height as u64) * (max_dimension as u64) + long_edge / 2) / long_edge).max(1) as u32;
	let base = normalized_rgba_base(rgba, width, height);
	let image = image::RgbaImage::from_raw(width, height, base).expect("normalized rgba dimensions");
	let resized = image::imageops::resize(&image, new_width, new_height, FilterType::Triangle);
	(new_width, new_height, resized.into_raw())
}

const PROCESSED_TEXTURE_CACHE_MAGIC: &[u8; 8] = b"UNATXC1\0";
const PROCESSED_TEXTURE_CACHE_VERSION: u64 = 7;
// v2 (2026-05-14): 圧縮 mip の width/height を block 整列 (4 の倍数) サイズで記録するよう変更。
// 旧 v1 では width=1023 等を保存していたが、wgpu の `write_texture` validation が block 整列値を要求するため、
// magic を bump して旧キャッシュを使わせない（自動的に再エンコードしてキャッシュを書き直す）。
// v3 (2026-05-22): clothing atlas textures keep only the base mip to avoid low-LOD UV island bleed.
// v4 (2026-05-22): mipmap_filter is part of the processed texture cache key.
// v5 (2026-06-01): transparent base-color RGB is alpha-bleed filled before upload/mips.
// v6 (2026-06-03): low-alpha atlas padding RGB is also alpha-bleed filled for opaque/cutout clothing islands.
// v7 (2026-06-04): generated normal-map mips are renormalized after downsampling.
const COMPRESSED_TEXTURE_CACHE_MAGIC: &[u8; 8] = b"UNATBC2\0";
const COMPRESSED_TEXTURE_CACHE_VERSION: u64 = 5;
// v3 (2026-05-27): compressed cache keys are derived from the source texture key and processing settings.
// This allows compressed cache hits to bypass loading or rebuilding the uncompressed processed mip chain.
// v5 (2026-05-28): compressed mip dimensions are stored as block-aligned upload dimensions.
const FNV64_OFFSET: u64 = 0xcbf29ce484222325;
const FNV64_PRIME: u64 = 0x100000001b3;
const PARALLEL_TEXTURE_MIN_ROWS_PER_WORKER: u32 = 64;
const TEXTURE_CACHE_READ_BUFFER_BYTES: usize = 1024 * 1024;

fn fnv1a64_update(mut hash: u64, bytes: &[u8]) -> u64 {
	for byte in bytes {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(FNV64_PRIME);
	}
	hash
}

pub(crate) fn texture_cache_key(
	width: u32,
	height: u32,
	max_dimension: Option<u32>,
	role: TextureRole,
	mipmap_filter: TextureMipmapFilter,
	rgba: &[u8],
) -> u64 {
	let mut hash = FNV64_OFFSET;
	hash = fnv1a64_update(hash, b"un-avatar-processed-texture-cache");
	hash = fnv1a64_update(hash, &PROCESSED_TEXTURE_CACHE_VERSION.to_le_bytes());
	hash = fnv1a64_update(hash, &width.to_le_bytes());
	hash = fnv1a64_update(hash, &height.to_le_bytes());
	hash = fnv1a64_update(hash, &max_dimension.unwrap_or(0).to_le_bytes());
	hash = fnv1a64_update(hash, &[role as u8, mipmap_filter as u8]);
	hash = fnv1a64_update(hash, &(rgba.len() as u64).to_le_bytes());
	fnv1a64_update(hash, rgba)
}

pub(crate) fn texture_cache_key_from_source_metadata(
	width: u32,
	height: u32,
	max_dimension: Option<u32>,
	role: TextureRole,
	mipmap_filter: TextureMipmapFilter,
	source: &UnaImageSourceMetadata,
) -> u64 {
	let mut hash = FNV64_OFFSET;
	hash = fnv1a64_update(hash, b"un-avatar-processed-texture-cache");
	hash = fnv1a64_update(hash, &PROCESSED_TEXTURE_CACHE_VERSION.to_le_bytes());
	hash = fnv1a64_update(hash, &width.to_le_bytes());
	hash = fnv1a64_update(hash, &height.to_le_bytes());
	hash = fnv1a64_update(hash, &max_dimension.unwrap_or(0).to_le_bytes());
	hash = fnv1a64_update(hash, &[role as u8, mipmap_filter as u8]);
	hash = fnv1a64_update(hash, &source.byte_length.to_le_bytes());
	hash = fnv1a64_update(hash, &source.source_hash.to_le_bytes());
	if let Some(mime_type) = &source.mime_type {
		hash = fnv1a64_update(hash, mime_type.as_bytes());
	}
	if let Some(uri) = &source.uri {
		hash = fnv1a64_update(hash, uri.as_bytes());
	}
	hash
}

fn compressed_texture_cache_key(
	source_key: u64,
	processed_width: u32,
	processed_height: u32,
	processed_mip_count: u32,
	mode: TextureCompressionMode,
	preference: TextureCompressionPreference,
	role: TextureRole,
	kind: TextureUploadKind,
) -> u64 {
	let mut hash = FNV64_OFFSET;
	hash = fnv1a64_update(hash, b"un-avatar-compressed-texture-cache");
	hash = fnv1a64_update(hash, &COMPRESSED_TEXTURE_CACHE_VERSION.to_le_bytes());
	hash = fnv1a64_update(hash, &source_key.to_le_bytes());
	hash = fnv1a64_update(hash, &processed_width.to_le_bytes());
	hash = fnv1a64_update(hash, &processed_height.to_le_bytes());
	hash = fnv1a64_update(hash, &[kind.cache_tag(), mode as u8, preference as u8, role as u8]);
	fnv1a64_update(hash, &(processed_mip_count as u64).to_le_bytes())
}

fn processed_texture_cache_dir() -> Option<PathBuf> {
	if let Some(path) = std::env::var_os("UN_AVATAR_TEXTURE_CACHE_DIR") {
		return Some(PathBuf::from(path));
	}
	#[cfg(windows)]
	{
		std::env::var_os("LOCALAPPDATA")
			.map(PathBuf::from)
			.map(|p| p.join("UN Avatar").join("texture-cache").join("v1"))
	}
	#[cfg(not(windows))]
	{
		std::env::var_os("XDG_CACHE_HOME")
			.map(PathBuf::from)
			.or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
			.map(|p| p.join("un-avatar").join("texture-cache").join("v1"))
	}
}

fn read_exact_array<const N: usize>(reader: &mut impl Read) -> Option<[u8; N]> {
	let mut bytes = [0u8; N];
	reader.read_exact(&mut bytes).ok()?;
	Some(bytes)
}

fn read_u32_le(reader: &mut impl Read) -> Option<u32> {
	Some(u32::from_le_bytes(read_exact_array(reader)?))
}

fn read_u64_le(reader: &mut impl Read) -> Option<u64> {
	Some(u64::from_le_bytes(read_exact_array(reader)?))
}

fn write_u32_le(writer: &mut impl Write, value: u32) -> std::io::Result<()> {
	writer.write_all(&value.to_le_bytes())
}

fn write_u64_le(writer: &mut impl Write, value: u64) -> std::io::Result<()> {
	writer.write_all(&value.to_le_bytes())
}

fn cache_temp_path(path: &Path) -> PathBuf {
	let stamp = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_nanos())
		.unwrap_or(0);
	let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("cache");
	path.with_file_name(format!("{file_name}.{}.{}.tmp", std::process::id(), stamp))
}

fn write_cache_file(path: &Path, write_contents: impl FnOnce(&mut BufWriter<fs::File>) -> std::io::Result<()>) -> bool {
	let Some(parent) = path.parent() else { return false };
	if fs::create_dir_all(parent).is_err() {
		return false;
	}
	let temp_path = cache_temp_path(path);
	let write_result = (|| -> std::io::Result<()> {
		let mut writer = BufWriter::new(fs::File::create(&temp_path)?);
		write_contents(&mut writer)?;
		writer.flush()
	})();
	if write_result.is_err() {
		let _ = fs::remove_file(&temp_path);
		return false;
	}
	if fs::rename(&temp_path, path).is_ok() {
		return true;
	}
	let _ = fs::remove_file(path);
	let renamed = fs::rename(&temp_path, path).is_ok();
	if !renamed {
		let _ = fs::remove_file(&temp_path);
	}
	renamed
}

fn read_processed_texture_cache(path: &Path, key: u64) -> Option<(ProcessedTexture, u64)> {
	let mut file = BufReader::with_capacity(TEXTURE_CACHE_READ_BUFFER_BYTES, fs::File::open(path).ok()?);
	if &read_exact_array::<8>(&mut file)? != PROCESSED_TEXTURE_CACHE_MAGIC {
		return None;
	}
	if read_u64_le(&mut file)? != key {
		return None;
	}
	let width = read_u32_le(&mut file)?.max(1);
	let height = read_u32_le(&mut file)?.max(1);
	let mip_count = read_u32_le(&mut file)? as usize;
	if mip_count == 0 || mip_count > 32 {
		return None;
	}
	let mut mips = Vec::with_capacity(mip_count);
	let mut read_bytes = 0u64;
	for _ in 0..mip_count {
		let mip_width = read_u32_le(&mut file)?.max(1);
		let mip_height = read_u32_le(&mut file)?.max(1);
		let len = read_u64_le(&mut file)? as usize;
		let expected = (mip_width as usize).checked_mul(mip_height as usize)?.checked_mul(4)?;
		if len != expected {
			return None;
		}
		let mut data = vec![0u8; len];
		file.read_exact(&mut data).ok()?;
		read_bytes = read_bytes.saturating_add(len as u64);
		mips.push((mip_width, mip_height, data));
	}
	Some((ProcessedTexture { width, height, mips }, read_bytes))
}

fn write_processed_texture_cache(path: &Path, key: u64, texture: &ProcessedTexture) -> bool {
	write_cache_file(path, |writer| {
		writer.write_all(PROCESSED_TEXTURE_CACHE_MAGIC)?;
		write_u64_le(writer, key)?;
		write_u32_le(writer, texture.width)?;
		write_u32_le(writer, texture.height)?;
		write_u32_le(writer, texture.mips.len() as u32)?;
		for (width, height, data) in &texture.mips {
			write_u32_le(writer, *width)?;
			write_u32_le(writer, *height)?;
			write_u64_le(writer, data.len() as u64)?;
			writer.write_all(data)?;
		}
		Ok(())
	})
}

pub(crate) fn read_compressed_texture_cache(path: &Path, key: u64, expected_kind: TextureUploadKind) -> Option<TextureUploadPayload> {
	let mut file = BufReader::with_capacity(TEXTURE_CACHE_READ_BUFFER_BYTES, fs::File::open(path).ok()?);
	if &read_exact_array::<8>(&mut file)? != COMPRESSED_TEXTURE_CACHE_MAGIC {
		return None;
	}
	if read_u64_le(&mut file)? != key {
		return None;
	}
	let kind = TextureUploadKind::from_cache_tag(read_exact_array::<1>(&mut file)?[0])?;
	if kind.cache_tag() != expected_kind.cache_tag() || !kind.is_compressed() {
		return None;
	}
	let mip_count = read_u32_le(&mut file)? as usize;
	if mip_count == 0 || mip_count > 32 {
		return None;
	}
	let mut mips = Vec::with_capacity(mip_count);
	for _ in 0..mip_count {
		let width = read_u32_le(&mut file)?.max(1);
		let height = read_u32_le(&mut file)?.max(1);
		let len = read_u64_le(&mut file)? as usize;
		let expected = (width.div_ceil(4) as usize)
			.checked_mul(height.div_ceil(4) as usize)?
			.checked_mul(kind.block_bytes() as usize)?;
		if len != expected {
			return None;
		}
		let mut data = vec![0u8; len];
		file.read_exact(&mut data).ok()?;
		mips.push(TextureUploadMip { width, height, data });
	}
	Some(TextureUploadPayload { kind, mips })
}

fn write_compressed_texture_cache(path: &Path, key: u64, payload: &TextureUploadPayload) -> bool {
	if !payload.kind.is_compressed() {
		return false;
	}
	write_cache_file(path, |writer| {
		writer.write_all(COMPRESSED_TEXTURE_CACHE_MAGIC)?;
		write_u64_le(writer, key)?;
		writer.write_all(&[payload.kind.cache_tag()])?;
		write_u32_le(writer, payload.mips.len() as u32)?;
		for mip in &payload.mips {
			write_u32_le(writer, mip.width)?;
			write_u32_le(writer, mip.height)?;
			write_u64_le(writer, mip.data.len() as u64)?;
			writer.write_all(&mip.data)?;
		}
		Ok(())
	})
}

fn build_processed_texture(
	rgba: &[u8],
	width: u32,
	height: u32,
	max_dimension: Option<u32>,
	role: TextureRole,
	mipmap_filter: TextureMipmapFilter,
) -> ProcessedTexture {
	let src_width = width.max(1);
	let src_height = height.max(1);
	let base = alpha_safe_rgba_base(rgba, src_width, src_height, role);
	let (width, height, rgba) = max_dimension
		.map(|max_dimension| resize_rgba_to_max_dimension(&base, src_width, src_height, max_dimension))
		.unwrap_or((src_width, src_height, base));
	let mips = if texture_role_uses_mips(role) {
		let alpha_weighted_rgb = texture_role_needs_alpha_weighted_rgb_mips(role, &rgba);
		let mut mips = build_rgba_mips_from_base(rgba, width, height, mipmap_filter, alpha_weighted_rgb);
		if role == TextureRole::Normal {
			renormalize_normal_mips(&mut mips);
		}
		mips
	} else {
		vec![(width, height, rgba)]
	};
	ProcessedTexture { width, height, mips }
}

pub(crate) fn load_or_build_processed_texture(
	rgba: &[u8],
	width: u32,
	height: u32,
	max_dimension: Option<u32>,
	role: TextureRole,
	mipmap_filter: TextureMipmapFilter,
	cache_enabled: bool,
	key: u64,
) -> (ProcessedTexture, TextureCacheEvent) {
	load_or_build_processed_texture_with_rgba(width, height, max_dimension, role, mipmap_filter, cache_enabled, key, || {
		Cow::Borrowed(rgba)
	})
}

pub(crate) fn load_or_build_processed_texture_with_rgba<'a>(
	width: u32,
	height: u32,
	max_dimension: Option<u32>,
	role: TextureRole,
	mipmap_filter: TextureMipmapFilter,
	cache_enabled: bool,
	key: u64,
	rgba: impl FnOnce() -> Cow<'a, [u8]>,
) -> (ProcessedTexture, TextureCacheEvent) {
	if !cache_enabled {
		let rgba = rgba();
		return (
			build_processed_texture(rgba.as_ref(), width, height, max_dimension, role, mipmap_filter),
			TextureCacheEvent::DISABLED,
		);
	}
	let Some(cache_dir) = processed_texture_cache_dir() else {
		let rgba = rgba();
		return (
			build_processed_texture(rgba.as_ref(), width, height, max_dimension, role, mipmap_filter),
			TextureCacheEvent::DISABLED,
		);
	};
	let path = cache_dir.join(format!("{key:016x}.utxc"));
	let read_started = Instant::now();
	if let Some((texture, read_bytes)) = read_processed_texture_cache(&path, key) {
		return (
			texture,
			TextureCacheEvent {
				hit: true,
				miss: false,
				write: false,
				read_elapsed: read_started.elapsed(),
				read_bytes,
			},
		);
	}
	let rgba = rgba();
	let texture = build_processed_texture(rgba.as_ref(), width, height, max_dimension, role, mipmap_filter);
	let write = write_processed_texture_cache(&path, key, &texture);
	(
		texture,
		TextureCacheEvent {
			hit: false,
			miss: true,
			write,
			read_elapsed: read_started.elapsed(),
			read_bytes: 0,
		},
	)
}

pub(crate) fn compression_preference_for_role(
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
) -> TextureCompressionPreference {
	match mode {
		TextureCompressionMode::Source => TextureCompressionPreference::Source,
		TextureCompressionMode::Compat => TextureCompressionPreference::Source,
		TextureCompressionMode::Balanced => match role {
			TextureRole::Face => advanced.face,
			TextureRole::Eyes => advanced.eyes,
			TextureRole::Data => advanced.data,
			TextureRole::Normal => advanced.normal,
			TextureRole::Occlusion => advanced.occlusion,
			TextureRole::Emissive => advanced.emissive,
			TextureRole::Clothing => advanced.clothing,
			TextureRole::GenericColor => advanced.generic_color,
		},
		TextureCompressionMode::Memory => match role {
			TextureRole::Face | TextureRole::Eyes => TextureCompressionPreference::HighQuality,
			TextureRole::Normal | TextureRole::Occlusion => TextureCompressionPreference::GpuNative,
			TextureRole::Clothing | TextureRole::GenericColor | TextureRole::Emissive => TextureCompressionPreference::Small,
			TextureRole::Data => advanced.data,
		},
	}
}

fn rgba_has_translucent_alpha(rgba: &[u8]) -> bool {
	rgba.chunks_exact(4).any(|px| px[3] < 250)
}

fn should_try_bc1_source(
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
	bc_supported: bool,
	base_rgba: &[u8],
) -> bool {
	if !bc_supported || matches!(mode, TextureCompressionMode::Source | TextureCompressionMode::Compat) {
		return false;
	}
	let preference = compression_preference_for_role(mode, advanced, role);
	if matches!(
		preference,
		TextureCompressionPreference::Source | TextureCompressionPreference::HighQuality
	) {
		return false;
	}
	if !matches!(role, TextureRole::Clothing | TextureRole::GenericColor | TextureRole::Emissive) {
		return false;
	}
	!rgba_has_translucent_alpha(base_rgba)
}

fn should_try_bc1(
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
	bc_supported: bool,
	mips: &[(u32, u32, Vec<u8>)],
) -> bool {
	let Some((_, _, base)) = mips.first() else { return false };
	should_try_bc1_source(mode, advanced, role, bc_supported, base)
}

fn should_try_bc5_normal(
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
	bc_supported: bool,
) -> bool {
	if !bc_supported || matches!(mode, TextureCompressionMode::Source | TextureCompressionMode::Compat) || role != TextureRole::Normal {
		return false;
	}
	compression_preference_for_role(mode, advanced, role) != TextureCompressionPreference::Source
}

fn should_try_bc7_color(
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
	bc_supported: bool,
) -> bool {
	if !bc_supported || matches!(mode, TextureCompressionMode::Source | TextureCompressionMode::Compat) {
		return false;
	}
	let preference = compression_preference_for_role(mode, advanced, role);
	if preference != TextureCompressionPreference::HighQuality {
		return false;
	}
	matches!(role, TextureRole::Clothing | TextureRole::Emissive | TextureRole::GenericColor)
}

fn should_try_bc7_data(
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
	bc_supported: bool,
) -> bool {
	if !bc_supported || matches!(mode, TextureCompressionMode::Source | TextureCompressionMode::Compat) || role != TextureRole::Data {
		return false;
	}
	!matches!(
		compression_preference_for_role(mode, advanced, role),
		TextureCompressionPreference::Source | TextureCompressionPreference::Auto
	)
}

fn padded_rgba_for_block_encoder(rgba: &[u8], width: u32, height: u32) -> PaddedRgba<'_> {
	let width = width.max(1);
	let height = height.max(1);
	let padded_width = width.div_ceil(4) * 4;
	let padded_height = height.div_ceil(4) * 4;
	let expected_len = (width as usize) * (height as usize) * 4;
	let src = if rgba.len() == expected_len {
		Cow::Borrowed(rgba)
	} else {
		Cow::Owned(normalized_rgba_base(rgba, width, height))
	};
	if padded_width == width && padded_height == height {
		return PaddedRgba { width, height, data: src };
	}
	let mut padded = vec![0; (padded_width as usize) * (padded_height as usize) * 4];
	let src_row_bytes = (width as usize) * 4;
	let padded_row_bytes = (padded_width as usize) * 4;
	let src = src.as_ref();
	for y in 0..height as usize {
		let src_row = &src[y * src_row_bytes..(y + 1) * src_row_bytes];
		let dst_row = &mut padded[y * padded_row_bytes..(y + 1) * padded_row_bytes];
		dst_row[..src_row_bytes].copy_from_slice(src_row);
		if padded_width > width {
			let last_pixel = [
				src_row[src_row_bytes - 4],
				src_row[src_row_bytes - 3],
				src_row[src_row_bytes - 2],
				src_row[src_row_bytes - 1],
			];
			for pixel in dst_row[src_row_bytes..].chunks_exact_mut(4) {
				pixel.copy_from_slice(&last_pixel);
			}
		}
	}
	if padded_height > height {
		let last_src_start = (height as usize - 1) * padded_row_bytes;
		for y in height as usize..padded_height as usize {
			padded.copy_within(last_src_start..last_src_start + padded_row_bytes, y * padded_row_bytes);
		}
	}
	PaddedRgba {
		width: padded_width,
		height: padded_height,
		data: Cow::Owned(padded),
	}
}

/// BCn にエンコードし、ブロック整列 (4 の倍数) に切り上げた幅・高さと圧縮データを返す。
/// 元 (width, height) が既に整列なら寸法はそのまま、そうでなければ padded 値が返る。
/// wgpu の圧縮テクスチャ upload は 4x4 block 単位のため、payload 寸法はエンコード済み
/// ブロック列の物理寸法に合わせる。
#[cfg(test)]
fn encode_block_compressed_rgba_mip_cpu(
	kind: TextureUploadKind,
	rgba: &[u8],
	width: u32,
	height: u32,
	cpu_threads: usize,
) -> (u32, u32, Vec<u8>) {
	let variant = kind.compression_variant(rgba).expect("compressed texture kind");
	encode_block_compressed_rgba_mip_cpu_with_variant(variant, rgba, width, height, cpu_threads)
}

fn encode_block_compressed_rgba_mip_cpu_with_variant(
	variant: block_compression::CompressionVariant,
	rgba: &[u8],
	width: u32,
	height: u32,
	cpu_threads: usize,
) -> (u32, u32, Vec<u8>) {
	let padded = padded_rgba_for_block_encoder(rgba, width, height);
	let padded_width = padded.width;
	let padded_height = padded.height;
	let padded_data = padded.data.as_ref();
	let cpu_threads = clamp_block_compression_cpu_threads(cpu_threads);
	if cpu_threads <= 1 || padded_height <= 4 {
		let mut out = vec![0u8; variant.blocks_byte_size(padded_width, padded_height)];
		block_compression::encode::compress_rgba8(variant, padded_data, &mut out, padded_width, padded_height, padded_width * 4);
		return (padded_width, padded_height, out);
	}
	let block_height = padded_height.div_ceil(4);
	let compressed_row_bytes = variant.bytes_per_row(padded_width) as usize;
	let stripes = block_compression_row_stripes(block_height, cpu_threads);
	let mut out = vec![0u8; variant.blocks_byte_size(padded_width, padded_height)];
	let row_bytes = (padded_width as usize) * 4;
	let mut remaining = out.as_mut_slice();
	thread::scope(|scope| {
		for stripe in stripes {
			let stripe_h = stripe.len * 4;
			let stripe_out_len = stripe.len as usize * compressed_row_bytes;
			let (stripe_out, rest) = remaining.split_at_mut(stripe_out_len);
			remaining = rest;
			let start_y = stripe.start * 4;
			let stripe_rgba = &padded_data[(start_y as usize) * row_bytes..(start_y as usize + stripe_h as usize) * row_bytes];
			scope.spawn(move || {
				block_compression::encode::compress_rgba8(variant, stripe_rgba, stripe_out, padded_width, stripe_h, padded_width * 4);
			});
		}
	});
	(padded_width, padded_height, out)
}

fn clamp_block_compression_cpu_threads(requested: usize) -> usize {
	let logical = thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
	requested.max(1).min(logical.max(1))
}

fn block_compression_row_stripes(block_height: u32, cpu_threads: usize) -> Vec<RowStripe> {
	let target = (block_height.div_ceil(cpu_threads.max(1) as u32)).max(1);
	let mut stripes = Vec::with_capacity(cpu_threads.max(1).min(block_height.max(1) as usize));
	let mut block_y = 0;
	while block_y < block_height {
		let blocks_h = target.min(block_height - block_y);
		stripes.push(RowStripe {
			start: block_y,
			len: blocks_h,
		});
		block_y += blocks_h;
	}
	stripes
}

fn encode_block_compressed_rgba_mip_gpu(
	context: &mut GpuTextureCompressionContext,
	variant: block_compression::CompressionVariant,
	rgba: &[u8],
	width: u32,
	height: u32,
) -> (u32, u32, Vec<u8>) {
	let padded = padded_rgba_for_block_encoder(rgba, width, height);
	let padded_width = padded.width;
	let padded_height = padded.height;
	let src = context.device.create_texture(&wgpu::TextureDescriptor {
		label: Some("bcn-gpu-src"),
		size: wgpu::Extent3d {
			width: padded_width,
			height: padded_height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Rgba8Unorm,
		usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
		view_formats: &[],
	});
	context.queue.write_texture(
		src.as_image_copy(),
		padded.data.as_ref(),
		wgpu::TexelCopyBufferLayout {
			offset: 0,
			bytes_per_row: Some(padded_width * 4),
			rows_per_image: Some(padded_height),
		},
		wgpu::Extent3d {
			width: padded_width,
			height: padded_height,
			depth_or_array_layers: 1,
		},
	);
	let view = src.create_view(&wgpu::TextureViewDescriptor::default());
	let size = variant.blocks_byte_size(padded_width, padded_height) as u64;
	let blocks = context.device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("bcn-gpu-blocks"),
		size,
		usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::STORAGE,
		mapped_at_creation: false,
	});
	context
		.compressor
		.add_compression_task(variant, &view, padded_width, padded_height, &blocks, None, None);
	let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
		label: Some("bcn-gpu-compress"),
	});
	{
		let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
			label: Some("bcn-gpu-compress"),
			timestamp_writes: None,
		});
		context.compressor.compress(&mut pass);
	}
	context.queue.submit([encoder.finish()]);
	let data = download_buffer(&context.device, &context.queue, &blocks);
	(padded_width, padded_height, data)
}

fn download_buffer(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer) -> Vec<u8> {
	let size = buffer.size();
	let staging = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("bcn-gpu-readback"),
		size,
		usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
		mapped_at_creation: false,
	});
	let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
		label: Some("bcn-gpu-copy"),
	});
	encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
	queue.submit([encoder.finish()]);
	let slice = staging.slice(..);
	let (tx, rx) = std::sync::mpsc::channel();
	slice.map_async(wgpu::MapMode::Read, move |result| {
		let _ = tx.send(result);
	});
	let _ = device.poll(wgpu::PollType::Wait {
		submission_index: None,
		timeout: Some(Duration::from_secs(120)),
	});
	rx.recv().expect("BCn readback callback").expect("BCn readback map");
	let data = slice.get_mapped_range().to_vec();
	staging.unmap();
	data
}

fn compressed_cache_path(key: u64, kind: TextureUploadKind) -> Option<PathBuf> {
	processed_texture_cache_dir().map(|cache_dir| cache_dir.join(format!("{key:016x}.{}.utbc", kind.cache_tag())))
}

fn compressed_upload_kind_for_source(
	rgba: &[u8],
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
	bc_supported: bool,
) -> Option<TextureUploadKind> {
	if should_try_bc5_normal(mode, advanced, role, bc_supported) {
		Some(TextureUploadKind::Bc5Unorm)
	} else if should_try_bc7_data(mode, advanced, role, bc_supported) {
		Some(TextureUploadKind::Bc7Unorm)
	} else if should_try_bc7_color(mode, advanced, role, bc_supported) {
		Some(TextureUploadKind::Bc7Srgb)
	} else if should_try_bc1_source(mode, advanced, role, bc_supported, rgba) {
		Some(TextureUploadKind::Bc1Srgb)
	} else {
		None
	}
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compressed_cache_lookup_from_source(
	rgba: &[u8],
	width: u32,
	height: u32,
	max_dimension: Option<u32>,
	role: TextureRole,
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	bc_supported: bool,
	source_key: u64,
) -> Option<CompressedTextureCacheLookup> {
	let kind = compressed_upload_kind_for_source(rgba, mode, advanced, role, bc_supported)?;
	let preference = compression_preference_for_role(mode, advanced, role);
	let (processed_width, processed_height) = resized_dimensions_to_max_dimension(width, height, max_dimension);
	let processed_mip_count = if texture_role_uses_mips(role) {
		mip_level_count(processed_width, processed_height)
	} else {
		1
	};
	let key = compressed_texture_cache_key(
		source_key,
		processed_width,
		processed_height,
		processed_mip_count,
		mode,
		preference,
		role,
		kind,
	);
	let path = compressed_cache_path(key, kind)?;
	Some(CompressedTextureCacheLookup {
		key,
		kind,
		path,
		processed_width,
		processed_height,
	})
}

pub(crate) fn compressed_cache_lookup_from_source_metadata(
	width: u32,
	height: u32,
	max_dimension: Option<u32>,
	role: TextureRole,
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	bc_supported: bool,
	source_key: u64,
) -> Option<CompressedTextureCacheLookup> {
	let kind = if should_try_bc5_normal(mode, advanced, role, bc_supported) {
		TextureUploadKind::Bc5Unorm
	} else if should_try_bc7_data(mode, advanced, role, bc_supported) {
		TextureUploadKind::Bc7Unorm
	} else if should_try_bc7_color(mode, advanced, role, bc_supported) {
		TextureUploadKind::Bc7Srgb
	} else if bc_supported
		&& !matches!(mode, TextureCompressionMode::Source | TextureCompressionMode::Compat)
		&& !matches!(
			compression_preference_for_role(mode, advanced, role),
			TextureCompressionPreference::Source | TextureCompressionPreference::HighQuality
		) && matches!(role, TextureRole::Clothing | TextureRole::GenericColor | TextureRole::Emissive)
	{
		TextureUploadKind::Bc1Srgb
	} else {
		return None;
	};
	let preference = compression_preference_for_role(mode, advanced, role);
	let (processed_width, processed_height) = resized_dimensions_to_max_dimension(width, height, max_dimension);
	let processed_mip_count = if texture_role_uses_mips(role) {
		mip_level_count(processed_width, processed_height)
	} else {
		1
	};
	let key = compressed_texture_cache_key(
		source_key,
		processed_width,
		processed_height,
		processed_mip_count,
		mode,
		preference,
		role,
		kind,
	);
	let path = compressed_cache_path(key, kind)?;
	Some(CompressedTextureCacheLookup {
		key,
		kind,
		path,
		processed_width,
		processed_height,
	})
}

fn build_texture_upload_payload_cpu(
	processed: ProcessedTexture,
	kind: TextureUploadKind,
	block_compression_cpu_threads: usize,
	progress: TextureUploadProgress<'_>,
) -> TextureUploadPayload {
	let mip_count = processed.mips.len() as u32;
	let mut mips = Vec::with_capacity(processed.mips.len());
	for (mip_index, (width, height, rgba)) in processed.mips.into_iter().enumerate() {
		progress(mip_index as u32 + 1, mip_count, width, height, kind);
		let (mip_width, mip_height, data) = match kind {
			TextureUploadKind::Bc1Srgb | TextureUploadKind::Bc5Unorm | TextureUploadKind::Bc7Unorm | TextureUploadKind::Bc7Srgb => {
				let compression_variant = kind.compression_variant(&rgba).expect("compressed texture kind");
				encode_block_compressed_rgba_mip_cpu_with_variant(compression_variant, &rgba, width, height, block_compression_cpu_threads)
			}
			TextureUploadKind::Rgba => (width, height, rgba),
		};
		mips.push(TextureUploadMip {
			width: mip_width,
			height: mip_height,
			data,
		});
	}
	TextureUploadPayload { kind, mips }
}

fn build_texture_upload_payload_with_selected_encoder(
	processed: ProcessedTexture,
	kind: TextureUploadKind,
	block_compression_cpu_threads: usize,
	gpu_texture_compression: Option<&mut GpuTextureCompressionContext>,
	progress: TextureUploadProgress<'_>,
) -> TextureUploadPayload {
	if kind.is_compressed() {
		if let Some(context) = gpu_texture_compression {
			return build_texture_upload_payload_gpu(processed, kind, context, progress);
		}
	}
	build_texture_upload_payload_cpu(processed, kind, block_compression_cpu_threads, progress)
}

fn build_texture_upload_payload_gpu(
	processed: ProcessedTexture,
	kind: TextureUploadKind,
	context: &mut GpuTextureCompressionContext,
	progress: TextureUploadProgress<'_>,
) -> TextureUploadPayload {
	let mip_count = processed.mips.len() as u32;
	let mut mips = Vec::with_capacity(processed.mips.len());
	for (mip_index, (width, height, rgba)) in processed.mips.into_iter().enumerate() {
		progress(mip_index as u32 + 1, mip_count, width, height, kind);
		let compression_variant = kind.compression_variant(&rgba).expect("compressed texture kind");
		let (mip_width, mip_height, data) = if width <= 16 || height <= 16 {
			encode_block_compressed_rgba_mip_cpu_with_variant(compression_variant, &rgba, width, height, 1)
		} else {
			encode_block_compressed_rgba_mip_gpu(context, compression_variant, &rgba, width, height)
		};
		mips.push(TextureUploadMip {
			width: mip_width,
			height: mip_height,
			data,
		});
	}
	TextureUploadPayload { kind, mips }
}

pub(crate) fn compressed_upload_kind_for_texture(
	processed: &ProcessedTexture,
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
	bc_supported: bool,
) -> Option<TextureUploadKind> {
	if should_try_bc5_normal(mode, advanced, role, bc_supported) {
		Some(TextureUploadKind::Bc5Unorm)
	} else if should_try_bc7_data(mode, advanced, role, bc_supported) {
		Some(TextureUploadKind::Bc7Unorm)
	} else if should_try_bc7_color(mode, advanced, role, bc_supported) {
		Some(TextureUploadKind::Bc7Srgb)
	} else if should_try_bc1(mode, advanced, role, bc_supported, &processed.mips) {
		Some(TextureUploadKind::Bc1Srgb)
	} else {
		None
	}
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn texture_upload_payload(
	processed: ProcessedTexture,
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
	bc_supported: bool,
	block_compression_encoder: BlockCompressionEncoder,
	block_compression_cpu_threads: usize,
	gpu_texture_compression: Option<&mut GpuTextureCompressionContext>,
	cache_enabled: bool,
	compressed_cache_lookup: Option<&CompressedTextureCacheLookup>,
	compressed_cache_already_missed: bool,
) -> (TextureUploadPayload, TextureCacheEvent) {
	texture_upload_payload_with_progress(
		processed,
		mode,
		advanced,
		role,
		bc_supported,
		block_compression_encoder,
		block_compression_cpu_threads,
		gpu_texture_compression,
		cache_enabled,
		compressed_cache_lookup,
		compressed_cache_already_missed,
		|_, _, _, _, _| {},
	)
}

#[allow(clippy::too_many_arguments)]
fn texture_upload_payload_with_progress(
	processed: ProcessedTexture,
	mode: TextureCompressionMode,
	advanced: &TextureCompressionAdvancedOptions,
	role: TextureRole,
	bc_supported: bool,
	block_compression_encoder: BlockCompressionEncoder,
	block_compression_cpu_threads: usize,
	mut gpu_texture_compression: Option<&mut GpuTextureCompressionContext>,
	cache_enabled: bool,
	compressed_cache_lookup: Option<&CompressedTextureCacheLookup>,
	compressed_cache_already_missed: bool,
	mut progress: impl FnMut(u32, u32, u32, u32, TextureUploadKind),
) -> (TextureUploadPayload, TextureCacheEvent) {
	if let Some(kind) = compressed_upload_kind_for_texture(&processed, mode, advanced, role, bc_supported) {
		let use_gpu_compression = kind.is_compressed() && block_compression_encoder == BlockCompressionEncoder::Gpu;
		if use_gpu_compression && gpu_texture_compression.is_none() {
			return (
				build_texture_upload_payload_cpu(processed, TextureUploadKind::Rgba, block_compression_cpu_threads, &mut progress),
				TextureCacheEvent::DISABLED,
			);
		}
		let gpu_texture_compression = if use_gpu_compression {
			gpu_texture_compression.as_deref_mut()
		} else {
			None
		};
		let lookup = compressed_cache_lookup.filter(|lookup| lookup.kind.cache_tag() == kind.cache_tag());
		if cache_enabled {
			let Some(lookup) = lookup else {
				return (
					build_texture_upload_payload_with_selected_encoder(
						processed,
						kind,
						block_compression_cpu_threads,
						gpu_texture_compression,
						&mut progress,
					),
					TextureCacheEvent::DISABLED,
				);
			};
			if !compressed_cache_already_missed {
				if let Some(payload) = read_compressed_texture_cache(&lookup.path, lookup.key, kind) {
					return (
						payload,
						TextureCacheEvent {
							hit: true,
							miss: false,
							write: false,
							read_elapsed: Duration::ZERO,
							read_bytes: 0,
						},
					);
				}
			}
			let payload = build_texture_upload_payload_with_selected_encoder(
				processed,
				kind,
				block_compression_cpu_threads,
				gpu_texture_compression,
				&mut progress,
			);
			let write = write_compressed_texture_cache(&lookup.path, lookup.key, &payload);
			return (
				payload,
				TextureCacheEvent {
					hit: false,
					miss: true,
					write,
					read_elapsed: Duration::ZERO,
					read_bytes: 0,
				},
			);
		}
		(
			build_texture_upload_payload_with_selected_encoder(
				processed,
				kind,
				block_compression_cpu_threads,
				gpu_texture_compression,
				&mut progress,
			),
			TextureCacheEvent::DISABLED,
		)
	} else {
		(
			build_texture_upload_payload_cpu(processed, TextureUploadKind::Rgba, block_compression_cpu_threads, &mut progress),
			TextureCacheEvent::DISABLED,
		)
	}
}

#[cfg(test)]
mod tests {
	use std::fs;

	use super::*;

	#[test]
	fn mip_level_count_reaches_one_by_one() {
		assert_eq!(mip_level_count(1, 1), 1);
		assert_eq!(mip_level_count(2, 1), 2);
		assert_eq!(mip_level_count(4, 4), 3);
		assert_eq!(mip_level_count(7, 3), 3);
	}

	#[test]
	fn parallel_row_stripes_cover_rows_without_overlap() {
		let stripes = parallel_row_stripes(257, 64);
		assert!(stripes.len() > 1);
		let mut next = 0;
		for stripe in stripes {
			assert_eq!(stripe.start, next);
			assert!(stripe.len > 0);
			next += stripe.len;
		}
		assert_eq!(next, 257);
	}

	#[test]
	fn parallel_row_stripes_keep_small_work_single_threaded() {
		assert_eq!(parallel_row_stripes(32, 64).len(), 1);
	}

	#[test]
	fn rgba_mips_downsample_to_single_pixel() {
		let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
		let mips = build_rgba_mips(&rgba, 2, 2);
		assert_eq!(mips.len(), 2);
		assert_eq!((mips[0].0, mips[0].1), (2, 2));
		assert_eq!((mips[1].0, mips[1].1), (1, 1));
		assert_eq!(mips[1].2, vec![127, 127, 127, 255]);
	}

	#[test]
	fn color_role_mips_alpha_weight_rgb() {
		let rgba = vec![
			0, 0, 0, 255, 255, 255, 255, 0, //
			255, 255, 255, 0, 255, 255, 255, 0,
		];
		let processed = build_processed_texture(&rgba, 2, 2, None, TextureRole::GenericColor, TextureMipmapFilter::Box2x2);
		assert_eq!(processed.mips.len(), 2);
		assert_eq!(processed.mips[1].2, vec![0, 0, 0, 63]);
	}

	#[test]
	fn transparent_rgb_is_filled_for_base_upload() {
		let rgba = vec![
			10, 20, 30, 255, 0, 0, 0, 0, //
			0, 0, 0, 0, 0, 0, 0, 0,
		];
		let processed = build_processed_texture(&rgba, 2, 2, None, TextureRole::Clothing, TextureMipmapFilter::Box2x2);
		assert_eq!(processed.mips.len(), 1);
		for pixel in processed.mips[0].2.chunks_exact(4) {
			assert_eq!(&pixel[..3], &[10, 20, 30]);
		}
		assert_eq!(processed.mips[0].2[3], 255);
		assert_eq!(processed.mips[0].2[7], 0);
	}

	#[test]
	fn low_alpha_rgb_is_filled_for_base_upload() {
		let rgba = vec![
			200, 180, 160, 255, 0, 0, 0, 96, //
			0, 0, 0, 128, 0, 0, 0, 0,
		];
		let processed = build_processed_texture(&rgba, 2, 2, None, TextureRole::Clothing, TextureMipmapFilter::Box2x2);
		assert_eq!(processed.mips.len(), 1);
		for pixel in processed.mips[0].2.chunks_exact(4) {
			assert_eq!(&pixel[..3], &[200, 180, 160]);
		}
		assert_eq!(processed.mips[0].2[3], 255);
		assert_eq!(processed.mips[0].2[7], 96);
		assert_eq!(processed.mips[0].2[11], 128);
		assert_eq!(processed.mips[0].2[15], 0);
	}

	#[test]
	fn opaque_color_role_mips_use_plain_average_path() {
		let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];

		assert!(!texture_role_needs_alpha_weighted_rgb_mips(TextureRole::GenericColor, &rgba));
	}

	#[test]
	fn clothing_role_keeps_base_mip_only_to_avoid_atlas_bleed() {
		let rgba = vec![255; 4 * 4 * 4];
		let processed = build_processed_texture(&rgba, 4, 4, None, TextureRole::Clothing, TextureMipmapFilter::Box2x2);
		assert_eq!(processed.mips.len(), 1);
		assert_eq!((processed.mips[0].0, processed.mips[0].1), (4, 4));
	}

	#[test]
	fn normal_role_mips_renormalize_after_downsample() {
		let rgba = vec![
			0, 0, 0, 255, 255, 255, 255, 0, //
			255, 255, 255, 0, 255, 255, 255, 0,
		];
		let processed = build_processed_texture(&rgba, 2, 2, None, TextureRole::Normal, TextureMipmapFilter::Box2x2);
		assert_eq!(processed.mips[1].2, vec![201, 201, 201, 63]);
	}

	#[test]
	fn texture_limit_resize_preserves_aspect_ratio() {
		let rgba = vec![255; 8 * 4 * 4];
		let (width, height, resized) = resize_rgba_to_max_dimension(&rgba, 8, 4, 4);
		assert_eq!((width, height), (4, 2));
		assert_eq!(resized.len(), 4 * 2 * 4);
	}

	#[test]
	fn texture_cache_key_includes_resolution_policy() {
		let rgba = vec![128; 4 * 4 * 4];
		let unlimited = texture_cache_key(4, 4, None, TextureRole::GenericColor, TextureMipmapFilter::Box2x2, &rgba);
		let limited = texture_cache_key(4, 4, Some(2), TextureRole::GenericColor, TextureMipmapFilter::Box2x2, &rgba);
		assert_ne!(unlimited, limited);
	}

	#[test]
	fn texture_cache_key_includes_texture_role() {
		let rgba = vec![128; 4 * 4 * 4];
		let color = texture_cache_key(4, 4, None, TextureRole::Clothing, TextureMipmapFilter::Box2x2, &rgba);
		let normal = texture_cache_key(4, 4, None, TextureRole::Normal, TextureMipmapFilter::Box2x2, &rgba);
		assert_ne!(color, normal);
	}

	#[test]
	fn texture_cache_key_can_use_source_metadata_without_hashing_decoded_pixels() {
		let source = UnaImageSourceMetadata {
			name: Some("main".to_string()),
			mime_type: Some("image/png".to_string()),
			uri: None,
			source_pixel_format: None,
			channels: None,
			color_space: None,
			texture_type: None,
			texture_shape: None,
			source_layout: None,
			unity_generate_cubemap: None,
			srgb: None,
			sampler: None,
			width: None,
			height: None,
			byte_offset: None,
			byte_length: 3,
			source_hash: 0x1234,
			source_file_path: None,
			encoded_bytes: None,
		};
		let key_a = texture_cache_key_from_source_metadata(4, 4, None, TextureRole::GenericColor, TextureMipmapFilter::Box2x2, &source);
		let mut changed_source = source.clone();
		changed_source.source_hash = 0x5678;
		let key_b =
			texture_cache_key_from_source_metadata(4, 4, None, TextureRole::GenericColor, TextureMipmapFilter::Box2x2, &changed_source);
		assert_ne!(key_a, key_b);
	}

	#[test]
	fn texture_cache_key_includes_mipmap_filter() {
		let rgba = vec![128; 4 * 4 * 4];
		let box2x2 = texture_cache_key(4, 4, None, TextureRole::GenericColor, TextureMipmapFilter::Box2x2, &rgba);
		let mitchell = texture_cache_key(4, 4, None, TextureRole::GenericColor, TextureMipmapFilter::Mitchell, &rgba);
		assert_ne!(box2x2, mitchell);
	}

	#[test]
	fn source_upload_preserves_r16_rgb_precision_as_rgba16_unorm() {
		let r = 0x1234u16;
		let g = 0x5678u16;
		let b = 0x9abcu16;
		let mut pixels = Vec::new();
		pixels.extend_from_slice(&r.to_ne_bytes());
		pixels.extend_from_slice(&g.to_ne_bytes());
		pixels.extend_from_slice(&b.to_ne_bytes());

		let image = UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: UnaImagePixelFormat::R16G16B16,
			pixels,
		};
		let upload = source_texture_upload(&image).expect("R16G16B16 should use precision-preserving source upload");

		let mut expected = Vec::new();
		for value in [r, g, b, u16::MAX] {
			expected.extend_from_slice(&value.to_ne_bytes());
		}
		assert_eq!(upload.format, wgpu::TextureFormat::Rgba16Unorm);
		assert_eq!(upload.bytes_per_row, 8);
		assert_eq!(upload.data, expected);
	}

	#[test]
	fn block_compression_encoder_emits_one_block_for_small_color_mips() {
		let rgba = [255, 64, 32, 255].repeat(2 * 2);
		let (w, h, bc1) = encode_block_compressed_rgba_mip_cpu(TextureUploadKind::Bc1Srgb, &rgba, 2, 2, 1);
		assert_eq!((w, h), (4, 4));
		assert_eq!(bc1.len(), 8);
		let (w, h, bc7) = encode_block_compressed_rgba_mip_cpu(TextureUploadKind::Bc7Srgb, &rgba, 2, 2, 1);
		assert_eq!((w, h), (4, 4));
		assert_eq!(bc7.len(), 16);
		let (w, h, bc7_unorm) = encode_block_compressed_rgba_mip_cpu(TextureUploadKind::Bc7Unorm, &rgba, 2, 2, 1);
		assert_eq!((w, h), (4, 4));
		assert_eq!(bc7_unorm.len(), 16);
		let normal = [128, 128, 255, 255].repeat(2 * 2);
		let (w, h, bc5) = encode_block_compressed_rgba_mip_cpu(TextureUploadKind::Bc5Unorm, &normal, 2, 2, 1);
		assert_eq!((w, h), (4, 4));
		assert_eq!(bc5.len(), 16);
	}

	#[test]
	fn block_compression_encoder_pads_non_aligned_dimensions() {
		let w_src = 5u32;
		let h_src = 3u32;
		let rgba = vec![200; (w_src as usize) * (h_src as usize) * 4];
		let (w, h, bc7) = encode_block_compressed_rgba_mip_cpu(TextureUploadKind::Bc7Srgb, &rgba, w_src, h_src, 1);
		assert_eq!((w, h), (8, 4));
		assert_eq!(bc7.len(), (8 / 4) * 16);
	}

	#[test]
	fn block_compression_parallel_matches_single_threaded_output() {
		let w_src = 32u32;
		let h_src = 24u32;
		let rgba: Vec<u8> = (0..w_src * h_src)
			.flat_map(|i| {
				let x = (i % w_src) as u8;
				let y = (i / w_src) as u8;
				[x.wrapping_mul(7), y.wrapping_mul(11), x.wrapping_add(y).wrapping_mul(3), 255]
			})
			.collect();
		let single = encode_block_compressed_rgba_mip_cpu(TextureUploadKind::Bc7Srgb, &rgba, w_src, h_src, 1);
		let parallel = encode_block_compressed_rgba_mip_cpu(TextureUploadKind::Bc7Srgb, &rgba, w_src, h_src, 4);

		assert_eq!(parallel, single);
	}

	#[test]
	fn compression_skips_face_and_alpha_textures() {
		let opaque = vec![255; 4 * 4 * 4];
		let translucent = vec![128; 4 * 4 * 4];
		let opaque_mips = build_rgba_mips(&opaque, 4, 4);
		let translucent_mips = build_rgba_mips(&translucent, 4, 4);

		assert!(!should_try_bc1(
			TextureCompressionMode::Balanced,
			&TextureCompressionAdvancedOptions::default(),
			TextureRole::Face,
			true,
			&opaque_mips
		));
		assert!(!should_try_bc1(
			TextureCompressionMode::Balanced,
			&TextureCompressionAdvancedOptions::default(),
			TextureRole::GenericColor,
			true,
			&translucent_mips
		));
		assert!(should_try_bc1(
			TextureCompressionMode::Balanced,
			&TextureCompressionAdvancedOptions::default(),
			TextureRole::GenericColor,
			true,
			&opaque_mips
		));
		assert!(should_try_bc5_normal(
			TextureCompressionMode::Balanced,
			&TextureCompressionAdvancedOptions::default(),
			TextureRole::Normal,
			true,
		));
		assert!(!should_try_bc5_normal(
			TextureCompressionMode::Source,
			&TextureCompressionAdvancedOptions::default(),
			TextureRole::Normal,
			true,
		));
	}

	#[test]
	fn high_quality_face_and_eye_textures_stay_source() {
		assert!(!should_try_bc7_color(
			TextureCompressionMode::Balanced,
			&TextureCompressionAdvancedOptions::default(),
			TextureRole::Face,
			true,
		));
		assert!(!should_try_bc7_color(
			TextureCompressionMode::Balanced,
			&TextureCompressionAdvancedOptions::default(),
			TextureRole::Eyes,
			true,
		));
	}

	#[test]
	fn balanced_emissive_prefers_bc7_when_bc_is_supported() {
		let rgba = [255, 64, 32, 255].repeat(4 * 4);
		let processed = build_processed_texture(&rgba, 4, 4, None, TextureRole::Emissive, TextureMipmapFilter::Box2x2);
		let (payload, cache_event) = texture_upload_payload(
			processed,
			TextureCompressionMode::Balanced,
			&TextureCompressionAdvancedOptions::default(),
			TextureRole::Emissive,
			true,
			BlockCompressionEncoder::Cpu,
			1,
			None,
			false,
			None,
			false,
		);

		assert_eq!(payload.kind.cache_tag(), TextureUploadKind::Bc7Srgb.cache_tag());
		assert_eq!(payload.mips[0].data.len(), 16);
		assert!(!cache_event.hit && !cache_event.miss && !cache_event.write);
	}

	#[test]
	fn balanced_advanced_color_roles_can_prefer_bc7() {
		let mut advanced = TextureCompressionAdvancedOptions::default();
		advanced.clothing = TextureCompressionPreference::HighQuality;
		advanced.generic_color = TextureCompressionPreference::HighQuality;

		assert!(should_try_bc7_color(
			TextureCompressionMode::Balanced,
			&advanced,
			TextureRole::Clothing,
			true,
		));
		assert!(should_try_bc7_color(
			TextureCompressionMode::Balanced,
			&advanced,
			TextureRole::GenericColor,
			true,
		));

		let rgba = [255, 64, 32, 192].repeat(4 * 4);
		let processed = build_processed_texture(&rgba, 4, 4, None, TextureRole::GenericColor, TextureMipmapFilter::Box2x2);
		let (payload, cache_event) = texture_upload_payload(
			processed,
			TextureCompressionMode::Balanced,
			&advanced,
			TextureRole::GenericColor,
			true,
			BlockCompressionEncoder::Cpu,
			1,
			None,
			false,
			None,
			false,
		);

		assert_eq!(payload.kind.cache_tag(), TextureUploadKind::Bc7Srgb.cache_tag());
		assert_eq!(payload.mips[0].data.len(), 16);
		assert!(!cache_event.hit && !cache_event.miss && !cache_event.write);
	}

	#[test]
	fn data_bc7_unorm_requires_explicit_advanced_preference() {
		assert!(!should_try_bc7_data(
			TextureCompressionMode::Balanced,
			&TextureCompressionAdvancedOptions::default(),
			TextureRole::Data,
			true,
		));

		let mut advanced = TextureCompressionAdvancedOptions::default();
		advanced.data = TextureCompressionPreference::HighQuality;
		assert!(should_try_bc7_data(
			TextureCompressionMode::Balanced,
			&advanced,
			TextureRole::Data,
			true,
		));
		let rgba = [10, 20, 30, 40].repeat(4 * 4);
		let processed = build_processed_texture(&rgba, 4, 4, None, TextureRole::Data, TextureMipmapFilter::Box2x2);
		let (payload, _) = texture_upload_payload(
			processed,
			TextureCompressionMode::Balanced,
			&advanced,
			TextureRole::Data,
			true,
			BlockCompressionEncoder::Cpu,
			1,
			None,
			false,
			None,
			false,
		);

		assert_eq!(payload.kind.cache_tag(), TextureUploadKind::Bc7Unorm.cache_tag());
		assert_eq!(payload.mips[0].data.len(), 16);
	}

	#[test]
	fn processed_texture_cache_roundtrips_mips() {
		let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
		let texture = build_processed_texture(&rgba, 2, 2, None, TextureRole::Normal, TextureMipmapFilter::Box2x2);
		let key = texture_cache_key(2, 2, None, TextureRole::Normal, TextureMipmapFilter::Box2x2, &rgba);
		let path = std::env::temp_dir().join(format!(
			"un-avatar-processed-texture-cache-test-{}-{key:016x}.utxc",
			std::process::id()
		));
		let _ = fs::remove_file(&path);

		assert!(write_processed_texture_cache(&path, key, &texture));
		let (loaded, read_bytes) = read_processed_texture_cache(&path, key).expect("cache should load");
		assert_eq!((loaded.width, loaded.height), (2, 2));
		assert_eq!(loaded.mips.len(), 2);
		assert_eq!(loaded.mips[1].2, vec![128, 128, 255, 255]);
		assert_eq!(read_bytes, loaded.mips.iter().map(|(_, _, data)| data.len() as u64).sum::<u64>());

		let _ = fs::remove_file(path);
	}

	#[test]
	fn compressed_texture_cache_roundtrips_bc7_unorm_payload() {
		let payload = TextureUploadPayload {
			kind: TextureUploadKind::Bc7Unorm,
			mips: vec![TextureUploadMip {
				width: 2,
				height: 2,
				data: vec![42; 16],
			}],
		};
		let key = 0xabcdu64;
		let path = std::env::temp_dir().join(format!(
			"un-avatar-compressed-texture-cache-test-{}-{key:016x}.utbc",
			std::process::id()
		));
		let _ = fs::remove_file(&path);

		assert!(write_compressed_texture_cache(&path, key, &payload));
		let loaded = read_compressed_texture_cache(&path, key, TextureUploadKind::Bc7Unorm).expect("cache should load");
		assert_eq!(loaded.kind.cache_tag(), TextureUploadKind::Bc7Unorm.cache_tag());
		assert_eq!(loaded.mips.len(), 1);
		assert_eq!(loaded.mips[0].data, vec![42; 16]);

		let _ = fs::remove_file(path);
	}
}
