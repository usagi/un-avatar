//! wgpu デバイス・スワップチェーン・深度・プロシージャル空スカイ（カメラ／ライトのユニフォーム検証用）。

use std::{
	borrow::Cow,
	net::SocketAddr,
	sync::{
		atomic::{AtomicU64, AtomicU8, Ordering},
		Arc, Mutex, RwLock,
	},
	time::{Duration, Instant},
};

use glam::{Mat4, Vec3, Vec4};
use un_avatar_core::{UnaDocument, UnaExpressionCatalog, UnaSceneNode};
use un_avatar_skeleton::{
	build_bone_colliders, collider_stats, BoneColliderConfig, BoneColliderPrimitive, BoneColliderSource, SpringBonePhysicsConfig,
	SpringBoneSimulator,
};
use winit::window::Window;

use crate::{
	camera::OrbitCamera,
	debug_dump::log_material_skin_report,
	debug_log::DebugLog,
	mesh_pass::{AvatarOutlineOptions, AvatarOutlinePolicy, SceneMeshBuildProgress, SceneMeshLoadOpts, SceneMeshes, TextureUploadSummary},
	options::{BloomOptions, ColorGradingLook, ContactShadowOptions, EnvironmentColorOptions, LightingOptions},
	post_process::PostProcess,
	AaMode, BlockCompressionEncoder, RenderBackend, SpoutWindowOptions, TextureCompressionAdvancedOptions, TextureCompressionMode,
	WindowDebugOptions,
};

const SHADER_SKY: &str = include_str!("../shaders/sky.wgsl");
const SHADER_AXES: &str = include_str!("../shaders/axes.wgsl");
const SHADER_BONE_COLLIDERS: &str = include_str!("../shaders/bone_colliders.wgsl");
const SHADER_STARTUP_SPLASH: &str = include_str!("../shaders/startup_splash.wgsl");
const SHADER_CONTACT_SHADOW: &str = include_str!("../shaders/contact_shadow.wgsl");

fn unmotion_frame_hand_summary(frame: &un_motion_frame::UNMotionFrame, document: &UnaDocument) -> String {
	let left_fingers = frame.left_hand.as_ref().map(|h| h.fingers.len()).unwrap_or(0);
	let right_fingers = frame.right_hand.as_ref().map(|h| h.fingers.len()).unwrap_or(0);
	let left_joints = frame
		.left_hand
		.as_ref()
		.map(|h| h.fingers.iter().map(|f| f.joints.len()).sum::<usize>())
		.unwrap_or(0);
	let right_joints = frame
		.right_hand
		.as_ref()
		.map(|h| h.fingers.iter().map(|f| f.joints.len()).sum::<usize>())
		.unwrap_or(0);
	let matched_finger_keys = document
		.humanoid_profile
		.as_ref()
		.map(|profile| {
			profile
				.bone_node_indices
				.keys()
				.filter(|key| {
					let normalized: String = key
						.chars()
						.filter(|ch| ch.is_ascii_alphanumeric())
						.map(|ch| ch.to_ascii_lowercase())
						.collect();
					let side = normalized.starts_with("left") || normalized.starts_with("right");
					let finger = ["thumb", "index", "middle", "ring", "little"]
						.iter()
						.any(|part| normalized.contains(part));
					let segment = ["proximal", "intermediate", "distal"].iter().any(|part| normalized.contains(part));
					side && finger && segment
				})
				.count()
		})
		.unwrap_or(0);
	let (finger_targets, matched_finger_targets) = document
		.humanoid_profile
		.as_ref()
		.map(|profile| {
			let mut targets = Vec::new();
			append_hand_finger_target_keys(&mut targets, frame.left_hand.as_ref(), "left");
			append_hand_finger_target_keys(&mut targets, frame.right_hand.as_ref(), "right");
			let matched = targets.iter().filter(|target| profile_has_key(profile, target)).count();
			(targets.len(), matched)
		})
		.unwrap_or((0, 0));
	format!(
		"space={:?} left_fingers={left_fingers} right_fingers={right_fingers} left_joints={left_joints} right_joints={right_joints} profile_finger_keys={matched_finger_keys} finger_targets={finger_targets} matched_finger_targets={matched_finger_targets}",
		frame.header.coordinate_space
	)
}

fn expression_presets_match_catalog(current: &[String], catalog: Option<&UnaExpressionCatalog>) -> bool {
	let Some(catalog) = catalog else {
		return current.is_empty();
	};
	current.len() == catalog.presets.len()
		&& current
			.iter()
			.zip(&catalog.presets)
			.all(|(current, preset)| current == &preset.name)
}

fn append_hand_finger_target_keys(keys: &mut Vec<String>, hand: Option<&un_motion_frame::HandMotion>, side_prefix: &str) {
	let Some(hand) = hand else {
		return;
	};
	for finger in &hand.fingers {
		let finger_key = match finger.finger {
			un_motion_frame::Finger::Thumb => "thumb",
			un_motion_frame::Finger::Index => "index",
			un_motion_frame::Finger::Middle => "middle",
			un_motion_frame::Finger::Ring => "ring",
			un_motion_frame::Finger::Little => "little",
		};
		for (index, _) in finger.joints.iter().enumerate() {
			let segment = match index {
				0 => "proximal",
				1 => "intermediate",
				2 => "distal",
				_ => continue,
			};
			keys.push(format!("{side_prefix}{finger_key}{segment}"));
		}
	}
}

fn profile_has_key(profile: &un_avatar_skeleton::HumanoidProfile, key: &str) -> bool {
	profile.bone_node_indices.contains_key(key) || {
		let target = normalize_profile_match_key(key);
		profile
			.bone_node_indices
			.keys()
			.any(|candidate| normalize_profile_match_key(candidate) == target)
	}
}

fn normalize_profile_match_key(name: &str) -> String {
	name.chars()
		.filter(|ch| ch.is_ascii_alphanumeric())
		.map(|ch| ch.to_ascii_lowercase())
		.collect()
}

/// GPU とシェーダに渡すグローバル（WGSL `Globals` と一致。末尾パディングで 256 バイトに揃える）。
#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct GlobalsGpu {
	pub(crate) view_proj: [[f32; 4]; 4],
	pub(crate) inv_view_proj: [[f32; 4]; 4],
	pub(crate) light_dir: [f32; 4],
	pub(crate) camera_pos: [f32; 4],
	_pad: [u8; 96],
}

const _: () = assert!(std::mem::size_of::<GlobalsGpu>() == 256);

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct StartupSplashGpu {
	time: f32,
	progress: f32,
	aspect: f32,
	phase: f32,
}

#[repr(C)]
#[derive(Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct ContactShadowGpu {
	params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct DebugLineVertex {
	position: [f32; 3],
	color: [f32; 4],
}

pub(crate) struct StartupSplashFrame {
	pub(crate) time_secs: f32,
	pub(crate) progress: f32,
	pub(crate) phase: f32,
}

#[derive(Clone)]
pub(crate) struct DocumentAttachOptions {
	pub(crate) mesh_diagnostics: SceneMeshLoadOpts,
	pub(crate) texture_max_dimension: Option<u32>,
	pub(crate) texture_compression: TextureCompressionMode,
	pub(crate) block_compression_encoder: crate::options::BlockCompressionEncoder,
	pub(crate) block_compression_cpu_threads: usize,
	pub(crate) mipmap_filter: crate::options::TextureMipmapFilter,
	pub(crate) texture_compression_advanced: TextureCompressionAdvancedOptions,
	pub(crate) texture_compression_bc_supported: bool,
	pub(crate) texture_compression_astc_supported: bool,
	pub(crate) texture_compression_etc2_supported: bool,
	pub(crate) processed_texture_cache: bool,
	pub(crate) enable_spring_bones: bool,
	pub(crate) bone_colliders: BoneColliderConfig,
	pub(crate) spring_bone_physics: SpringBonePhysicsConfig,
	pub(crate) debug_material_dump: bool,
	pub(crate) vmc_address: Option<SocketAddr>,
	pub(crate) unmotion_zenoh: crate::options::UnmotionZenohOptions,
	pub(crate) debug_vmc: bool,
}

pub(crate) struct PreparedDocumentScene {
	document: Arc<RwLock<UnaDocument>>,
	scene_meshes: Option<SceneMeshes>,
	texture_summary: Option<TextureUploadSummary>,
	spring_sim: Option<SpringBoneSimulator>,
	bone_colliders: Vec<BoneColliderPrimitive>,
	bone_collider_count: u32,
	bone_collider_source: BoneColliderSource,
	expression_presets: Vec<String>,
}

pub(crate) struct GpuSceneBuildContext {
	device: wgpu::Device,
	queue: wgpu::Queue,
	format: wgpu::TextureFormat,
	aa: AaMode,
}

/// `Mat4::perspective_rh` 用の縦方向 FOV（ラジアン）を、対角画角と幅÷高さから求める。
/// 対角画角の既定値はフルサイズ換算 35mm レンズ相当（36×24mm センサーの対角と焦点距離 35mm から
/// `2 * atan(sqrt(36² + 24²) / (2 * 35)) ≈ 63.45°`）を `crate::camera::DEFAULT_DIAGONAL_FOV_DEG` に置く。
fn vertical_fov_from_diagonal(diagonal_rad: f32, aspect_wh: f32) -> f32 {
	let t = (diagonal_rad * 0.5).tan();
	2.0 * (t / (1.0 + aspect_wh * aspect_wh).sqrt()).atan()
}

/// 1 フレームあたりの計測（壁時計間隔・CPU 記録時間・GPU メインパス時間）。
///
/// `gpu_ms` は `Features::TIMESTAMP_QUERY` 対応 GPU では真の GPU 時間（メインパスの開始から終了まで）。
/// 非対応 GPU では 0 を返す。CPU は `desired_maximum_frame_latency` と present_mode で律速されるため
/// 旧実装のブロッキング `device.poll(wait_indefinitely)` は不要。
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTimings {
	pub wall_since_last_ms: f32,
	pub cpu_record_ms: f32,
	pub gpu_ms: f32,
}

const TS_RING_LEN: usize = 2;
const TS_BYTES_PER_FRAME: u64 = 16;

const TS_STATE_IDLE: u8 = 0;
const TS_STATE_PENDING: u8 = 1;
const TS_STATE_READY: u8 = 2;

fn instance_descriptor_for_backend(backend: RenderBackend) -> wgpu::InstanceDescriptor {
	let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
	descriptor.backends = match backend {
		RenderBackend::Auto => wgpu::Backends::all(),
		RenderBackend::Vulkan => wgpu::Backends::VULKAN,
		RenderBackend::Dx12 => wgpu::Backends::DX12,
	};
	#[cfg(windows)]
	if backend == RenderBackend::Dx12 {
		// DX12 HWND swapchains expose only Opaque alpha; DirectComposition visual swapchains expose real alpha modes.
		descriptor.backend_options.dx12.presentation_system = wgpu::Dx12SwapchainKind::DxgiFromVisual;
	}
	descriptor
}

fn effective_window_backend(backend: RenderBackend) -> RenderBackend {
	#[cfg(windows)]
	{
		// Windows Vulkan HWND surfaces commonly expose only Opaque alpha. The renderer
		// supports runtime transparency toggles, so prefer the DX12 DirectComposition path.
		if backend == RenderBackend::Vulkan {
			return RenderBackend::Dx12;
		}
	}
	backend
}

struct TimestampRingSlot {
	buf: wgpu::Buffer,
	state: Arc<AtomicU8>,
}

/// GPU タイムスタンプによるメインパス計測。`TIMESTAMP_QUERY` 対応時のみ生成される。
struct GpuTimestamps {
	qset: wgpu::QuerySet,
	resolve_buf: wgpu::Buffer,
	rings: [TimestampRingSlot; TS_RING_LEN],
	period_ns: f32,
	write_idx: usize,
	last_gpu_ms: Option<f32>,
}

impl GpuTimestamps {
	fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
		let qset = device.create_query_set(&wgpu::QuerySetDescriptor {
			label: Some("frame-ts"),
			ty: wgpu::QueryType::Timestamp,
			count: 2,
		});
		let resolve_buf = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("frame-ts-resolve"),
			size: TS_BYTES_PER_FRAME,
			usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
			mapped_at_creation: false,
		});
		let mut rings: [Option<TimestampRingSlot>; TS_RING_LEN] = [None, None];
		for slot in &mut rings {
			*slot = Some(TimestampRingSlot {
				buf: device.create_buffer(&wgpu::BufferDescriptor {
					label: Some("frame-ts-readback"),
					size: TS_BYTES_PER_FRAME,
					usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
					mapped_at_creation: false,
				}),
				state: Arc::new(AtomicU8::new(TS_STATE_IDLE)),
			});
		}
		Self {
			qset,
			resolve_buf,
			rings: [rings[0].take().unwrap(), rings[1].take().unwrap()],
			period_ns: queue.get_timestamp_period(),
			write_idx: 0,
			last_gpu_ms: None,
		}
	}

	/// 完了済みのリングスロットを読み出し、`last_gpu_ms` を更新する。呼び出し側で `device.poll(Poll)` を済ませておくこと。
	fn drain_ready(&mut self) {
		for slot in &self.rings {
			if slot.state.load(Ordering::Acquire) == TS_STATE_READY {
				let raw = {
					let view = slot.buf.slice(..).get_mapped_range();
					let pair: [u64; 2] = bytemuck::pod_read_unaligned(&view[..16]);
					pair
				};
				slot.buf.unmap();
				slot.state.store(TS_STATE_IDLE, Ordering::Release);
				let diff = raw[1].saturating_sub(raw[0]);
				let ms = (diff as f64 * self.period_ns as f64) / 1_000_000.0;
				self.last_gpu_ms = Some(ms as f32);
			}
		}
	}

	/// 今フレームで書き込めるスロットがあれば、メインパスに渡す timestamp_writes と書き込みインデックスを返す。
	fn begin_pass(&self) -> Option<(wgpu::RenderPassTimestampWrites<'_>, usize)> {
		let idx = self.write_idx;
		if self.rings[idx].state.load(Ordering::Acquire) != TS_STATE_IDLE {
			return None;
		}
		Some((
			wgpu::RenderPassTimestampWrites {
				query_set: &self.qset,
				beginning_of_pass_write_index: Some(0),
				end_of_pass_write_index: Some(1),
			},
			idx,
		))
	}

	fn encode_resolve(&self, encoder: &mut wgpu::CommandEncoder, idx: usize) {
		encoder.resolve_query_set(&self.qset, 0..2, &self.resolve_buf, 0);
		encoder.copy_buffer_to_buffer(&self.resolve_buf, 0, &self.rings[idx].buf, 0, TS_BYTES_PER_FRAME);
	}

	fn after_submit(&mut self, idx: usize) {
		let cb_state = Arc::clone(&self.rings[idx].state);
		cb_state.store(TS_STATE_PENDING, Ordering::Release);
		self.rings[idx].buf.slice(..).map_async(wgpu::MapMode::Read, move |result| {
			if result.is_ok() {
				cb_state.store(TS_STATE_READY, Ordering::Release);
			} else {
				cb_state.store(TS_STATE_IDLE, Ordering::Release);
			}
		});
		self.write_idx = (self.write_idx + 1) % TS_RING_LEN;
	}

	fn last_gpu_ms(&self) -> Option<f32> {
		self.last_gpu_ms
	}
}

