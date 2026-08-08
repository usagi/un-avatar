//! UN Avatar の renderer が UNMotionFrame を **Zenoh から受信** するための薄いランタイム。
//!
//! `un-motion-frame-zenoh::Subscriber` をワーカースレッドで保持し、内部チャネルから取り出した
//! `UNMotionFrame` を `crossbeam_channel::Receiver<UNMotionFrame>` でレンダラーのメインループへ
//! 流す。Zenoh のコールバックが裏スレッドで実行される性質を渡る境界をここに集約することで、
//! レンダラー側のスレッド構成 (winit + wgpu のメインスレッド + VMC スレッド) に Zenoh を
//! 自然に組み込めるようにする。
//!
//! また、`un-motion-frame-zenoh::InMemoryBackend` / `ReplayBackend` を使った in-process テストを
//! 同じインターフェースで実行できるよう、`UnAvatarZenohReceiver::declare_with_backend` を
//! generic で提供している。

#![forbid(unsafe_code)]

use std::{
	sync::{
		atomic::{AtomicBool, Ordering},
		Arc,
	},
	thread::JoinHandle,
	time::Duration,
};

use crossbeam_channel::{Receiver, RecvTimeoutError, TryRecvError};
use un_motion_frame::UNMotionFrame;
use un_motion_frame_zenoh::{Error, Subscriber, SubscriberBackend, ZenohTopicStrategy};

#[cfg(feature = "zenoh-transport")]
use un_motion_frame_zenoh::{ZenohSessionConfig, ZenohSubscriberBackend};

/// レンダラー側がポーリングする受信ハンドル。
///
/// drop 時にワーカースレッドへ停止フラグを立て、最大 200ms 以内で停止する。Subscriber の
/// `keep_alive` (`zenoh::pubsub::Subscriber` を box で握っている) もそのタイミングで drop され、
/// 実 Zenoh セッション側の subscription も解除される。
pub struct UnAvatarZenohReceiver {
	rx: Receiver<UNMotionFrame>,
	shutdown: Arc<AtomicBool>,
	join: Option<JoinHandle<()>>,
	strategy: ZenohTopicStrategy,
}

impl UnAvatarZenohReceiver {
	/// 実 Zenoh セッションを既定設定で開いて Subscriber を作る。
	#[cfg(feature = "zenoh-transport")]
	pub fn declare_zenoh_default(strategy: ZenohTopicStrategy) -> Result<Self, Error> {
		let backend = ZenohSubscriberBackend::open_default()?;
		Self::declare_with_backend(backend, strategy)
	}

	/// 指定したZenoh session設定でSubscriberを作る。
	#[cfg(feature = "zenoh-transport")]
	pub fn declare_zenoh(config: &ZenohSessionConfig, strategy: ZenohTopicStrategy) -> Result<Self, Error> {
		let backend = ZenohSubscriberBackend::open(config)?;
		Self::declare_with_backend(backend, strategy)
	}

	/// 任意の `SubscriberBackend` 実装を渡して Subscriber を作る。
	///
	/// テスト用途で `un_motion_frame_zenoh::InMemoryBackend` / `ReplayBackend` を渡すときに使う。
	pub fn declare_with_backend<B>(mut backend: B, strategy: ZenohTopicStrategy) -> Result<Self, Error>
	where
		B: SubscriberBackend + Send + 'static,
	{
		let subscriber = Subscriber::declare(&mut backend, strategy.clone())?;
		// backend 自身を thread へ移し replicas / cleanup を握ってもらう (一部 backend は drop されると
		// subscription が消えるので保持しておく)。
		Self::start(subscriber, backend, strategy)
	}

	fn start<B: Send + 'static>(subscriber: Subscriber, backend_keep_alive: B, strategy: ZenohTopicStrategy) -> Result<Self, Error> {
		let (tx, rx) = crossbeam_channel::unbounded::<UNMotionFrame>();
		let shutdown = Arc::new(AtomicBool::new(false));
		let s = Arc::clone(&shutdown);

