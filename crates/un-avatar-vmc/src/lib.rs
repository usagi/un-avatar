//! VMC Protocol（OSC over UDP）の受信側ブートストラップと [`UNMotionFrame`] への変換。
//!
//! 仕様: <https://protocol.vmc.info/english.html>（Marionette 受信・Performer 送信）
//!
//! 設計正本: `docs/development-plan.md` Commit 1.7–1.8

#![forbid(unsafe_code)]

use std::{
	collections::BTreeMap,
	io,
	net::{SocketAddr, UdpSocket},
	time::{Duration, SystemTime, UNIX_EPOCH},
};

/// rosc が想定する Ethernet 向け MTU（参考。Marionette 受信には [`MARIONETTE_RECV_BUFFER`] を使う）。
pub use rosc::decoder::MTU;
use rosc::{OscMessage, OscPacket, OscType};
use serde::{Deserialize, Serialize};
use un_motion_frame::{
	BodyMotion, BoneSample, CoordinateSpace, ExpressionSample, FaceMotion, Finger, FingerPose, HandMotion, Handedness, HumanoidBone,
	HumanoidPose, LengthUnit, MotionSourceInfo, MotionSourceKind, Quatf, SampleState, TimestampBasis, TrackingState, TransformSample,
	UNMotionFrame, Vec3f,
};

/// 公式でよく使われる Marionette 待受ポート。
pub const DEFAULT_MARIONETTE_PORT: u16 = 39539;

/// Marionette 用 `recv_from` バッファ長（バイト）。
///
/// rosc の [`MTU`]（約 1536）では、送信側が骨・ブレンドを 1 つの OSC bundle にまとめた UDP が収まらず、
/// Windows では **10040 (WSAEMSGSIZE)** になりやすい。IPv4 UDP の扱える上限に近いサイズにする。
pub const MARIONETTE_RECV_BUFFER: usize = 65535;

pub const ADDR_ROOT_POS: &str = "/VMC/Ext/Root/Pos";
pub const ADDR_BONE_POS: &str = "/VMC/Ext/Bone/Pos";
pub const ADDR_BLEND_VAL: &str = "/VMC/Ext/Blend/Val";
pub const ADDR_BLEND_APPLY: &str = "/VMC/Ext/Blend/Apply";
pub const ADDR_RELATIVE_TIME: &str = "/VMC/Ext/T";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BonePoseLocal {
	pub px: f32,
	pub py: f32,
	pub pz: f32,
	pub qx: f32,
	pub qy: f32,
	pub qz: f32,
	pub qw: f32,
}

/// デコード済みの Marionette 向けイベント（ログ・テスト・中間表現用）。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum VmcEvent {
	RootPos {
		bone_name: String,
		#[serde(flatten)]
		pose: BonePoseLocal,
	},
	BonePos {
		bone_name: String,
		#[serde(flatten)]
		pose: BonePoseLocal,
	},
	BlendVal {
		name: String,
		value: f32,
	},
	BlendApply,
	RelativeTime {
		seconds: f32,
	},
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VmcDecodeError {
	Osc(String),
}

impl std::fmt::Display for VmcDecodeError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			VmcDecodeError::Osc(s) => write!(f, "OSC: {s}"),
		}
	}
}

impl std::error::Error for VmcDecodeError {}

fn hex_preview(bytes: &[u8], max: usize) -> String {
	bytes.iter().take(max).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// [`VmcMarionette::recv_and_apply`] の I/O 以外の失敗（主に OSC デコード）。
#[derive(Debug)]
pub enum RecvApplyError {
	Io(io::Error),
	Decode {
		from: SocketAddr,
		nbytes: usize,
		err: VmcDecodeError,
		payload_head_hex: String,
	},
}

impl std::fmt::Display for RecvApplyError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			RecvApplyError::Io(e) => write!(f, "{e}"),
			RecvApplyError::Decode {
				from,
				nbytes,
				err,
				payload_head_hex,
			} => write!(f, "decode from {from} nbytes={nbytes}: {err}; hex_head={payload_head_hex}"),
		}
	}
}

impl std::error::Error for RecvApplyError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			RecvApplyError::Io(e) => Some(e),
			RecvApplyError::Decode { .. } => None,
		}
	}
}

impl From<io::Error> for RecvApplyError {
	fn from(e: io::Error) -> Self {
		RecvApplyError::Io(e)
	}
}