/// `primary_motion_source` 共有 atomic で使う数値表現。
///
/// `crate::options::PrimaryMotionSource` の repr ではなく、別途固定値を用意することで
/// `PrimaryMotionSource` 側に repr を強制しなくて済む (Serialize の互換も保ちやすい)。
const PRIMARY_VMC: u8 = 0;
const PRIMARY_UNMOTION_ZENOH: u8 = 1;

fn primary_motion_source_to_u8(source: crate::options::PrimaryMotionSource) -> u8 {
	match source {
		crate::options::PrimaryMotionSource::Vmc => PRIMARY_VMC,
		crate::options::PrimaryMotionSource::UnmotionZenoh => PRIMARY_UNMOTION_ZENOH,
	}
}

fn primary_motion_source_from_u8(value: u8) -> crate::options::PrimaryMotionSource {
	match value {
		PRIMARY_UNMOTION_ZENOH => crate::options::PrimaryMotionSource::UnmotionZenoh,
		_ => crate::options::PrimaryMotionSource::Vmc,
	}
}

#[derive(Default)]
struct MotionControlBuffer {
	state: Mutex<MotionControlBufferState>,
}

impl MotionControlBuffer {
	fn push_frame(&self, frame: un_motion_frame::UNMotionFrame) {
		if let Ok(mut state) = self.state.lock() {
			let write_idx = state.write_idx;
			state.buffers[write_idx].push_frame(frame);
		}
	}

	fn take_pending_frames_into(&self, out: &mut Vec<un_motion_frame::UNMotionFrame>) {
		out.clear();
		let Ok(mut state) = self.state.lock() else {
			return;
		};
		let read_idx = state.write_idx;
		let next_write_idx = 1 - read_idx;
		state.write_idx = next_write_idx;
		state.buffers[next_write_idx].clear();
		state.buffers[read_idx].take_frames_into(out);
	}
}

#[derive(Default)]
struct MotionControlBufferState {
	write_idx: usize,
	buffers: [MotionFrameAccumulator; 2],
}

#[derive(Default)]
struct MotionFrameAccumulator {
	buckets: Vec<MotionFrameBucket>,
	sequence: u64,
}

impl MotionFrameAccumulator {
	fn clear(&mut self) {
		self.buckets.clear();
	}

	fn push_frame(&mut self, frame: un_motion_frame::UNMotionFrame) {
		let bucket = self.bucket_for_frame(&frame);
		bucket.merge_frame(frame);
	}

	fn take_frames_into(&mut self, frames: &mut Vec<un_motion_frame::UNMotionFrame>) {
		if self.buckets.is_empty() {
			return;
		}
		frames.reserve(self.buckets.len());
		for bucket in self.buckets.drain(..) {
			self.sequence = self.sequence.wrapping_add(1);
			if let Some(frame) = bucket.into_frame(self.sequence) {
				frames.push(frame);
			}
		}
	}

	fn bucket_for_frame(&mut self, frame: &un_motion_frame::UNMotionFrame) -> &mut MotionFrameBucket {
		if let Some(index) = self.buckets.iter().position(|bucket| bucket.matches(frame)) {
			return &mut self.buckets[index];
		}
		self.buckets.push(MotionFrameBucket::from_frame_space(frame));
		self.buckets.last_mut().expect("bucket just pushed")
	}
}

struct MotionFrameBucket {
	header: un_motion_frame::MotionHeader,
	sources: Vec<un_motion_frame::MotionSourceInfo>,
	metadata: un_motion_frame::MotionMetadata,
	body_tracking_state: un_motion_frame::TrackingState,
	body_confidence: f32,
	body_root: Option<un_motion_frame::TransformSample>,
	body_bones: Vec<un_motion_frame::BoneSample>,
	face_tracking_state: un_motion_frame::TrackingState,
	face_confidence: f32,
	face_head: Option<un_motion_frame::TransformSample>,
	expressions: Vec<un_motion_frame::ExpressionSample>,
	eyes: Option<un_motion_frame::EyeMotion>,
	left_tracking_state: un_motion_frame::TrackingState,
	left_confidence: f32,
	left_wrist: Option<un_motion_frame::TransformSample>,
	left_fingers: Vec<un_motion_frame::FingerPose>,
	right_tracking_state: un_motion_frame::TrackingState,
	right_confidence: f32,
	right_wrist: Option<un_motion_frame::TransformSample>,
	right_fingers: Vec<un_motion_frame::FingerPose>,
	signals: Vec<un_motion_frame::MotionSignal>,
}

impl MotionFrameBucket {
	fn from_frame_space(frame: &un_motion_frame::UNMotionFrame) -> Self {
		let left_finger_capacity = frame.left_hand.as_ref().map_or(0, |hand| hand.fingers.len());
		let right_finger_capacity = frame.right_hand.as_ref().map_or(0, |hand| hand.fingers.len());
		Self {
			header: frame.header.clone(),
			sources: Vec::with_capacity(frame.sources.len()),
			metadata: frame.metadata.clone(),
			body_tracking_state: un_motion_frame::TrackingState::Unknown,
			body_confidence: 0.0,
			body_root: None,
			body_bones: Vec::new(),
			face_tracking_state: un_motion_frame::TrackingState::Unknown,
			face_confidence: 0.0,
			face_head: None,
			expressions: Vec::new(),
			eyes: None,
			left_tracking_state: un_motion_frame::TrackingState::Unknown,
			left_confidence: 0.0,
			left_wrist: None,
			left_fingers: Vec::with_capacity(left_finger_capacity),
			right_tracking_state: un_motion_frame::TrackingState::Unknown,
			right_confidence: 0.0,
			right_wrist: None,
			right_fingers: Vec::with_capacity(right_finger_capacity),
			signals: Vec::new(),
		}
	}

	fn matches(&self, frame: &un_motion_frame::UNMotionFrame) -> bool {
		self.header.coordinate_space == frame.header.coordinate_space
			&& self.header.handedness == frame.header.handedness
			&& self.header.length_unit == frame.header.length_unit
	}

	fn merge_frame(&mut self, frame: un_motion_frame::UNMotionFrame) {
		self.header = frame.header;
		self.metadata = frame.metadata;
		self.sources.extend(frame.sources);
		if let Some(body) = frame.body {
			self.body_tracking_state = body.tracking_state;
			self.body_confidence = body.confidence;
			if let Some(humanoid) = body.humanoid {
				if humanoid.root.is_some() {
					self.body_root = humanoid.root;
				}
				for bone in humanoid.bones {
					upsert_bone_sample(&mut self.body_bones, bone);
				}
			}
		}
		if let Some(face) = frame.face {
			self.face_tracking_state = face.tracking_state;
			self.face_confidence = face.confidence;
			if face.head.is_some() {
				self.face_head = face.head;
			}
			for expression in face.expressions {
				upsert_expression_sample(&mut self.expressions, expression);
			}
		}
		if let Some(eyes) = frame.eyes {
			self.eyes = Some(eyes);
		}
		if let Some(hand) = frame.left_hand {
			self.left_tracking_state = hand.tracking_state;
			self.left_confidence = hand.confidence;
			if hand.wrist.is_some() {
				self.left_wrist = hand.wrist;
			}
			for finger in hand.fingers {
				upsert_finger_pose(&mut self.left_fingers, finger);
			}
		}
		if let Some(hand) = frame.right_hand {
			self.right_tracking_state = hand.tracking_state;
			self.right_confidence = hand.confidence;
			if hand.wrist.is_some() {
				self.right_wrist = hand.wrist;
			}
			for finger in hand.fingers {
				upsert_finger_pose(&mut self.right_fingers, finger);
			}
		}
		for signal in frame.signals {
			upsert_motion_signal(&mut self.signals, signal);
		}
	}

	fn into_frame(mut self, sequence: u64) -> Option<un_motion_frame::UNMotionFrame> {
		let mut frame = un_motion_frame::UNMotionFrame::new(sequence);
		self.header.sequence = sequence;
		frame.header = self.header;
		frame.sources = self.sources;
		frame.metadata = self.metadata;
		if self.body_root.is_some() || !self.body_bones.is_empty() {
			frame.body = Some(un_motion_frame::BodyMotion {
				tracking_state: self.body_tracking_state,
				confidence: self.body_confidence,
				humanoid: Some(un_motion_frame::HumanoidPose {
					root: self.body_root,
					bones: self.body_bones,
				}),
			});
		}
		if self.face_head.is_some() || !self.expressions.is_empty() {
			frame.face = Some(un_motion_frame::FaceMotion {
				tracking_state: self.face_tracking_state,
				confidence: self.face_confidence,
				head: self.face_head,
				expressions: self.expressions,
			});
		}
		frame.eyes = self.eyes;
		if self.left_wrist.is_some() || !self.left_fingers.is_empty() {
			frame.left_hand = Some(un_motion_frame::HandMotion {
				tracking_state: self.left_tracking_state,
				confidence: self.left_confidence,
				wrist: self.left_wrist,
				fingers: self.left_fingers,
			});
		}
		if self.right_wrist.is_some() || !self.right_fingers.is_empty() {
			frame.right_hand = Some(un_motion_frame::HandMotion {
				tracking_state: self.right_tracking_state,
				confidence: self.right_confidence,
				wrist: self.right_wrist,
				fingers: self.right_fingers,
			});
		}
		frame.signals = self.signals;
		if frame.body.is_none()
			&& frame.face.is_none()
			&& frame.eyes.is_none()
			&& frame.left_hand.is_none()
			&& frame.right_hand.is_none()
			&& frame.signals.is_empty()
		{
			return None;
		}
		Some(frame)
	}
}

fn upsert_finger_pose(fingers: &mut Vec<un_motion_frame::FingerPose>, next: un_motion_frame::FingerPose) {
	if let Some(existing) = fingers.iter_mut().find(|finger| finger.finger == next.finger) {
		*existing = next;
	} else {
		fingers.push(next);
	}
}

fn upsert_bone_sample(bones: &mut Vec<un_motion_frame::BoneSample>, next: un_motion_frame::BoneSample) {
	if let Some(existing) = bones.iter_mut().find(|bone| bone.bone == next.bone) {
		*existing = next;
	} else {
		bones.push(next);
	}
}

fn upsert_expression_sample(expressions: &mut Vec<un_motion_frame::ExpressionSample>, next: un_motion_frame::ExpressionSample) {
	if let Some(existing) = expressions.iter_mut().find(|expression| expression.name == next.name) {
		*existing = next;
	} else {
		expressions.push(next);
	}
}

fn upsert_motion_signal(signals: &mut Vec<un_motion_frame::MotionSignal>, next: un_motion_frame::MotionSignal) {
	if let Some(existing) = signals.iter_mut().find(|signal| signal.name == next.name) {
		*existing = next;
	} else {
		signals.push(next);
	}
}

#[cfg(test)]
mod motion_buffer_tests {
	use super::*;

	fn expression_frame(sequence: u64, name: &str, value: f32) -> un_motion_frame::UNMotionFrame {
		let mut frame = un_motion_frame::UNMotionFrame::new(sequence);
		frame.header.coordinate_space = un_motion_frame::CoordinateSpace::UNMotion;
		frame.face = Some(un_motion_frame::FaceMotion {
			tracking_state: un_motion_frame::TrackingState::Valid,
			confidence: 1.0,
			head: None,
			expressions: vec![un_motion_frame::ExpressionSample {
				name: name.to_string(),
				value,
				confidence: 1.0,
				source_index: None,
				state: un_motion_frame::SampleState::Valid,
			}],
		});
		frame
	}

	#[test]
	fn motion_buffer_keeps_latest_value_per_key_until_frame_read() {
		let buffer = MotionControlBuffer::default();
		buffer.push_frame(expression_frame(1, "Joy", 0.25));
		buffer.push_frame(expression_frame(2, "Joy", 0.75));

		let mut frames = Vec::new();
		buffer.take_pending_frames_into(&mut frames);
		assert_eq!(frames.len(), 1);
		let expressions = &frames[0].face.as_ref().unwrap().expressions;
		assert_eq!(expressions.len(), 1);
		assert_eq!(expressions[0].name, "Joy");
		assert!((expressions[0].value - 0.75).abs() < f32::EPSILON);
		buffer.take_pending_frames_into(&mut frames);
		assert!(frames.is_empty());
	}

	#[test]
	fn motion_buffer_switches_write_side_after_read() {
		let buffer = MotionControlBuffer::default();
		let mut frames = Vec::new();
		buffer.push_frame(expression_frame(1, "Joy", 0.25));
		buffer.take_pending_frames_into(&mut frames);
		assert_eq!(frames.len(), 1);

		buffer.push_frame(expression_frame(2, "Angry", 0.5));
		buffer.take_pending_frames_into(&mut frames);
		assert_eq!(frames.len(), 1);
		let expressions = &frames[0].face.as_ref().unwrap().expressions;
		assert_eq!(expressions.len(), 1);
		assert_eq!(expressions[0].name, "Angry");
	}
}

/// IPC / status snapshot 用のカメラ状態（profile 保存・UI 表示用）。
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize)]
pub struct CameraStateSnapshot {
	/// target ワールド座標 \[x, y, z\]。
	pub target: [f32; 3],
	/// 緯度・経度は度（UI に出すときに馴染みやすいよう degrees で公開）。
	pub longitude_deg: f32,
	pub latitude_deg: f32,
	/// target からカメラ位置までの距離。
	pub radius: f32,
	/// 対角画角（度）。
	pub diagonal_fov_deg: f32,
}