		let join = std::thread::Builder::new()
			.name("un-avatar-zenoh-recv".into())
			.spawn(move || {
				// backend を所有することで、subscriber と一緒に生存させる。drop されない限り
				// 内部の Mutex / Channel が無効化されないので、無音区間でも問題ない。
				let _keep = backend_keep_alive;
				loop {
					if s.load(Ordering::Relaxed) {
						break;
					}
					match subscriber.recv_frame_timeout(Duration::from_millis(100)) {
						Ok(Some(frame)) => {
							if tx.send(frame).is_err() {
								// 受信側 (renderer) が drop されている。スレッドを畳む。
								break;
							}
						}
						Ok(None) => {
							// timeout。次の loop で shutdown 確認 → 続行。
							continue;
						}
						Err(_e) => {
							// 個別 frame のデコードエラーは握り潰し、次の frame を待つ。
							// 連続失敗のメトリクスはこの crate では取らない。必要なら呼出側で `try_recv` 回数を計測する。
							continue;
						}
					}
				}
			})
			.map_err(|e| Error::transport(format!("spawn un-avatar-zenoh-recv thread failed: {e}")))?;

		Ok(Self {
			rx,
			shutdown,
			join: Some(join),
			strategy,
		})
	}

	pub fn strategy(&self) -> &ZenohTopicStrategy {
		&self.strategy
	}

	/// 直近の 1 フレームをノンブロッキングで取り出す。
	pub fn try_recv(&self) -> Option<UNMotionFrame> {
		match self.rx.try_recv() {
			Ok(frame) => Some(frame),
			Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
		}
	}

	/// 溜まっているフレームを **最新 1 件だけ** 取り出し、それより古い frame は捨てる。
	///
	/// 60Hz レンダリングに 30Hz 以上のソースを繋ぐと、何かの拍子に複数フレームが溜まっている
	/// ことがある。レンダラー視点ではほぼ常に最新を優先したいので、コンビニエンス API として
	/// 提供しておく。
	pub fn drain_latest(&self) -> Option<UNMotionFrame> {
		let mut latest = None;
		while let Ok(frame) = self.rx.try_recv() {
			latest = Some(frame);
		}
		latest
	}

	/// 溜まっているフレームを受信順に取り出す。
	///
	/// 複数の UNMotion profile が同じ key expression へ publish している場合、最新 1 件だけを
	/// 適用すると profile 間の合成が崩れる。呼び出し側で順に適用できるよう、bounded batch として
	/// 返す。
	pub fn drain_available(&self, max_frames: usize) -> Vec<UNMotionFrame> {
		if max_frames == 0 {
			return Vec::new();
		}
		let mut frames = Vec::with_capacity(max_frames);
		while frames.len() < max_frames {
			match self.rx.try_recv() {
				Ok(frame) => frames.push(frame),
				Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
			}
		}
		frames
	}

	/// 最初の 1 件だけ timeout 付きで待ち、続くフレームは受信順に即時 drain する。
	///
	/// 呼び出し側で固定 sleep polling を行うと、入力 FPS と表示 FPS の位相差が粗いバッチ間隔として
	/// 表面化しやすい。受信チャネル側で待つことで、無音時だけ低負荷にしつつ入力到着時は即座に渡す。
	pub fn recv_batch_timeout(&self, max_frames: usize, timeout: Duration) -> Vec<UNMotionFrame> {
		if max_frames == 0 {
			return Vec::new();
		}
		let mut frames = Vec::with_capacity(max_frames);
		match self.rx.recv_timeout(timeout) {
			Ok(frame) => frames.push(frame),
			Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => return frames,
		}
		while frames.len() < max_frames {
			match self.rx.try_recv() {
				Ok(frame) => frames.push(frame),
				Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
			}
		}
		frames
	}
}

