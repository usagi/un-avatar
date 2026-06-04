use std::{
	collections::VecDeque,
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	},
	thread,
	time::Duration,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use crossbeam_channel::{bounded, Receiver, Sender};
use crossbeam_queue::ArrayQueue;
use rustfft::{num_complex::Complex, Fft, FftPlanner};

use crate::options::AudioLinkOptions;

pub(crate) const AUDIO_LINK_TEXTURE_WIDTH: u32 = 128;
pub(crate) const AUDIO_LINK_TEXTURE_HEIGHT: u32 = 64;
const AUDIO_LINK_SAMPLE_WINDOW: usize = 1024;
const AUDIO_LINK_MAX_CAPTURE_SAMPLES: usize = AUDIO_LINK_SAMPLE_WINDOW * 4;

#[derive(Clone, Debug, Default)]
pub(crate) struct AudioLinkTextureFrame {
	pub(crate) sequence: u64,
	pub(crate) rms: f32,
	pub(crate) peak: f32,
	pub(crate) pixels: Vec<u8>,
}

pub(crate) struct AudioLinkInputRuntime {
	_stream: cpal::Stream,
	latest: Arc<ArrayQueue<AudioLinkTextureFrame>>,
	stop: Arc<AtomicBool>,
	worker: Option<thread::JoinHandle<()>>,
	last_sequence: u64,
}

impl AudioLinkInputRuntime {
	pub(crate) fn start(options: &AudioLinkOptions) -> Result<Self, String> {
		let host = cpal::default_host();
		let device = select_input_device(&host, options)?;
		let supported = device.default_input_config().map_err(|e| format!("AudioLink input config: {e}"))?;
		let config = supported.config();
		let (sample_tx, sample_rx) = bounded::<Vec<f32>>(8);
		let latest = Arc::new(ArrayQueue::<AudioLinkTextureFrame>::new(2));
		let stop = Arc::new(AtomicBool::new(false));
		let err_fn = |err| eprintln!("un-avatar-renderer: AudioLink input stream error: {err}");
		let stream = match supported.sample_format() {
			cpal::SampleFormat::F32 => {
				let tx = sample_tx.clone();
				device.build_input_stream(
					&config,
					move |data: &[f32], _| push_sample_chunk(&tx, data.iter().copied()),
					err_fn,
					None,
				)
			}
			cpal::SampleFormat::I16 => {
				let tx = sample_tx.clone();
				device.build_input_stream(
					&config,
					move |data: &[i16], _| push_sample_chunk(&tx, data.iter().map(|sample| *sample as f32 / i16::MAX as f32)),
					err_fn,
					None,
				)
			}
			cpal::SampleFormat::U16 => {
				let tx = sample_tx.clone();
				device.build_input_stream(
					&config,
					move |data: &[u16], _| push_sample_chunk(&tx, data.iter().map(|sample| (*sample as f32 - 32768.0) / 32768.0)),
					err_fn,
					None,
				)
			}
			format => return Err(format!("AudioLink unsupported input sample format: {format:?}")),
		}
		.map_err(|e| format!("AudioLink input stream: {e}"))?;
		stream.play().map_err(|e| format!("AudioLink input stream play: {e}"))?;

		let device_name = device.name().unwrap_or_else(|_| "unknown input".to_string());
		eprintln!("un-avatar-renderer: AudioLink input device active: {device_name}");
		let worker = spawn_audio_link_fft_worker(sample_rx, Arc::clone(&latest), Arc::clone(&stop));
		Ok(Self {
			_stream: stream,
			latest,
			stop,
			worker: Some(worker),
			last_sequence: 0,
		})
	}

	pub(crate) fn next_texture_frame(&mut self) -> Option<AudioLinkTextureFrame> {
		let mut frame = None;
		while let Some(next) = self.latest.pop() {
			frame = Some(next);
		}
		let frame = frame?;
		if frame.sequence == self.last_sequence {
			return None;
		}
		self.last_sequence = frame.sequence;
		Some(frame)
	}
}