pub(crate) struct GpuState {
	pub(crate) surface: wgpu::Surface<'static>,
	pub(crate) device: wgpu::Device,
	pub(crate) queue: wgpu::Queue,
	pub(crate) config: wgpu::SurfaceConfiguration,
	alpha_modes: Vec<wgpu::CompositeAlphaMode>,
	depth_texture: wgpu::Texture,
	depth_view: wgpu::TextureView,
	uniform_buffer: wgpu::Buffer,
	globals_uploaded: Option<GlobalsGpu>,
	bind_group: wgpu::BindGroup,
	pipeline: wgpu::RenderPipeline,
	axes_pipeline: wgpu::RenderPipeline,
	bone_collider_pipeline: wgpu::RenderPipeline,
	bone_collider_vertex_buffer: Option<wgpu::Buffer>,
	bone_collider_vertex_capacity: usize,
	bone_collider_vertex_count: u32,
	bone_collider_vertices: Vec<DebugLineVertex>,
	startup_splash_pipeline: wgpu::RenderPipeline,
	startup_splash_buffer: wgpu::Buffer,
	startup_splash_bind_group: wgpu::BindGroup,
	contact_shadow_pipeline: wgpu::RenderPipeline,
	contact_shadow_buffer: wgpu::Buffer,
	contact_shadow_bind_group: wgpu::BindGroup,
	document: Option<Arc<RwLock<UnaDocument>>>,
	document_revision: Arc<AtomicU64>,
	applied_document_revision: u64,
	/// VMC 受信スレッドが起動済みか。受信データは描画直前に pending buffer から適用する。
	vmc_live: bool,
	scene_meshes: Option<SceneMeshes>,
	avatar_outline: AvatarOutlineOptions,
	environment_color: EnvironmentColorOptions,
	lighting: LightingOptions,
	bloom: BloomOptions,
	ssao: crate::SsaoOptions,
	contact_shadow: ContactShadowOptions,
	texture_summary: Option<TextureUploadSummary>,
	spring_sim: Option<SpringBoneSimulator>,
	bone_colliders: Vec<BoneColliderPrimitive>,
	aa: AaMode,
	post_process: Option<PostProcess>,
	msaa_target: Option<crate::post_process::MsaaTarget>,
	#[cfg(windows)]
	spout: Option<crate::spout::SpoutCapture>,
	#[cfg(windows)]
	spout_launch: Option<crate::spout::SpoutLaunchConfig>,
	debug_log: DebugLog,
	debug_scene: bool,
	debug_morph: bool,
	debug_frame_seq: u64,
	animation_time_secs: f32,
	disable_expression_morphs: bool,
	camera: OrbitCamera,
	world_scratch: Vec<Mat4>,
	gpu_timestamps: Option<GpuTimestamps>,
	expression_overrides: std::collections::BTreeMap<String, f32>,
	expression_overrides_revision: u64,
	applied_expression_overrides_revision: u64,
	expression_presets: Vec<String>,
	motion_apply_shared: Arc<Mutex<un_avatar_skeleton::ApplyUnMotionFrameOpts>>,
	motion_buffer: Arc<MotionControlBuffer>,
	pending_motion_frames: Vec<un_motion_frame::UNMotionFrame>,
	motion_rest_nodes: Option<Arc<Vec<UnaSceneNode>>>,
	spring_rest_nodes: Option<Arc<Vec<UnaSceneNode>>>,
	/// 旧 IPC / status 互換の primary source 値。現在の姿勢適用は key 単位の後着優先。
	primary_motion_source: Arc<AtomicU8>,
	/// UNMotion/Zenoh 受信が live で動いているか (subscriber スレッド起動済み)。
	unmotion_zenoh_live: bool,
	/// UNMotion/Zenoh subscriber が受信したフレーム数。適用前で数える。
	unmotion_zenoh_received_frames: Arc<AtomicU64>,
	/// 描画直前に motion buffer から取り出して document に適用したフレーム数。
	motion_applied_frames: Arc<AtomicU64>,
	motion_receiver_generation: Arc<AtomicU64>,
	/// XYZ デバッグ軸描画の表示フラグ。manifest `[debug] show_axes` / CLI `--show-axes` / IPC で切替可能。
	show_axes: bool,
	show_bone_colliders: bool,
	bone_collider_count: u32,
	bone_collider_source: BoneColliderSource,
}

