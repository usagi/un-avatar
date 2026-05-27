use std::{
	cmp::min,
	sync::{Arc, Mutex},
	thread,
	time::Instant,
};
#[cfg(feature = "bcn-gpu-bench")]
use std::{io::Write, time::Duration};

use block_compression::{encode::compress_rgba8, BC7Settings, CompressionVariant};
use clap::Parser;
use image::{imageops::FilterType, RgbaImage};
use pic_scale::{ImageSize, ImageStore, ImageStoreMut, ResamplingFunction, Scaler, ThreadingPolicy};
#[cfg(feature = "bcn-gpu-bench")]
use wgpu::util::DeviceExt;

#[derive(Debug, Parser)]
struct Args {
	#[arg(long, default_value_t = 2048)]
	width: u32,
	#[arg(long, default_value_t = 2048)]
	height: u32,
	#[arg(long, default_value_t = 3)]
	iterations: u32,
	#[arg(long, default_value_t = 128)]
	stripe_px: u32,
	#[arg(long)]
	image: Option<std::path::PathBuf>,
	#[arg(long, default_value_t = false)]
	skip_gpu: bool,
	#[arg(long, default_value_t = false)]
	skip_compression: bool,
	#[arg(long, default_value = default_backend())]
	backend: String,
}

fn main() {
	let args = Args::parse();
	let (width, height, rgba) = load_or_generate_rgba(&args);
	let (width, height, rgba) = pad_rgba_4(width, height, &rgba);
	println!(
		"bcn-bench input={}x{} pixels={} bytes={}",
		width,
		height,
		width as u64 * height as u64,
		rgba.len()
	);
	println!("iterations={} stripe_px={}", args.iterations, args.stripe_px.max(4) & !3);
	if let Ok(cores) = thread::available_parallelism() {
		println!("logical_cores={}", cores.get());
	}

	for mip in [
		MipFilter::Box2x2,
		MipFilter::Triangle,
		MipFilter::CatmullRom,
		MipFilter::Lanczos3,
		MipFilter::MitchellNetravali,
		MipFilter::PicScaleBilinearSingle,
		MipFilter::PicScaleBicubicSingle,
		MipFilter::PicScaleCatmullRomSingle,
		MipFilter::PicScaleLanczos3Single,
		MipFilter::PicScaleMitchellSingle,
		MipFilter::PicScaleMitchellAdaptive,
	] {
		let elapsed = bench(args.iterations, || {
			let _ = build_mip_chain(&rgba, width, height, mip);
		});
		println!("mip {:>18}: {:>8.3} ms/iter", mip.name(), elapsed);
	}

	if !args.skip_compression {
		for codec in CompressionCodec::all() {
			for threads in [1usize, 2, 4, 8] {
				let elapsed = bench(args.iterations, || {
					let _ = encode_cpu_striped_parallel(codec.variant(&rgba), &rgba, width, height, args.stripe_px, threads);
				});
				println!("{} cpu stripe {:>2}t: {:>8.3} ms/iter", codec.name(), threads, elapsed);
			}
		}
	}

	if !args.skip_gpu {
		#[cfg(not(feature = "bcn-gpu-bench"))]
		{
			eprintln!("bcn gpu skipped: build with --features bcn-gpu-bench");
		}
		#[cfg(feature = "bcn-gpu-bench")]
		{
			eprintln!("starting gpu bcn backend={}", args.backend);
			let _ = std::io::stderr().flush();
			match bench_gpu_bcn(&rgba, width, height, args.iterations, &args.backend) {
				Ok((init_ms, results)) => {
					println!("bcn gpu init       : {:>8.3} ms", init_ms);
					for (codec, gpu_ms, gpu_readback_ms) in results {
						println!("{} gpu dispatch   : {:>8.3} ms/iter", codec.name(), gpu_ms);
						println!("{} gpu readback   : {:>8.3} ms/iter", codec.name(), gpu_readback_ms);
					}
				}
				Err(e) => {
					eprintln!("bcn gpu skipped: {e}");
				}
			}
		}
	}
}

fn default_backend() -> &'static str {
	#[cfg(windows)]
	{
		"vulkan"
	}
	#[cfg(not(windows))]
	{
		"all"
	}
}

fn bench(iterations: u32, mut f: impl FnMut()) -> f64 {
	let iterations = iterations.max(1);
	f();
	let start = Instant::now();
	for _ in 0..iterations {
		f();
	}
	start.elapsed().as_secs_f64() * 1000.0 / f64::from(iterations)
}