impl Drop for AudioLinkInputRuntime {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Release);
		if let Some(worker) = self.worker.take() {
			let _ = worker.join();
		}
	}
}

fn spawn_audio_link_fft_worker(
	sample_rx: Receiver<Vec<f32>>,
	latest: Arc<ArrayQueue<AudioLinkTextureFrame>>,
	stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
	thread::Builder::new()
		.name("un-avatar-audio-link-fft".to_string())
		.spawn(move || {
			let mut planner = FftPlanner::<f32>::new();
			let fft = planner.plan_fft_forward(AUDIO_LINK_SAMPLE_WINDOW);
			let mut fft_buffer = vec![Complex::new(0.0, 0.0); AUDIO_LINK_SAMPLE_WINDOW];
			let mut pixels = vec![0; (AUDIO_LINK_TEXTURE_WIDTH * AUDIO_LINK_TEXTURE_HEIGHT * 4) as usize];
			let mut samples = VecDeque::with_capacity(AUDIO_LINK_MAX_CAPTURE_SAMPLES);
			let mut sequence = 0u64;
			let mut last_tail = 0.0f32;
			while !stop.load(Ordering::Acquire) {
				for chunk in sample_rx.try_iter() {
					for sample in chunk {
						samples.push_back(sample);
					}
				}
				while samples.len() > AUDIO_LINK_MAX_CAPTURE_SAMPLES {
					let _ = samples.pop_front();
				}
				let snapshot: Vec<f32> = samples
					.iter()
					.rev()
					.take(AUDIO_LINK_SAMPLE_WINDOW)
					.copied()
					.collect::<Vec<_>>()
					.into_iter()
					.rev()
					.collect();
				if snapshot.len() < 64 {
					thread::sleep(Duration::from_millis(16));
					continue;
				}
				let tail = snapshot.last().copied().unwrap_or(0.0);
				if (tail - last_tail).abs() < 0.000001 {
					thread::sleep(Duration::from_millis(16));
					continue;
				}
				last_tail = tail;
				let frame = build_audio_link_texture_frame(&mut fft_buffer, &fft, &mut pixels, &snapshot, sequence.wrapping_add(1));
				sequence = frame.sequence;
				if let Err(frame) = latest.push(frame) {
					let _ = latest.pop();
					let _ = latest.push(frame);
				}
				thread::sleep(Duration::from_millis(16));
			}
		})
		.expect("spawn AudioLink FFT worker")
}

fn select_input_device(host: &cpal::Host, options: &AudioLinkOptions) -> Result<cpal::Device, String> {
	let id = options.input_device_id.as_deref().map(str::trim).filter(|value| !value.is_empty());
	let hint = options
		.input_device_name_hint
		.as_deref()
		.map(str::trim)
		.filter(|value| !value.is_empty());
	if let Some(match_text) = id.or(hint) {
		let normalized = normalize_device_match(match_text);
		let devices = host.input_devices().map_err(|e| format!("AudioLink input devices: {e}"))?;
		for device in devices {
			let name = device.name().unwrap_or_default();
			let normalized_name = normalize_device_match(&name);
			if normalized_name == normalized || normalized_name.contains(&normalized) {
				return Ok(device);
			}
		}
		eprintln!("un-avatar-renderer: AudioLink input device `{match_text}` not found; falling back to default input");
	}
	host.default_input_device()
		.ok_or_else(|| "AudioLink default input device not found".to_string())
}