fn as_f32(t: &OscType) -> Option<f32> {
	match t {
		OscType::Float(v) => Some(*v),
		OscType::Double(v) => Some(*v as f32),
		OscType::Int(v) => Some(*v as f32),
		OscType::Long(v) => Some(*v as f32),
		_ => None,
	}
}

fn as_string(t: &OscType) -> Option<String> {
	match t {
		OscType::String(s) => Some(s.clone()),
		_ => None,
	}
}

fn parse_pose_local(args: &[OscType]) -> Option<(String, BonePoseLocal)> {
	if args.len() < 8 {
		return None;
	}
	let bone_name = as_string(&args[0])?;
	let px = as_f32(&args[1])?;
	let py = as_f32(&args[2])?;
	let pz = as_f32(&args[3])?;
	let qx = as_f32(&args[4])?;
	let qy = as_f32(&args[5])?;
	let qz = as_f32(&args[6])?;
	let qw = as_f32(&args[7])?;
	Some((
		bone_name,
		BonePoseLocal {
			px,
			py,
			pz,
			qx,
			qy,
			qz,
			qw,
		},
	))
}

fn message_to_event(msg: OscMessage) -> Option<VmcEvent> {
	let OscMessage { addr, args } = msg;
	match addr.as_str() {
		ADDR_ROOT_POS => {
			let (bone_name, pose) = parse_pose_local(&args)?;
			Some(VmcEvent::RootPos { bone_name, pose })
		}
		ADDR_BONE_POS => {
			let (bone_name, pose) = parse_pose_local(&args)?;
			Some(VmcEvent::BonePos { bone_name, pose })
		}
		ADDR_BLEND_VAL => {
			if args.len() < 2 {
				return None;
			}
			let name = as_string(&args[0])?;
			let value = as_f32(&args[1])?;
			Some(VmcEvent::BlendVal { name, value })
		}
		ADDR_BLEND_APPLY => Some(VmcEvent::BlendApply),
		ADDR_RELATIVE_TIME => {
			if args.is_empty() {
				return None;
			}
			let seconds = as_f32(&args[0])?;
			Some(VmcEvent::RelativeTime { seconds })
		}
		_ => None,
	}
}

fn push_events_from_packet(packet: OscPacket, events: &mut Vec<VmcEvent>) {
	match packet {
		OscPacket::Message(message) => {
			if let Some(event) = message_to_event(message) {
				events.push(event);
			}
		}
		OscPacket::Bundle(bundle) => {
			for packet in bundle.content {
				push_events_from_packet(packet, events);
			}
		}
	}
}

/// 1 つの UDP ダタグラム（OSC メッセージまたは bundle）を Marionette 向けイベントに展開する。
pub fn decode_vmcp_osc_bytes(buf: &[u8]) -> Result<Vec<VmcEvent>, VmcDecodeError> {
	let (_rem, packet) = rosc::decoder::decode_udp(buf).map_err(|e| VmcDecodeError::Osc(e.to_string()))?;
	let mut events = Vec::new();
	push_events_from_packet(packet, &mut events);
	Ok(events)
}

/// 送信側が `Chest_LeftArm` やパス付き文字列を使う場合に最後のセグメントだけを見る。
fn normalize_vmc_bone_token(name: &str) -> String {
	let t = name.trim().trim_matches('\0');
	let leaf = t.rsplit(['/', '\\']).next().unwrap_or(t);
	leaf.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase()
}

/// Unity [`HumanBodyBones`](https://docs.unity3d.com/ScriptReference/HumanBodyBones.html) 相当名（正規化済み小文字）→ [`HumanoidBone`]。
fn humanoid_from_ascii_token(k: &str) -> Option<HumanoidBone> {
	match k {
		"hips" | "pelvis" => Some(HumanoidBone::Hips),
		"spine" => Some(HumanoidBone::Spine),
		"chest" => Some(HumanoidBone::Chest),
		"upperchest" => Some(HumanoidBone::UpperChest),
		"neck" => Some(HumanoidBone::Neck),
		"head" => Some(HumanoidBone::Head),
		"leftshoulder" => Some(HumanoidBone::LeftShoulder),
		"leftupperarm" => Some(HumanoidBone::LeftUpperArm),
		"leftlowerarm" => Some(HumanoidBone::LeftLowerArm),
		"lefthand" => Some(HumanoidBone::LeftHand),
		"rightshoulder" => Some(HumanoidBone::RightShoulder),
		"rightupperarm" => Some(HumanoidBone::RightUpperArm),
		"rightlowerarm" => Some(HumanoidBone::RightLowerArm),
		"righthand" => Some(HumanoidBone::RightHand),
		"leftupperleg" => Some(HumanoidBone::LeftUpperLeg),
		"leftlowerleg" => Some(HumanoidBone::LeftLowerLeg),
		"leftfoot" => Some(HumanoidBone::LeftFoot),
		"lefttoes" => Some(HumanoidBone::LeftToes),
		"rightupperleg" => Some(HumanoidBone::RightUpperLeg),
		"rightlowerleg" => Some(HumanoidBone::RightLowerLeg),
		"rightfoot" => Some(HumanoidBone::RightFoot),
		"righttoes" => Some(HumanoidBone::RightToes),
		"lefteye" => Some(HumanoidBone::LeftEye),
		"righteye" => Some(HumanoidBone::RightEye),
		"jaw" => Some(HumanoidBone::Jaw),
		_ => None,
	}
}

