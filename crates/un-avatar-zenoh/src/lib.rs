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
	time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, TryRecvError};
use un_motion_frame::UNMotionFrame;
use un_motion_frame_zenoh::{Error, Subscriber, SubscriberBackend, ZenohTopicStrategy};

#[cfg(feature = "zenoh-transport")]
use un_motion_frame_zenoh::{ZenohSessionConfig, ZenohSubscriberBackend};

/// Explicit motion endpoints are upstreams, not peer mesh discovery hints.
#[cfg(feature = "zenoh-transport")]
fn open_motion_backend(config: &ZenohSessionConfig) -> Result<ZenohSubscriberBackend, Error> {
    use zenoh::Wait as _;
    let Some(endpoints) = config.connect_endpoints().filter(|e| !e.is_empty()) else {
        return ZenohSubscriberBackend::open(config);
    };
    let mut direct = zenoh::Config::default();
    for (key, value) in [
        ("mode", if endpoints.len() > 1 { "\"peer\"" } else { "\"client\"" }.to_string()),
        ("listen/endpoints", "[]".to_string()),
        ("scouting/gossip/enabled", "false".to_string()),
        ("connect/endpoints", serde_json::to_string(endpoints).map_err(|e| Error::transport(e.to_string()))?),
        ("scouting/multicast/enabled", "false".to_string()),
        ("connect/timeout_ms", "5000".to_string()),
        ("connect/exit_on_failure", "true".to_string()),
    ] {
        direct.insert_json5(key, &value).map_err(|e| Error::transport(e.to_string()))?;
    }
    let session = zenoh::open(direct).wait().map_err(|e| Error::transport(e.to_string()))?;
    Ok(ZenohSubscriberBackend::from_session(session))
}

/// Decoded motion and its local queue-entry time. No sender-clock conversion.
pub struct TimedMotionFrame {
	pub frame: UNMotionFrame,
	/// After decode, before the receiver-to-renderer queue; not socket arrival.
	pub decoded_at: Instant,
}