impl Drop for UnAvatarZenohReceiver {
	fn drop(&mut self) {
		self.shutdown.store(true, Ordering::Relaxed);
		if let Some(handle) = self.join.take() {
			// 上限を切らずに join する。`recv_frame_timeout(100ms)` で抜けるので最大 100ms 程度。
			let _ = handle.join();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use un_motion_frame::{MotionSourceInfo, MotionSourceKind, TrackingState};
	use un_motion_frame_zenoh::{InMemoryBackend, Publisher, ZenohTopicStrategy};

	fn make_frame(seq: u64) -> UNMotionFrame {
		let mut frame = UNMotionFrame::new(seq);
		frame.sources.push(MotionSourceInfo {
			source_id: "test:cam0".to_string(),
			source_kind: MotionSourceKind::WebcamPose,
			display_name: None,
			confidence: 1.0,
			latency_ns: None,
			state: TrackingState::Valid,
		});
		frame
	}

	#[test]
	fn receiver_forwards_frames_via_channel() {
		let backend = InMemoryBackend::new();
		let strategy = ZenohTopicStrategy::default();
		let receiver = UnAvatarZenohReceiver::declare_with_backend(backend.clone(), strategy.clone()).expect("declare");

		let mut publisher = Publisher::new(backend.clone()).with_strategy(strategy);
		publisher.send(&make_frame(1)).unwrap();
		publisher.send(&make_frame(2)).unwrap();
		publisher.send(&make_frame(3)).unwrap();

		// 軽くスピンしてスレッドが drain するのを待つ。
		let deadline = std::time::Instant::now() + Duration::from_secs(2);
		let mut got = Vec::new();
		while std::time::Instant::now() < deadline && got.len() < 3 {
			while let Some(f) = receiver.try_recv() {
				got.push(f.header.sequence);
			}
			std::thread::sleep(Duration::from_millis(10));
		}

		assert_eq!(got, vec![1, 2, 3]);
	}

	#[test]
	fn drain_latest_keeps_only_newest_frame() {
		let backend = InMemoryBackend::new();
		let strategy = ZenohTopicStrategy::default();
		let receiver = UnAvatarZenohReceiver::declare_with_backend(backend.clone(), strategy.clone()).expect("declare");

		let mut publisher = Publisher::new(backend.clone()).with_strategy(strategy);
		for i in 1..=5u64 {
			publisher.send(&make_frame(i)).unwrap();
		}

		// 受信スレッドが拾うのを待つ。
		let deadline = std::time::Instant::now() + Duration::from_secs(2);
		while std::time::Instant::now() < deadline {
			std::thread::sleep(Duration::from_millis(50));
			if let Some(frame) = receiver.drain_latest() {
				if frame.header.sequence == 5 {
					return;
				}
			}
		}
		panic!("drain_latest did not converge to seq=5");
	}

	#[test]
	fn drain_available_keeps_receive_order() {
		let backend = InMemoryBackend::new();
		let strategy = ZenohTopicStrategy::default();
		let receiver = UnAvatarZenohReceiver::declare_with_backend(backend.clone(), strategy.clone()).expect("declare");

		let mut publisher = Publisher::new(backend.clone()).with_strategy(strategy);
		for i in 1..=5u64 {
			publisher.send(&make_frame(i)).unwrap();
		}

		let deadline = std::time::Instant::now() + Duration::from_secs(2);
		while std::time::Instant::now() < deadline {
			std::thread::sleep(Duration::from_millis(50));
			let frames = receiver.drain_available(5);
			if frames.len() == 5 {
				let seqs = frames.into_iter().map(|f| f.header.sequence).collect::<Vec<_>>();
				assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
				assert!(receiver.drain_available(5).is_empty());
				return;
			}
		}
		panic!("drain_available did not converge to 5 frames");
	}

	#[test]
	fn recv_batch_timeout_waits_for_first_frame_then_drains_available() {
		let backend = InMemoryBackend::new();
		let strategy = ZenohTopicStrategy::default();
		let receiver = UnAvatarZenohReceiver::declare_with_backend(backend.clone(), strategy.clone()).expect("receiver");
		let mut publisher = Publisher::new(backend.clone()).with_strategy(strategy);

		publisher.send(&make_frame(1)).expect("publish 1");
		publisher.send(&make_frame(2)).expect("publish 2");

		let mut seqs = Vec::new();
		for _ in 0..20 {
			let frames = receiver.recv_batch_timeout(8, Duration::from_millis(50));
			seqs.extend(frames.into_iter().map(|f| f.header.sequence));
			if seqs.len() >= 2 {
				assert_eq!(seqs, vec![1, 2]);
				return;
			}
		}
		panic!("recv_batch_timeout did not collect published frames");
	}
}