fn humanoid_from_suffix_token(k: &str) -> Option<HumanoidBone> {
	for sep in [':', '.'] {
		if let Some((_, tail)) = k.rsplit_once(sep) {
			if let Some(b) = humanoid_from_ascii_token(tail) {
				return Some(b);
			}
		}
	}
	None
}

/// VMC のボーン文字列（[`normalize_vmc_bone_token`] 前でも可）→ [`HumanoidBone`]。
///
/// - Unity `HumanBodyBones.ToString()`（例: `LeftUpperArm`）に加え、VRoid の `J_Bip_*` や Mixamo 風の接尾辞を扱う。
pub fn vmc_bone_name_to_humanoid(name: &str) -> Option<HumanoidBone> {
	let k = normalize_vmc_bone_token(name);
	if let Some(b) = humanoid_from_ascii_token(k.as_str()) {
		return Some(b);
	}
	if let Some(b) = humanoid_from_suffix_token(k.as_str()) {
		return Some(b);
	}
	// VRoid 標準: J_Bip_C_Hips, J_Bip_L_Shoulder, …
	if let Some(rest) = k.strip_prefix("j_bip_c_") {
		return humanoid_from_ascii_token(rest);
	}
	if let Some(rest) = k.strip_prefix("j_bip_l_") {
		let key = format!("left{rest}");
		return humanoid_from_ascii_token(&key);
	}
	if let Some(rest) = k.strip_prefix("j_bip_r_") {
		let key = format!("right{rest}");
		return humanoid_from_ascii_token(&key);
	}
	None
}

fn pose_to_transform(p: &BonePoseLocal) -> TransformSample {
	TransformSample {
		translation: Some(Vec3f { x: p.px, y: p.py, z: p.pz }),
		rotation: Some(Quatf {
			x: p.qx,
			y: p.qy,
			z: p.qz,
			w: p.qw,
		}),
		scale: None,
		linear_velocity: None,
		angular_velocity: None,
	}
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VmcHandSide {
	Left,
	Right,
}

fn vmc_finger_bone_route(name: &str) -> Option<(VmcHandSide, Finger, usize)> {
	let k = normalize_vmc_bone_token(name);
	let (side, rest) = if let Some(rest) = k.strip_prefix("left") {
		(VmcHandSide::Left, rest)
	} else {
		(VmcHandSide::Right, k.strip_prefix("right")?)
	};
	let (finger, segment) = if let Some(segment) = rest.strip_prefix("thumb") {
		(Finger::Thumb, segment)
	} else if let Some(segment) = rest.strip_prefix("index") {
		(Finger::Index, segment)
	} else if let Some(segment) = rest.strip_prefix("middle") {
		(Finger::Middle, segment)
	} else if let Some(segment) = rest.strip_prefix("ring") {
		(Finger::Ring, segment)
	} else if let Some(segment) = rest.strip_prefix("little") {
		(Finger::Little, segment)
	} else {
		return None;
	};
	let joint_index = match segment {
		"proximal" => 0,
		"intermediate" => 1,
		"distal" => 2,
		_ => return None,
	};
	Some((side, finger, joint_index))
}

fn hand_motion_from_vmc_finger_bones(bones: &BTreeMap<String, BonePoseLocal>, side: VmcHandSide) -> Option<HandMotion> {
	let mut finger_poses = Vec::with_capacity(5);
	for finger in [Finger::Thumb, Finger::Index, Finger::Middle, Finger::Ring, Finger::Little] {
		let mut joints = [None, None, None];
		for (name, pose) in bones {
			let Some((bone_side, bone_finger, joint_index)) = vmc_finger_bone_route(name) else {
				continue;
			};
			if bone_side == side && bone_finger == finger {
				joints[joint_index] = Some(pose_to_transform(pose));
			}
		}
		if joints.iter().any(Option::is_some) {
			finger_poses.push(FingerPose {
				finger,
				joints: joints.into_iter().map(|joint| joint.unwrap_or_else(identity_transform)).collect(),
				confidence: 1.0,
			});
		}
	}
	if finger_poses.is_empty() {
		return None;
	}
	Some(HandMotion {
		tracking_state: TrackingState::Valid,
		confidence: 1.0,
		wrist: None,
		fingers: finger_poses,
	})
}

fn identity_transform() -> TransformSample {
	TransformSample {
		translation: None,
		rotation: Some(Quatf {
			x: 0.0,
			y: 0.0,
			z: 0.0,
			w: 1.0,
		}),
		scale: None,
		linear_velocity: None,
		angular_velocity: None,
	}
}

/// ストリームから蓄積したボーン／ブレンドシェイプを保持し、[`UNMotionFrame`] にまとめる。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VmcMotionAssembler {
	pub root: Option<(String, BonePoseLocal)>,
	pub bones: BTreeMap<String, BonePoseLocal>,
	pub blend_shapes: BTreeMap<String, f32>,
	pub last_relative_time: Option<f32>,
}