/// レンダラー側がポーリングする受信ハンドル。drop時に受信ワーカーを停止する。
pub struct UnAvatarZenohReceiver {
	rx: Receiver<TimedMotionFrame>,
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
		let backend = open_motion_backend(config)?;
		Self::declare_with_backend(backend, strategy)
	}

	/// 一つのZenoh sessionを共有し、複数のUNMF/Z keyを購読する。
	#[cfg(feature = "zenoh-transport")]
	pub fn declare_zenoh_many(
		config: &ZenohSessionConfig,
		strategies: impl IntoIterator<Item = ZenohTopicStrategy>,
	) -> Result<Vec<Self>, Error> {
		let backend = open_motion_backend(config)?;
		let session = backend.session().clone();
		strategies
			.into_iter()
			.map(|strategy| Self::declare_with_backend(ZenohSubscriberBackend::from_session(session.clone()), strategy))
			.collect()
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
		let (tx, rx) = crossbeam_channel::unbounded::<TimedMotionFrame>();
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
							if tx.send(TimedMotionFrame { frame, decoded_at: Instant::now() }).is_err() {
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
			Ok(frame) => Some(frame.frame),
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
			latest = Some(frame.frame);
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
				Ok(frame) => frames.push(frame.frame),
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
		self.recv_timed_batch_timeout(max_frames, timeout).into_iter().map(|item| item.frame).collect()
	}

	/// Same ordered batch, retaining the post-decode queue entry timestamp.
	pub fn recv_timed_batch_timeout(&self, max_frames: usize, timeout: Duration) -> Vec<TimedMotionFrame> {
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
	fn timed_queue_retains_entry_time_and_legacy_order_without_transport() {
		let (tx, rx) = crossbeam_channel::unbounded();
		let receiver = UnAvatarZenohReceiver {
			rx, shutdown: Arc::new(AtomicBool::new(false)), join: None, strategy: ZenohTopicStrategy::default(),
		};
		let first = Instant::now() - Duration::from_millis(200);
		let second = Instant::now() - Duration::from_millis(100);
		for (sequence, decoded_at) in [(1, first), (2, second)] {
			tx.send(TimedMotionFrame { frame: make_frame(sequence), decoded_at }).unwrap();
		}
		let batch = receiver.recv_timed_batch_timeout(2, Duration::ZERO);
		assert_eq!(batch.iter().map(|x| x.frame.header.sequence).collect::<Vec<_>>(), vec![1, 2]);
		assert_eq!(batch[0].decoded_at, first);
		assert_eq!(batch[1].decoded_at, second);
		for sequence in 3..=5 {
			tx.send(TimedMotionFrame { frame: make_frame(sequence), decoded_at: Instant::now() }).unwrap();
		}
		assert_eq!(receiver.try_recv().unwrap().header.sequence, 3);
		assert_eq!(receiver.recv_batch_timeout(1, Duration::ZERO)[0].header.sequence, 4);
		assert_eq!(receiver.drain_available(1)[0].header.sequence, 5);
		assert!(receiver.drain_latest().is_none());
	}

	#[test]
	fn shared_key_backlog_preserves_both_streams_across_apply_batches() {
		let (tx, rx) = crossbeam_channel::unbounded();
		let receiver = UnAvatarZenohReceiver {
			rx, shutdown: Arc::new(AtomicBool::new(false)), join: None, strategy: ZenohTopicStrategy::default(),
		};
		let start = Instant::now() - Duration::from_secs(1);
		// Simulate an apply-worker stall, with independent sequence counters on
		// the body and face publishers sharing one subscription key.
		for index in 0..130u64 {
			let mut frame = make_frame(index / 2 + 1);
			frame.header.stream_id = Some(if index % 2 == 0 { "body" } else { "face" }.into());
			tx.send(TimedMotionFrame { frame, decoded_at: start + Duration::from_millis(index) }).unwrap();
		}
		let mut received = Vec::new();
		for expected_len in [64, 64, 2] {
			let batch = receiver.recv_timed_batch_timeout(64, Duration::ZERO);
			assert_eq!(batch.len(), expected_len);
			received.extend(batch);
		}
		for (index, item) in received.iter().enumerate() {
			assert_eq!(item.frame.header.stream_id.as_deref(), Some(if index % 2 == 0 { "body" } else { "face" }));
			assert_eq!(item.frame.header.sequence, index as u64 / 2 + 1);
			assert_eq!(item.decoded_at, start + Duration::from_millis(index as u64));
		}
		assert!(receiver.recv_timed_batch_timeout(64, Duration::ZERO).is_empty());
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
    #[cfg(feature = "zenoh-transport")]
    #[test]
    fn explicit_upstream_delivers_multiple_streams_without_discovery() {
        use zenoh::Wait as _;
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("tcp/{}", reservation.local_addr().unwrap());
        drop(reservation);
        let mut config = zenoh::Config::default();
        config.insert_json5("listen/endpoints", &serde_json::to_string(&[&endpoint]).unwrap()).unwrap();
        config.insert_json5("scouting/multicast/enabled", "false").unwrap();
        config.insert_json5("scouting/gossip/enabled", "false").unwrap();
        let publisher = zenoh::open(config).wait().unwrap();
        let strategies: Vec<_> = ["test/body", "test/face"].into_iter().map(|key| ZenohTopicStrategy { base_key_expr:key.into(), ..Default::default() }).collect();
        let receivers = UnAvatarZenohReceiver::declare_zenoh_many(&ZenohSessionConfig::default().with_connect_endpoint(endpoint), strategies.clone()).unwrap();
        let mut seen = [false; 2];
        for seq in 1..=50 {
            for (i, strategy) in strategies.iter().enumerate() {
                publisher.put(strategy.subscribe_key_expr(), un_motion_frame_zenoh::encode_frame(&make_frame(seq)).unwrap()).wait().unwrap();
                seen[i] |= !receivers[i].recv_batch_timeout(4, Duration::from_millis(20)).is_empty();
            }
            if seen.iter().all(|s| *s) { return; }
        }
        panic!("explicit endpoint did not deliver both streams: {seen:?}");
    }

    #[cfg(feature = "zenoh-transport")]
    #[test]
    fn explicit_endpoints_receive_from_two_isolated_publishers() {
        use zenoh::Wait as _;
        let mut publishers = Vec::new();
        let mut receiver_config = ZenohSessionConfig::default();
        for _ in 0..2 {
            let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("tcp/{}", port.local_addr().unwrap());
            drop(port);
            let mut config = zenoh::Config::default();
            config.insert_json5("listen/endpoints", &serde_json::to_string(&[&endpoint]).unwrap()).unwrap();
            config.insert_json5("scouting/multicast/enabled", "false").unwrap();
            config.insert_json5("scouting/gossip/enabled", "false").unwrap();
            publishers.push(zenoh::open(config).wait().unwrap());
            receiver_config = receiver_config.with_connect_endpoint(endpoint);
        }
        let strategy = ZenohTopicStrategy { base_key_expr: "test/isolated".into(), ..Default::default() };
        let receiver = UnAvatarZenohReceiver::declare_zenoh(&receiver_config, strategy.clone()).unwrap();
        let mut seen = [false; 2];
        for _ in 0..50 {
            for (i, publisher) in publishers.iter().enumerate() {
                publisher.put(strategy.subscribe_key_expr(), un_motion_frame_zenoh::encode_frame(&make_frame(i as u64)).unwrap()).wait().unwrap();
            }
            for frame in receiver.recv_batch_timeout(8, Duration::from_millis(50)) {
                seen[frame.header.sequence as usize] = true;
            }
            if seen.iter().all(|s| *s) { return; }
        }
        panic!("both isolated producers must be received: {seen:?}");
    }

}
