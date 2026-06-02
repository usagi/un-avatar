use std::borrow::Cow;

use wgpu::util::DeviceExt;

use crate::{
	mesh_pass::AvatarOutlineOptions,
	options::{BloomOptions, EnvironmentColorOptions, SsaoOptions},
};

const SHADER_AVATAR_OUTLINE: &str = include_str!("../shaders/avatar_outline.wgsl");
const SHADER_BLOOM: &str = include_str!("../shaders/bloom.wgsl");
const SHADER_COLOR_ADJUST: &str = include_str!("../shaders/color_adjust.wgsl");
const SHADER_FXAA: &str = include_str!("../shaders/fxaa.wgsl");
const SHADER_SMAA: &str = include_str!("../shaders/smaa.wgsl");

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct AvatarOutlineUniform {
	color: [f32; 4],
	params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PostUniform {
	color: [f32; 4],
	bloom: [f32; 4],
	ssao: [f32; 4],
	grading: [f32; 4],
}

pub(crate) struct MsaaTarget {
	width: u32,
	height: u32,
	format: wgpu::TextureFormat,
	sample_count: u32,
	color_texture: wgpu::Texture,
	color_view: wgpu::TextureView,
	depth_texture: wgpu::Texture,
	depth_view: wgpu::TextureView,
}

impl MsaaTarget {
	pub(crate) fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat, sample_count: u32) -> Self {
		let width = width.max(1);
		let height = height.max(1);
		let sample_count = sample_count.max(1);
		let (color_texture, color_view) = create_msaa_color_texture(device, width, height, format, sample_count);
		let (depth_texture, depth_view) = create_depth_texture(device, width, height, sample_count);
		Self {
			width,
			height,
			format,
			sample_count,
			color_texture,
			color_view,
			depth_texture,
			depth_view,
		}
	}

	pub(crate) fn resize_to(&mut self, device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat, sample_count: u32) {
		let width = width.max(1);
		let height = height.max(1);
		let sample_count = sample_count.max(1);
		if self.width == width && self.height == height && self.format == format && self.sample_count == sample_count {
			return;
		}
		self.color_texture.destroy();
		self.depth_texture.destroy();
		let (color_texture, color_view) = create_msaa_color_texture(device, width, height, format, sample_count);
		let (depth_texture, depth_view) = create_depth_texture(device, width, height, sample_count);
		self.width = width;
		self.height = height;
		self.format = format;
		self.sample_count = sample_count;
		self.color_texture = color_texture;
		self.color_view = color_view;
		self.depth_texture = depth_texture;
		self.depth_view = depth_view;
	}

	pub(crate) fn color_view(&self) -> &wgpu::TextureView {
		&self.color_view
	}

	pub(crate) fn depth_view(&self) -> &wgpu::TextureView {
		&self.depth_view
	}
}

pub(crate) struct PostProcess {
	width: u32,
	height: u32,
	format: wgpu::TextureFormat,
	source_texture: wgpu::Texture,
	source_view: wgpu::TextureView,
	depth_texture: wgpu::Texture,
	depth_view: wgpu::TextureView,
	smaa_targets: Option<SmaaTargets>,
	bloom_targets: Option<BloomTargets>,
	outline_targets: Option<OutlineTargets>,
	one_texture_layout: wgpu::BindGroupLayout,
	outline_layout: wgpu::BindGroupLayout,
	two_texture_layout: wgpu::BindGroupLayout,
	sampler: wgpu::Sampler,
	nearest_sampler: wgpu::Sampler,
	outline_uniform: wgpu::Buffer,
	post_uniform: wgpu::Buffer,
	source_bind_group: wgpu::BindGroup,
	avatar_outline_mask_pipeline: wgpu::RenderPipeline,
	avatar_outline_smooth_pipeline: wgpu::RenderPipeline,
	avatar_outline_pipeline: wgpu::RenderPipeline,
	color_adjust_pipeline: wgpu::RenderPipeline,
	fxaa_pipeline: wgpu::RenderPipeline,
	bloom_extract_pipeline: wgpu::RenderPipeline,
	bloom_blur_h_pipeline: wgpu::RenderPipeline,
	bloom_blur_v_pipeline: wgpu::RenderPipeline,
	smaa_edge_pipeline: wgpu::RenderPipeline,
	smaa_blend_pipeline: wgpu::RenderPipeline,
	smaa_neighborhood_pipeline: wgpu::RenderPipeline,
}