fn load_or_generate_rgba(args: &Args) -> (u32, u32, Vec<u8>) {
	if let Some(path) = &args.image {
		let image = image::open(path)
			.unwrap_or_else(|e| panic!("load image {}: {e}", path.display()))
			.to_rgba8();
		return (image.width(), image.height(), image.into_raw());
	}
	let width = args.width.max(4);
	let height = args.height.max(4);
	let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
	for y in 0..height {
		for x in 0..width {
			let i = ((y * width + x) as usize) * 4;
			let checker = (((x / 31) ^ (y / 29)) & 1) as u8;
			rgba[i] = ((x * 255 / width) as u8).saturating_add(checker * 21);
			rgba[i + 1] = ((y * 255 / height) as u8).saturating_add(checker * 13);
			rgba[i + 2] = (((x + y) * 255 / (width + height)) as u8).saturating_add(checker * 37);
			rgba[i + 3] = 255;
		}
	}
	(width, height, rgba)
}

fn pad_rgba_4(width: u32, height: u32, rgba: &[u8]) -> (u32, u32, Vec<u8>) {
	let width = width.max(1);
	let height = height.max(1);
	let padded_width = width.div_ceil(4) * 4;
	let padded_height = height.div_ceil(4) * 4;
	if padded_width == width && padded_height == height {
		return (width, height, rgba.to_vec());
	}
	let mut out = vec![0; (padded_width as usize) * (padded_height as usize) * 4];
	for y in 0..padded_height {
		let sy = y.min(height - 1);
		for x in 0..padded_width {
			let sx = x.min(width - 1);
			let si = ((sy * width + sx) as usize) * 4;
			let di = ((y * padded_width + x) as usize) * 4;
			out[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
		}
	}
	(padded_width, padded_height, out)
}

#[derive(Clone, Copy)]
enum MipFilter {
	Box2x2,
	Triangle,
	CatmullRom,
	Lanczos3,
	MitchellNetravali,
	PicScaleBilinearSingle,
	PicScaleBicubicSingle,
	PicScaleCatmullRomSingle,
	PicScaleLanczos3Single,
	PicScaleMitchellSingle,
	PicScaleMitchellAdaptive,
}

impl MipFilter {
	fn name(self) -> &'static str {
		match self {
			Self::Box2x2 => "box2x2",
			Self::Triangle => "image triangle",
			Self::CatmullRom => "image catmull",
			Self::Lanczos3 => "image lanczos3",
			Self::MitchellNetravali => "custom mitchell",
			Self::PicScaleBilinearSingle => "pic bilinear 1t",
			Self::PicScaleBicubicSingle => "pic bicubic 1t",
			Self::PicScaleCatmullRomSingle => "pic catmull 1t",
			Self::PicScaleLanczos3Single => "pic lanczos3 1t",
			Self::PicScaleMitchellSingle => "pic mitchell 1t",
			Self::PicScaleMitchellAdaptive => "pic mitchell adaptive",
		}
	}
}

fn build_mip_chain(rgba: &[u8], width: u32, height: u32, filter: MipFilter) -> Vec<(u32, u32, Vec<u8>)> {
	let mut mips = Vec::new();
	let mut w = width.max(1);
	let mut h = height.max(1);
	let mut data = rgba.to_vec();
	loop {
		mips.push((w, h, data.clone()));
		if w == 1 && h == 1 {
			break;
		}
		let nw = (w / 2).max(1);
		let nh = (h / 2).max(1);
		data = match filter {
			MipFilter::Box2x2 => downsample_box2x2(&data, w, h),
			MipFilter::Triangle => resize_image_crate(&data, w, h, nw, nh, FilterType::Triangle),
			MipFilter::CatmullRom => resize_image_crate(&data, w, h, nw, nh, FilterType::CatmullRom),
			MipFilter::Lanczos3 => resize_image_crate(&data, w, h, nw, nh, FilterType::Lanczos3),
			MipFilter::MitchellNetravali => resize_mitchell(&data, w, h, nw, nh),
			MipFilter::PicScaleBilinearSingle => {
				resize_pic_scale(&data, w, h, nw, nh, ResamplingFunction::Bilinear, ThreadingPolicy::Single)
			}
			MipFilter::PicScaleBicubicSingle => resize_pic_scale(&data, w, h, nw, nh, ResamplingFunction::Bicubic, ThreadingPolicy::Single),
			MipFilter::PicScaleCatmullRomSingle => {
				resize_pic_scale(&data, w, h, nw, nh, ResamplingFunction::CatmullRom, ThreadingPolicy::Single)
			}
			MipFilter::PicScaleLanczos3Single => {
				resize_pic_scale(&data, w, h, nw, nh, ResamplingFunction::Lanczos3, ThreadingPolicy::Single)
			}
			MipFilter::PicScaleMitchellSingle => {
				resize_pic_scale(&data, w, h, nw, nh, ResamplingFunction::MitchellNetravalli, ThreadingPolicy::Single)
			}
			MipFilter::PicScaleMitchellAdaptive => resize_pic_scale(
				&data,
				w,
				h,
				nw,
				nh,
				ResamplingFunction::MitchellNetravalli,
				ThreadingPolicy::Adaptive,
			),
		};
		w = nw;
		h = nh;
	}
	mips
}

