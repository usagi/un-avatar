//! Receive-clock playback. Events bypass the delay; continuous channels retain
//! independent history so sparse producers cannot erase one another's fields.
use super::{interpolate_transform_sample, interpolate_vec3f, MotionFrameBucketKey};
use std::collections::VecDeque;
use un_motion_frame::*;

const RETAIN_NS: u64 = 5_000_000_000;
const DEFAULT_DT: u64 = 16_666_667;
const HISTORY: usize = 8;
const CHANNEL_LIMIT: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Path {
	Root,
	Bone(HumanoidBone),
	Head,
	Expression(String),
	Gaze(u8),
	Blink(u8),
	Wrist(bool),
	Finger(bool, Finger),
}

struct Channel {
	path: Path,
	samples: VecDeque<(u64, UNMotionFrame)>,
	last_emitted: Option<UNMotionFrame>,
}

struct Bucket {
	key: MotionFrameBucketKey,
	last_receive: u64,
	intervals: VecDeque<u64>,
	expected_intervals: VecDeque<u64>,
	last_target: u64,
	channels: Vec<Channel>,
}

#[derive(Default)]
pub(super) struct Playback {
	buckets: Vec<Bucket>,
	owners: Vec<(Path, MotionFrameBucketKey)>,
}

fn empty_like(frame: &UNMotionFrame) -> UNMotionFrame {
	let mut result = UNMotionFrame::new(frame.header.sequence);
	result.header = frame.header.clone();
	result.metadata = frame.metadata.clone();
	result.sources = frame.sources.clone();
	result
}

fn fragments(frame: &UNMotionFrame) -> Vec<(Path, UNMotionFrame)> {
	let mut result = Vec::new();
	if let Some(body) = &frame.body {
		if let Some(pose) = &body.humanoid {
			if let Some(root) = &pose.root {
				let mut f = empty_like(frame);
				f.body = Some(BodyMotion {
					humanoid: Some(HumanoidPose {
						root: Some(root.clone()),
						bones: vec![],
					}),
					tracking_state: body.tracking_state,
					confidence: body.confidence,
				});
				result.push((Path::Root, f));
			}
			for bone in &pose.bones {
				let mut f = empty_like(frame);
				f.body = Some(BodyMotion {
					humanoid: Some(HumanoidPose {
						root: None,
						bones: vec![bone.clone()],
					}),
					tracking_state: body.tracking_state,
					confidence: body.confidence,
				});
				result.push((Path::Bone(bone.bone), f));
			}
		}
	}
	if let Some(face) = &frame.face {
		if let Some(head) = &face.head {
			let mut f = empty_like(frame);
			f.face = Some(FaceMotion {
				head: Some(head.clone()),
				expressions: vec![],
				tracking_state: face.tracking_state,
				confidence: face.confidence,
			});
			result.push((Path::Head, f));
		}
		for expression in &face.expressions {
			let mut f = empty_like(frame);
			f.face = Some(FaceMotion {
				head: None,
				expressions: vec![expression.clone()],
				tracking_state: face.tracking_state,
				confidence: face.confidence,
			});
			result.push((Path::Expression(expression.name.clone()), f));
		}
	}
	if let Some(eyes) = &frame.eyes {
		let blank = EyeMotion {
			tracking_state: eyes.tracking_state,
			confidence: eyes.confidence,
			left_gaze: None,
			right_gaze: None,
			combined_gaze: None,
			blink_left: None,
			blink_right: None,
		};
		for (i, gaze) in [&eyes.left_gaze, &eyes.right_gaze, &eyes.combined_gaze].into_iter().enumerate() {
			if let Some(gaze) = gaze {
				let mut e = blank.clone();
				match i {
					0 => e.left_gaze = Some(gaze.clone()),
					1 => e.right_gaze = Some(gaze.clone()),
					_ => e.combined_gaze = Some(gaze.clone()),
				}
				let mut f = empty_like(frame);
				f.eyes = Some(e);
				result.push((Path::Gaze(i as u8), f));
			}
		}
		for (i, blink) in [eyes.blink_left, eyes.blink_right].into_iter().enumerate() {
			if let Some(blink) = blink {
				let mut e = blank.clone();
				if i == 0 {
					e.blink_left = Some(blink);
				} else {
					e.blink_right = Some(blink);
				}
				let mut f = empty_like(frame);
				f.eyes = Some(e);
				result.push((Path::Blink(i as u8), f));
			}
		}
	}
	for (right, hand) in [(false, &frame.left_hand), (true, &frame.right_hand)] {
		if let Some(hand) = hand {
			if let Some(wrist) = &hand.wrist {
				let h = HandMotion {
					wrist: Some(wrist.clone()),
					fingers: vec![],
					tracking_state: hand.tracking_state,
					confidence: hand.confidence,
				};
				let mut f = empty_like(frame);
				if right {
					f.right_hand = Some(h);
				} else {
					f.left_hand = Some(h);
				}
				result.push((Path::Wrist(right), f));
			}
			for finger in &hand.fingers {
				let h = HandMotion {
					wrist: None,
					fingers: vec![finger.clone()],
					tracking_state: hand.tracking_state,
					confidence: hand.confidence,
				};
				let mut f = empty_like(frame);
				if right {
					f.right_hand = Some(h);
				} else {
					f.left_hand = Some(h);
				}
				result.push((Path::Finger(right, finger.finger), f));
			}
		}
	}
	result
}