struct SmaaTargets {
	_edge_texture: wgpu::Texture,
	edge_view: wgpu::TextureView,
	_blend_texture: wgpu::Texture,
	blend_view: wgpu::TextureView,
	source_bind_group: wgpu::BindGroup,
	edge_bind_group: wgpu::BindGroup,
	neighborhood_bind_group: wgpu::BindGroup,
	neighborhood_bloom_bind_group: Option<wgpu::BindGroup>,
}

struct BloomTargets {
	_a_texture: wgpu::Texture,
	a_view: wgpu::TextureView,
	_b_texture: wgpu::Texture,
	b_view: wgpu::TextureView,
	extract_bind_group: wgpu::BindGroup,
	blur_a_bind_group: wgpu::BindGroup,
	blur_b_bind_group: wgpu::BindGroup,
	final_bind_group: wgpu::BindGroup,
}

struct OutlineTargets {
	_mask_texture: wgpu::Texture,
	mask_view: wgpu::TextureView,
	_smooth_texture: wgpu::Texture,
	smooth_view: wgpu::TextureView,
	mask_bind_group: wgpu::BindGroup,
	smooth_bind_group: wgpu::BindGroup,
	final_bind_group: wgpu::BindGroup,
}

impl PostProcess {
	pub(crate) fn new(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Self {
		let width = width.max(1);
		let height = height.max(1);
		let (source_texture, source_view) = create_source_texture(device, width, height, format);
		let (depth_texture, depth_view) = create_depth_texture(device, width, height, 1);
		let one_texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("post-process"),
			entries: &[
				wgpu::BindGroupLayoutEntry {
					binding: 0,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Texture {
						multisampled: false,
						view_dimension: wgpu::TextureViewDimension::D2,
						sample_type: wgpu::TextureSampleType::Float { filterable: true },
					},
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<PostUniform>() as u64),
					},
					count: None,
				},
				depth_layout_entry(3),
				texture_layout_entry(4),
			],
		});
		let outline_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("avatar-outline-post"),
			entries: &[
				texture_layout_entry(0),
				wgpu::BindGroupLayoutEntry {
					binding: 1,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: None,
					},
					count: None,
				},
			],
		});
		let two_texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("post-process-two-texture"),
			entries: &[
				texture_layout_entry(0),
				texture_layout_entry(1),
				wgpu::BindGroupLayoutEntry {
					binding: 2,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
					count: None,
				},
				wgpu::BindGroupLayoutEntry {
					binding: 3,
					visibility: wgpu::ShaderStages::FRAGMENT,
					ty: wgpu::BindingType::Buffer {
						ty: wgpu::BufferBindingType::Uniform,
						has_dynamic_offset: false,
						min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<PostUniform>() as u64),
					},
					count: None,
				},
				depth_layout_entry(4),
				texture_layout_entry(5),
			],
		});
		let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("post-process"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			..Default::default()
		});
		let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("post-process-nearest"),
			address_mode_u: wgpu::AddressMode::ClampToEdge,
			address_mode_v: wgpu::AddressMode::ClampToEdge,
			mag_filter: wgpu::FilterMode::Nearest,
			min_filter: wgpu::FilterMode::Nearest,
			..Default::default()
		});
		let outline_uniform = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("avatar-outline-post-uniform"),
			size: std::mem::size_of::<AvatarOutlineUniform>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let post_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
			label: Some("post-uniform"),
			contents: bytemuck::bytes_of(&post_uniform(
				EnvironmentColorOptions::default(),
				BloomOptions::default(),
				SsaoOptions::default(),
			)),
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
		});
		let source_bind_group = create_one_texture_bind_group(
			device,
			&one_texture_layout,
			&source_view,
			&depth_view,
			&source_view,
			&sampler,
			&post_uniform,
			"post-source",
		);
		let avatar_outline_mask_pipeline = create_post_pipeline(
			device,
			&outline_layout,
			wgpu::TextureFormat::Rgba16Float,
			SHADER_AVATAR_OUTLINE,
			"avatar-outline-seed",
			"fs_seed",
		);
		let avatar_outline_smooth_pipeline = create_post_pipeline(
			device,
			&outline_layout,
			wgpu::TextureFormat::Rgba16Float,
			SHADER_AVATAR_OUTLINE,
			"avatar-outline-jump",
			"fs_jump",
		);
		let avatar_outline_pipeline = create_post_pipeline_with_blend(
			device,
			&outline_layout,
			format,
			SHADER_AVATAR_OUTLINE,
			"avatar-outline-post",
			"fs_main",
			Some(wgpu::BlendState::ALPHA_BLENDING),
		);
		let color_adjust_pipeline =
			create_post_pipeline(device, &one_texture_layout, format, SHADER_COLOR_ADJUST, "color-adjust", "fs_main");
		let fxaa_pipeline = create_post_pipeline(device, &one_texture_layout, format, SHADER_FXAA, "fxaa", "fs_main");
		let bloom_extract_pipeline = create_post_pipeline(
			device,
			&one_texture_layout,
			wgpu::TextureFormat::Rgba16Float,
			SHADER_BLOOM,
			"bloom-extract",
			"fs_extract",
		);
		let bloom_blur_h_pipeline = create_post_pipeline(
			device,
			&one_texture_layout,
			wgpu::TextureFormat::Rgba16Float,
			SHADER_BLOOM,
			"bloom-blur-h",
			"fs_blur_h",
		);
		let bloom_blur_v_pipeline = create_post_pipeline(
			device,
			&one_texture_layout,
			wgpu::TextureFormat::Rgba16Float,
			SHADER_BLOOM,
			"bloom-blur-v",
			"fs_blur_v",
		);
		let smaa_edge_pipeline = create_post_pipeline(
			device,
			&two_texture_layout,
			wgpu::TextureFormat::Rgba8Unorm,
			SHADER_SMAA,
			"smaa-edge",
			"fs_edge",
		);
		let smaa_blend_pipeline = create_post_pipeline(
			device,
			&two_texture_layout,
			wgpu::TextureFormat::Rgba8Unorm,
			SHADER_SMAA,
			"smaa-blend",
			"fs_blend",
		);
		let smaa_neighborhood_pipeline = create_post_pipeline(
			device,
			&two_texture_layout,
			format,
			SHADER_SMAA,
			"smaa-neighborhood",
			"fs_neighborhood",
		);
		Self {
			width,
			height,
			format,
			source_texture,
			source_view,
			depth_texture,
			depth_view,
			smaa_targets: None,
			bloom_targets: None,
			outline_targets: None,
			one_texture_layout,
			outline_layout,
			two_texture_layout,
			sampler,
			nearest_sampler,
			outline_uniform,
			post_uniform,
			source_bind_group,
			avatar_outline_mask_pipeline,
			avatar_outline_smooth_pipeline,
			avatar_outline_pipeline,
			color_adjust_pipeline,
			fxaa_pipeline,
			bloom_extract_pipeline,
			bloom_blur_h_pipeline,
			bloom_blur_v_pipeline,
			smaa_edge_pipeline,
			smaa_blend_pipeline,
			smaa_neighborhood_pipeline,
		}
	}

	pub(crate) fn resize_to(&mut self, device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) {
		let width = width.max(1);
		let height = height.max(1);
		if self.width == width && self.height == height && self.format == format {
			return;
		}
		self.source_texture.destroy();
		self.depth_texture.destroy();
		let (source_texture, source_view) = create_source_texture(device, width, height, format);
		let (depth_texture, depth_view) = create_depth_texture(device, width, height, 1);
		self.width = width;
		self.height = height;
		self.format = format;
		self.source_texture = source_texture;
		self.source_view = source_view;
		self.depth_texture = depth_texture;
		self.depth_view = depth_view;
		self.smaa_targets = None;
		self.bloom_targets = None;
		self.outline_targets = None;
		self.source_bind_group = create_one_texture_bind_group(
			device,
			&self.one_texture_layout,
			&self.source_view,
			&self.depth_view,
			&self.source_view,
			&self.sampler,
			&self.post_uniform,
			"post-source",
		);
		self.fxaa_pipeline = create_post_pipeline(device, &self.one_texture_layout, format, SHADER_FXAA, "fxaa", "fs_main");
		self.color_adjust_pipeline = create_post_pipeline(
			device,
			&self.one_texture_layout,
			format,
			SHADER_COLOR_ADJUST,
			"color-adjust",
			"fs_main",
		);
		self.avatar_outline_pipeline = create_post_pipeline_with_blend(
			device,
			&self.outline_layout,
			format,
			SHADER_AVATAR_OUTLINE,
			"avatar-outline-post",
			"fs_main",
			Some(wgpu::BlendState::ALPHA_BLENDING),
		);
		self.smaa_neighborhood_pipeline = create_post_pipeline(
			device,
			&self.two_texture_layout,
			format,
			SHADER_SMAA,
			"smaa-neighborhood",
			"fs_neighborhood",
		);
	}

	pub(crate) fn source_view(&self) -> &wgpu::TextureView {
		&self.source_view
	}

	pub(crate) fn source_texture(&self) -> &wgpu::Texture {
		&self.source_texture
	}

	pub(crate) fn depth_view(&self) -> &wgpu::TextureView {
		&self.depth_view
	}

	fn ensure_smaa_targets(&mut self, device: &wgpu::Device, with_bloom: bool) {
		if self.smaa_targets.is_none() {
			let (edge_texture, edge_view) = create_intermediate_texture(device, self.width, self.height, "smaa-edges");
			let (blend_texture, blend_view) = create_intermediate_texture(device, self.width, self.height, "smaa-blend");
			let source_bind_group = create_two_texture_bind_group(
				device,
				&self.two_texture_layout,
				&self.source_view,
				&self.source_view,
				&self.depth_view,
				&self.source_view,
				&self.sampler,
				&self.post_uniform,
				"smaa-source",
			);
			let edge_bind_group = create_two_texture_bind_group(
				device,
				&self.two_texture_layout,
				&edge_view,
				&edge_view,
				&self.depth_view,
				&self.source_view,
				&self.nearest_sampler,
				&self.post_uniform,
				"smaa-edges",
			);
			let neighborhood_bind_group = create_two_texture_bind_group(
				device,
				&self.two_texture_layout,
				&self.source_view,
				&blend_view,
				&self.depth_view,
				&self.source_view,
				&self.sampler,
				&self.post_uniform,
				"smaa-neighborhood",
			);
			self.smaa_targets = Some(SmaaTargets {
				_edge_texture: edge_texture,
				edge_view,
				_blend_texture: blend_texture,
				blend_view,
				source_bind_group,
				edge_bind_group,
				neighborhood_bind_group,
				neighborhood_bloom_bind_group: None,
			});
		}
		if with_bloom {
			self.ensure_bloom_targets(device);
			let bloom_view = &self.bloom_targets.as_ref().expect("bloom targets are initialized").a_view;
			let targets = self.smaa_targets.as_mut().expect("smaa targets are initialized");
			if targets.neighborhood_bloom_bind_group.is_none() {
				targets.neighborhood_bloom_bind_group = Some(create_two_texture_bind_group(
					device,
					&self.two_texture_layout,
					&self.source_view,
					&targets.blend_view,
					&self.depth_view,
					bloom_view,
					&self.sampler,
					&self.post_uniform,
					"smaa-neighborhood-bloom",
				));
			}
		}
	}

	fn ensure_bloom_targets(&mut self, device: &wgpu::Device) {
		if self.bloom_targets.is_some() {
			return;
		}
		let (a_texture, a_view) = create_bloom_texture(device, self.width, self.height, "bloom-a");
		let (b_texture, b_view) = create_bloom_texture(device, self.width, self.height, "bloom-b");
		let extract_bind_group = create_one_texture_bind_group(
			device,
			&self.one_texture_layout,
			&self.source_view,
			&self.depth_view,
			&self.source_view,
			&self.sampler,
			&self.post_uniform,
			"bloom-extract",
		);
		let blur_a_bind_group = create_one_texture_bind_group(
			device,
			&self.one_texture_layout,
			&a_view,
			&self.depth_view,
			&a_view,
			&self.sampler,
			&self.post_uniform,
			"bloom-blur-a",
		);
		let blur_b_bind_group = create_one_texture_bind_group(
			device,
			&self.one_texture_layout,
			&b_view,
			&self.depth_view,
			&b_view,
			&self.sampler,
			&self.post_uniform,
			"bloom-blur-b",
		);
		let final_bind_group = create_one_texture_bind_group(
			device,
			&self.one_texture_layout,
			&self.source_view,
			&self.depth_view,
			&a_view,
			&self.sampler,
			&self.post_uniform,
			"post-source-bloom",
		);
		self.bloom_targets = Some(BloomTargets {
			_a_texture: a_texture,
			a_view,
			_b_texture: b_texture,
			b_view,
			extract_bind_group,
			blur_a_bind_group,
			blur_b_bind_group,
			final_bind_group,
		});
	}

	fn ensure_outline_targets(&mut self, device: &wgpu::Device) {
		if self.outline_targets.is_some() {
			return;
		}
		let (mask_texture, mask_view) = create_outline_distance_texture(device, self.width, self.height, "avatar-outline-distance-a");
		let (smooth_texture, smooth_view) = create_outline_distance_texture(device, self.width, self.height, "avatar-outline-distance-b");
		let mask_bind_group = create_outline_bind_group(
			device,
			&self.outline_layout,
			&self.source_view,
			&self.sampler,
			&self.outline_uniform,
		);
		let smooth_bind_group = create_outline_bind_group(device, &self.outline_layout, &mask_view, &self.sampler, &self.outline_uniform);
		let final_bind_group = create_outline_bind_group(device, &self.outline_layout, &smooth_view, &self.sampler, &self.outline_uniform);
		self.outline_targets = Some(OutlineTargets {
			_mask_texture: mask_texture,
			mask_view,
			_smooth_texture: smooth_texture,
			smooth_view,
			mask_bind_group,
			smooth_bind_group,
			final_bind_group,
		});
	}

	pub(crate) fn encode_color_adjust(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		encoder: &mut wgpu::CommandEncoder,
		target_view: &wgpu::TextureView,
		color: EnvironmentColorOptions,
		bloom: BloomOptions,
		ssao: SsaoOptions,
	) {
		queue.write_buffer(&self.post_uniform, 0, bytemuck::bytes_of(&post_uniform(color, bloom, ssao)));
		let high_quality_bloom = self.prepare_bloom(device, encoder, bloom);
		let bind_group = if high_quality_bloom {
			&self.bloom_targets.as_ref().expect("bloom targets are initialized").final_bind_group
		} else {
			&self.source_bind_group
		};
		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			label: Some("color-adjust"),
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: target_view,
				depth_slice: None,
				resolve_target: None,
				ops: wgpu::Operations {
					load: wgpu::LoadOp::Load,
					store: wgpu::StoreOp::Store,
				},
			})],
			depth_stencil_attachment: None,
			timestamp_writes: None,
			occlusion_query_set: None,
			multiview_mask: None,
		});
		pass.set_pipeline(&self.color_adjust_pipeline);
		pass.set_bind_group(0, bind_group, &[]);
		pass.draw(0..3, 0..1);
	}

	pub(crate) fn encode_fxaa(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		encoder: &mut wgpu::CommandEncoder,
		target_view: &wgpu::TextureView,
		color: EnvironmentColorOptions,
		bloom: BloomOptions,
		ssao: SsaoOptions,
	) {
		queue.write_buffer(&self.post_uniform, 0, bytemuck::bytes_of(&post_uniform(color, bloom, ssao)));
		let high_quality_bloom = self.prepare_bloom(device, encoder, bloom);
		let bind_group = if high_quality_bloom {
			&self.bloom_targets.as_ref().expect("bloom targets are initialized").final_bind_group
		} else {
			&self.source_bind_group
		};
		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			label: Some("fxaa"),
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: target_view,
				depth_slice: None,
				resolve_target: None,
				ops: wgpu::Operations {
					load: wgpu::LoadOp::Load,
					store: wgpu::StoreOp::Store,
				},
			})],
			depth_stencil_attachment: None,
			timestamp_writes: None,
			occlusion_query_set: None,
			multiview_mask: None,
		});
		pass.set_pipeline(&self.fxaa_pipeline);
		pass.set_bind_group(0, bind_group, &[]);
		pass.draw(0..3, 0..1);
	}

	pub(crate) fn encode_avatar_outline(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		encoder: &mut wgpu::CommandEncoder,
		target_view: &wgpu::TextureView,
		outline: AvatarOutlineOptions,
		width_px: f32,
	) {
		self.ensure_outline_targets(device);
		let targets = self.outline_targets.as_ref().expect("outline targets are initialized");
		let color = outline.color.unwrap_or([0.02, 0.01, 0.03]);
		let roundness = outline.roundness.unwrap_or(0.5).clamp(0.0, 1.0);
		let write_uniform = |jump_step: f32| {
			let uniform = AvatarOutlineUniform {
				color: [color[0], color[1], color[2], 1.0],
				params: [width_px.clamp(0.0, 96.0), outline.lighting_mix.unwrap_or(0.0), roundness, jump_step],
			};
			queue.write_buffer(&self.outline_uniform, 0, bytemuck::bytes_of(&uniform));
		};
		write_uniform(0.0);
		self.encode_avatar_outline_stage(
			encoder,
			"avatar-outline-seed",
			&targets.mask_view,
			&self.avatar_outline_mask_pipeline,
			&targets.mask_bind_group,
			wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
		);
		let mut source_is_mask = true;
		for jump_step in [64.0, 32.0, 16.0, 8.0, 4.0, 2.0, 1.0] {
			write_uniform(jump_step);
			if source_is_mask {
				self.encode_avatar_outline_stage(
					encoder,
					"avatar-outline-jump",
					&targets.smooth_view,
					&self.avatar_outline_smooth_pipeline,
					&targets.smooth_bind_group,
					wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
				);
			} else {
				self.encode_avatar_outline_stage(
					encoder,
					"avatar-outline-jump",
					&targets.mask_view,
					&self.avatar_outline_smooth_pipeline,
					&targets.final_bind_group,
					wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
				);
			}
			source_is_mask = !source_is_mask;
		}
		write_uniform(0.0);
		self.encode_avatar_outline_stage(
			encoder,
			"avatar-outline-post",
			target_view,
			&self.avatar_outline_pipeline,
			if source_is_mask {
				&targets.smooth_bind_group
			} else {
				&targets.final_bind_group
			},
			wgpu::LoadOp::Load,
		);
	}

	fn encode_avatar_outline_stage(
		&self,
		encoder: &mut wgpu::CommandEncoder,
		label: &'static str,
		target_view: &wgpu::TextureView,
		pipeline: &wgpu::RenderPipeline,
		bind_group: &wgpu::BindGroup,
		load: wgpu::LoadOp<wgpu::Color>,
	) {
		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			label: Some(label),
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: target_view,
				depth_slice: None,
				resolve_target: None,
				ops: wgpu::Operations {
					load,
					store: wgpu::StoreOp::Store,
				},
			})],
			depth_stencil_attachment: None,
			timestamp_writes: None,
			occlusion_query_set: None,
			multiview_mask: None,
		});
		pass.set_pipeline(pipeline);
		pass.set_bind_group(0, bind_group, &[]);
		pass.draw(0..3, 0..1);
	}

	pub(crate) fn encode_smaa(
		&mut self,
		device: &wgpu::Device,
		queue: &wgpu::Queue,
		encoder: &mut wgpu::CommandEncoder,
		target_view: &wgpu::TextureView,
		color: EnvironmentColorOptions,
		bloom: BloomOptions,
		ssao: SsaoOptions,
	) {
		queue.write_buffer(&self.post_uniform, 0, bytemuck::bytes_of(&post_uniform(color, bloom, ssao)));
		let use_high_quality_bloom = bloom.is_enabled() && bloom.quality.is_high_quality();
		if use_high_quality_bloom {
			self.ensure_bloom_targets(device);
		}
		self.ensure_smaa_targets(device, use_high_quality_bloom);
		self.prepare_bloom(device, encoder, bloom);
		let targets = self.smaa_targets.as_ref().expect("smaa targets are initialized");
		self.encode_one_texture_pass(
			encoder,
			"smaa-edge",
			&targets.edge_view,
			&self.smaa_edge_pipeline,
			&targets.source_bind_group,
		);
		self.encode_one_texture_pass(
			encoder,
			"smaa-blend",
			&targets.blend_view,
			&self.smaa_blend_pipeline,
			&targets.edge_bind_group,
		);

		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			label: Some("smaa-neighborhood"),
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: target_view,
				depth_slice: None,
				resolve_target: None,
				ops: wgpu::Operations {
					load: wgpu::LoadOp::Load,
					store: wgpu::StoreOp::Store,
				},
			})],
			depth_stencil_attachment: None,
			timestamp_writes: None,
			occlusion_query_set: None,
			multiview_mask: None,
		});
		pass.set_pipeline(&self.smaa_neighborhood_pipeline);
		pass.set_bind_group(
			0,
			if use_high_quality_bloom {
				targets
					.neighborhood_bloom_bind_group
					.as_ref()
					.expect("smaa bloom bind group is initialized")
			} else {
				&targets.neighborhood_bind_group
			},
			&[],
		);
		pass.draw(0..3, 0..1);
	}

	fn prepare_bloom(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, bloom: BloomOptions) -> bool {
		if !bloom.is_enabled() || !bloom.quality.is_high_quality() {
			return false;
		}
		self.ensure_bloom_targets(device);
		let targets = self.bloom_targets.as_ref().expect("bloom targets are initialized");
		self.encode_one_texture_pass(
			encoder,
			"bloom-extract",
			&targets.a_view,
			&self.bloom_extract_pipeline,
			&targets.extract_bind_group,
		);
		self.encode_one_texture_pass(
			encoder,
			"bloom-blur-h",
			&targets.b_view,
			&self.bloom_blur_h_pipeline,
			&targets.blur_a_bind_group,
		);
		self.encode_one_texture_pass(
			encoder,
			"bloom-blur-v",
			&targets.a_view,
			&self.bloom_blur_v_pipeline,
			&targets.blur_b_bind_group,
		);
		true
	}

	fn encode_one_texture_pass(
		&self,
		encoder: &mut wgpu::CommandEncoder,
		label: &'static str,
		target_view: &wgpu::TextureView,
		pipeline: &wgpu::RenderPipeline,
		bind_group: &wgpu::BindGroup,
	) {
		let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
			label: Some(label),
			color_attachments: &[Some(wgpu::RenderPassColorAttachment {
				view: target_view,
				depth_slice: None,
				resolve_target: None,
				ops: wgpu::Operations {
					load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
					store: wgpu::StoreOp::Store,
				},
			})],
			depth_stencil_attachment: None,
			timestamp_writes: None,
			occlusion_query_set: None,
			multiview_mask: None,
		});
		pass.set_pipeline(pipeline);
		pass.set_bind_group(0, bind_group, &[]);
		pass.draw(0..3, 0..1);
	}
}