impl VmcMotionAssembler {
	/// Humanoid にマップされない VMC ボーン名（`/VMC/Ext/Bone/Pos` 由来。正規化前のキー）。
	pub fn unmapped_bone_keys(&self) -> Vec<String> {
		let mut v: Vec<String> = self
			.bones
			.keys()
			.filter(|name| vmc_bone_name_to_humanoid(name).is_none())
			.cloned()
			.collect();
		if let Some((root_name, _)) = &self.root {
			if vmc_bone_name_to_humanoid(root_name).is_none() {
				v.push(format!("root:{root_name}"));
			}
		}
		v.sort();
		v.dedup();
		v
	}

	pub fn apply_event(&mut self, e: &VmcEvent) {
		match e {
			VmcEvent::RootPos { bone_name, pose } => {
				self.root = Some((bone_name.clone(), pose.clone()));
			}
			VmcEvent::BonePos { bone_name, pose } => {
				self.bones.insert(bone_name.clone(), pose.clone());
			}
			VmcEvent::BlendVal { name, value } => {
				self.blend_shapes.insert(name.clone(), *value);
			}
			VmcEvent::BlendApply => {}
			VmcEvent::RelativeTime { seconds } => {
				self.last_relative_time = Some(*seconds);
			}
		}
	}

	/// `sequence` は受信ごとに増やす想定。`frame_timestamp_ns` は wall-clock など任意。
	pub fn to_un_motion_frame(&self, sequence: u64, frame_timestamp_ns: u64) -> UNMotionFrame {
		let mut frame = UNMotionFrame::new(sequence);
		frame.header.timestamp_basis = TimestampBasis::Monotonic;
		frame.header.frame_timestamp_ns = frame_timestamp_ns;
		frame.header.coordinate_space = CoordinateSpace::Vmc;
		frame.header.handedness = Handedness::LeftHanded;
		frame.header.length_unit = LengthUnit::Meter;
		frame.sources.push(MotionSourceInfo {
			source_id: "vmc.osc".to_string(),
			source_kind: MotionSourceKind::VmcInput,
			display_name: Some("VMC Protocol".to_string()),
			confidence: 1.0,
			latency_ns: None,
			state: TrackingState::Valid,
		});

		let root_ts = self.root.as_ref().map(|(_, p)| pose_to_transform(p));

		let mut bone_samples: Vec<BoneSample> = self
			.bones
			.iter()
			.filter_map(|(name, pose)| {
				vmc_bone_name_to_humanoid(name).map(|bone| BoneSample {
					bone,
					transform: pose_to_transform(pose),
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				})
			})
			.collect();
		bone_samples.sort_by_key(|b| b.bone as u16);

		if root_ts.is_some() || !bone_samples.is_empty() {
			frame.body = Some(BodyMotion {
				tracking_state: TrackingState::Valid,
				confidence: 1.0,
				humanoid: Some(HumanoidPose {
					root: root_ts,
					bones: bone_samples,
				}),
			});
		}

		if !self.blend_shapes.is_empty() {
			let expressions: Vec<ExpressionSample> = self
				.blend_shapes
				.iter()
				.map(|(name, value)| ExpressionSample {
					name: name.clone(),
					value: *value,
					confidence: 1.0,
					source_index: Some(0),
					state: SampleState::Valid,
				})
				.collect();
			frame.face = Some(FaceMotion {
				tracking_state: TrackingState::Valid,
				confidence: 1.0,
				head: None,
				expressions,
			});
		}

		frame.left_hand = hand_motion_from_vmc_finger_bones(&self.bones, VmcHandSide::Left);
		frame.right_hand = hand_motion_from_vmc_finger_bones(&self.bones, VmcHandSide::Right);

		if let Some(t) = self.last_relative_time {
			frame.metadata.notes.push(format!("vmc_relative_time_s={t}"));
		}

		frame
	}
}