fn lost(frame: &UNMotionFrame, path: &Path) -> bool {
	match path {
		Path::Root | Path::Bone(_) => frame.body.as_ref().is_some_and(|v| v.tracking_state == TrackingState::Lost),
		Path::Head | Path::Expression(_) => frame.face.as_ref().is_some_and(|v| v.tracking_state == TrackingState::Lost),
		Path::Gaze(_) | Path::Blink(_) => frame.eyes.as_ref().is_some_and(|v| v.tracking_state == TrackingState::Lost),
		Path::Wrist(right) | Path::Finger(right, _) => if *right { &frame.right_hand } else { &frame.left_hand }
			.as_ref()
			.is_some_and(|v| v.tracking_state == TrackingState::Lost),
	}
}

fn missing(frame: &UNMotionFrame) -> bool {
	frame
		.body
		.as_ref()
		.and_then(|b| b.humanoid.as_ref())
		.is_some_and(|p| p.bones.iter().any(|b| b.state == SampleState::Missing))
		|| frame
			.face
			.as_ref()
			.is_some_and(|f| f.expressions.iter().any(|e| e.state == SampleState::Missing))
}

// Preserve discontinuities/events once, separately from interpolated channels.
pub(super) fn retain_immediate(frame: &mut UNMotionFrame) -> bool {
	if let Some(body) = &mut frame.body {
		if body.tracking_state == TrackingState::Lost {
			body.humanoid = None;
		} else {
			if let Some(p) = &mut body.humanoid {
				p.root = None;
				p.bones.retain(|b| b.state == SampleState::Missing);
			}
			if body.humanoid.as_ref().is_none_or(|p| p.bones.is_empty()) {
				frame.body = None;
			}
		}
	}
	if let Some(face) = &mut frame.face {
		if face.tracking_state == TrackingState::Lost {
			face.head = None;
			face.expressions.clear();
		} else {
			face.head = None;
			face.expressions.retain(|e| e.state == SampleState::Missing);
			if face.expressions.is_empty() {
				frame.face = None;
			}
		}
	}
	if frame.eyes.as_ref().is_some_and(|e| e.tracking_state != TrackingState::Lost) {
		frame.eyes = None;
	}
	if frame.left_hand.as_ref().is_some_and(|h| h.tracking_state != TrackingState::Lost) {
		frame.left_hand = None;
	}
	if frame.right_hand.as_ref().is_some_and(|h| h.tracking_state != TrackingState::Lost) {
		frame.right_hand = None;
	}
	for hand in [&mut frame.left_hand, &mut frame.right_hand].into_iter().flatten() {
		hand.wrist = None;
		hand.fingers.clear();
	}
	if let Some(eyes) = &mut frame.eyes {
		eyes.left_gaze = None;
		eyes.right_gaze = None;
		eyes.combined_gaze = None;
		eyes.blink_left = None;
		eyes.blink_right = None;
	}
	frame.body.is_some()
		|| frame.face.is_some()
		|| frame.eyes.is_some()
		|| frame.left_hand.is_some()
		|| frame.right_hand.is_some()
		|| !frame.signals.is_empty()
}