fn create_source_texture(
	device: &wgpu::Device,
	width: u32,
	height: u32,
	format: wgpu::TextureFormat,
) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("post-source"),
		size: wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}

fn create_msaa_color_texture(
	device: &wgpu::Device,
	width: u32,
	height: u32,
	format: wgpu::TextureFormat,
	sample_count: u32,
) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("msaa-color"),
		size: wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count,
		dimension: wgpu::TextureDimension::D2,
		format,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}

fn create_intermediate_texture(device: &wgpu::Device, width: u32, height: u32, label: &'static str) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some(label),
		size: wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Rgba8Unorm,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}

fn create_bloom_texture(device: &wgpu::Device, width: u32, height: u32, label: &'static str) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some(label),
		size: wgpu::Extent3d {
			width: (width / 2).max(1),
			height: (height / 2).max(1),
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Rgba16Float,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}

fn create_outline_distance_texture(
	device: &wgpu::Device,
	width: u32,
	height: u32,
	label: &'static str,
) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some(label),
		size: wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Rgba16Float,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}

fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32, sample_count: u32) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("post-depth"),
		size: wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Depth24Plus,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}

fn texture_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
	wgpu::BindGroupLayoutEntry {
		binding,
		visibility: wgpu::ShaderStages::FRAGMENT,
		ty: wgpu::BindingType::Texture {
			multisampled: false,
			view_dimension: wgpu::TextureViewDimension::D2,
			sample_type: wgpu::TextureSampleType::Float { filterable: true },
		},
		count: None,
	}
}