fn build_audio_link_texture_frame(
	fft_buffer: &mut [Complex<f32>],
	fft: &Arc<dyn Fft<f32>>,
	pixels: &mut [u8],
	snapshot: &[f32],
	sequence: u64,
) -> AudioLinkTextureFrame {
	let mut peak = 0.0f32;
	let mut sum_sq = 0.0f32;
	for sample in snapshot {
		let abs = sample.abs();
		peak = peak.max(abs);
		sum_sq += sample * sample;
	}
	let rms = (sum_sq / snapshot.len().max(1) as f32).sqrt().clamp(0.0, 1.0);

	fft_buffer.fill(Complex::new(0.0, 0.0));
	let offset = AUDIO_LINK_SAMPLE_WINDOW.saturating_sub(snapshot.len());
	for (i, sample) in snapshot.iter().enumerate() {
		let t = (i as f32 / snapshot.len().max(1) as f32).clamp(0.0, 1.0);
		let window = 0.5 - 0.5 * (std::f32::consts::TAU * t).cos();
		fft_buffer[offset + i] = Complex::new(sample * window, 0.0);
	}
	fft.process(fft_buffer);

	fill_audio_link_pixels(pixels, fft_buffer, rms, peak);
	AudioLinkTextureFrame {
		sequence,
		rms,
		peak,
		pixels: pixels.to_vec(),
	}
}

fn normalize_device_match(value: &str) -> String {
	value
		.trim()
		.strip_prefix("cpal:")
		.unwrap_or(value.trim())
		.chars()
		.filter(|ch| !ch.is_whitespace())
		.flat_map(char::to_lowercase)
		.collect()
}

fn push_sample_chunk(tx: &Sender<Vec<f32>>, input: impl Iterator<Item = f32>) {
	let chunk: Vec<f32> = input.map(|sample| sample.clamp(-1.0, 1.0)).collect();
	let _ = tx.try_send(chunk);
}

fn fill_audio_link_pixels(pixels: &mut [u8], spectrum: &[Complex<f32>], rms: f32, peak: f32) {
	pixels.fill(0);
	let width = AUDIO_LINK_TEXTURE_WIDTH as usize;
	let height = AUDIO_LINK_TEXTURE_HEIGHT as usize;
	for x in 0..width {
		let bin = 1 + x * (AUDIO_LINK_SAMPLE_WINDOW / 2 - 1) / width.max(1);
		let mag = spectrum
			.get(bin)
			.map_or(0.0, |value| value.norm() / AUDIO_LINK_SAMPLE_WINDOW as f32);
		let spectrum_value = (mag * 18.0).sqrt().clamp(0.0, 1.0);
		write_pixel(pixels, width, x, 0, spectrum_value, spectrum_value, spectrum_value, 1.0);
	}
	let bass = band_energy(spectrum, 1, 8);
	let low_mid = band_energy(spectrum, 8, 24);
	let high_mid = band_energy(spectrum, 24, 64);
	let high = band_energy(spectrum, 64, 256);
	for x in 0..width {
		write_pixel(pixels, width, x, 1, rms, peak, bass, 1.0);
		write_pixel(pixels, width, x, 2, bass, low_mid, high_mid, high);
	}
	for y in 3..height {
		let fade = 1.0 - ((y - 3) as f32 / (height - 3).max(1) as f32);
		let value = (rms * fade).clamp(0.0, 1.0);
		for x in 0..width {
			write_pixel(pixels, width, x, y, value, value, value, 1.0);
		}
	}
}

fn band_energy(spectrum: &[Complex<f32>], start: usize, end: usize) -> f32 {
	let mut sum = 0.0;
	let mut count = 0usize;
	for bin in start..end {
		if let Some(value) = spectrum.get(bin) {
			sum += value.norm();
			count += 1;
		}
	}
	((sum / count.max(1) as f32) / AUDIO_LINK_SAMPLE_WINDOW as f32 * 24.0)
		.sqrt()
		.clamp(0.0, 1.0)
}

fn write_pixel(pixels: &mut [u8], width: usize, x: usize, y: usize, r: f32, g: f32, b: f32, a: f32) {
	let offset = (y * width + x) * 4;
	if offset + 3 >= pixels.len() {
		return;
	}
	pixels[offset] = (r.clamp(0.0, 1.0) * 255.0).round() as u8;
	pixels[offset + 1] = (g.clamp(0.0, 1.0) * 255.0).round() as u8;
	pixels[offset + 2] = (b.clamp(0.0, 1.0) * 255.0).round() as u8;
	pixels[offset + 3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
}