impl Playback {
	pub(super) fn push(&mut self, frame: &UNMotionFrame, time: u64) {
		self.prune(time);
		if frame.body.is_none() && frame.face.is_none() && frame.eyes.is_none() && frame.left_hand.is_none() && frame.right_hand.is_none() {
			return;
		}
		let key = MotionFrameBucketKey::from_frame(frame);
		let idx = if let Some(i) = self.buckets.iter().position(|b| b.key == key) {
			i
		} else {
			if self.buckets.len() == 8 {
				let oldest = self.buckets.iter().enumerate().min_by_key(|(_, b)| b.last_receive).unwrap().0;
				self.buckets.remove(oldest);
			}
			self.buckets.push(Bucket {
				key: key.clone(),
				last_receive: time,
				intervals: VecDeque::new(),
				expected_intervals: VecDeque::new(),
				last_target: 0,
				channels: vec![],
			});
			self.buckets.len() - 1
		};
		let bucket = &mut self.buckets[idx];
		if time > bucket.last_receive {
			bucket.intervals.push_back(time - bucket.last_receive);
			if bucket.intervals.len() > HISTORY {
				bucket.intervals.pop_front();
			}
		}
		bucket.last_receive = time;
		if let Some(dt) = frame.header.expected_dt_ns.filter(|dt| *dt > 0) {
			bucket.expected_intervals.push_back(dt);
			if bucket.expected_intervals.len() > HISTORY {
				bucket.expected_intervals.pop_front();
			}
		}
		// A loss-only packet must cancel retained motion even without a pose payload.
		bucket.channels.retain(|c| !lost(frame, &c.path));
		for (path, part) in fragments(frame) {
			let owner = self.owners.iter_mut().find(|(p, _)| p == &path);
			// No owner also means a discontinuity (for example B was lost after
			// taking over A). A's retained pre-takeover samples are not current.
			let changed_owner = !owner.as_ref().is_some_and(|(_, k)| *k == key);
			if let Some((_, owner)) = owner {
				*owner = key.clone();
			} else {
				self.owners.push((path.clone(), key.clone()));
			}
			if lost(&part, &path) || missing(&part) {
				bucket.channels.retain(|c| c.path != path);
				continue;
			}
			if let Some(c) = bucket.channels.iter_mut().find(|c| c.path == path) {
				// Do not interpolate from a producer's old pose when ownership returns.
				if changed_owner {
					c.samples.clear();
					c.last_emitted = None;
				}
				if c.samples.back().is_some_and(|(t, _)| *t == time) {
					c.samples.pop_back();
				}
				c.samples.push_back((time, part));
				if c.samples.len() > HISTORY {
					c.samples.pop_front();
				}
			} else {
				if bucket.channels.len() == CHANNEL_LIMIT {
					let oldest = bucket
						.channels
						.iter()
						.enumerate()
						.min_by_key(|(_, c)| c.samples.back().map(|(t, _)| *t))
						.unwrap()
						.0;
					bucket.channels.remove(oldest);
				}
				bucket.channels.push(Channel {
					path,
					samples: VecDeque::from([(time, part)]),
					last_emitted: None,
				});
			}
		}
	}

	fn prune(&mut self, now: u64) {
		self.buckets.retain(|b| now.saturating_sub(b.last_receive) <= RETAIN_NS);
		for b in &mut self.buckets {
			b.channels
				.retain(|c| c.samples.back().is_some_and(|(t, _)| now.saturating_sub(*t) <= RETAIN_NS));
		}
		self.owners.retain(|(path, key)| {
			self.buckets
				.iter()
				.any(|b| &b.key == key && b.channels.iter().any(|c| &c.path == path))
		});
	}