impl GpuState {
	#[allow(clippy::too_many_arguments)]
	pub fn new_shell(
		window: Arc<Window>,
		transparent: bool,
		primary_motion_source: crate::options::PrimaryMotionSource,
		spout_opts: SpoutWindowOptions,
		environment_color: EnvironmentColorOptions,
		lighting: LightingOptions,
		bloom: BloomOptions,
		ssao: crate::SsaoOptions,
		contact_shadow: ContactShadowOptions,
		aa: AaMode,
		render_backend: RenderBackend,
		texture_compression: TextureCompressionMode,
		debug: WindowDebugOptions,
		disable_expression_morphs: bool,
		disable_vmc_eye_look: bool,
		eye_look_at_clamp_deg: Option<f32>,
		apply_vmc_root_translation: bool,
		mesh_diagnostics: SceneMeshLoadOpts,
	) -> Result<Self, String> {
		let debug_log = DebugLog::from_options(&debug).map_err(|e| e.to_string())?;
		let debug_scene = debug.scene;
		let debug_morph = debug.morph;
		let motion_apply_shared = Arc::new(Mutex::new(un_avatar_skeleton::ApplyUnMotionFrameOpts {
			apply_expressions: !disable_expression_morphs,
			apply_eye_bones: !disable_vmc_eye_look,
			eye_look_at_clamp_deg,
			apply_root_translation: apply_vmc_root_translation,
		}));
		let size = window.inner_size();
		let width = size.width.max(1);
		let height = size.height.max(1);
		let document_wrapped: Option<Arc<RwLock<UnaDocument>>> = None;
		let vmc_live = false;
		let unmotion_zenoh_live = false;
		let unmotion_zenoh_received_frames = Arc::new(AtomicU64::new(0));
		let motion_applied_frames = Arc::new(AtomicU64::new(0));
		let document_revision = Arc::new(AtomicU64::new(0));
		let primary_motion_source = Arc::new(AtomicU8::new(primary_motion_source_to_u8(primary_motion_source)));
		let motion_buffer = Arc::new(MotionControlBuffer::default());

		let render_backend = effective_window_backend(render_backend);
		let instance_descriptor = instance_descriptor_for_backend(render_backend);
		let instance = wgpu::Instance::new(instance_descriptor);

		let surface: wgpu::Surface<'static> = instance.create_surface(window).map_err(|e| format!("create_surface: {e}"))?;

		let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
			power_preference: wgpu::PowerPreference::HighPerformance,
			compatible_surface: Some(&surface),
			force_fallback_adapter: false,
		}))
		.map_err(|e| format!("request_adapter: {e}"))?;

		let mut limits = wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits());
		limits.max_texture_dimension_2d = limits.max_texture_dimension_2d.max(4096);
		limits.max_sampled_textures_per_shader_stage = limits
			.max_sampled_textures_per_shader_stage
			.max(21)
			.min(adapter.limits().max_sampled_textures_per_shader_stage);
		limits.max_samplers_per_shader_stage = limits
			.max_samplers_per_shader_stage
			.max(18)
			.min(adapter.limits().max_samplers_per_shader_stage);

		let adapter_features = adapter.features();
		let texture_compression_features = if matches!(texture_compression, TextureCompressionMode::Source | TextureCompressionMode::Compat)
		{
			wgpu::Features::empty()
		} else {
			adapter_features
				& (wgpu::Features::TEXTURE_COMPRESSION_BC
					| wgpu::Features::TEXTURE_COMPRESSION_ASTC
					| wgpu::Features::TEXTURE_COMPRESSION_ETC2)
		};
		let timestamp_features = adapter_features & wgpu::Features::TIMESTAMP_QUERY;
		let texture_format_features = adapter_features & wgpu::Features::TEXTURE_FORMAT_16BIT_NORM;
		let required_features = texture_compression_features | timestamp_features | texture_format_features;

		let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
			label: Some("un-avatar-renderer"),
			required_features,
			required_limits: limits,
			memory_hints: Default::default(),
			..Default::default()
		}))
		.map_err(|e| format!("request_device: {e}"))?;

		let caps = surface.get_capabilities(&adapter);
		let format = *caps
			.formats
			.first()
			.ok_or_else(|| "get_capabilities: スワップチェーン形式がありません".to_owned())?;

		let alpha_mode = if transparent {
			transparent_alpha_mode(&caps.alpha_modes)
		} else if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
			wgpu::CompositeAlphaMode::Opaque
		} else {
			caps.alpha_modes[0]
		};

		let present_mode = caps
			.present_modes
			.iter()
			.copied()
			.find(|m| *m == wgpu::PresentMode::Fifo)
			.unwrap_or(caps.present_modes[0]);

		let config = wgpu::SurfaceConfiguration {
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
			format,
			width,
			height,
			present_mode,
			alpha_mode,
			view_formats: vec![],
			desired_maximum_frame_latency: 2,
		};

		surface.configure(&device, &config);

		let (depth_texture, depth_view) = create_depth(&device, width, height);

		let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("globals"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: None,
				},
				count: None,
			}],
		});

		let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("globals"),
			size: std::mem::size_of::<GlobalsGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});

		let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("globals"),
			layout: &bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: uniform_buffer.as_entire_binding(),
			}],
		});

		let aa_sample_count = aa_sample_count(aa);
		let pipeline = create_sky_pipeline(&device, &bind_group_layout, format, aa_sample_count);
		let axes_pipeline = create_axes_pipeline(&device, &bind_group_layout, format, aa_sample_count);
		let bone_collider_pipeline = create_bone_collider_pipeline(&device, &bind_group_layout, format, aa_sample_count);
		let startup_splash_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("startup_splash"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<StartupSplashGpu>() as u64),
				},
				count: None,
			}],
		});
		let startup_splash_pipeline = create_startup_splash_pipeline(&device, &startup_splash_bind_group_layout, format, aa_sample_count);
		let startup_splash_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("startup_splash"),
			size: std::mem::size_of::<StartupSplashGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let startup_splash_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("startup_splash"),
			layout: &startup_splash_bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: startup_splash_buffer.as_entire_binding(),
			}],
		});
		let contact_shadow_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
			label: Some("contact_shadow"),
			entries: &[wgpu::BindGroupLayoutEntry {
				binding: 0,
				visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
				ty: wgpu::BindingType::Buffer {
					ty: wgpu::BufferBindingType::Uniform,
					has_dynamic_offset: false,
					min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<ContactShadowGpu>() as u64),
				},
				count: None,
			}],
		});
		let contact_shadow_pipeline = create_contact_shadow_pipeline(
			&device,
			&bind_group_layout,
			&contact_shadow_bind_group_layout,
			format,
			aa_sample_count,
		);
		let contact_shadow_buffer = device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("contact_shadow"),
			size: std::mem::size_of::<ContactShadowGpu>() as u64,
			usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
			mapped_at_creation: false,
		});
		let contact_shadow_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
			label: Some("contact_shadow"),
			layout: &contact_shadow_bind_group_layout,
			entries: &[wgpu::BindGroupEntry {
				binding: 0,
				resource: contact_shadow_buffer.as_entire_binding(),
			}],
		});

		let texture_summary = None;
		let avatar_outline = mesh_diagnostics.avatar_outline;
		let scene_meshes = None;
		let spring_sim = None;
		let bone_collider_count = 0;
		let bone_collider_source = BoneColliderSource::Off;

		#[cfg(windows)]
		let spout_launch = if spout_opts.enabled {
			let name = if spout_opts.name.is_empty() {
				"UN Avatar".to_string()
			} else {
				spout_opts.name.clone()
			};
			Some(crate::spout::SpoutLaunchConfig {
				name,
				width: spout_opts.width,
				height: spout_opts.height,
			})
		} else {
			None
		};
		#[cfg(windows)]
		let spout = spout_launch
			.as_ref()
			.and_then(|lc| crate::spout::SpoutCapture::try_new(&device, format, width, height, lc.clone()));
		#[cfg(windows)]
		if spout_opts.enabled && spout.is_none() {
			eprintln!(
				"un-avatar-renderer: Spout2 実バックエンドがこのビルドで利用できません。標準配布は `cargo xtask package` で Spout2 込みビルドを作成します。開発手動ビルドでは `--features spout-sdk` と SPOUT2_SDK_DIR / SPOUT2_LIB_DIR / 起動前 Spout.dll PATH が必要です。"
			);
		}
		#[cfg(not(windows))]
		if spout_opts.enabled {
			eprintln!("un-avatar-renderer: Spout は現状 Windows のみ対応です");
		}

		let (gw, gh) = Self::buffer_dims(width, height, &spout_opts);

		let mut gpu = Self {
			surface,
			device,
			queue,
			config,
			alpha_modes: caps.alpha_modes,
			depth_texture,
			depth_view,
			uniform_buffer,
			globals_uploaded: None,
			bind_group,
			pipeline,
			axes_pipeline,
			bone_collider_pipeline,
			bone_collider_vertex_buffer: None,
			bone_collider_vertex_capacity: 0,
			bone_collider_vertex_count: 0,
			bone_collider_vertices: Vec::new(),
			startup_splash_pipeline,
			startup_splash_buffer,
			startup_splash_bind_group,
			contact_shadow_pipeline,
			contact_shadow_buffer,
			contact_shadow_bind_group,
			document: document_wrapped,
			document_revision,
			applied_document_revision: 0,
			vmc_live,
			scene_meshes,
			avatar_outline,
			environment_color,
			lighting,
			bloom,
			ssao,
			contact_shadow,
			texture_summary,
			spring_sim,
			bone_colliders: Vec::new(),
			bone_collider_count,
			bone_collider_source,
			aa,
			post_process: None,
			msaa_target: None,
			#[cfg(windows)]
			spout,
			#[cfg(windows)]
			spout_launch,
			debug_log,
			debug_scene,
			debug_morph,
			debug_frame_seq: 0,
			animation_time_secs: 0.0,
			disable_expression_morphs,
			camera: OrbitCamera::default(),
			world_scratch: Vec::new(),
			gpu_timestamps: None,
			expression_overrides: std::collections::BTreeMap::new(),
			expression_overrides_revision: 0,
			applied_expression_overrides_revision: 0,
			expression_presets: Vec::new(),
			motion_apply_shared,
			motion_buffer,
			pending_motion_frames: Vec::new(),
			motion_rest_nodes: None,
			spring_rest_nodes: None,
			primary_motion_source,
			unmotion_zenoh_live,
			unmotion_zenoh_received_frames,
			motion_applied_frames,
			motion_receiver_generation: Arc::new(AtomicU64::new(0)),
			// XYZ 軸はデフォルト Off（manifest や CLI、UI からの明示指示で表示）。
			show_axes: false,
			show_bone_colliders: false,
		};
		if timestamp_features.contains(wgpu::Features::TIMESTAMP_QUERY) {
			gpu.gpu_timestamps = Some(GpuTimestamps::new(&gpu.device, &gpu.queue));
		}
		if let Some(doc_arc) = &gpu.document {
			if let Ok(doc) = doc_arc.read() {
				if let Some(catalog) = doc.expression_catalog.as_ref() {
					gpu.expression_presets = catalog.presets.iter().map(|p| p.name.clone()).collect();
				}
			}
		}
		gpu.write_globals(gw, gh);
		Ok(gpu)
	}

	pub fn expression_presets(&self) -> &[String] {
		&self.expression_presets
	}

	pub fn set_expression_override(&mut self, name: &str, weight: f32) {
		if !weight.is_finite() {
			return;
		}
		let w = weight.clamp(0.0, 1.0);
		if self.expression_overrides.get(name).is_some_and(|current| *current == w) {
			return;
		}
		self.expression_overrides.insert(name.to_string(), w);
		self.expression_overrides_revision = self.expression_overrides_revision.wrapping_add(1);
	}

	pub fn clear_expression_overrides(&mut self) {
		if self.expression_overrides.is_empty() {
			return;
		}
		self.expression_overrides.clear();
		self.expression_overrides_revision = self.expression_overrides_revision.wrapping_add(1);
	}

	/// VRM 1.0 LookAt 簡易クランプ角度を更新する。`None` でクランプ無効化。
	pub fn set_eye_look_at_clamp_deg(&self, clamp_deg: Option<f32>) {
		if let Ok(mut g) = self.motion_apply_shared.lock() {
			g.eye_look_at_clamp_deg = clamp_deg.filter(|d| d.is_finite() && *d >= 0.0);
		}
	}

	/// 現在の LookAt クランプ角度を返す（`None` なら無効）。
	pub fn eye_look_at_clamp_deg(&self) -> Option<f32> {
		self.motion_apply_shared.lock().ok().and_then(|g| g.eye_look_at_clamp_deg)
	}

	/// VMC `Root.translation` を scene root に加算するか。OFF (既定) ならアバターの位置は rest pose を保つ。
	/// Waidayo 等の calibration の都合で意図せず非ゼロな translation が送られる場合に、アバターが
	/// 前後にズレないようにするためのスイッチ。
	pub fn set_apply_vmc_root_translation(&self, enabled: bool) {
		if let Ok(mut g) = self.motion_apply_shared.lock() {
			g.apply_root_translation = enabled;
		}
	}

	/// 現在の VMC Root translation 適用フラグ。
	pub fn apply_vmc_root_translation(&self) -> bool {
		self.motion_apply_shared.lock().map(|g| g.apply_root_translation).unwrap_or(false)
	}

	/// 旧 IPC 互換の primary source 更新。現在の姿勢適用は key 単位の後着優先。
	pub fn set_primary_motion_source(&self, source: crate::options::PrimaryMotionSource) {
		self.primary_motion_source
			.store(primary_motion_source_to_u8(source), Ordering::Relaxed);
	}

	/// 旧 status 互換の primary source 値。
	pub fn primary_motion_source(&self) -> crate::options::PrimaryMotionSource {
		primary_motion_source_from_u8(self.primary_motion_source.load(Ordering::Relaxed))
	}

	/// UNMotion/Zenoh subscriber が起動済みか。`new()` 時の `unmotion_zenoh.enabled` で決定する。
	pub fn unmotion_zenoh_live(&self) -> bool {
		self.unmotion_zenoh_live
	}

	pub fn unmotion_zenoh_received_frames(&self) -> u64 {
		self.unmotion_zenoh_received_frames.load(Ordering::Relaxed)
	}

	pub fn motion_applied_frames(&self) -> u64 {
		self.motion_applied_frames.load(Ordering::Relaxed)
	}

	/// XYZ デバッグ軸表示の ON/OFF。
	pub fn set_show_axes(&mut self, enabled: bool) {
		self.show_axes = enabled;
	}

	pub fn set_show_bone_colliders(&mut self, enabled: bool) {
		self.show_bone_colliders = enabled;
	}

	pub fn reconfigure_spring_bones(
		&mut self,
		enabled: bool,
		bone_collider_config: BoneColliderConfig,
		spring_bone_physics: SpringBonePhysicsConfig,
	) {
		self.reset_spring_bone_nodes_to_rest();
		let Some(doc_arc) = self.document.as_ref() else {
			self.spring_sim = None;
			self.bone_colliders.clear();
			self.bone_collider_count = 0;
			self.bone_collider_source = BoneColliderSource::Off;
			self.bone_collider_vertex_buffer = None;
			self.bone_collider_vertex_capacity = 0;
			self.bone_collider_vertices.clear();
			return;
		};
		let Ok(doc) = doc_arc.read() else {
			return;
		};
		let (colliders, stats) = if let Some(scene) = doc.scene.as_ref() {
			let colliders = build_bone_colliders(scene, doc.humanoid_profile.as_ref(), bone_collider_config);
			let stats = collider_stats(&colliders);
			(colliders, stats)
		} else {
			(Vec::new(), collider_stats(&[]))
		};
		self.bone_collider_count = stats.count;
		self.bone_collider_source = stats.source;
		self.bone_collider_vertex_buffer = None;
		self.bone_collider_vertex_capacity = 0;
		self.bone_collider_vertex_count = 0;
		self.bone_collider_vertices.clear();
		self.bone_colliders = colliders.clone();
		self.spring_sim = if enabled {
			match (doc.scene.as_ref(), doc.spring_bones.as_ref()) {
				(Some(scene), Some(settings)) => SpringBoneSimulator::new_with_config(scene, settings, colliders, spring_bone_physics),
				_ => None,
			}
		} else {
			None
		};
	}

	fn reset_spring_bone_nodes_to_rest(&mut self) {
		let (Some(doc_arc), Some(rest_nodes)) = (self.document.as_ref(), self.spring_rest_nodes.as_ref()) else {
			return;
		};
		let Ok(mut doc) = doc_arc.write() else {
			return;
		};
		let Some(settings) = doc.spring_bones.clone() else {
			return;
		};
		let Some(scene) = doc.scene.as_mut() else {
			return;
		};
		for node_index in settings.groups.iter().flat_map(|group| group.bone_node_indices.iter().copied()) {
			if let (Some(dst), Some(src)) = (scene.nodes.get_mut(node_index), rest_nodes.get(node_index)) {
				dst.transform = src.transform;
			}
		}
		self.applied_document_revision = 0;
		self.document_revision.fetch_add(1, Ordering::Release);
	}

	/// Avatar outline effect を実行中 renderer に即時反映する。
	pub fn set_avatar_outline(&mut self, outline: AvatarOutlineOptions) {
		self.avatar_outline = outline;
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_outline(&self.queue, outline);
		}
	}

	/// Avatar rim light effect を実行中 renderer に即時反映する。
	pub fn set_avatar_rim(&mut self, rim: crate::AvatarRimOptions) {
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_rim(&self.queue, rim);
		}
	}

	/// Avatar matcap strength を実行中 renderer に即時反映する。
	pub fn set_avatar_matcap(&mut self, matcap: crate::AvatarMatcapOptions) {
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_matcap(&self.queue, matcap);
		}
	}

	/// Synthetic specular accent を実行中 renderer に即時反映する。
	pub fn set_avatar_specular(&mut self, specular: crate::AvatarSpecularOptions) {
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_specular(&self.queue, specular);
		}
	}

	/// Authored ambient occlusion strength を実行中 renderer に即時反映する。
	pub fn set_avatar_ambient_occlusion(&mut self, ambient_occlusion: crate::AvatarAmbientOcclusionOptions) {
		if let Some(sm) = &mut self.scene_meshes {
			sm.set_avatar_ambient_occlusion(&self.queue, ambient_occlusion);
		}
	}

	/// Final post color adjustment を実行中 renderer に即時反映する。
	pub fn set_environment_color(&mut self, color: EnvironmentColorOptions) {
		self.environment_color = EnvironmentColorOptions {
			exposure: color.exposure.clamp(-4.0, 4.0),
			contrast: color.contrast.clamp(0.0, 4.0),
			saturation: color.saturation.clamp(0.0, 4.0),
			look: color.look,
			look_intensity: color.look_intensity.clamp(0.0, 1.0),
			temperature: color.temperature.clamp(-1.0, 1.0),
			tint: color.tint.clamp(-1.0, 1.0),
		};
		if matches!(self.environment_color.look, ColorGradingLook::Neutral) {
			self.environment_color.look_intensity = 0.0;
		}
	}

	pub fn set_lighting(&mut self, lighting: LightingOptions) {
		self.lighting = LightingOptions {
			environment: crate::options::EnvironmentLightOptions {
				enabled: lighting.environment.enabled,
				color: [
					lighting.environment.color[0].clamp(0.0, 1.0),
					lighting.environment.color[1].clamp(0.0, 1.0),
					lighting.environment.color[2].clamp(0.0, 1.0),
				],
				intensity: lighting.environment.intensity.clamp(0.0, 2.0),
			},
			directional: crate::options::DirectionalLightOptions {
				enabled: lighting.directional.enabled,
				color: [
					lighting.directional.color[0].clamp(0.0, 1.0),
					lighting.directional.color[1].clamp(0.0, 1.0),
					lighting.directional.color[2].clamp(0.0, 1.0),
				],
				intensity: lighting.directional.intensity.clamp(0.0, 4.0),
				azimuth_deg: lighting.directional.azimuth_deg.clamp(-360.0, 360.0),
				elevation_deg: lighting.directional.elevation_deg.clamp(-89.0, 89.0),
				follow_camera_yaw: lighting.directional.follow_camera_yaw,
				follow_camera_pitch: lighting.directional.follow_camera_pitch,
			},
		};
		self.globals_uploaded = None;
	}

	pub fn set_bloom(&mut self, bloom: BloomOptions) {
		self.bloom = BloomOptions {
			enabled: bloom.enabled,
			strength: bloom.strength.clamp(0.0, 2.0),
			threshold: bloom.threshold.clamp(0.0, 2.0),
			radius: bloom.radius.clamp(0.0, 32.0),
			quality: bloom.quality,
		};
	}

	pub fn set_ssao(&mut self, ssao: crate::SsaoOptions) {
		self.ssao = crate::SsaoOptions {
			enabled: ssao.enabled,
			strength: ssao.strength.clamp(0.0, 1.0),
			radius: ssao.radius.clamp(1.0, 24.0),
			bias: ssao.bias.clamp(0.0, 0.02),
			range: ssao.range.clamp(0.001, 0.2),
		};
	}

	pub fn set_contact_shadow(&mut self, contact_shadow: ContactShadowOptions) {
		self.contact_shadow = ContactShadowOptions {
			enabled: contact_shadow.enabled,
			strength: contact_shadow.strength.clamp(0.0, 1.0),
			radius: contact_shadow.radius.clamp(0.05, 3.0),
			softness: contact_shadow.softness.clamp(0.1, 8.0),
			height: contact_shadow.height.clamp(-1.0, 1.0),
		};
	}

	fn write_contact_shadow_uniform(&self) {
		self.queue.write_buffer(
			&self.contact_shadow_buffer,
			0,
			bytemuck::bytes_of(&ContactShadowGpu {
				params: [
					self.contact_shadow.strength.clamp(0.0, 1.0),
					self.contact_shadow.radius.clamp(0.05, 3.0),
					self.contact_shadow.softness.clamp(0.1, 8.0),
					self.contact_shadow.height.clamp(-1.0, 1.0),
				],
			}),
		);
	}

	fn draw_contact_shadow(&self, pass: &mut wgpu::RenderPass<'_>) {
		pass.set_pipeline(&self.contact_shadow_pipeline);
		pass.set_bind_group(0, &self.bind_group, &[]);
		pass.set_bind_group(1, &self.contact_shadow_bind_group, &[]);
		pass.draw(0..6, 0..1);
	}

	/// XYZ デバッグ軸表示が ON か。
	pub fn show_axes(&self) -> bool {
		self.show_axes
	}

	pub fn show_bone_colliders(&self) -> bool {
		self.show_bone_colliders
	}

	pub fn bone_collider_count(&self) -> u32 {
		self.bone_collider_count
	}

	pub fn bone_collider_source(&self) -> &'static str {
		self.bone_collider_source.as_str()
	}

	fn update_bone_collider_debug_vertices(&mut self) {
		if !self.show_bone_colliders {
			self.bone_collider_vertex_count = 0;
			return;
		}
		let Some(doc_arc) = self.document.as_ref().map(Arc::clone) else {
			self.bone_collider_vertex_count = 0;
			return;
		};
		let Ok(doc) = doc_arc.read() else {
			self.bone_collider_vertex_count = 0;
			return;
		};
		let Some(scene) = doc.scene.as_ref() else {
			self.bone_collider_vertex_count = 0;
			return;
		};
		crate::scene_transform::write_world_from_nodes(scene, &mut self.world_scratch);
		self.rebuild_bone_collider_debug_vertices_from_world();
	}

	fn rebuild_bone_collider_debug_vertices_from_world(&mut self) {
		self.bone_collider_vertices.clear();
		let colliders = self
			.spring_sim
			.as_ref()
			.map(SpringBoneSimulator::bone_colliders)
			.unwrap_or(&self.bone_colliders);
		for collider in colliders {
			append_collider_wire_vertices(*collider, &self.world_scratch, &mut self.bone_collider_vertices);
		}
		if self.bone_collider_vertices.is_empty() {
			self.bone_collider_vertex_count = 0;
			return;
		}
		let vertex_count = self.bone_collider_vertices.len();
		self.bone_collider_vertex_count = vertex_count as u32;
		if self.bone_collider_vertex_capacity < vertex_count || self.bone_collider_vertex_buffer.is_none() {
			let next_capacity = vertex_count.next_power_of_two();
			self.bone_collider_vertex_buffer = Some(self.device.create_buffer(&wgpu::BufferDescriptor {
				label: Some("debug_bone_colliders"),
				size: (next_capacity * std::mem::size_of::<DebugLineVertex>()) as u64,
				usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
				mapped_at_creation: false,
			}));
			self.bone_collider_vertex_capacity = next_capacity;
		}
		if let Some(buffer) = &self.bone_collider_vertex_buffer {
			self.queue
				.write_buffer(buffer, 0, bytemuck::cast_slice(&self.bone_collider_vertices));
		}
	}

	/// 対角画角（度）を設定する。範囲外は内部で clamp。
	pub fn set_camera_fov_diagonal_deg(&mut self, deg: f32) {
		self.camera.set_diagonal_fov_deg(deg);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// 現在のカメラパラメータをスナップショットとして返す（IPC/UI からの読み取り用）。
	pub fn camera_state_snapshot(&self) -> CameraStateSnapshot {
		CameraStateSnapshot {
			target: [self.camera.target.x, self.camera.target.y, self.camera.target.z],
			longitude_deg: self.camera.longitude.to_degrees(),
			latitude_deg: self.camera.latitude.to_degrees(),
			radius: self.camera.radius,
			diagonal_fov_deg: self.camera.diagonal_fov_deg,
		}
	}

	/// IPC から渡された target/orbit/fov 値を一度に上書きする（profile からのロード等で使用）。
	pub fn set_camera_state(
		&mut self,
		target: Option<[f32; 3]>,
		longitude_deg: Option<f32>,
		latitude_deg: Option<f32>,
		radius: Option<f32>,
		diagonal_fov_deg: Option<f32>,
	) {
		if let Some([x, y, z]) = target {
			self.camera.target = glam::Vec3::new(x, y, z);
		}
		self.camera
			.set_orbit(longitude_deg.map(f32::to_radians), latitude_deg.map(f32::to_radians), radius);
		if let Some(deg) = diagonal_fov_deg {
			self.camera.set_diagonal_fov_deg(deg);
		}
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	fn buffer_dims(window_w: u32, window_h: u32, spout_opts: &SpoutWindowOptions) -> (u32, u32) {
		#[cfg(windows)]
		if spout_opts.enabled {
			return (
				spout_opts.width.unwrap_or(window_w).max(1),
				spout_opts.height.unwrap_or(window_h).max(1),
			);
		}
		(window_w.max(1), window_h.max(1))
	}

	fn render_pixel_dims(&self) -> (u32, u32) {
		#[cfg(windows)]
		if let Some(ref sp) = self.spout {
			return sp.dimensions();
		}
		(self.config.width.max(1), self.config.height.max(1))
	}

	pub fn orbit_camera_pixels(&mut self, delta_x: f64, delta_y: f64) {
		const ORBIT_RADIANS_PER_PIXEL: f32 = 0.006;
		self.camera
			.orbit(delta_x as f32 * ORBIT_RADIANS_PER_PIXEL, delta_y as f32 * ORBIT_RADIANS_PER_PIXEL);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	pub fn zoom_camera_wheel(&mut self, wheel_positive_units: f32) {
		self.camera.zoom(wheel_positive_units);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	pub fn reset_camera(&mut self) {
		self.camera = OrbitCamera::default();
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// 視線方向に直交する平面で target を移動する（マウス中ボタンドラッグでのパン用）。
	/// 画面ピクセル基準で `delta_x`/`delta_y` を渡す。
	pub fn pan_camera_pixels(&mut self, delta_x: f64, delta_y: f64) {
		self.camera.pan(delta_x as f32, delta_y as f32);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// orbit (longitude/latitude) のみを初期値に戻す。target/radius は保持。
	pub fn reset_camera_rotation(&mut self) {
		self.camera.reset_rotation();
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// target（pan 位置）のみを初期値に戻す。orbit/radius/FOV は保持。
	/// ミドルダブルクリックで「パン操作のリセット」を行う用途。
	pub fn reset_camera_pan(&mut self) {
		self.camera.reset_pan();
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	/// 単発のオフスクリーンレンダリングで PNG を保存する。透過設定をそのまま含む。
	pub fn capture_screenshot(&mut self, path: &std::path::Path, clear_color: wgpu::Color) -> Result<(), String> {
		let (w, h) = self.render_pixel_dims();
		let format = self.config.format;
		let aa_sample_count = aa_sample_count(self.aa);

		// シーンノードがある場合は現在の pose を再アップロードしておく（前フレーム未提出の可能性に備える）。
		if let (Some(sm), Some(doc_arc)) = (&mut self.scene_meshes, &self.document) {
			if let Ok(doc) = doc_arc.read() {
				if let Some(sc) = &doc.scene {
					crate::scene_transform::write_world_from_nodes(sc, &mut self.world_scratch);
					let expr_weights = if self.disable_expression_morphs {
						None
					} else {
						doc.expression_weights.as_ref()
					};
					let expression_overrides = (!self.expression_overrides.is_empty()).then_some(&self.expression_overrides);
					sm.update_draw_transforms(&self.queue, sc, &self.world_scratch, expr_weights, expression_overrides);
				}
			}
		}
		self.write_globals(w, h);

		let target_tex = self.device.create_texture(&wgpu::TextureDescriptor {
			label: Some("screenshot-target"),
			size: wgpu::Extent3d {
				width: w,
				height: h,
				depth_or_array_layers: 1,
			},
			mip_level_count: 1,
			sample_count: 1,
			dimension: wgpu::TextureDimension::D2,
			format,
			usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
			view_formats: &[],
		});
		let target_view = target_tex.create_view(&wgpu::TextureViewDescriptor::default());

		let mut msaa: Option<crate::post_process::MsaaTarget> = None;
		let mut post: Option<PostProcess> = None;
		let use_msaa = matches!(self.aa, AaMode::Msaa);
		let use_post_aa = matches!(self.aa, AaMode::Fxaa | AaMode::Smaa);
		let use_avatar_outline =
			self.avatar_outline.policy == AvatarOutlinePolicy::Override && self.avatar_outline.width.unwrap_or(0.003) > 0.0;
		let use_color_adjust = !self.environment_color.is_identity();
		let use_bloom = self.bloom.is_enabled();
		let use_ssao = self.ssao.is_enabled();
		let use_post = use_post_aa || use_avatar_outline || use_color_adjust || use_bloom || use_ssao;
		if use_msaa {
			msaa = Some(crate::post_process::MsaaTarget::new(&self.device, w, h, format, aa_sample_count));
		}
		if use_post {
			post = Some(PostProcess::new(&self.device, w, h, format));
		}
		let (depth_tex, depth_view) = create_depth(&self.device, w, h);
		let draw_scene = self.scene_meshes.as_ref().is_some_and(|m| !m.is_empty());
		let draw_contact_shadow = draw_scene && self.contact_shadow.is_enabled();
		let draw_contact_shadow_in_main = draw_contact_shadow && !use_avatar_outline;
		let (main_color, main_depth, main_resolve) = if let Some(post) = &post {
			(post.source_view(), post.depth_view(), None)
		} else if let Some(msaa) = &msaa {
			(msaa.color_view(), msaa.depth_view(), Some(&target_view))
		} else {
			(&target_view, &depth_view, None)
		};
		// depth_tex は MSAA/PostAA で使われないが、Drop されないよう束縛しておく。
		let _ = &depth_tex;

		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("screenshot") });
		{
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("screenshot-main"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: main_color,
					depth_slice: None,
					resolve_target: main_resolve,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(clear_color),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
					view: main_depth,
					depth_ops: Some(wgpu::Operations {
						load: wgpu::LoadOp::Clear(1.0),
						store: wgpu::StoreOp::Store,
					}),
					stencil_ops: None,
				}),
				timestamp_writes: None,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if draw_scene {
				if let Some(sm) = &self.scene_meshes {
					sm.draw_opaque(&mut pass);
					if draw_contact_shadow_in_main {
						self.write_contact_shadow_uniform();
						self.draw_contact_shadow(&mut pass);
					}
					sm.draw_toon_outlines(&mut pass);
					sm.draw_blended(&mut pass);
				}
			} else {
				pass.set_pipeline(&self.pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..3, 0..1);
			}
			if self.show_axes && draw_scene {
				pass.set_pipeline(&self.axes_pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..6, 0..1);
			}
		}
		if post.is_some() {
			{
				let post = post.as_mut().expect("post target is initialized");
				match self.aa {
					AaMode::Fxaa => post.encode_fxaa(
						&self.device,
						&self.queue,
						&mut encoder,
						&target_view,
						self.environment_color,
						self.bloom,
						self.ssao,
					),
					AaMode::Smaa => post.encode_smaa(
						&self.device,
						&self.queue,
						&mut encoder,
						&target_view,
						self.environment_color,
						self.bloom,
						self.ssao,
					),
					AaMode::Off | AaMode::Msaa => {
						if use_color_adjust || use_bloom || use_ssao {
							post.encode_color_adjust(
								&self.device,
								&self.queue,
								&mut encoder,
								&target_view,
								self.environment_color,
								self.bloom,
								self.ssao,
							);
						} else {
							post.encode_fxaa(
								&self.device,
								&self.queue,
								&mut encoder,
								&target_view,
								self.environment_color,
								self.bloom,
								self.ssao,
							);
						}
					}
				}
			}
			if draw_contact_shadow && use_avatar_outline {
				self.write_contact_shadow_uniform();
				let shadow_depth = post.as_ref().expect("post target is initialized").depth_view();
				let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("screenshot-contact-shadow"),
					color_attachments: &[Some(wgpu::RenderPassColorAttachment {
						view: &target_view,
						depth_slice: None,
						resolve_target: None,
						ops: wgpu::Operations {
							load: wgpu::LoadOp::Load,
							store: wgpu::StoreOp::Store,
						},
					})],
					depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
						view: shadow_depth,
						depth_ops: Some(wgpu::Operations {
							load: wgpu::LoadOp::Load,
							store: wgpu::StoreOp::Store,
						}),
						stencil_ops: None,
					}),
					timestamp_writes: None,
					occlusion_query_set: None,
					multiview_mask: None,
				});
				self.draw_contact_shadow(&mut pass);
			}
			if use_avatar_outline {
				let width_px = self.avatar_outline_width_px_for(w, h);
				let post = post.as_mut().expect("post target is initialized");
				post.encode_avatar_outline(&self.device, &self.queue, &mut encoder, &target_view, self.avatar_outline, width_px);
			}
		}

		let unpadded_bpr = w * 4;
		let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
		let padded_bpr = unpadded_bpr.div_ceil(align) * align;
		let staging_size = (padded_bpr as u64) * (h as u64);
		let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
			label: Some("screenshot-staging"),
			size: staging_size,
			usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
			mapped_at_creation: false,
		});
		encoder.copy_texture_to_buffer(
			wgpu::TexelCopyTextureInfo {
				texture: &target_tex,
				mip_level: 0,
				origin: wgpu::Origin3d::ZERO,
				aspect: wgpu::TextureAspect::All,
			},
			wgpu::TexelCopyBufferInfo {
				buffer: &staging,
				layout: wgpu::TexelCopyBufferLayout {
					offset: 0,
					bytes_per_row: Some(padded_bpr),
					rows_per_image: Some(h),
				},
			},
			wgpu::Extent3d {
				width: w,
				height: h,
				depth_or_array_layers: 1,
			},
		);
		self.queue.submit(std::iter::once(encoder.finish()));

		staging.slice(..).map_async(wgpu::MapMode::Read, |_| ());
		self.device.poll(wgpu::PollType::wait_indefinitely()).ok();

		let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
		{
			let view = staging.slice(..).get_mapped_range();
			let row_dst = (w as usize) * 4;
			let row_src = padded_bpr as usize;
			match format {
				wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
					for y in 0..(h as usize) {
						let s = y * row_src;
						let d = y * row_dst;
						for x in 0..(w as usize) {
							rgba[d + x * 4] = view[s + x * 4 + 2];
							rgba[d + x * 4 + 1] = view[s + x * 4 + 1];
							rgba[d + x * 4 + 2] = view[s + x * 4];
							rgba[d + x * 4 + 3] = view[s + x * 4 + 3];
						}
					}
				}
				_ => {
					for y in 0..(h as usize) {
						let s = y * row_src;
						let d = y * row_dst;
						rgba[d..d + row_dst].copy_from_slice(&view[s..s + row_dst]);
					}
				}
			}
		}
		staging.unmap();

		if let Some(parent) = path.parent() {
			if !parent.as_os_str().is_empty() {
				std::fs::create_dir_all(parent).map_err(|e| format!("create screenshot dir {}: {e}", parent.display()))?;
			}
		}
		image::save_buffer(path, &rgba, w, h, image::ColorType::Rgba8).map_err(|e| format!("save screenshot {}: {e}", path.display()))
	}

	pub fn set_camera_orbit(&mut self, longitude: Option<f32>, latitude: Option<f32>, radius: Option<f32>) {
		self.camera.set_orbit(longitude, latitude, radius);
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	pub fn set_spout_output(&mut self, enabled: bool, spout_opts: SpoutWindowOptions) -> bool {
		#[cfg(windows)]
		{
			if !enabled {
				self.spout = None;
				self.spout_launch = None;
				let (gw, gh) = self.render_pixel_dims();
				self.write_globals(gw, gh);
				return false;
			}
			let name = if spout_opts.name.is_empty() {
				"UN Avatar".to_string()
			} else {
				spout_opts.name.clone()
			};
			let launch = crate::spout::SpoutLaunchConfig {
				name,
				width: spout_opts.width,
				height: spout_opts.height,
			};
			self.spout = crate::spout::SpoutCapture::try_new(
				&self.device,
				self.config.format,
				self.config.width,
				self.config.height,
				launch.clone(),
			);
			self.spout_launch = Some(launch);
			let (gw, gh) = self.render_pixel_dims();
			self.write_globals(gw, gh);
			self.spout.is_some()
		}
		#[cfg(not(windows))]
		{
			let _ = (enabled, spout_opts);
			false
		}
	}

	pub fn spout_active(&self) -> bool {
		#[cfg(windows)]
		{
			self.spout.is_some()
		}
		#[cfg(not(windows))]
		{
			false
		}
	}

	#[cfg(windows)]
	pub(crate) fn spout_stats(&self) -> Option<crate::spout::SpoutFrameStats> {
		self.spout.as_ref().map(|spout| spout.stats())
	}

	pub(crate) fn texture_summary(&self) -> Option<TextureUploadSummary> {
		self.texture_summary.clone()
	}

	pub(crate) fn scene_build_context(&self) -> GpuSceneBuildContext {
		GpuSceneBuildContext {
			device: self.device.clone(),
			queue: self.queue.clone(),
			format: self.config.format,
			aa: self.aa,
		}
	}

	pub(crate) fn attach_prepared_document(
		&mut self,
		prepared: PreparedDocumentScene,
		options: DocumentAttachOptions,
	) -> Result<(), String> {
		self.expression_presets = prepared.expression_presets;
		self.document = Some(prepared.document);
		self.spring_rest_nodes = self.document.as_ref().and_then(|doc| {
			doc.read()
				.ok()
				.and_then(|doc| doc.scene.as_ref().map(|scene| Arc::new(scene.nodes.clone())))
		});
		self.document_revision.fetch_add(1, Ordering::Release);
		self.applied_document_revision = 0;
		self.scene_meshes = prepared.scene_meshes;
		self.texture_summary = prepared.texture_summary;
		self.spring_sim = prepared.spring_sim;
		self.bone_colliders = prepared.bone_colliders;
		self.bone_collider_count = prepared.bone_collider_count;
		self.bone_collider_source = prepared.bone_collider_source;
		self.reconfigure_motion_receivers(options.vmc_address, options.unmotion_zenoh, options.debug_vmc)?;
		let (gw, gh) = self.render_pixel_dims();
		self.globals_uploaded = None;
		self.write_globals(gw, gh);
		Ok(())
	}
}