fn downsample_box2x2(src: &[u8], width: u32, height: u32) -> Vec<u8> {
	let dst_width = (width / 2).max(1);
	let dst_height = (height / 2).max(1);
	let mut dst = vec![0; (dst_width as usize) * (dst_height as usize) * 4];
	for y in 0..dst_height {
		for x in 0..dst_width {
			let mut acc = [0u32; 4];
			let mut count = 0u32;
			for oy in 0..2 {
				for ox in 0..2 {
					let sx = (x * 2 + ox).min(width - 1);
					let sy = (y * 2 + oy).min(height - 1);
					let si = ((sy * width + sx) as usize) * 4;
					for c in 0..4 {
						acc[c] += u32::from(src[si + c]);
					}
					count += 1;
				}
			}
			let di = ((y * dst_width + x) as usize) * 4;
			for c in 0..4 {
				dst[di + c] = (acc[c] / count) as u8;
			}
		}
	}
	dst
}

fn resize_image_crate(src: &[u8], width: u32, height: u32, dst_width: u32, dst_height: u32, filter: FilterType) -> Vec<u8> {
	let image = RgbaImage::from_raw(width, height, src.to_vec()).expect("rgba dimensions");
	image::imageops::resize(&image, dst_width, dst_height, filter).into_raw()
}

fn resize_pic_scale(
	src: &[u8],
	width: u32,
	height: u32,
	dst_width: u32,
	dst_height: u32,
	filter: ResamplingFunction,
	threading_policy: ThreadingPolicy,
) -> Vec<u8> {
	let width = width as usize;
	let height = height as usize;
	let dst_width = dst_width as usize;
	let dst_height = dst_height as usize;
	let src_store = ImageStore::<u8, 4>::from_slice(src, width, height).expect("pic-scale source dimensions");
	let mut dst_store = ImageStoreMut::<u8, 4>::alloc(dst_width, dst_height);
	let scaler = Scaler::new(filter).set_threading_policy(threading_policy);
	let plan = scaler
		.plan_rgba_resampling(ImageSize::new(width, height), ImageSize::new(dst_width, dst_height), true)
		.expect("pic-scale RGBA plan");
	plan.resample(&src_store, &mut dst_store).expect("pic-scale resample");
	dst_store.as_bytes().to_vec()
}

fn mitchell_weight(x: f32) -> f32 {
	let b = 1.0 / 3.0;
	let c = 1.0 / 3.0;
	let x = x.abs();
	if x < 1.0 {
		((12.0 - 9.0 * b - 6.0 * c) * x * x * x + (-18.0 + 12.0 * b + 6.0 * c) * x * x + (6.0 - 2.0 * b)) / 6.0
	} else if x < 2.0 {
		((-b - 6.0 * c) * x * x * x + (6.0 * b + 30.0 * c) * x * x + (-12.0 * b - 48.0 * c) * x + (8.0 * b + 24.0 * c)) / 6.0
	} else {
		0.0
	}
}