	pub(super) fn sample(&mut self, now: u64, out: &mut Vec<UNMotionFrame>) {
		self.prune(now);
		for bucket in &mut self.buckets {
			// Senders may report the last measured interval, including isolated
			// stalls. Do not turn one such packet into a playback-delay jump.
			let intervals = if bucket.expected_intervals.is_empty() {
				&bucket.intervals
			} else {
				&bucket.expected_intervals
			};
			let mut times: Vec<_> = intervals.iter().copied().collect();
			times.sort_unstable();
			let dt = times.get(times.len() / 2).copied().unwrap_or(DEFAULT_DT);
			let delay = dt.saturating_add(dt / 2).max(DEFAULT_DT);
			let target = now.saturating_sub(delay).max(bucket.last_target);
			bucket.last_target = target;
			let mut combined = None;
			for c in &mut bucket.channels {
				if !self.owners.iter().any(|(p, k)| p == &c.path && k == &bucket.key) {
					continue;
				}
				let Some((_, last)) = c.samples.back() else {
					continue;
				};
				let mut value = last.clone();
				if let Some((_, first)) = c.samples.front().filter(|(t, _)| target < *t) {
					value = first.clone();
				} else {
					for i in 1..c.samples.len() {
						let (ta, a) = &c.samples[i - 1];
						let (tb, b) = &c.samples[i];
						if target <= *tb {
							let alpha = target.saturating_sub(*ta) as f32 / tb.saturating_sub(*ta).max(1) as f32;
							value = interpolate(a, b, alpha.clamp(0., 1.));
							break;
						}
					}
				}
				if c.last_emitted.as_ref() == Some(&value) {
					continue;
				}
				c.last_emitted = Some(value.clone());
				let frame = combined.get_or_insert_with(|| {
					let mut f = empty_like(&value);
					f.sources.clear();
					f
				});
				merge(frame, value);
			}
			if let Some(frame) = combined {
				out.push(frame);
			}
		}
	}
}

fn interpolate(a: &UNMotionFrame, b: &UNMotionFrame, t: f32) -> UNMotionFrame {
	if t <= 0. {
		return a.clone();
	}
	if t >= 1. {
		return b.clone();
	}
	let mut result = b.clone();
	if let (Some(a), Some(b)) = (&a.body, &mut result.body) {
		if let (Some(a), Some(b)) = (&a.humanoid, &mut b.humanoid) {
			if let (Some(a), Some(b)) = (&a.root, &mut b.root) {
				*b = interpolate_transform_sample(a, b, t);
			}
			for (a, b) in a.bones.iter().zip(&mut b.bones) {
				b.transform = interpolate_transform_sample(&a.transform, &b.transform, t);
			}
		}
	}
	if let (Some(a), Some(b)) = (&a.face, &mut result.face) {
		if let (Some(a), Some(b)) = (&a.head, &mut b.head) {
			*b = interpolate_transform_sample(a, b, t);
		}
		for (a, b) in a.expressions.iter().zip(&mut b.expressions) {
			b.value = a.value + (b.value - a.value) * t;
		}
	}
	for (a, b) in [(&a.left_hand, &mut result.left_hand), (&a.right_hand, &mut result.right_hand)] {
		if let (Some(a), Some(b)) = (a, b) {
			if let (Some(a), Some(b)) = (&a.wrist, &mut b.wrist) {
				*b = interpolate_transform_sample(a, b, t);
			}
			for (a, b) in a.fingers.iter().zip(&mut b.fingers) {
				if a.joints.len() == b.joints.len() {
					for (a, b) in a.joints.iter().zip(&mut b.joints) {
						*b = interpolate_transform_sample(a, b, t);
					}
				}
			}
		}
	}
	if let (Some(a), Some(b)) = (&a.eyes, &mut result.eyes) {
		for (a, b) in [
			(&a.left_gaze, &mut b.left_gaze),
			(&a.right_gaze, &mut b.right_gaze),
			(&a.combined_gaze, &mut b.combined_gaze),
		] {
			if let (Some(a), Some(b)) = (a, b) {
				b.origin = interpolate_vec3f(a.origin, b.origin, t);
				b.target = interpolate_vec3f(a.target, b.target, t);
				if let Some(v) = interpolate_vec3f(Some(a.direction), Some(b.direction), t) {
					let length = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
					if length > 1e-6 {
						b.direction = Vec3f {
							x: v.x / length,
							y: v.y / length,
							z: v.z / length,
						};
					}
				}
			}
		}
		for (a, b) in [(a.blink_left, &mut b.blink_left), (a.blink_right, &mut b.blink_right)] {
			if let (Some(a), Some(b)) = (a, b) {
				*b = a + (*b - a) * t;
			}
		}
	}
	result
}

