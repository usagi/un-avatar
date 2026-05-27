//! 調査用のファイル／stderr への追記ログ（VMC スレッドとメイン描画から共有）。

use std::{
	fs::OpenOptions,
	io::{self, Write},
	path::PathBuf,
	sync::{Arc, Mutex},
	time::{SystemTime, UNIX_EPOCH},
};

/// ウィンドウ・CLI 共通のデバッグフラグ。
#[derive(Clone, Debug, Default)]
pub struct WindowDebugOptions {
	/// 追記するログファイル（省略時はカテゴリが有効なら stderr のみ）。
	pub log_path: Option<PathBuf>,
	/// ログファイルがある場合でも stderr に同じ内容を出す。
	pub mirror_stderr: bool,
	pub vmc: bool,
	pub scene: bool,
	pub morph: bool,
}

impl WindowDebugOptions {
	pub fn any_category(&self) -> bool {
		self.vmc || self.scene || self.morph
	}

	/// `mirror_stderr`: 明示指定。`log_path` が無いときはカテゴリがあれば stderr に出す。
	pub fn resolve_mirror_stderr(&self) -> bool {
		self.mirror_stderr || self.log_path.is_none()
	}

	pub fn is_active(&self) -> bool {
		self.any_category()
	}
}

struct DebugLogInner {
	file: Mutex<Option<std::fs::File>>,
	mirror_stderr: bool,
}

/// スレッドセーフな 1 行ログ。無効時は [`DebugLog::line`] は何もしない。
#[derive(Clone)]
pub struct DebugLog {
	inner: Arc<DebugLogInner>,
	enabled: bool,
}

impl DebugLog {
	pub fn disabled() -> Self {
		Self {
			inner: Arc::new(DebugLogInner {
				file: Mutex::new(None),
				mirror_stderr: false,
			}),
			enabled: false,
		}
	}

	/// カテゴリが 1 つも無いときは無効なロガーを返す（ファイルを開かない）。
	pub fn from_options(opts: &WindowDebugOptions) -> Result<Self, String> {
		if !opts.any_category() {
			return Ok(Self::disabled());
		}
		let mirror = opts.resolve_mirror_stderr();
		let file = if let Some(ref p) = opts.log_path {
			Some(
				OpenOptions::new()
					.create(true)
					.append(true)
					.open(p)
					.map_err(|e| format!("--debug-log {}: {e}", p.display()))?,
			)
		} else {
			None
		};
		Ok(Self {
			inner: Arc::new(DebugLogInner {
				file: Mutex::new(file),
				mirror_stderr: mirror,
			}),
			enabled: true,
		})
	}

	pub fn is_enabled(&self) -> bool {
		self.enabled
	}

	pub fn line(&self, tag: &str, msg: impl AsRef<str>) {
		if !self.enabled {
			return;
		}
		let ts = match SystemTime::now().duration_since(UNIX_EPOCH) {
			Ok(d) => format!("{:.3}", d.as_secs_f64()),
			Err(_) => "?".to_string(),
		};
		let line = format!("[{ts}] [{tag}] {}\n", msg.as_ref());
		if let Ok(mut g) = self.inner.file.lock() {
			if let Some(ref mut f) = *g {
				let _ = f.write_all(line.as_bytes());
				let _ = f.flush();
			}
		}
		if self.inner.mirror_stderr {
			let _ = io::stderr().write_all(line.as_bytes());
			let _ = io::stderr().flush();
		}
	}
}