fn resize_mitchell(src: &[u8], width: u32, height: u32, dst_width: u32, dst_height: u32) -> Vec<u8> {
	let scale_x = width as f32 / dst_width as f32;
	let scale_y = height as f32 / dst_height as f32;
	let support_x = 2.0 * scale_x.max(1.0);
	let support_y = 2.0 * scale_y.max(1.0);
	let mut dst = vec![0u8; (dst_width as usize) * (dst_height as usize) * 4];
	for y in 0..dst_height {
		let center_y = (y as f32 + 0.5) * scale_y - 0.5;
		let y0 = (center_y - support_y).floor() as i32;
		let y1 = (center_y + support_y).ceil() as i32;
		for x in 0..dst_width {
			let center_x = (x as f32 + 0.5) * scale_x - 0.5;
			let x0 = (center_x - support_x).floor() as i32;
			let x1 = (center_x + support_x).ceil() as i32;
			let mut acc = [0.0f32; 4];
			let mut total = 0.0f32;
			for sy in y0..=y1 {
				let cy = sy.clamp(0, height as i32 - 1) as u32;
				let wy = mitchell_weight((center_y - sy as f32) / scale_y.max(1.0));
				if wy == 0.0 {
					continue;
				}
				for sx in x0..=x1 {
					let cx = sx.clamp(0, width as i32 - 1) as u32;
					let wx = mitchell_weight((center_x - sx as f32) / scale_x.max(1.0));
					let w = wx * wy;
					if w == 0.0 {
						continue;
					}
					let si = ((cy * width + cx) as usize) * 4;
					for c in 0..4 {
						acc[c] += f32::from(src[si + c]) * w;
					}
					total += w;
				}
			}
			let di = ((y * dst_width + x) as usize) * 4;
			for c in 0..4 {
				dst[di + c] = (acc[c] / total.max(f32::EPSILON)).round().clamp(0.0, 255.0) as u8;
			}
		}
	}
	dst
}

#[derive(Clone, Copy)]
enum CompressionCodec {
	Bc1,
	Bc5,
	Bc7,
}

impl CompressionCodec {
	fn all() -> [Self; 3] {
		[Self::Bc1, Self::Bc5, Self::Bc7]
	}

	fn name(self) -> &'static str {
		match self {
			Self::Bc1 => "bc1",
			Self::Bc5 => "bc5",
			Self::Bc7 => "bc7",
		}
	}

	fn variant(self, rgba: &[u8]) -> CompressionVariant {
		match self {
			Self::Bc1 => CompressionVariant::BC1,
			Self::Bc5 => CompressionVariant::BC5,
			Self::Bc7 => {
				let settings = if has_translucent_alpha(rgba) {
					BC7Settings::alpha_basic()
				} else {
					BC7Settings::opaque_basic()
				};
				CompressionVariant::BC7(settings)
			}
		}
	}
}

fn encode_cpu_striped_parallel(
	variant: CompressionVariant,
	rgba: &[u8],
	width: u32,
	height: u32,
	stripe_px: u32,
	threads: usize,
) -> Vec<u8> {
	let mut stripes = Vec::new();
	let stripe_px = (stripe_px.max(4) & !3).max(4);
	let row_bytes = variant.bytes_per_row(width) as usize;
	let mut y = 0;
	while y < height {
		let h = min(stripe_px, height - y);
		let h = if y + h < height { h & !3 } else { h };
		stripes.push((y, h.max(4).min(height - y)));
		y += h.max(4);
	}
	let result = Arc::new(Mutex::new(vec![0u8; variant.blocks_byte_size(width, height)]));
	let next = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let rgba = Arc::new(rgba.to_vec());
	let stripes = Arc::new(stripes);
	thread::scope(|scope| {
		for _ in 0..threads.max(1) {
			let result = Arc::clone(&result);
			let next = Arc::clone(&next);
			let rgba = Arc::clone(&rgba);
			let stripes = Arc::clone(&stripes);
			scope.spawn(move || loop {
				let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
				let Some(&(start_y, stripe_h)) = stripes.get(index) else {
					break;
				};
				let mut stripe_rgba = vec![0u8; (width as usize) * (stripe_h as usize) * 4];
				for row in 0..stripe_h {
					let src = (((start_y + row) * width) as usize) * 4;
					let dst = ((row * width) as usize) * 4;
					let len = (width as usize) * 4;
					stripe_rgba[dst..dst + len].copy_from_slice(&rgba[src..src + len]);
				}
				let mut stripe_out = vec![0u8; variant.blocks_byte_size(width, stripe_h)];
				compress_rgba8(variant, &stripe_rgba, &mut stripe_out, width, stripe_h, width * 4);
				let dst_offset = (start_y as usize / 4) * row_bytes;
				let mut result = result.lock().unwrap();
				result[dst_offset..dst_offset + stripe_out.len()].copy_from_slice(&stripe_out);
			});
		}
	});
	Arc::try_unwrap(result).unwrap().into_inner().unwrap()
}

fn has_translucent_alpha(rgba: &[u8]) -> bool {
	rgba.chunks_exact(4).any(|px| px[3] < 250)
}