fn merge(out: &mut UNMotionFrame, mut f: UNMotionFrame) {
	// Each retained channel may refer to a different source-table revision.
	let map: Vec<_> = f
		.sources
		.drain(..)
		.map(|s| {
			if let Some(i) = out.sources.iter().position(|old| old == &s) {
				i as u16
			} else {
				let i = out.sources.len() as u16;
				out.sources.push(s);
				i
			}
		})
		.collect();
	let remap = |i: &mut Option<u16>| {
		*i = i.and_then(|i| map.get(i as usize).copied());
	};
	if let Some(mut b) = f.body {
		if let Some(p) = &mut b.humanoid {
			for bone in &mut p.bones {
				remap(&mut bone.source_index);
			}
		}
		if let Some(old) = &mut out.body {
			if let (Some(old), Some(p)) = (&mut old.humanoid, b.humanoid) {
				if p.root.is_some() {
					old.root = p.root;
				}
				old.bones.extend(p.bones);
			}
		} else {
			out.body = Some(b);
		}
	}
	if let Some(mut f) = f.face {
		for e in &mut f.expressions {
			remap(&mut e.source_index);
		}
		if let Some(old) = &mut out.face {
			if f.head.is_some() {
				old.head = f.head;
			}
			old.expressions.extend(f.expressions);
		} else {
			out.face = Some(f);
		}
	}
	for (h, old) in [(f.left_hand, &mut out.left_hand), (f.right_hand, &mut out.right_hand)] {
		if let Some(h) = h {
			if let Some(old) = old {
				if h.wrist.is_some() {
					old.wrist = h.wrist;
				}
				old.fingers.extend(h.fingers);
			} else {
				*old = Some(h);
			}
		}
	}
	if let Some(mut e) = f.eyes {
		for g in [&mut e.left_gaze, &mut e.right_gaze, &mut e.combined_gaze].into_iter().flatten() {
			remap(&mut g.source_index);
		}
		if let Some(old) = &mut out.eyes {
			if e.left_gaze.is_some() {
				old.left_gaze = e.left_gaze;
			}
			if e.right_gaze.is_some() {
				old.right_gaze = e.right_gaze;
			}
			if e.combined_gaze.is_some() {
				old.combined_gaze = e.combined_gaze;
			}
			if e.blink_left.is_some() {
				old.blink_left = e.blink_left;
			}
			if e.blink_right.is_some() {
				old.blink_right = e.blink_right;
			}
		} else {
			out.eyes = Some(e);
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	fn transform(x: f32) -> TransformSample {
		TransformSample {
			translation: Some(Vec3f { x, y: 0., z: 0. }),
			rotation: None,
			scale: None,
			linear_velocity: None,
			angular_velocity: None,
		}
	}
	fn frame(sequence: u64, x: f32) -> UNMotionFrame {
		let mut f = UNMotionFrame::new(sequence);
		f.header.expected_dt_ns = Some(33_333_334);
		f.header.stream_id = Some("a".into());
		f.body = Some(BodyMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.,
			humanoid: Some(HumanoidPose {
				root: Some(transform(x)),
				bones: vec![BoneSample {
					bone: HumanoidBone::LeftUpperArm,
					transform: transform(x),
					confidence: 1.,
					source_index: None,
					state: SampleState::Valid,
				}],
			}),
		});
		f.face = Some(FaceMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.,
			head: Some(transform(x)),
			expressions: vec![ExpressionSample {
				name: "Joy".into(),
				value: x,
				confidence: 1.,
				source_index: None,
				state: SampleState::Valid,
			}],
		});
		f.left_hand = Some(HandMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.,
			wrist: Some(transform(x)),
			fingers: vec![FingerPose {
				finger: Finger::Index,
				joints: vec![transform(x)],
				confidence: 1.,
			}],
		});
		f.eyes = Some(EyeMotion {
			tracking_state: TrackingState::Valid,
			confidence: 1.,
			left_gaze: None,
			right_gaze: None,
			combined_gaze: None,
			blink_left: Some(x),
			blink_right: None,
		});
		f
	}
	fn read(p: &mut Playback, now: u64) -> Vec<UNMotionFrame> {
		let mut out = vec![];
		p.sample(now, &mut out);
		out
	}
	fn x(f: &UNMotionFrame) -> f32 {
		f.face.as_ref().unwrap().head.as_ref().unwrap().translation.unwrap().x
	}
	#[test]
	fn playback_30hz_input_advances_at_60hz_and_drains_once() {
		let mut p = Playback::default();
		let dt = 33_333_334;
		let mut values = vec![];
		for tick in 0..14 {
			let now = tick * 16_666_667;
			if tick % 2 == 0 && tick <= 8 {
				p.push(&frame(tick / 2, (tick / 2) as f32), now);
			}
			for f in read(&mut p, now) {
				values.push(x(&f));
			}
		}
		assert_eq!(values, vec![0., 0.5, 1., 1.5, 2., 2.5, 3., 3.5, 4.]);
		assert!(read(&mut p, dt * 10).is_empty(), "final pose must not be replayed forever");
	}
	#[test]
	fn playback_all_channels_share_time() {
		let mut p = Playback::default();
		for i in 0..4 {
			p.push(&frame(i, i as f32), i * 33_333_334);
		}
		let out = read(&mut p, 100_000_002);
		let f = &out[0];
		let expected = 1.5;
		assert_eq!(x(f), expected);
		let body = f.body.as_ref().unwrap().humanoid.as_ref().unwrap();
		assert_eq!(body.root.as_ref().unwrap().translation.unwrap().x, expected);
		assert_eq!(body.bones[0].transform.translation.unwrap().x, expected);
		assert_eq!(f.face.as_ref().unwrap().expressions[0].value, expected);
		let hand = f.left_hand.as_ref().unwrap();
		assert_eq!(hand.wrist.as_ref().unwrap().translation.unwrap().x, expected);
		assert_eq!(hand.fingers[0].joints[0].translation.unwrap().x, expected);
		assert_eq!(f.eyes.as_ref().unwrap().blink_left, Some(expected));
	}
	#[test]
	fn playback_sparse_sources_and_ownership_do_not_replay_old_values() {
		let mut p = Playback::default();
		p.push(&frame(0, 0.), 0);
		p.push(&frame(1, 1.), 33_333_334);
		let mut b = frame(2, 10.);
		b.header.stream_id = Some("b".into());
		b.body = None;
		b.left_hand = None;
		b.eyes = None;
		b.face.as_mut().unwrap().expressions.clear();
		p.push(&b, 40_000_000);
		let out = read(&mut p, 60_000_000);
		assert_eq!(out.len(), 2);
		assert!(out[0].face.as_ref().unwrap().head.is_none());
		assert!(!out[0].face.as_ref().unwrap().expressions.is_empty());
		assert_eq!(x(&out[1]), 10.);
		let later = read(&mut p, 70_000_000);
		assert!(
			later.iter().all(|f| f.face.as_ref().is_none_or(|f| f.head.is_none())),
			"old producer cannot retake head without new input"
		);
	}
	#[test]
	fn playback_loss_and_missing_cancel_retained_channels() {
		let mut p = Playback::default();
		p.push(&frame(0, 0.), 0);
		p.push(&frame(1, 1.), 33_333_334);
		let mut lost = UNMotionFrame::new(2);
		lost.header.stream_id = Some("a".into());
		lost.face = Some(FaceMotion {
			tracking_state: TrackingState::Lost,
			confidence: 0.,
			head: None,
			expressions: vec![],
		});
		p.push(&lost, 40_000_000);
		assert!(read(&mut p, 60_000_000).iter().all(|f| f.face.is_none()));
		let mut missing = frame(3, 9.);
		missing.face = None;
		missing.left_hand = None;
		missing.eyes = None;
		let pose = missing.body.as_mut().unwrap().humanoid.as_mut().unwrap();
		pose.root = None;
		pose.bones[0].state = SampleState::Missing;
		p.push(&missing, 70_000_000);
		assert!(read(&mut p, 80_000_000)
			.iter()
			.all(|f| f.body.as_ref().unwrap().humanoid.as_ref().unwrap().bones.is_empty()));
	}
	#[test]
	fn playback_jitter_never_rewinds_and_expires() {
		let mut p = Playback::default();
		let mut previous = -1.;
		let mut index = 0;
		let times = [0, 38_000_000, 62_000_000, 104_000_000, 130_000_000];
		for tick in 0..16 {
			let now = tick * 16_666_667;
			while index < times.len() && times[index] <= now {
				p.push(&frame(index as u64, index as f32), times[index]);
				index += 1;
			}
			for f in read(&mut p, now) {
				let next = x(&f);
				assert!(next >= previous);
				previous = next;
			}
		}
		assert_eq!(previous, 4.);
		assert!(read(&mut p, RETAIN_NS + times[4] + 1).is_empty());
		assert!(p.buckets.is_empty());
	}

	#[test]
	fn playback_returning_owner_after_loss_does_not_reuse_old_history() {
		let mut p = Playback::default();
		let mut a = frame(0, 0.);
		a.body = None;
		a.left_hand = None;
		a.eyes = None;
		a.face.as_mut().unwrap().expressions.clear();
		p.push(&a, 0);
		read(&mut p, 10_000_000);
		let mut b = a.clone();
		b.header.stream_id = Some("b".into());
		b.face.as_mut().unwrap().head = Some(transform(10.));
		p.push(&b, 20_000_000);
		assert_eq!(x(&read(&mut p, 30_000_000)[0]), 10.);
		b.face.as_mut().unwrap().tracking_state = TrackingState::Lost;
		b.face.as_mut().unwrap().head = None;
		p.push(&b, 40_000_000);
		assert!(read(&mut p, 50_000_000).is_empty());
		a.face.as_mut().unwrap().head = Some(transform(20.));
		p.push(&a, 100_000_000);
		assert_eq!(
			x(&read(&mut p, 100_000_000)[0]),
			20.,
			"ownership gap must not interpolate from A's pre-B history"
		);
	}

	#[test]
	fn playback_sparse_channels_remap_their_own_source_tables() {
		let mut p = Playback::default();
		let mut a = frame(0, 0.);
		a.face = None;
		a.left_hand = None;
		a.eyes = None;
		a.sources.push(MotionSourceInfo {
			source_id: "body-camera".into(),
			source_kind: MotionSourceKind::VmcInput,
			display_name: None,
			confidence: 1.,
			latency_ns: None,
			state: TrackingState::Valid,
		});
		a.body.as_mut().unwrap().humanoid.as_mut().unwrap().bones[0].source_index = Some(0);
		p.push(&a, 0);
		let mut b = frame(1, 1.);
		b.body = None;
		b.left_hand = None;
		b.eyes = None;
		b.face.as_mut().unwrap().head = None;
		b.sources = a.sources.clone();
		b.sources[0].source_id = "face-camera".into();
		b.face.as_mut().unwrap().expressions[0].source_index = Some(0);
		p.push(&b, 10_000_000);
		let out = read(&mut p, 100_000_000);
		let f = &out[0];
		let bone = &f.body.as_ref().unwrap().humanoid.as_ref().unwrap().bones[0];
		let expression = &f.face.as_ref().unwrap().expressions[0];
		assert_eq!(f.sources[bone.source_index.unwrap() as usize].source_id, "body-camera");
		assert_eq!(f.sources[expression.source_index.unwrap() as usize].source_id, "face-camera");
	}

	#[test]
	fn playback_single_reported_interval_spike_does_not_freeze_clock() {
		let mut p = Playback::default();
		for i in 0..4 {
			let mut f = frame(i, i as f32);
			f.header.expected_dt_ns = Some(16_000_000);
			p.push(&f, i * 16_000_000);
			read(&mut p, i * 16_000_000);
		}
		let mut spike = frame(4, 4.);
		spike.header.expected_dt_ns = Some(200_000_000);
		p.push(&spike, 64_000_000);
		let out = read(&mut p, 64_000_000);
		assert!(!out.is_empty(), "one reported interval outlier must not stall playback");
		assert!((x(&out[0]) - 2.5).abs() < 0.001);
	}

	#[test]
	fn playback_independent_body_and_face_release_without_erasing_each_other() {
		let mut p = Playback::default();
		let step = 16_666_667;
		let mut body_values = Vec::new();
		let mut smiles = Vec::new();
		let mut mouths = Vec::new();
		let mut blinks = Vec::new();
		for tick in 0..70u64 {
			let now = tick * step;
			if tick <= 40 {
				let mut body = frame(tick, tick as f32 / 40.);
				body.header.stream_id = Some("body-60hz".into());
				body.header.expected_dt_ns = Some(step);
				body.face = None;
				body.eyes = None;
				body.left_hand = None;
				p.push(&body, now);
			}
			if tick <= 40 && tick % 2 == 0 {
				let smile = (1. - (tick as f32 - 10.).abs() / 10.).max(0.);
				let mouth = if tick < 30 && tick % 4 == 0 { 0.8 } else { 0. };
				let blink = if (8..=12).contains(&tick) { 1. } else { 0. };
				let mut face = frame(tick, smile);
				face.header.stream_id = Some("face-30hz".into());
				face.body = None;
				face.left_hand = None;
				let f = face.face.as_mut().unwrap();
				f.head = None;
				let mut a = f.expressions[0].clone();
				a.name = "A".into();
				a.value = mouth;
				f.expressions.push(a);
				face.eyes.as_mut().unwrap().blink_left = Some(blink);
				p.push(&face, now);
			}
			for f in read(&mut p, now) {
				match f.header.stream_id.as_deref() {
					Some("body-60hz") => {
						assert!(f.face.is_none() && f.eyes.is_none());
						body_values.push(f.body.unwrap().humanoid.unwrap().bones[0].transform.translation.unwrap().x);
					}
					Some("face-30hz") => {
						assert!(f.body.is_none());
						if let Some(face) = f.face {
							for e in face.expressions {
								match e.name.as_str() {
									"Joy" => smiles.push(e.value),
									"A" => mouths.push(e.value),
									_ => panic!("unexpected expression"),
								}
							}
						}
						if let Some(e) = f.eyes {
							if let Some(v) = e.blink_left {
								blinks.push(v);
							}
						}
					}
					_ => panic!("source identity changed"),
				}
			}
		}
		assert!(body_values.len() >= 40 && body_values.windows(2).all(|w| w[1] >= w[0]));
		assert_eq!(body_values.last(), Some(&1.));
		for values in [&smiles, &mouths, &blinks] {
			assert!(values.iter().any(|v| *v > 0.7), "channel must actually become active");
			assert!(values.iter().all(|v| (0.0..=1.0).contains(v)));
			assert_eq!(values.last(), Some(&0.), "explicit neutral/release must arrive");
		}
		assert!(
			read(&mut p, 80 * step).is_empty(),
			"drained sources must not replay actions or poses"
		);
	}
}