impl GpuSceneBuildContext {
	pub(crate) fn prepare_document_scene(
		self,
		document: Arc<UnaDocument>,
		options: &DocumentAttachOptions,
		mut progress: impl FnMut(SceneMeshBuildProgress),
	) -> Result<PreparedDocumentScene, String> {
		let GpuSceneBuildContext { device, queue, format, aa } = self;
		let mut document = Arc::try_unwrap(document).unwrap_or_else(|document| (*document).clone());
		if document.expression_catalog.as_ref().is_some_and(|c| !c.presets.is_empty()) {
			document.expression_weights.get_or_insert_with(Default::default);
		}
		let document_wrapped = Arc::new(RwLock::new(document));
		let mut scene_meshes = None;
		let mut texture_summary = None;
		{
			let guard = document_wrapped.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			if let Some(sc) = &guard.scene {
				if options.debug_material_dump {
					log_material_skin_report(&guard);
				}
				let mut gpu_texture_compression = if options.block_compression_encoder == BlockCompressionEncoder::Gpu
					&& !matches!(
						options.texture_compression,
						TextureCompressionMode::Source | TextureCompressionMode::Compat
					) {
					Some(crate::texture_pipeline::create_vulkan_gpu_texture_compression_context()?)
				} else {
					None
				};
				let mut sm = SceneMeshes::new(
					&device,
					&queue,
					format,
					aa_sample_count(aa),
					sc,
					guard.expression_catalog.as_ref(),
					options.mesh_diagnostics.clone(),
					options.texture_max_dimension,
					options.texture_compression,
					options.block_compression_encoder,
					options.block_compression_cpu_threads,
					options.mipmap_filter,
					&options.texture_compression_advanced,
					options.texture_compression_bc_supported,
					options.texture_compression_astc_supported,
					options.texture_compression_etc2_supported,
					options.processed_texture_cache,
					gpu_texture_compression.as_mut(),
					&mut progress,
				)?;
				if !sm.is_empty() {
					texture_summary = Some(sm.texture_summary());
					let world = crate::scene_transform::scene_world_matrices(sc);
					sm.update_draw_transforms(&queue, sc, &world, guard.expression_weights.as_ref(), None);
					scene_meshes = Some(sm);
				}
			}
		}
		let bone_colliders: Vec<BoneColliderPrimitive> = {
			let guard = document_wrapped.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			if let Some(scene) = guard.scene.as_ref() {
				build_bone_colliders(scene, guard.humanoid_profile.as_ref(), options.bone_colliders)
			} else {
				Vec::new()
			}
		};
		let bone_collider_stats = collider_stats(&bone_colliders);
		let spring_sim = if options.enable_spring_bones {
			let guard = document_wrapped.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			match (&guard.scene, &guard.spring_bones) {
				(Some(sc), Some(sb)) => {
					SpringBoneSimulator::new_with_config(sc, sb, bone_colliders.clone(), options.spring_bone_physics.clone())
				}
				_ => None,
			}
		} else {
			None
		};
		let expression_presets = document_wrapped
			.read()
			.ok()
			.and_then(|doc| {
				doc.expression_catalog
					.as_ref()
					.map(|catalog| catalog.presets.iter().map(|p| p.name.clone()).collect())
			})
			.unwrap_or_default();
		Ok(PreparedDocumentScene {
			document: document_wrapped,
			scene_meshes,
			texture_summary,
			spring_sim,
			bone_colliders,
			bone_collider_count: bone_collider_stats.count,
			bone_collider_source: bone_collider_stats.source,
			expression_presets,
		})
	}
}