#[cfg(feature = "bcn-gpu-bench")]
fn bench_gpu_bcn(
	rgba: &[u8],
	width: u32,
	height: u32,
	iterations: u32,
	backend: &str,
) -> Result<(f64, Vec<(CompressionCodec, f64, f64)>), String> {
	let init_start = Instant::now();
	let (device, queue) = pollster::block_on(create_wgpu_resources(backend))?;
	let texture = device.create_texture_with_data(
		&queue,
		&wgpu::TextureDescriptor {
			label: Some("bcn-bench-src"),
			size: wgpu::Extent3d {
				width,
				height,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format: wgpu::TextureFormat::Rgba8Unorm,
			usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
			view_formats: &[],
		},
		wgpu::util::TextureDataOrder::LayerMajor,
		rgba,
	);
	let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
	let mut compressor = block_compression::GpuBlockCompressor::new(device.clone(), queue.clone());
	let init_ms = init_start.elapsed().as_secs_f64() * 1000.0;
	let mut results = Vec::new();
	for codec in CompressionCodec::all() {
		let variant = codec.variant(rgba);
		let block_size = variant.blocks_byte_size(width, height) as u64;
		let gpu_ms = bench(iterations, || {
			let blocks = create_gpu_blocks_buffer(&device, block_size);
			compressor.add_compression_task(variant, &view, width, height, &blocks, None, None);
			let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("bcn-bench-compress"),
			});
			{
				let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
					label: Some("bcn-bench-compress"),
					timestamp_writes: None,
				});
				compressor.compress(&mut pass);
			}
			queue.submit([encoder.finish()]);
			let _ = device.poll(wgpu::PollType::Wait {
				submission_index: None,
				timeout: Some(Duration::from_secs(60)),
			});
		});
		let gpu_readback_ms = bench(iterations, || {
			let blocks = create_gpu_blocks_buffer(&device, block_size);
			compressor.add_compression_task(variant, &view, width, height, &blocks, None, None);
			let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("bcn-bench-compress"),
			});
			{
				let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
					label: Some("bcn-bench-compress"),
					timestamp_writes: None,
				});
				compressor.compress(&mut pass);
			}
			queue.submit([encoder.finish()]);
			let _ = download_gpu_buffer(&device, &queue, &blocks);
		});
		results.push((codec, gpu_ms, gpu_readback_ms));
	}
	Ok((init_ms, results))
}

#[cfg(feature = "bcn-gpu-bench")]
async fn create_wgpu_resources(backend: &str) -> Result<(wgpu::Device, wgpu::Queue), String> {
	let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
	instance_descriptor.backends = match backend.to_ascii_lowercase().as_str() {
		"dx12" => wgpu::Backends::DX12,
		"vulkan" => wgpu::Backends::VULKAN,
		"gles" | "gl" => wgpu::Backends::GL,
		"all" => wgpu::Backends::all(),
		other => return Err(format!("unknown backend '{other}', use dx12, vulkan, gles, or all")),
	};
	let instance = wgpu::Instance::new(instance_descriptor);
	let adapter = instance
		.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::HighPerformance,
			compatible_surface: None,
			force_fallback_adapter: false,
		})
		.await
		.map_err(|e| format!("request_adapter: {e}"))?;
	let info = adapter.get_info();
	eprintln!("gpu adapter: {} ({:?}) backend={:?}", info.name, info.device_type, info.backend);
	let _ = std::io::stderr().flush();
	adapter
		.request_device(&wgpu::DeviceDescriptor {
			label: Some("bcn-bench"),
			required_features: wgpu::Features::empty(),
			required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
			memory_hints: Default::default(),
			..Default::default()
		})
		.await
		.map_err(|e| format!("request_device: {e}"))
}

#[cfg(feature = "bcn-gpu-bench")]
fn create_gpu_blocks_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
	device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("bcn-bench-blocks"),
		size,
		usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::STORAGE,
		mapped_at_creation: false,
	})
}

#[cfg(feature = "bcn-gpu-bench")]
fn download_gpu_buffer(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer) -> Vec<u8> {
	let size = buffer.size();
	let staging = device.create_buffer(&wgpu::BufferDescriptor {
		label: Some("bcn-bench-staging"),
		size,
		usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
		mapped_at_creation: false,
	});
	let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
		label: Some("bcn-bench-copy"),
	});
	encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
	queue.submit([encoder.finish()]);
	let slice = staging.slice(..);
	let (tx, rx) = std::sync::mpsc::channel();
	slice.map_async(wgpu::MapMode::Read, move |v| tx.send(v).unwrap());
	let _ = device.poll(wgpu::PollType::Wait {
		submission_index: None,
		timeout: Some(Duration::from_secs(60)),
	});
	rx.recv().unwrap().unwrap();
	let data = slice.get_mapped_range().to_vec();
	staging.unmap();
	data
}