fn depth_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
	wgpu::BindGroupLayoutEntry {
		binding,
		visibility: wgpu::ShaderStages::FRAGMENT,
		ty: wgpu::BindingType::Texture {
			multisampled: false,
			view_dimension: wgpu::TextureViewDimension::D2,
			sample_type: wgpu::TextureSampleType::Depth,
		},
		count: None,
	}
}

#[allow(clippy::too_many_arguments)]
fn create_one_texture_bind_group(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	view: &wgpu::TextureView,
	depth_view: &wgpu::TextureView,
	bloom_view: &wgpu::TextureView,
	sampler: &wgpu::Sampler,
	post_uniform: &wgpu::Buffer,
	label: &'static str,
) -> wgpu::BindGroup {
	device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some(label),
		layout,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: wgpu::BindingResource::TextureView(view),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: wgpu::BindingResource::Sampler(sampler),
			},
			wgpu::BindGroupEntry {
				binding: 2,
				resource: post_uniform.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 3,
				resource: wgpu::BindingResource::TextureView(depth_view),
			},
			wgpu::BindGroupEntry {
				binding: 4,
				resource: wgpu::BindingResource::TextureView(bloom_view),
			},
		],
	})
}

fn create_outline_bind_group(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	view: &wgpu::TextureView,
	sampler: &wgpu::Sampler,
	uniform: &wgpu::Buffer,
) -> wgpu::BindGroup {
	device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some("avatar-outline-post"),
		layout,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: wgpu::BindingResource::TextureView(view),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: wgpu::BindingResource::Sampler(sampler),
			},
			wgpu::BindGroupEntry {
				binding: 2,
				resource: uniform.as_entire_binding(),
			},
		],
	})
}