impl GpuState {
	pub fn reconfigure_motion_receivers(
		&mut self,
		vmc_address: Option<SocketAddr>,
		unmotion_zenoh: crate::options::UnmotionZenohOptions,
		debug_vmc: bool,
	) -> Result<(), String> {
		let generation = self.motion_receiver_generation.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
		if self.vmc_live || self.unmotion_zenoh_live {
			std::thread::sleep(Duration::from_millis(60));
		}
		self.vmc_live = false;
		self.unmotion_zenoh_live = false;
		self.unmotion_zenoh_received_frames.store(0, Ordering::Relaxed);
		self.start_motion_receivers(vmc_address, unmotion_zenoh, debug_vmc, generation)
	}

	fn start_motion_receivers(
		&mut self,
		vmc_address: Option<SocketAddr>,
		unmotion_zenoh: crate::options::UnmotionZenohOptions,
		debug_vmc: bool,
		generation: u64,
	) -> Result<(), String> {
		let Some(doc_arc) = self.document.as_ref().map(Arc::clone) else {
			if vmc_address.is_some() {
				eprintln!("un-avatar-renderer: --vmc-address は --gltf でモデルを読み込んだときに指定してください");
			}
			if unmotion_zenoh.enabled {
				eprintln!("un-avatar-renderer: UNMotion/Zenoh 受信を有効化したが、モデル (--gltf) が指定されていません");
			}
			return Ok(());
		};
		let humanoid_ok = {
			let d = doc_arc.read().map_err(|_| "document: RwLock poisoned".to_string())?;
			d.humanoid_profile.is_some() && d.scene.is_some()
		};
		let rest_nodes: Arc<Vec<UnaSceneNode>> = Arc::new(
			doc_arc
				.read()
				.map_err(|_| "document: RwLock poisoned".to_string())?
				.scene
				.as_ref()
				.map(|scene| scene.nodes.clone())
				.unwrap_or_default(),
		);
		self.motion_rest_nodes = Some(Arc::clone(&rest_nodes));
		if let Some(addr) = vmc_address {
			let humanoid_keys_csv = doc_arc
				.read()
				.map_err(|_| "document: RwLock poisoned".to_string())?
				.humanoid_profile
				.as_ref()
				.map(|p| p.bone_node_indices.keys().cloned().collect::<Vec<_>>().join(","))
				.unwrap_or_default();
			if humanoid_ok {
				let log = self.debug_log.clone();
				let motion_buffer_for_vmc = Arc::clone(&self.motion_buffer);
				let receiver_generation = Arc::clone(&self.motion_receiver_generation);
				std::thread::Builder::new()
					.name("un-avatar-vmc".into())
					.spawn(move || {
						let mut marionette = match un_avatar_vmc::VmcMarionette::bind(addr) {
							Ok(m) => m,
							Err(e) => {
								eprintln!("[un-avatar-vmc] bind FAILED addr={addr}: {e}");
								if debug_vmc && log.is_enabled() {
									log.line("vmc", format!("bind_failed {addr}: {e}"));
								}
								return;
							}
						};
						match marionette.local_addr() {
							Ok(local) => eprintln!("[un-avatar-vmc] bind OK requested={addr} local={local}"),
							Err(e) => eprintln!("[un-avatar-vmc] bind OK requested={addr} but local_addr() failed: {e}"),
						}
						if debug_vmc && log.is_enabled() {
							log.line("vmc", format!("thread_start bind={addr} humanoid_profile_keys={humanoid_keys_csv}"));
						}
						let mut seq = 0u64;
						let mut recv_i = 0u64;
						while receiver_generation.load(Ordering::Acquire) == generation {
							match marionette.recv_and_apply() {
								Ok((from, n, events)) => {
									if n == 0 {
										continue;
									}
									recv_i = recv_i.wrapping_add(1);
									if recv_i == 1 {
										eprintln!(
											"[un-avatar-vmc] first packet received from={from} nbytes={n} ev_count={}",
											events.len()
										);
									}
									if events.is_empty() {
										continue;
									}
									seq = seq.wrapping_add(1);
									let frame = marionette.assemble_frame(seq, un_avatar_vmc::wall_clock_ns());
									motion_buffer_for_vmc.push_frame(frame);
								}
								Err(un_avatar_vmc::RecvApplyError::Io(e)) => {
									if matches!(
										e.kind(),
										std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut | std::io::ErrorKind::Interrupted
									) {
										continue;
									}
									if debug_vmc && log.is_enabled() {
										log.line("vmc", format!("recv_io_error: {e}"));
									}
								}
								Err(un_avatar_vmc::RecvApplyError::Decode {
									from,
									nbytes,
									err,
									ref payload_head_hex,
								}) => {
									if debug_vmc && log.is_enabled() {
										log.line(
											"vmc",
											format!("recv_decode_error from={from} nbytes={nbytes} err={err} hex_head={payload_head_hex}"),
										);
									}
								}
							}
						}
						if debug_vmc && log.is_enabled() {
							log.line("vmc", "thread_stop generation_changed");
						}
					})
					.map_err(|e| format!("spawn un-avatar-vmc thread failed: {e}"))?;
				self.vmc_live = true;
			} else {
				eprintln!("un-avatar-renderer: --vmc-address は Humanoid とシーンがあるモデルでのみ有効です");
				if debug_vmc && self.debug_log.is_enabled() {
					self.debug_log.line(
						"vmc",
						format!(
							"marionette thread not started (--vmc-address {addr}): need humanoid_profile + scene (keys_if_any={humanoid_keys_csv})"
						),
					);
				}
			}
		}
		if unmotion_zenoh.enabled {
			if humanoid_ok {
				let strategy = un_motion_frame_zenoh::ZenohTopicStrategy {
					base_key_expr: if unmotion_zenoh.base_key_expr.trim().is_empty() {
						"un-motion/frame".to_string()
					} else {
						unmotion_zenoh.base_key_expr.trim().to_string()
					},
					..un_motion_frame_zenoh::ZenohTopicStrategy::default()
				};
				let log_for_recv = self.debug_log.clone();
				let motion_buffer_for_zenoh = Arc::clone(&self.motion_buffer);
				let received_frames_counter = Arc::clone(&self.unmotion_zenoh_received_frames);
				let receiver_generation = Arc::clone(&self.motion_receiver_generation);
				let key_expr_for_log = strategy.subscribe_key_expr();
				match un_avatar_zenoh::UnAvatarZenohReceiver::declare_zenoh_default(strategy) {
					Ok(receiver) => {
						eprintln!("[un-avatar-zenoh] subscribed key='{key_expr_for_log}'");
						if log_for_recv.is_enabled() {
							log_for_recv.line("unmotion_zenoh", format!("subscribed key={key_expr_for_log}"));
						}
						self.unmotion_zenoh_live = true;
						std::thread::Builder::new()
							.name("un-avatar-zenoh-apply".into())
							.spawn(move || {
								const MAX_ZENOH_APPLY_BATCH: usize = 64;
								let mut received = 0u64;
								while receiver_generation.load(Ordering::Acquire) == generation {
									let frames = receiver.drain_available(MAX_ZENOH_APPLY_BATCH);
									if frames.is_empty() {
										std::thread::sleep(std::time::Duration::from_millis(8));
										continue;
									}
									let batch_len = frames.len();
									received_frames_counter.fetch_add(batch_len as u64, Ordering::Relaxed);
									let mut last_seq = None;
									for frame in frames {
										last_seq = Some(frame.header.sequence);
										motion_buffer_for_zenoh.push_frame(frame);
									}
									received = received.wrapping_add(1);
									if log_for_recv.is_enabled() && (received == 1 || received.is_multiple_of(120)) {
										log_for_recv.line(
											"unmotion_zenoh",
											format!(
												"received batch#{received} frames={batch_len} last_seq={}",
												last_seq.unwrap_or_default()
											),
										);
									}
								}
								if log_for_recv.is_enabled() {
									log_for_recv.line("unmotion_zenoh", "thread_stop generation_changed");
								}
							})
							.map_err(|e| format!("spawn un-avatar-zenoh-apply thread failed: {e}"))?;
					}
					Err(e) => {
						eprintln!("[un-avatar-zenoh] declare failed: {e}");
						if log_for_recv.is_enabled() {
							log_for_recv.line("unmotion_zenoh", format!("declare_failed: {e}"));
						}
					}
				}
			} else {
				eprintln!("un-avatar-renderer: UNMotion/Zenoh 受信は Humanoid とシーンがあるモデルでのみ有効です");
			}
		}
		Ok(())
	}

	fn apply_pending_motion_frames(&mut self) {
		self.motion_buffer.take_pending_frames_into(&mut self.pending_motion_frames);
		if self.pending_motion_frames.is_empty() {
			return;
		}
		let Some(doc_arc) = self.document.as_ref() else {
			return;
		};
		let Ok(mut document) = doc_arc.write() else {
			return;
		};
		let opts = self.motion_apply_shared.lock().map(|g| *g).unwrap_or_default();
		let rest_nodes = self.motion_rest_nodes.as_ref().map(|nodes| nodes.as_slice());
		let should_log = self.debug_log.is_enabled() && self.debug_frame_seq.is_multiple_of(120);
		for frame in &self.pending_motion_frames {
			if should_log {
				self.debug_log.line(
					"motion",
					format!(
						"apply seq={} space={:?} {}",
						frame.header.sequence,
						frame.header.coordinate_space,
						unmotion_frame_hand_summary(frame, &document)
					),
				);
			}
			un_avatar_skeleton::apply_un_motion_frame_to_document_with_rest(&mut document, frame, opts, rest_nodes);
		}
		self.motion_applied_frames
			.fetch_add(self.pending_motion_frames.len() as u64, Ordering::Relaxed);
		self.document_revision.fetch_add(1, Ordering::Release);
	}