/// UDP で Marionette として待受し、イベントを蓄積する。
pub struct VmcMarionette {
	socket: UdpSocket,
	buf: Vec<u8>,
	assembler: VmcMotionAssembler,
}

impl VmcMarionette {
	pub fn bind(addr: SocketAddr) -> io::Result<Self> {
		let socket = UdpSocket::bind(addr)?;
		socket.set_read_timeout(Some(Duration::from_millis(50)))?;
		Ok(Self {
			socket,
			buf: vec![0u8; MARIONETTE_RECV_BUFFER],
			assembler: VmcMotionAssembler::default(),
		})
	}

	/// 実際にバインドされたアドレス（OS が割り当てた `local_addr` をそのまま返す）。
	/// Windows で `0.0.0.0:39540` 等の wildcard listen が成功したかを診断するために使う。
	pub fn local_addr(&self) -> io::Result<SocketAddr> {
		self.socket.local_addr()
	}

	/// UDP 受信・デコード・アセンブラ適用。戻り値は `(送信元, バイト数, Marionette イベント)`。`nbytes == 0` のときイベントは空。
	pub fn recv_and_apply(&mut self) -> Result<(SocketAddr, usize, Vec<VmcEvent>), RecvApplyError> {
		let (n, from) = self.socket.recv_from(&mut self.buf)?;
		if n == 0 {
			return Ok((from, 0, Vec::new()));
		}
		let slice = &self.buf[..n];
		match decode_vmcp_osc_bytes(slice) {
			Ok(events) => {
				for e in &events {
					self.assembler.apply_event(e);
				}
				Ok((from, n, events))
			}
			Err(err) => Err(RecvApplyError::Decode {
				from,
				nbytes: n,
				err,
				payload_head_hex: hex_preview(slice, 64),
			}),
		}
	}

	pub fn assembler(&self) -> &VmcMotionAssembler {
		&self.assembler
	}

	pub fn assembler_mut(&mut self) -> &mut VmcMotionAssembler {
		&mut self.assembler
	}

	pub fn assemble_frame(&self, sequence: u64, frame_timestamp_ns: u64) -> UNMotionFrame {
		self.assembler.to_un_motion_frame(sequence, frame_timestamp_ns)
	}
}

/// 現在時刻をナノ秒（UNIX エポック）で返す（ヘッダの目安用）。
pub fn wall_clock_ns() -> u64 {
	SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|d| d.as_nanos().min(u128::from(u64::MAX)) as u64)
		.unwrap_or(0)
}

#[cfg(test)]
mod tests {
	use super::*;
	use rosc::encoder;
	use rosc::{OscBundle, OscMessage, OscPacket, OscTime, OscType};

	fn encode(msg: OscMessage) -> Vec<u8> {
		encoder::encode(&OscPacket::Message(msg)).expect("encode")
	}

	#[test]
	fn decodes_bone_pos_with_int_coordinates() {
		let bytes = encode(OscMessage {
			addr: ADDR_BONE_POS.to_string(),
			args: vec![
				OscType::String("Hips".into()),
				OscType::Int(0),
				OscType::Int(1),
				OscType::Int(0),
				OscType::Float(0.0),
				OscType::Float(0.0),
				OscType::Float(0.0),
				OscType::Float(1.0),
			],
		});
		let ev = decode_vmcp_osc_bytes(&bytes).unwrap();
		assert_eq!(ev.len(), 1);
		match &ev[0] {
			VmcEvent::BonePos { pose, .. } => {
				assert!((pose.py - 1.0).abs() < 1e-5);
			}
			_ => panic!("expected BonePos"),
		}
	}