fn post_uniform(color: EnvironmentColorOptions, bloom: BloomOptions, ssao: SsaoOptions) -> PostUniform {
	PostUniform {
		color: [
			color.exposure.clamp(-4.0, 4.0),
			color.contrast.clamp(0.0, 4.0),
			color.saturation.clamp(0.0, 4.0),
			if bloom.is_enabled() { bloom.strength.clamp(0.0, 2.0) } else { 0.0 },
		],
		bloom: [
			bloom.threshold.clamp(0.0, 2.0),
			bloom.radius.clamp(0.0, 32.0),
			if bloom.is_enabled() && bloom.quality.is_high_quality() {
				1.0
			} else {
				0.0
			},
			0.0,
		],
		ssao: [
			if ssao.is_enabled() { ssao.strength.clamp(0.0, 1.0) } else { 0.0 },
			ssao.radius.clamp(1.0, 24.0),
			ssao.bias.clamp(0.0, 0.02),
			ssao.range.clamp(0.001, 0.2),
		],
		grading: [
			color.look.shader_id(),
			color.look_intensity.clamp(0.0, 1.0),
			color.temperature.clamp(-1.0, 1.0),
			color.tint.clamp(-1.0, 1.0),
		],
	}
}

#[allow(clippy::too_many_arguments)]
fn create_two_texture_bind_group(
	device: &wgpu::Device,
	layout: &wgpu::BindGroupLayout,
	first_view: &wgpu::TextureView,
	second_view: &wgpu::TextureView,
	depth_view: &wgpu::TextureView,
	bloom_view: &wgpu::TextureView,
	sampler: &wgpu::Sampler,
	post_uniform: &wgpu::Buffer,
	label: &'static str,
) -> wgpu::BindGroup {
	device.create_bind_group(&wgpu::BindGroupDescriptor {
		label: Some(label),
		layout,
		entries: &[
			wgpu::BindGroupEntry {
				binding: 0,
				resource: wgpu::BindingResource::TextureView(first_view),
			},
			wgpu::BindGroupEntry {
				binding: 1,
				resource: wgpu::BindingResource::TextureView(second_view),
			},
			wgpu::BindGroupEntry {
				binding: 2,
				resource: wgpu::BindingResource::Sampler(sampler),
			},
			wgpu::BindGroupEntry {
				binding: 3,
				resource: post_uniform.as_entire_binding(),
			},
			wgpu::BindGroupEntry {
				binding: 4,
				resource: wgpu::BindingResource::TextureView(depth_view),
			},
			wgpu::BindGroupEntry {
				binding: 5,
				resource: wgpu::BindingResource::TextureView(bloom_view),
			},
		],
	})
}