	pub fn resize(&mut self, width: u32, height: u32) {
		if width == 0 || height == 0 {
			return;
		}
		let w = width.max(1);
		let h = height.max(1);
		self.config.width = w;
		self.config.height = h;
		self.surface.configure(&self.device, &self.config);
		self.depth_texture.destroy();
		let (tex, view) = create_depth(&self.device, w, h);
		self.depth_texture = tex;
		self.depth_view = view;
		#[cfg(windows)]
		if let (Some(ref mut sp), Some(ref lc)) = (&mut self.spout, &self.spout_launch) {
			sp.resize_to(&self.device, w, h, lc, self.config.format);
		}
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
	}

	pub fn set_transparent(&mut self, transparent: bool) {
		let next = if transparent {
			transparent_alpha_mode(&self.alpha_modes)
		} else if self.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
			wgpu::CompositeAlphaMode::Opaque
		} else {
			self.alpha_modes[0]
		};
		if self.config.alpha_mode == next {
			return;
		}
		self.config.alpha_mode = next;
		self.surface.configure(&self.device, &self.config);
	}

	fn write_globals(&mut self, width: u32, height: u32) {
		let aspect = width.max(1) as f32 / height.max(1) as f32;
		let diagonal_rad = self.camera.diagonal_fov_deg.to_radians();
		let fovy = vertical_fov_from_diagonal(diagonal_rad, aspect);
		let proj = Mat4::perspective_rh(fovy, aspect, 0.1, 200.0);
		let cam_pos = self.camera.position();
		let look_at = self.camera.target;
		let view = Mat4::look_at_rh(cam_pos, look_at, Vec3::Y);
		let view_proj = proj * view;
		let inv_view_proj = view_proj.inverse();
		let light_dir = self.directional_light_dir(cam_pos, look_at, view);
		let light = Vec4::from((light_dir, 0.0));
		let directional_light_color = self.directional_light_color();
		let environment_light_color = self.environment_light_color();
		let globals = GlobalsGpu {
			view_proj: view_proj.to_cols_array_2d(),
			inv_view_proj: inv_view_proj.to_cols_array_2d(),
			light_dir: light.to_array(),
			camera_pos: Vec4::from((cam_pos, 1.0)).to_array(),
			_pad: [0u8; 96],
		};
		if self.globals_uploaded != Some(globals) {
			self.queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&globals));
			self.globals_uploaded = Some(globals);
		}
		if let Some(sm) = &mut self.scene_meshes {
			sm.prepare_frame(
				&self.queue,
				view_proj,
				light,
				Vec4::from((cam_pos, 1.0)),
				directional_light_color,
				environment_light_color,
				self.animation_time_secs,
			);
		}
	}

	fn directional_light_dir(&self, cam_pos: Vec3, look_at: Vec3, _view: Mat4) -> Vec3 {
		let directional = self.lighting.directional;
		if !directional.enabled || directional.intensity <= 0.0 {
			return Vec3::Y;
		}
		let camera_dir = (cam_pos - look_at).try_normalize().unwrap_or(Vec3::Z);
		let camera_yaw = Vec3::new(camera_dir.x, 0.0, camera_dir.z).try_normalize().unwrap_or(Vec3::Z);
		let yaw_basis = if directional.follow_camera_yaw { camera_yaw } else { Vec3::Z };
		let yaw_right = Vec3::new(yaw_basis.z, 0.0, -yaw_basis.x).try_normalize().unwrap_or(Vec3::X);
		let azimuth = directional.azimuth_deg.to_radians();
		let horizontal = (yaw_right * azimuth.sin() + yaw_basis * azimuth.cos())
			.try_normalize()
			.unwrap_or(yaw_basis);
		let camera_pitch = if directional.follow_camera_pitch {
			camera_dir.y.clamp(-1.0, 1.0).asin().to_degrees()
		} else {
			0.0
		};
		let elevation = (directional.elevation_deg + camera_pitch).clamp(-89.0, 89.0).to_radians();
		(horizontal * elevation.cos() + Vec3::Y * elevation.sin())
			.try_normalize()
			.unwrap_or(horizontal)
	}

	fn directional_light_color(&self) -> Vec4 {
		let light = self.lighting.directional;
		let intensity = if light.enabled { light.intensity.clamp(0.0, 4.0) } else { 0.0 };
		Vec4::new(
			light.color[0].clamp(0.0, 1.0),
			light.color[1].clamp(0.0, 1.0),
			light.color[2].clamp(0.0, 1.0),
			intensity,
		)
	}

	fn environment_light_color(&self) -> Vec4 {
		let light = self.lighting.environment;
		let intensity = if light.enabled { light.intensity.clamp(0.0, 2.0) } else { 0.0 };
		Vec4::new(
			light.color[0].clamp(0.0, 1.0),
			light.color[1].clamp(0.0, 1.0),
			light.color[2].clamp(0.0, 1.0),
			intensity,
		)
	}

	fn avatar_outline_width_px_for(&self, width: u32, height: u32) -> f32 {
		let outline_m = self.avatar_outline.width.unwrap_or(0.003).clamp(0.0, 0.05);
		let aspect = width.max(1) as f32 / height.max(1) as f32;
		let diagonal_rad = self.camera.diagonal_fov_deg.to_radians();
		let fovy = vertical_fov_from_diagonal(diagonal_rad, aspect);
		let distance_m = self.camera.radius.max(0.05);
		let pixels_per_meter = height.max(1) as f32 / (2.0 * (fovy * 0.5).tan() * distance_m);
		(outline_m * pixels_per_meter).clamp(0.0, 96.0)
	}

	/// 空シーン（プロシージャルスカイ）を 1 フレーム描画する。`Lost` / `Outdated` 時はリサイズして `None`。
	pub fn render_frame(
		&mut self,
		window: &Window,
		clear_color: wgpu::Color,
		wall_since_last: Duration,
		startup_splash: Option<StartupSplashFrame>,
	) -> Option<FrameTimings> {
		let t_cpu0 = Instant::now();
		// 前フレーム以降に完了した GPU タイムスタンプの readback を進める。
		if self.gpu_timestamps.is_some() {
			self.device.poll(wgpu::PollType::Poll).ok();
			if let Some(ts) = self.gpu_timestamps.as_mut() {
				ts.drain_ready();
			}
		}
		self.animation_time_secs += wall_since_last.as_secs_f32();
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);
		self.debug_frame_seq = self.debug_frame_seq.wrapping_add(1);
		if let (Some(doc_arc), true) = (
			&self.document,
			self.debug_scene && self.debug_log.is_enabled() && self.debug_frame_seq.is_multiple_of(180),
		) {
			if let Ok(g) = doc_arc.read() {
				let roots_str = g
					.scene
					.as_ref()
					.map(|s| format!("{:?}", s.roots))
					.unwrap_or_else(|| "none".to_string());
				let keys = g
					.humanoid_profile
					.as_ref()
					.map(|p| p.bone_node_indices.keys().cloned().collect::<Vec<_>>().join(","))
					.unwrap_or_default();
				self.debug_log.line(
					"scene",
					format!(
						"frame seq={} vmc_live={} scene_roots={} humanoid_keys={}",
						self.debug_frame_seq, self.vmc_live, roots_str, keys
					),
				);
			}
		}
		if let (Some(doc_arc), true) = (
			&self.document,
			self.debug_morph && self.debug_log.is_enabled() && self.debug_frame_seq.is_multiple_of(180),
		) {
			if let Ok(g) = doc_arc.read() {
				let n_presets = g.expression_catalog.as_ref().map(|c| c.presets.len()).unwrap_or(0);
				if let Some(ew) = g.expression_weights.as_ref() {
					let mut pairs: Vec<(&str, f32)> = ew.preset_weights.iter().map(|(k, w)| (k.as_str(), *w)).collect();
					pairs.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(std::cmp::Ordering::Equal));
					let top: Vec<String> = pairs.iter().take(16).map(|(k, w)| format!("{}={:.3}", k, w)).collect();
					self.debug_log.line(
						"morph",
						format!(
							"frame seq={} catalog_presets={} top_weights=[{}]",
							self.debug_frame_seq,
							n_presets,
							top.join(", ")
						),
					);
				} else {
					self.debug_log.line(
						"morph",
						format!(
							"frame seq={} catalog_presets={} no_expression_weights",
							self.debug_frame_seq, n_presets
						),
					);
				}
			}
		}
		let dt = wall_since_last.as_secs_f32();
		self.apply_pending_motion_frames();
		if let (Some(doc_arc), Some(sim)) = (&self.document, &mut self.spring_sim) {
			if let Ok(mut doc) = doc_arc.write() {
				let UnaDocument { scene, spring_bones, .. } = &mut *doc;
				if let (Some(scene), Some(settings)) = (scene, spring_bones.as_ref()) {
					sim.step(scene, settings, dt);
				}
			}
		}
		let (gw, gh) = self.render_pixel_dims();
		self.write_globals(gw, gh);

		let frame = match self.surface.get_current_texture() {
			wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
			wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
				let s = window.inner_size();
				self.resize(s.width, s.height);
				return None;
			}
			wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return None,
			wgpu::CurrentSurfaceTexture::Validation => {
				eprintln!("un-avatar-renderer: get_current_texture: validation error");
				return None;
			}
		};

		let swap_view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());

		#[cfg(windows)]
		if let (Some(ref mut sp), Some(ref lc)) = (&mut self.spout, &self.spout_launch) {
			sp.resize_to(&self.device, self.config.width, self.config.height, lc, self.config.format);
		}

		let draw_scene = self.scene_meshes.as_ref().is_some_and(|m| !m.is_empty());
		let use_spout = {
			#[cfg(windows)]
			{
				self.spout.is_some() && draw_scene
			}
			#[cfg(not(windows))]
			{
				false
			}
		};
		let use_post_aa = matches!(self.aa, AaMode::Fxaa | AaMode::Smaa);
		let use_avatar_outline =
			self.avatar_outline.policy == AvatarOutlinePolicy::Override && self.avatar_outline.width.unwrap_or(0.003) > 0.0;
		let use_color_adjust = !self.environment_color.is_identity();
		let use_bloom = self.bloom.is_enabled();
		let use_ssao = self.ssao.is_enabled();
		let use_post = use_post_aa || use_avatar_outline || use_color_adjust || use_bloom || use_ssao;
		let use_msaa = matches!(self.aa, AaMode::Msaa);
		if use_post {
			if let Some(post) = &mut self.post_process {
				post.resize_to(&self.device, gw, gh, self.config.format);
			} else {
				self.post_process = Some(PostProcess::new(&self.device, gw, gh, self.config.format));
			}
		}
		if use_msaa {
			let sample_count = aa_sample_count(self.aa);
			if let Some(msaa) = &mut self.msaa_target {
				msaa.resize_to(&self.device, gw, gh, self.config.format, sample_count);
			} else {
				self.msaa_target = Some(crate::post_process::MsaaTarget::new(
					&self.device,
					gw,
					gh,
					self.config.format,
					sample_count,
				));
			}
		}

		let draw_contact_shadow = draw_scene && self.contact_shadow.is_enabled();
		let draw_contact_shadow_in_main = draw_contact_shadow && !use_avatar_outline;
		let document_revision = self.document_revision.load(Ordering::Acquire);
		let overrides_active = !self.expression_overrides.is_empty();
		let expression_overrides_changed = self.expression_overrides_revision != self.applied_expression_overrides_revision;
		let scene_pose_may_change =
			self.spring_sim.is_some() || document_revision != self.applied_document_revision || expression_overrides_changed;
		let mut world_scratch_current = false;
		if draw_scene && scene_pose_may_change {
			if let (Some(sm), Some(doc_arc)) = (&mut self.scene_meshes, &self.document) {
				if let Ok(doc) = doc_arc.read() {
					if let Some(sc) = &doc.scene {
						crate::scene_transform::write_world_from_nodes(sc, &mut self.world_scratch);
						world_scratch_current = true;
						if document_revision != self.applied_document_revision {
							if !expression_presets_match_catalog(&self.expression_presets, doc.expression_catalog.as_ref()) {
								self.expression_presets = doc
									.expression_catalog
									.as_ref()
									.map(|c| c.presets.iter().map(|p| p.name.clone()).collect())
									.unwrap_or_default();
							}
						}
						let expr_weights = if self.disable_expression_morphs {
							None
						} else {
							doc.expression_weights.as_ref()
						};
						let expression_overrides =
							(overrides_active && !self.disable_expression_morphs).then_some(&self.expression_overrides);
						sm.update_draw_transforms(&self.queue, sc, &self.world_scratch, expr_weights, expression_overrides);
						self.applied_document_revision = document_revision;
						self.applied_expression_overrides_revision = self.expression_overrides_revision;
					}
				}
			}
		}
		if self.show_bone_colliders && draw_scene {
			if world_scratch_current {
				self.rebuild_bone_collider_debug_vertices_from_world();
			} else {
				self.update_bone_collider_debug_vertices();
			}
		} else {
			self.bone_collider_vertex_count = 0;
		}

		#[cfg(windows)]
		let final_target_view = if use_spout {
			self.spout.as_ref().unwrap().color_view()
		} else {
			&swap_view
		};
		#[cfg(not(windows))]
		let final_target_view = &swap_view;

		let mut main_resolve_target: Option<&wgpu::TextureView> = None;
		let (main_color, main_depth): (&wgpu::TextureView, &wgpu::TextureView) = if use_spout {
			if use_post {
				let post = self.post_process.as_ref().expect("post target is initialized");
				(post.source_view(), post.depth_view())
			} else if use_msaa {
				let msaa = self.msaa_target.as_ref().expect("msaa target is initialized");
				main_resolve_target = Some(final_target_view);
				(msaa.color_view(), msaa.depth_view())
			} else {
				#[cfg(windows)]
				{
					let sp = self.spout.as_ref().unwrap();
					(sp.color_view(), sp.depth_view())
				}
				#[cfg(not(windows))]
				{
					unreachable!()
				}
			}
		} else if use_post {
			let post = self.post_process.as_ref().expect("post target is initialized");
			(post.source_view(), post.depth_view())
		} else if use_msaa {
			let msaa = self.msaa_target.as_ref().expect("msaa target is initialized");
			main_resolve_target = Some(final_target_view);
			(msaa.color_view(), msaa.depth_view())
		} else {
			(&swap_view, &self.depth_view)
		};

		let mut encoder = self
			.device
			.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });

		let timestamp_pass = self.gpu_timestamps.as_ref().and_then(|ts| ts.begin_pass());
		let (timestamp_writes, timestamp_write_idx) = match timestamp_pass {
			Some((writes, idx)) => (Some(writes), Some(idx)),
			None => (None, None),
		};
		let scene_clear_color = if use_spout || clear_color.a <= 0.0 {
			wgpu::Color {
				r: 0.0,
				g: 0.0,
				b: 0.0,
				a: 0.0,
			}
		} else {
			clear_color
		};

		{
			let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
				label: Some("main"),
				color_attachments: &[Some(wgpu::RenderPassColorAttachment {
					view: main_color,
					depth_slice: None,
					resolve_target: main_resolve_target,
					ops: wgpu::Operations {
						load: wgpu::LoadOp::Clear(scene_clear_color),
						store: wgpu::StoreOp::Store,
					},
				})],
				depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
					view: main_depth,
					depth_ops: Some(wgpu::Operations {
						load: wgpu::LoadOp::Clear(1.0),
						store: wgpu::StoreOp::Store,
					}),
					stencil_ops: None,
				}),
				timestamp_writes,
				occlusion_query_set: None,
				multiview_mask: None,
			});
			if draw_scene {
				if let Some(sm) = &self.scene_meshes {
					sm.draw_opaque(&mut pass);
					if draw_contact_shadow_in_main {
						self.write_contact_shadow_uniform();
						self.draw_contact_shadow(&mut pass);
					}
					sm.draw_toon_outlines(&mut pass);
					sm.draw_blended(&mut pass);
				}
			} else {
				pass.set_pipeline(&self.pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..3, 0..1);
			}
			if self.show_axes && draw_scene {
				pass.set_pipeline(&self.axes_pipeline);
				pass.set_bind_group(0, &self.bind_group, &[]);
				pass.draw(0..6, 0..1);
			}
			if self.show_bone_colliders && self.bone_collider_vertex_count > 0 {
				if let Some(buffer) = &self.bone_collider_vertex_buffer {
					pass.set_pipeline(&self.bone_collider_pipeline);
					pass.set_bind_group(0, &self.bind_group, &[]);
					pass.set_vertex_buffer(0, buffer.slice(..));
					pass.draw(0..self.bone_collider_vertex_count, 0..1);
				}
			}
			if let Some(splash) = startup_splash {
				let aspect = gw.max(1) as f32 / gh.max(1) as f32;
				self.queue.write_buffer(
					&self.startup_splash_buffer,
					0,
					bytemuck::bytes_of(&StartupSplashGpu {
						time: splash.time_secs,
						progress: splash.progress,
						aspect,
						phase: splash.phase,
					}),
				);
				pass.set_pipeline(&self.startup_splash_pipeline);
				pass.set_bind_group(0, &self.startup_splash_bind_group, &[]);
				pass.draw(0..3, 0..1);
			}
		}

		if let (Some(ts), Some(idx)) = (self.gpu_timestamps.as_ref(), timestamp_write_idx) {
			ts.encode_resolve(&mut encoder, idx);
		}

		if use_post {
			{
				let post = self.post_process.as_mut().expect("post target is initialized");
				match self.aa {
					AaMode::Fxaa => post.encode_fxaa(
						&self.device,
						&self.queue,
						&mut encoder,
						final_target_view,
						self.environment_color,
						self.bloom,
						self.ssao,
					),
					AaMode::Smaa => post.encode_smaa(
						&self.device,
						&self.queue,
						&mut encoder,
						final_target_view,
						self.environment_color,
						self.bloom,
						self.ssao,
					),
					AaMode::Off | AaMode::Msaa => {
						if use_color_adjust || use_bloom || use_ssao {
							post.encode_color_adjust(
								&self.device,
								&self.queue,
								&mut encoder,
								final_target_view,
								self.environment_color,
								self.bloom,
								self.ssao,
							);
						} else {
							post.encode_fxaa(
								&self.device,
								&self.queue,
								&mut encoder,
								final_target_view,
								self.environment_color,
								self.bloom,
								self.ssao,
							);
						}
					}
				}
			}
			if draw_contact_shadow && use_avatar_outline {
				self.write_contact_shadow_uniform();
				let shadow_depth = self.post_process.as_ref().expect("post target is initialized").depth_view();
				let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
					label: Some("contact-shadow"),
					color_attachments: &[Some(wgpu::RenderPassColorAttachment {
						view: final_target_view,
						depth_slice: None,
						resolve_target: None,
						ops: wgpu::Operations {
							load: wgpu::LoadOp::Load,
							store: wgpu::StoreOp::Store,
						},
					})],
					depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
						view: shadow_depth,
						depth_ops: Some(wgpu::Operations {
							load: wgpu::LoadOp::Load,
							store: wgpu::StoreOp::Store,
						}),
						stencil_ops: None,
					}),
					timestamp_writes: None,
					occlusion_query_set: None,
					multiview_mask: None,
				});
				self.draw_contact_shadow(&mut pass);
			}
			if use_avatar_outline {
				let width_px = self.avatar_outline_width_px_for(gw, gh);
				let post = self.post_process.as_mut().expect("post target is initialized");
				post.encode_avatar_outline(
					&self.device,
					&self.queue,
					&mut encoder,
					final_target_view,
					self.avatar_outline,
					width_px,
				);
			}
		}

		let t_before_submit = Instant::now();
		self.queue.submit(std::iter::once(encoder.finish()));
		if let (Some(ts), Some(idx)) = (self.gpu_timestamps.as_mut(), timestamp_write_idx) {
			ts.after_submit(idx);
		}

		#[cfg(windows)]
		if use_spout {
			let sp = self.spout.as_mut().expect("spout is initialized while active");
			// 1) 前フレーム以降に map が完了したスロットがあれば Spout2 に送る（非ブロッキング）。
			sp.send_mapped_rgba(&self.device);
			// 2) 今フレームの swizzle + readback を encode。リングが空いていれば map を要求する。
			let mut enc2 = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
				label: Some("spout-staging"),
			});
			let staged_slot = sp.copy_to_staging(&mut enc2);
			self.queue.submit(std::iter::once(enc2.finish()));
			if let Some(idx) = staged_slot {
				sp.after_submit_request_map(idx);
			}
			// 3) swap chain にプレビュー用にコピー。
			let mut enc3 = self
				.device
				.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("spout-blit") });
			sp.encode_blit(&mut enc3, &swap_view, clear_color);
			self.queue.submit(std::iter::once(enc3.finish()));
		}

		frame.present();

		Some(FrameTimings {
			wall_since_last_ms: wall_since_last.as_secs_f32() * 1000.0,
			cpu_record_ms: (t_before_submit - t_cpu0).as_secs_f32() * 1000.0,
			gpu_ms: self.gpu_timestamps.as_ref().and_then(|ts| ts.last_gpu_ms()).unwrap_or(0.0),
		})
	}
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> (wgpu::Texture, wgpu::TextureView) {
	let texture = device.create_texture(&wgpu::TextureDescriptor {
		label: Some("un-avatar-depth"),
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

fn append_collider_wire_vertices(collider: BoneColliderPrimitive, world: &[Mat4], out: &mut Vec<DebugLineVertex>) {
	const COLOR: [f32; 4] = [1.0, 0.78, 0.12, 0.72];
	match collider {
		BoneColliderPrimitive::Sphere { node, radius } => {
			if let Some(center) = world.get(node).map(|m| m.transform_point3(Vec3::ZERO)) {
				append_wire_sphere(center, radius, COLOR, out);
			}
		}
		BoneColliderPrimitive::Capsule {
			start_node,
			end_node,
			radius,
		} => {
			let (Some(a), Some(b)) = (
				world.get(start_node).map(|m| m.transform_point3(Vec3::ZERO)),
				world.get(end_node).map(|m| m.transform_point3(Vec3::ZERO)),
			) else {
				return;
			};
			push_debug_line(a, b, COLOR, out);
			append_wire_sphere(a, radius, COLOR, out);
			append_wire_sphere(b, radius, COLOR, out);
		}
	}
}

fn append_wire_sphere(center: Vec3, radius: f32, color: [f32; 4], out: &mut Vec<DebugLineVertex>) {
	if !radius.is_finite() || radius <= 0.0 {
		return;
	}
	const N: usize = 24;
	for plane in 0..3 {
		for i in 0..N {
			let a0 = i as f32 / N as f32 * std::f32::consts::TAU;
			let a1 = (i + 1) as f32 / N as f32 * std::f32::consts::TAU;
			let p0 = circle_point(center, radius, a0, plane);
			let p1 = circle_point(center, radius, a1, plane);
			push_debug_line(p0, p1, color, out);
		}
	}
}

fn circle_point(center: Vec3, radius: f32, angle: f32, plane: usize) -> Vec3 {
	let c = angle.cos() * radius;
	let s = angle.sin() * radius;
	match plane {
		0 => center + Vec3::new(c, s, 0.0),
		1 => center + Vec3::new(c, 0.0, s),
		_ => center + Vec3::new(0.0, c, s),
	}
}

fn push_debug_line(a: Vec3, b: Vec3, color: [f32; 4], out: &mut Vec<DebugLineVertex>) {
	out.push(DebugLineVertex {
		position: a.to_array(),
		color,
	});
	out.push(DebugLineVertex {
		position: b.to_array(),
		color,
	});
}

fn transparent_alpha_mode(alpha_modes: &[wgpu::CompositeAlphaMode]) -> wgpu::CompositeAlphaMode {
	const PREFERRED: [wgpu::CompositeAlphaMode; 4] = [
		wgpu::CompositeAlphaMode::PreMultiplied,
		wgpu::CompositeAlphaMode::PostMultiplied,
		wgpu::CompositeAlphaMode::Inherit,
		wgpu::CompositeAlphaMode::Auto,
	];
	PREFERRED
		.into_iter()
		.find(|mode| alpha_modes.contains(mode))
		.unwrap_or_else(|| alpha_modes.first().copied().unwrap_or(wgpu::CompositeAlphaMode::Opaque))
}

fn aa_sample_count(aa: AaMode) -> u32 {
	match aa {
		AaMode::Msaa => 4,
		AaMode::Off | AaMode::Fxaa | AaMode::Smaa => 1,
	}
}

fn create_sky_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("sky"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_SKY)),
	});

	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("sky"),
		bind_group_layouts: &[Some(bind_group_layout)],
		immediate_size: 0,
	});

	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("sky"),
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
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(true),
			depth_compare: Some(wgpu::CompareFunction::LessEqual),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