	#[test]
	fn bone_name_trims_path_segments() {
		assert_eq!(vmc_bone_name_to_humanoid("LeftUpperArm"), Some(HumanoidBone::LeftUpperArm));
		assert_eq!(
			vmc_bone_name_to_humanoid("Armature/Hips/LeftUpperArm"),
			Some(HumanoidBone::LeftUpperArm)
		);
	}

	#[test]
	fn maps_vroid_j_bip_and_mixamo_style_names() {
		assert_eq!(vmc_bone_name_to_humanoid("J_Bip_C_Hips"), Some(HumanoidBone::Hips));
		assert_eq!(vmc_bone_name_to_humanoid("J_Bip_L_Shoulder"), Some(HumanoidBone::LeftShoulder));
		assert_eq!(vmc_bone_name_to_humanoid("J_Bip_R_LowerArm"), Some(HumanoidBone::RightLowerArm));
		assert_eq!(vmc_bone_name_to_humanoid("mixamorig:LeftFoot"), Some(HumanoidBone::LeftFoot));
	}

	#[test]
	fn decodes_bone_pos() {
		let bytes = encode(OscMessage {
			addr: ADDR_BONE_POS.to_string(),
			args: vec![
				OscType::String("Hips".into()),
				OscType::Float(0.0),
				OscType::Float(1.0),
				OscType::Float(0.0),
				OscType::Float(0.0),
				OscType::Float(0.0),
				OscType::Float(0.0),
				OscType::Float(1.0),
			],
		});
		let ev = decode_vmcp_osc_bytes(&bytes).unwrap();
		assert_eq!(ev.len(), 1);
		match &ev[0] {
			VmcEvent::BonePos { bone_name, pose } => {
				assert_eq!(bone_name, "Hips");
				assert!((pose.py - 1.0).abs() < 1e-5);
			}
			_ => panic!("expected BonePos"),
		}
	}

	#[test]
	fn decodes_blend_and_assembler_frame() {
		let mut asm = VmcMotionAssembler::default();
		let bytes = encode(OscMessage {
			addr: ADDR_BLEND_VAL.to_string(),
			args: vec![OscType::String("Joy".into()), OscType::Float(0.5)],
		});
		for e in decode_vmcp_osc_bytes(&bytes).unwrap() {
			asm.apply_event(&e);
		}
		let frame = asm.to_un_motion_frame(1, 123);
		let face = frame.face.as_ref().expect("face");
		assert_eq!(face.expressions.len(), 1);
		assert_eq!(face.expressions[0].name, "Joy");
		assert!((face.expressions[0].value - 0.5).abs() < 1e-5);
	}

	#[test]
	fn assembler_maps_vmc_finger_bones_to_typed_hand_motion() {
		let mut asm = VmcMotionAssembler::default();
		asm.apply_event(&VmcEvent::BonePos {
			bone_name: "RightIndexIntermediate".to_string(),
			pose: BonePoseLocal {
				px: 0.0,
				py: 0.0,
				pz: 0.0,
				qx: 0.0,
				qy: 0.0,
				qz: 0.25,
				qw: 0.9682458,
			},
		});

		let frame = asm.to_un_motion_frame(2, 456);

		assert!(
			frame
				.body
				.as_ref()
				.and_then(|body| body.humanoid.as_ref())
				.is_none_or(|humanoid| humanoid.bones.is_empty()),
			"finger VMC bones must not be dropped into body humanoid bones"
		);
		let right = frame.right_hand.as_ref().expect("right hand");
		let index = right
			.fingers
			.iter()
			.find(|pose| pose.finger == Finger::Index)
			.expect("index finger");
		let rotation = index.joints[1].rotation.expect("intermediate rotation");
		assert!((rotation.z - 0.25).abs() < 1e-5);
		assert!(frame.left_hand.is_none());
	}

	#[test]
	fn bundle_flattens() {
		let inner = encode(OscMessage {
			addr: ADDR_RELATIVE_TIME.to_string(),
			args: vec![OscType::Float(3.25)],
		});
		let (_, inner_packet) = rosc::decoder::decode_udp(&inner).unwrap();
		let bundle = OscPacket::Bundle(OscBundle {
			timetag: OscTime::from((1, 0)),
			content: vec![inner_packet],
		});
		let buf = encoder::encode(&bundle).unwrap();
		let ev = decode_vmcp_osc_bytes(&buf).unwrap();
		assert_eq!(ev, vec![VmcEvent::RelativeTime { seconds: 3.25 }]);
	}
}