fn create_post_pipeline(
	device: &wgpu::Device,
	bind_layout: &wgpu::BindGroupLayout,
	format: wgpu::TextureFormat,
	shader_source: &'static str,
	label: &'static str,
	fragment_entry: &'static str,
) -> wgpu::RenderPipeline {
	create_post_pipeline_with_blend(device, bind_layout, format, shader_source, label, fragment_entry, None)
}

fn create_post_pipeline_with_blend(
	device: &wgpu::Device,
	bind_layout: &wgpu::BindGroupLayout,
	format: wgpu::TextureFormat,
	shader_source: &'static str,
	label: &'static str,
	fragment_entry: &'static str,
	blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some(label),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(shader_source)),
	});
	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some(label),
		bind_group_layouts: &[Some(bind_layout)],
		immediate_size: 0,
	});
	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some(label),
		layout: Some(&layout),
		cache: None,
		vertex: wgpu::VertexState {
			module: &shader,
			entry_point: Some("vs_main"),
			compilation_options: Default::default(),
			buffers: &[],
		},
		fragment: Some(wgpu::FragmentState {
			module: &shader,
			entry_point: Some(fragment_entry),
			compilation_options: Default::default(),
			targets: &[Some(wgpu::ColorTargetState {
				format,
				blend,
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState {
			topology: wgpu::PrimitiveTopology::TriangleList,
			..Default::default()
		},
		depth_stencil: None,
		multisample: wgpu::MultisampleState::default(),
		multiview_mask: None,
	})
}