fn create_axes_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("debug_axes"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_AXES)),
	});

	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("debug_axes"),
		bind_group_layouts: &[Some(bind_group_layout)],
		immediate_size: 0,
	});

	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("debug_axes"),
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
			topology: wgpu::PrimitiveTopology::LineList,
			..Default::default()
		},
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(false),
			depth_compare: Some(wgpu::CompareFunction::LessEqual),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

fn create_bone_collider_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("debug_bone_colliders"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_BONE_COLLIDERS)),
	});
	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("debug_bone_colliders"),
		bind_group_layouts: &[Some(bind_group_layout)],
		immediate_size: 0,
	});
	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("debug_bone_colliders"),
		layout: Some(&layout),
		cache: None,
		vertex: wgpu::VertexState {
			module: &shader,
			entry_point: Some("vs_main"),
			compilation_options: Default::default(),
			buffers: &[wgpu::VertexBufferLayout {
				array_stride: std::mem::size_of::<DebugLineVertex>() as u64,
				step_mode: wgpu::VertexStepMode::Vertex,
				attributes: &[
					wgpu::VertexAttribute {
						format: wgpu::VertexFormat::Float32x3,
						offset: 0,
						shader_location: 0,
					},
					wgpu::VertexAttribute {
						format: wgpu::VertexFormat::Float32x4,
						offset: 12,
						shader_location: 1,
					},
				],
			}],
		},
		fragment: Some(wgpu::FragmentState {
			module: &shader,
			entry_point: Some("fs_main"),
			compilation_options: Default::default(),
			targets: &[Some(wgpu::ColorTargetState {
				format: surface_format,
				blend: Some(wgpu::BlendState::ALPHA_BLENDING),
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState {
			topology: wgpu::PrimitiveTopology::LineList,
			..Default::default()
		},
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(false),
			depth_compare: Some(wgpu::CompareFunction::LessEqual),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

fn create_contact_shadow_pipeline(
	device: &wgpu::Device,
	globals_layout: &wgpu::BindGroupLayout,
	shadow_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("contact_shadow"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_CONTACT_SHADOW)),
	});

	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("contact_shadow"),
		bind_group_layouts: &[Some(globals_layout), Some(shadow_layout)],
		immediate_size: 0,
	});

	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("contact_shadow"),
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
				blend: Some(wgpu::BlendState::ALPHA_BLENDING),
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState {
			topology: wgpu::PrimitiveTopology::TriangleList,
			..Default::default()
		},
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(false),
			depth_compare: Some(wgpu::CompareFunction::Always),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

fn create_startup_splash_pipeline(
	device: &wgpu::Device,
	bind_group_layout: &wgpu::BindGroupLayout,
	surface_format: wgpu::TextureFormat,
	sample_count: u32,
) -> wgpu::RenderPipeline {
	let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
		label: Some("startup_splash"),
		source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_STARTUP_SPLASH)),
	});

	let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
		label: Some("startup_splash"),
		bind_group_layouts: &[Some(bind_group_layout)],
		immediate_size: 0,
	});

	device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
		label: Some("startup_splash"),
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
				blend: Some(wgpu::BlendState::ALPHA_BLENDING),
				write_mask: wgpu::ColorWrites::ALL,
			})],
		}),
		primitive: wgpu::PrimitiveState {
			topology: wgpu::PrimitiveTopology::TriangleList,
			..Default::default()
		},
		depth_stencil: Some(wgpu::DepthStencilState {
			format: wgpu::TextureFormat::Depth24Plus,
			depth_write_enabled: Some(false),
			depth_compare: Some(wgpu::CompareFunction::Always),
			stencil: wgpu::StencilState::default(),
			bias: wgpu::DepthBiasState::default(),
		}),
		multisample: wgpu::MultisampleState {
			count: sample_count,
			..Default::default()
		},
		multiview_mask: None,
	})
}

#[cfg(test)]
mod tests {
	use super::transparent_alpha_mode;
	use wgpu::CompositeAlphaMode::{Auto, Opaque, PostMultiplied, PreMultiplied};

	#[test]
	fn transparent_alpha_mode_prefers_explicit_premultiplied_alpha_over_auto() {
		assert_eq!(
			transparent_alpha_mode(&[Auto, Opaque, PostMultiplied, PreMultiplied]),
			PreMultiplied
		);
	}

	#[test]
	fn transparent_alpha_mode_uses_straight_alpha_when_premultiplied_is_missing() {
		assert_eq!(transparent_alpha_mode(&[Auto, Opaque, PostMultiplied]), PostMultiplied);
	}
}
