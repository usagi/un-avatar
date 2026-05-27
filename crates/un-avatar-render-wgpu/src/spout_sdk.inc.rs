use std::{
	borrow::Cow,
	sync::{
		atomic::{AtomicU8, Ordering},
		Arc,
	},
	time::Instant,
};

use spout_rs::SpoutSender;

/// Readback リングの 1 スロット。GPU からの copy_texture_to_buffer 先と map_async の状態を持つ。
const READBACK_STATE_IDLE: u8 = 0;
const READBACK_STATE_PENDING: u8 = 1;
const READBACK_STATE_READY: u8 = 2;
const READBACK_RING_LEN: usize = 2;

struct ReadbackSlot {
	buf: wgpu::Buffer,
	state: Arc<AtomicU8>,
}

pub(crate) struct SpoutCapture {
	sender: SpoutSender,
	logical_w: u32,
	logical_h: u32,
	format: wgpu::TextureFormat,
	rgba_format: wgpu::TextureFormat,
	color_tex: wgpu::Texture,
	color_view: wgpu::TextureView,
	depth_tex: wgpu::Texture,
	depth_view: wgpu::TextureView,
	rgba_tex: wgpu::Texture,
	rgba_view: wgpu::TextureView,
	readback_ring: [ReadbackSlot; READBACK_RING_LEN],
	write_idx: usize,
	padded_bpr: u32,
	rgba_scratch: Vec<u8>,
	stats: SpoutFrameStats,
	blit_pipeline: wgpu::RenderPipeline,
	blit_bind_layout: wgpu::BindGroupLayout,
	blit_sampler: wgpu::Sampler,
	blit_bind: wgpu::BindGroup,
	swizzle_pipeline: wgpu::RenderPipeline,
	swizzle_bind: wgpu::BindGroup,
}

/// Spout2 が受け取る RGBA フォーマット。送出元 surface の sRGB 有無に合わせる。
fn rgba_format_for(source_format: wgpu::TextureFormat) -> wgpu::TextureFormat {
	match source_format {
		wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Rgba8UnormSrgb => wgpu::TextureFormat::Rgba8UnormSrgb,
		_ => wgpu::TextureFormat::Rgba8Unorm,
	}
}

impl SpoutCapture {
	pub fn try_new(
		device: &wgpu::Device,
		surface_format: wgpu::TextureFormat,
		window_w: u32,
		window_h: u32,
		cfg: SpoutLaunchConfig,
	) -> Option<Self> {
		let logical_w = cfg.width.unwrap_or(window_w).max(1);
		let logical_h = cfg.height.unwrap_or(window_h).max(1);
		let mut sender = SpoutSender::new(&cfg.name);
		let sender_initialized = sender.is_initialized();
		let (sender_width, sender_height) = sender.size();

		let (color_tex, color_view) = Self::make_color(device, logical_w, logical_h, surface_format);
		let (depth_tex, depth_view) = create_spout_depth(device, logical_w, logical_h);
		let rgba_format = rgba_format_for(surface_format);
		let (rgba_tex, rgba_view) = Self::make_rgba(device, logical_w, logical_h, rgba_format);
		let (readback_ring, padded_bpr) = Self::make_readback_ring(device, logical_w, logical_h);

		let blit_bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("spout-blit"),
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
			],
		});
		let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
			label: Some("spout-blit"),
			mag_filter: wgpu::FilterMode::Linear,
			min_filter: wgpu::FilterMode::Linear,
			..Default::default()
		});
		let blit_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("spout-blit"),
			layout: &blit_bind_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&color_view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&blit_sampler),
				},
			],
		});

		let blit_pipeline = {
			let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
				label: Some("spout-blit"),
				source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("../shaders/blit.wgsl"))),
			});
			let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("spout-blit"),
				bind_group_layouts: &[Some(&blit_bind_layout)],
				immediate_size: 0,
			});
			device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("spout-blit"),
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
					entry_point: Some("fs_main"),
					compilation_options: Default::default(),
					targets: &[Some(wgpu::ColorTargetState {
						format: surface_format,
						blend: None,
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
		};

		let swizzle_pipeline = {
			let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
				label: Some("spout-swizzle"),
				source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("../shaders/blit.wgsl"))),
			});
			let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
				label: Some("spout-swizzle"),
				bind_group_layouts: &[Some(&blit_bind_layout)],
				immediate_size: 0,
			});
			device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
				label: Some("spout-swizzle"),
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
					entry_point: Some("fs_main"),
					compilation_options: Default::default(),
					targets: &[Some(wgpu::ColorTargetState {
						format: rgba_format,
						blend: None,
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
		};
		let swizzle_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("spout-swizzle"),
			layout: &blit_bind_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&color_view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&blit_sampler),
				},
			],
		});

		Some(Self {
			sender,
			logical_w,
			logical_h,
			format: surface_format,
			rgba_format,
			color_tex,
			color_view,
			depth_tex,
			depth_view,
			rgba_tex,
			rgba_view,
			readback_ring,
			write_idx: 0,
			padded_bpr,
			rgba_scratch: vec![0; (logical_w * logical_h * 4) as usize],
			stats: SpoutFrameStats {
				sender_initialized: Some(sender_initialized),
				sender_width: Some(sender_width),
				sender_height: Some(sender_height),
				..Default::default()
			},
			blit_pipeline,
			blit_bind_layout,
			blit_sampler,
			blit_bind,
			swizzle_pipeline,
			swizzle_bind,
		})
	}

	fn make_readback_ring(device: &wgpu::Device, width: u32, height: u32) -> ([ReadbackSlot; READBACK_RING_LEN], u32) {
		let unpadded = width * 4;
		let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
		let padded_bpr = unpadded.div_ceil(align) * align;
		let size = (padded_bpr as u64) * (height as u64);
		let mut slots: [Option<ReadbackSlot>; READBACK_RING_LEN] = [const { None }; READBACK_RING_LEN];
		for slot in &mut slots {
			*slot = Some(ReadbackSlot {
				buf: device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("spout-readback"),
					size,
					usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
					mapped_at_creation: false,
				}),
				state: Arc::new(AtomicU8::new(READBACK_STATE_IDLE)),
			});
		}
		let ring = [slots[0].take().unwrap(), slots[1].take().unwrap()];
		(ring, padded_bpr)
	}

	fn make_rgba(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> (wgpu::Texture, wgpu::TextureView) {
		let tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("spout-rgba"),
			size: wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
			view_formats: &[],
		});
		let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
		(tex, view)
	}

	fn make_color(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> (wgpu::Texture, wgpu::TextureView) {
		let tex = device.create_texture(&wgpu::TextureDescriptor {
			label: Some("spout-color"),
			size: wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
			view_formats: &[],
		});
		let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
		(tex, view)
	}

	pub fn dimensions(&self) -> (u32, u32) {
		(self.logical_w, self.logical_h)
	}

	pub fn resize_to(
		&mut self,
		device: &wgpu::Device,
		window_w: u32,
		window_h: u32,
		cfg: &SpoutLaunchConfig,
		surface_format: wgpu::TextureFormat,
	) {
		let nw = cfg.width.unwrap_or(window_w).max(1);
		let nh = cfg.height.unwrap_or(window_h).max(1);
		if nw == self.logical_w && nh == self.logical_h && surface_format == self.format {
			return;
		}
		self.logical_w = nw;
		self.logical_h = nh;
		self.format = surface_format;
		self.rgba_format = rgba_format_for(surface_format);
		self.color_tex.destroy();
		self.depth_tex.destroy();
		self.rgba_tex.destroy();
		let (ct, cv) = Self::make_color(device, nw, nh, surface_format);
		let (dt, dv) = create_spout_depth(device, nw, nh);
		let (rt, rv) = Self::make_rgba(device, nw, nh, self.rgba_format);
		self.color_tex = ct;
		self.color_view = cv;
		self.depth_tex = dt;
		self.depth_view = dv;
		self.rgba_tex = rt;
		self.rgba_view = rv;
		for slot in &mut self.readback_ring {
			slot.buf.destroy();
		}
		let (ring, pad) = Self::make_readback_ring(device, nw, nh);
		self.readback_ring = ring;
		self.write_idx = 0;
		self.padded_bpr = pad;
		self.rgba_scratch.resize((nw * nh * 4) as usize, 0);

		self.blit_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("spout-blit"),
			layout: &self.blit_bind_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&self.color_view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
				},
			],
		});
		self.swizzle_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("spout-swizzle"),
			layout: &self.blit_bind_layout,
			entries: &[
				wgpu::BindGroupEntry {
					binding: 0,
					resource: wgpu::BindingResource::TextureView(&self.color_view),
				},
				wgpu::BindGroupEntry {
					binding: 1,
					resource: wgpu::BindingResource::Sampler(&self.blit_sampler),
				},
			],
		});
	}

	pub fn color_view(&self) -> &wgpu::TextureView {
		&self.color_view
	}

	pub fn depth_view(&self) -> &wgpu::TextureView {
		&self.depth_view
	}

	/// 現フレームの swizzle 描画とリング書き込みを encode する。リングに空きが無ければ何もしない。
	/// 戻り値は書き込んだスロット index（map 要求対象）。
	pub fn copy_to_staging(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<usize> {
		let idx = self.write_idx;
		if self.readback_ring[idx].state.load(Ordering::Acquire) != READBACK_STATE_IDLE {
			return None;
		}
		{
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("spout-swizzle"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: &self.rgba_view,
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
			pass.set_pipeline(&self.swizzle_pipeline);
			pass.set_bind_group(0, &self.swizzle_bind, &[]);
			pass.draw(0..3, 0..1);
		}
		encoder.copy_texture_to_buffer(
			wgpu::TexelCopyTextureInfo {
				texture: &self.rgba_tex,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			wgpu::TexelCopyBufferInfo {
				buffer: &self.readback_ring[idx].buf,
				layout: wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(self.padded_bpr),
					rows_per_image: Some(self.logical_h),
				},
			},
			wgpu::Extent3d {
				width: self.logical_w,
				height: self.logical_h,
				depth_or_array_layers: 1,
			},
		);
		Some(idx)
	}

	/// `copy_to_staging` で書いたスロットに対し submit 後に map_async を要求する。
	pub fn after_submit_request_map(&mut self, idx: usize) {
		let state = Arc::clone(&self.readback_ring[idx].state);
		state.store(READBACK_STATE_PENDING, Ordering::Release);
		let cb_state = Arc::clone(&state);
		self.readback_ring[idx].buf.slice(..).map_async(wgpu::MapMode::Read, move |result| {
			if result.is_ok() {
				cb_state.store(READBACK_STATE_READY, Ordering::Release);
			} else {
				cb_state.store(READBACK_STATE_IDLE, Ordering::Release);
			}
		});
		self.write_idx = (self.write_idx + 1) % READBACK_RING_LEN;
	}

	/// Ready 状態のリングスロットを 1 つドレインして Spout2 に送る。`device.poll(Poll)` でコールバックを進めるのは呼び出し側に任せる。
	pub fn send_mapped_rgba(&mut self, device: &wgpu::Device) {
		// map_async コールバックの完了を進める（非ブロッキング）。
		device.poll(wgpu::PollType::Poll).ok();

		// 最も古い Ready スロット = (write_idx) → (write_idx - 1) の順。先に書いたほうから送る。
		let ready_idx = (0..READBACK_RING_LEN).find_map(|offset| {
			let idx = (self.write_idx + offset) % READBACK_RING_LEN;
			if self.readback_ring[idx].state.load(Ordering::Acquire) == READBACK_STATE_READY {
				Some(idx)
			} else {
				None
			}
		});
		let Some(idx) = ready_idx else {
			return;
		};

		let total_start = Instant::now();
		let readback_start = Instant::now();
		{
			let data = self.readback_ring[idx].buf.slice(..).get_mapped_range();
			let w = self.logical_w as usize;
			let h = self.logical_h as usize;
			let bpp = 4;
			let row_src = self.padded_bpr as usize;
			let row_dst = w * bpp;
			self.rgba_scratch.resize(w * h * bpp, 0);
			if row_src == row_dst {
				self.rgba_scratch[..h * row_dst].copy_from_slice(&data[..h * row_dst]);
			} else {
				for row in 0..h {
					let s_off = row * row_src;
					let d_off = row * row_dst;
					self.rgba_scratch[d_off..d_off + row_dst].copy_from_slice(&data[s_off..s_off + row_dst]);
				}
			}
		}
		self.readback_ring[idx].buf.unmap();
		self.readback_ring[idx].state.store(READBACK_STATE_IDLE, Ordering::Release);
		let readback_ms = readback_start.elapsed().as_secs_f32() * 1000.0;
		let send_start = Instant::now();
		let send_ok = self.sender.send_image_rgba(&self.rgba_scratch, self.logical_w, self.logical_h);
		let send_ms = send_start.elapsed().as_secs_f32() * 1000.0;
		let sender_initialized = self.sender.is_initialized();
		let (sender_width, sender_height) = self.sender.size();

		self.stats.frames_attempted = self.stats.frames_attempted.saturating_add(1);
		if send_ok {
			self.stats.frames_sent = self.stats.frames_sent.saturating_add(1);
			self.stats.consecutive_failures = 0;
		} else {
			self.stats.frame_failures = self.stats.frame_failures.saturating_add(1);
			self.stats.consecutive_failures = self.stats.consecutive_failures.saturating_add(1);
		}
		self.stats.last_send_ok = Some(send_ok);
		self.stats.last_readback_ms = Some(readback_ms);
		self.stats.last_send_ms = Some(send_ms);
		self.stats.last_total_ms = Some(total_start.elapsed().as_secs_f32() * 1000.0);
		self.stats.sender_initialized = Some(sender_initialized);
		self.stats.sender_width = Some(sender_width);
		self.stats.sender_height = Some(sender_height);
	}

	pub fn stats(&self) -> SpoutFrameStats {
		self.stats
	}

	pub fn encode_blit(&self, encoder: &mut wgpu::CommandEncoder, swap_view: &wgpu::TextureView, clear: wgpu::Color) {
		{
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("spout-blit"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: swap_view,
					depth_slice: None,
					resolve_target: None,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(clear),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: None,
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			pass.set_pipeline(&self.blit_pipeline);
			pass.set_bind_group(0, &self.blit_bind, &[]);
			pass.draw(0..3, 0..1);
		}
	}
}

fn create_spout_depth(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("spout-depth"),
		size: wgpu::Extent3d {
			width,
			height,
			depth_or_array_layers: 1,
		},
		mip_level_count: 1,
		sample_count: 1,
		dimension: wgpu::TextureDimension::D2,
		format: wgpu::TextureFormat::Depth24Plus,
		usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
		view_formats: &[],
	});
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	(texture, view)
}
