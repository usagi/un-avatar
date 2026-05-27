//! 子プロセス stdin/stdout に **改行区切り JSON-RPC 2.0** を送受信する。

use std::{
	io::{BufRead, BufReader, BufWriter, Write},
	path::Path,
	process::{Child, ChildStdin, ChildStdout, Command, Stdio},
	sync::{mpsc, Arc, Mutex},
	thread,
	time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use un_avatar_io::{ExportResult, ImportResult, UnaDocument};

/// ホストとプラグインが合意するプロトコル版（握手で交換）。
pub const PROTOCOL_VERSION: &str = "0.1";

/// 1 回の JSON-RPC 往復で stdout から 1 行が来るまで待つ既定上限（`initialize`・`import` 共用の退避先）。
pub const DEFAULT_RPC_READ_TIMEOUT: Duration = Duration::from_secs(120);

const RPC_TIMEOUT_MAX_SECS: u64 = 86_400;

fn rpc_timeout_secs_from_var(key: &str) -> Option<Duration> {
	let raw = std::env::var_os(key)?;
	let secs: u64 = raw.to_string_lossy().trim().parse().ok()?;
	if secs == 0 {
		return None;
	}
	Some(Duration::from_secs(secs.min(RPC_TIMEOUT_MAX_SECS)))
}

/// 環境変数 **`UN_AVATAR_PLUGIN_RPC_TIMEOUT_SECS`**（秒、正の整数）で上書き。無効・0・未設定は [`DEFAULT_RPC_READ_TIMEOUT`]。
pub fn rpc_read_timeout_from_env() -> Duration {
	rpc_timeout_secs_from_var("UN_AVATAR_PLUGIN_RPC_TIMEOUT_SECS").unwrap_or(DEFAULT_RPC_READ_TIMEOUT)
}

/// **`initialize` 握手**の応答 1 行の読取上限。未設定時は [`rpc_read_timeout_from_env`]。
pub fn rpc_handshake_timeout_from_env() -> Duration {
	rpc_timeout_secs_from_var("UN_AVATAR_PLUGIN_RPC_HANDSHAKE_TIMEOUT_SECS").unwrap_or_else(rpc_read_timeout_from_env)
}

/// **`import` RPC** の応答 1 行の読取上限。未設定時は [`rpc_read_timeout_from_env`]。
pub fn rpc_import_timeout_from_env() -> Duration {
	rpc_timeout_secs_from_var("UN_AVATAR_PLUGIN_RPC_IMPORT_TIMEOUT_SECS").unwrap_or_else(rpc_read_timeout_from_env)
}

/// **`export` RPC** の応答 1 行の読取上限（環境変数のみから解決する際に使う）。未設定・0・無効は [`rpc_import_timeout_from_env`] と同じ鎖。
pub fn rpc_export_timeout_from_env() -> Duration {
	rpc_timeout_secs_from_var("UN_AVATAR_PLUGIN_RPC_EXPORT_TIMEOUT_SECS").unwrap_or_else(rpc_import_timeout_from_env)
}

fn export_read_timeout_for_session(import_read_timeout: Duration) -> Duration {
	rpc_timeout_secs_from_var("UN_AVATAR_PLUGIN_RPC_EXPORT_TIMEOUT_SECS").unwrap_or(import_read_timeout)
}

/// **1 セッションあたりの壁時計**（子起動からの経過上限）。未設定・0・無効は **無制限**（行読取タイムアウトのみ）。
///
/// 設定時は各 RPC 応答行の待ち上限を **残り時間と行ごとの上限の短い方**に切り詰める。全体が超過したあとに次の RPC に入ると [`HandshakeError::SessionWallTimeout`]。
pub fn rpc_session_wall_from_env() -> Option<Duration> {
	rpc_timeout_secs_from_var("UN_AVATAR_PLUGIN_RPC_SESSION_WALL_SECS")
}

#[derive(Debug)]
pub enum RpcError {
	IdMismatch { expected: i64, got: Value },
	NotJsonRpcResponse,
	Remote { code: i64, message: String },
	UnexpectedResult,
}

impl std::fmt::Display for RpcError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			RpcError::IdMismatch { expected, got } => {
				write!(f, "JSON-RPC id mismatch (expected {expected}, got {got})")
			}
			RpcError::NotJsonRpcResponse => write!(f, "not a JSON-RPC 2.0 response"),
			RpcError::Remote { code, message } => write!(f, "rpc error {code}: {message}"),
			RpcError::UnexpectedResult => write!(f, "unexpected RPC result shape"),
		}
	}
}

impl std::error::Error for RpcError {}

#[derive(Debug)]
pub enum HandshakeError {
	Io(std::io::Error),
	Json(serde_json::Error),
	LineEmpty,
	ChildGone,
	/// stdout から応答行が [`PluginChild`] の読取タイムアウト内に届かなかった（子は kill 済みのことがある）。
	ReadTimeout,
	/// 起動からの **セッション壁時計**（[`rpc_session_wall_from_env`]）を超過したあとに RPC を送ろうとした（子は kill 済みのことがある）。
	SessionWallTimeout,
	Rpc(RpcError),
	VersionMismatch {
		got: String,
	},
	PluginIdMissing,
}

impl std::fmt::Display for HandshakeError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			HandshakeError::Io(e) => write!(f, "{e}"),
			HandshakeError::Json(e) => write!(f, "{e}"),
			HandshakeError::LineEmpty => write!(f, "plugin closed stdout without response"),
			HandshakeError::ChildGone => write!(f, "plugin process ended before response"),
			HandshakeError::ReadTimeout => write!(
				f,
				"timed out waiting for plugin RPC response (UN_AVATAR_PLUGIN_RPC_TIMEOUT_SECS, or UN_AVATAR_PLUGIN_RPC_HANDSHAKE_TIMEOUT_SECS / UN_AVATAR_PLUGIN_RPC_IMPORT_TIMEOUT_SECS / UN_AVATAR_PLUGIN_RPC_EXPORT_TIMEOUT_SECS)"
			),
			HandshakeError::SessionWallTimeout => write!(
				f,
				"plugin RPC session exceeded wall clock limit (UN_AVATAR_PLUGIN_RPC_SESSION_WALL_SECS)"
			),
			HandshakeError::Rpc(e) => write!(f, "{e}"),
			HandshakeError::VersionMismatch { got } => {
				write!(f, "protocol_version mismatch (expected {PROTOCOL_VERSION}, got {got})")
			}
			HandshakeError::PluginIdMissing => write!(f, "initialize result missing plugin_id"),
		}
	}
}

impl std::error::Error for HandshakeError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			HandshakeError::Io(e) => Some(e),
			HandshakeError::Json(e) => Some(e),
			HandshakeError::Rpc(e) => Some(e),
			_ => None,
		}
	}
}

impl From<std::io::Error> for HandshakeError {
	fn from(e: std::io::Error) -> Self {
		HandshakeError::Io(e)
	}
}

impl From<serde_json::Error> for HandshakeError {
	fn from(e: serde_json::Error) -> Self {
		HandshakeError::Json(e)
	}
}

impl From<RpcError> for HandshakeError {
	fn from(e: RpcError) -> Self {
		HandshakeError::Rpc(e)
	}
}

/// 子プロセスを起動したハンドル（stdio 片側バッファ済み）。
pub struct PluginChild {
	child: Child,
	writer: BufWriter<ChildStdin>,
	reader: Arc<Mutex<BufReader<ChildStdout>>>,
	next_id: i64,
	handshake_read_timeout: Duration,
	import_read_timeout: Duration,
	export_read_timeout: Duration,
	session_deadline: Option<Instant>,
}

impl PluginChild {
	/// `program` を引数なしで起動し、stdin/stdout をパイプする。読取タイムアウトは [`rpc_handshake_timeout_from_env`] / [`rpc_import_timeout_from_env`]。**export** 応答は **環境変数 `UN_AVATAR_PLUGIN_RPC_EXPORT_TIMEOUT_SECS`**（有効時）または **当該インスタンスの import 上限**。
	pub fn spawn(program: &Path) -> Result<Self, std::io::Error> {
		let cmd = Command::new(program);
		Self::from_command(cmd, rpc_handshake_timeout_from_env(), rpc_import_timeout_from_env())
	}

	/// `initialize` と `import` で同じ読取上限を使う（テスト向け。セッション壁時計は無効）。
	pub fn spawn_with_read_timeout(program: &Path, rpc_read_timeout: Duration) -> Result<Self, std::io::Error> {
		let cmd = Command::new(program);
		Self::from_command_with_session_wall(cmd, rpc_read_timeout, rpc_read_timeout, None)
	}

	/// カスタム [`Command`]（引数・環境など）で起動する。stdio はパイプに置き換えられる。セッション壁時計は [`rpc_session_wall_from_env`]。
	pub fn from_command(cmd: Command, handshake_read_timeout: Duration, import_read_timeout: Duration) -> Result<Self, std::io::Error> {
		Self::from_command_with_session_wall(cmd, handshake_read_timeout, import_read_timeout, rpc_session_wall_from_env())
	}

	/// [`Self::from_command`] と同じだが、セッション壁時計を明示する（テスト・上書き用）。
	pub fn from_command_with_session_wall(
		mut cmd: Command,
		handshake_read_timeout: Duration,
		import_read_timeout: Duration,
		session_wall: Option<Duration>,
	) -> Result<Self, std::io::Error> {
		cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit());
		let mut child = cmd.spawn()?;
		let stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("missing child stdin"))?;
		let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("missing child stdout"))?;
		let started = Instant::now();
		let session_deadline = session_wall.map(|d| started + d);
		let export_read_timeout = export_read_timeout_for_session(import_read_timeout);
		Ok(Self {
			child,
			writer: BufWriter::new(stdin),
			reader: Arc::new(Mutex::new(BufReader::new(stdout))),
			next_id: 0,
			handshake_read_timeout,
			import_read_timeout,
			export_read_timeout,
			session_deadline,
		})
	}

	/// `initialize` と `import` で同じ読取上限を使う（[`Self::from_command`] の糖衣）。
	pub fn from_command_uniform(cmd: Command, read_timeout: Duration) -> Result<Self, std::io::Error> {
		Self::from_command_with_session_wall(cmd, read_timeout, read_timeout, rpc_session_wall_from_env())
	}

	fn alloc_id(&mut self) -> i64 {
		self.next_id += 1;
		self.next_id
	}

	fn ensure_session_wall_alive(&mut self) -> Result<(), HandshakeError> {
		let Some(deadline) = self.session_deadline else {
			return Ok(());
		};
		if Instant::now() >= deadline {
			let _ = self.child.kill();
			return Err(HandshakeError::SessionWallTimeout);
		}
		Ok(())
	}

	fn effective_recv_timeout(&self, per_rpc: Duration) -> Duration {
		let Some(deadline) = self.session_deadline else {
			return per_rpc;
		};
		let remain = deadline.saturating_duration_since(Instant::now());
		per_rpc.min(remain)
	}

	fn request(&mut self, method: &str, params: Value, id: i64, read_timeout: Duration) -> Result<Value, HandshakeError> {
		self.ensure_session_wall_alive()?;
		let recv_timeout = self.effective_recv_timeout(read_timeout);
		if recv_timeout.is_zero() {
			let _ = self.child.kill();
			return Err(HandshakeError::SessionWallTimeout);
		}
		let req = JsonRpcRequest {
			jsonrpc: "2.0",
			method,
			params: Some(params),
			id: Value::from(id),
		};
		let line = serde_json::to_string(&req)?;
		self.writer.write_all(line.as_bytes())?;
		self.writer.write_all(b"\n")?;
		self.writer.flush()?;

		let reader = Arc::clone(&self.reader);
		let (tx, rx) = mpsc::channel();
		thread::spawn(move || {
			let res = (|| {
				let mut line = String::new();
				let mut g = reader.lock().unwrap_or_else(|e| e.into_inner());
				let n = g.read_line(&mut line)?;
				Ok::<(usize, String), std::io::Error>((n, line))
			})();
			let _ = tx.send(res);
		});

		match rx.recv_timeout(recv_timeout) {
			Ok(Ok((n, line))) => {
				if n == 0 {
					if self.child.try_wait()?.is_some() {
						return Err(HandshakeError::ChildGone);
					}
					return Err(HandshakeError::LineEmpty);
				}
				let resp: JsonRpcResponse = serde_json::from_str(line.trim())?;
				if resp.jsonrpc != "2.0" {
					return Err(RpcError::NotJsonRpcResponse.into());
				}
				if resp.id.as_i64() != Some(id) {
					return Err(RpcError::IdMismatch {
						expected: id,
						got: resp.id,
					}
					.into());
				}
				if let Some(err) = resp.error {
					return Err(HandshakeError::Rpc(RpcError::Remote {
						code: err.code,
						message: err.message,
					}));
				}
				resp.result.ok_or(RpcError::UnexpectedResult.into())
			}
			Ok(Err(e)) => Err(HandshakeError::Io(e)),
			Err(mpsc::RecvTimeoutError::Timeout) => {
				let _ = self.child.kill();
				Err(HandshakeError::ReadTimeout)
			}
			Err(mpsc::RecvTimeoutError::Disconnected) => Err(HandshakeError::ChildGone),
		}
	}

	/// `import` を送り、プラグインが返した JSON を [`ImportResult`] としてパースする（Commit 2.5）。
	pub fn rpc_import_path(&mut self, path: &Path) -> Result<ImportResult, HandshakeError> {
		let id = self.alloc_id();
		let params = serde_json::json!({
			"path": path.to_string_lossy(),
		});
		let t = self.import_read_timeout;
		let v = self.request("import", params, id, t)?;
		Ok(serde_json::from_value(v)?)
	}

	/// `export` を送り、プラグインが返した JSON を [`ExportResult`] としてパースする。
	///
	/// 応答 1 行の待ち上限は **`UN_AVATAR_PLUGIN_RPC_EXPORT_TIMEOUT_SECS`**（有効時）。未設定・0・無効は **この子の** [`Self::rpc_import_path`] と同じ上限。
	pub fn rpc_export_path(&mut self, path: &Path, document: &UnaDocument) -> Result<ExportResult, HandshakeError> {
		let id = self.alloc_id();
		let params = serde_json::json!({
			"path": path.to_string_lossy(),
			"document": document,
		});
		let t = self.export_read_timeout;
		let v = self.request("export", params, id, t)?;
		Ok(serde_json::from_value(v)?)
	}

	/// `initialize` を送り、プラグイン ID とプロトコル版を確認する。
	pub fn handshake(&mut self) -> Result<InitializeAck, HandshakeError> {
		let id = self.alloc_id();
		let params = serde_json::json!({
			"protocol_version": PROTOCOL_VERSION,
			"host": "un-avatar-plugin-host",
		});
		let t = self.handshake_read_timeout;
		let result = self.request("initialize", params, id, t)?;
		let ack: InitializeAck = serde_json::from_value(result)?;
		if ack.protocol_version != PROTOCOL_VERSION {
			return Err(HandshakeError::VersionMismatch { got: ack.protocol_version });
		}
		if ack.plugin_id.is_empty() {
			return Err(HandshakeError::PluginIdMissing);
		}
		Ok(ack)
	}

	/// 子の終了を待つ（テストやシャットダウン用）。
	pub fn wait(&mut self) -> Result<std::process::ExitStatus, std::io::Error> {
		self.child.wait()
	}

	/// 子プロセスを強制終了する。
	pub fn kill(&mut self) -> Result<(), std::io::Error> {
		self.child.kill()
	}
}

#[derive(Serialize)]
struct JsonRpcRequest<'a> {
	jsonrpc: &'a str,
	method: &'a str,
	#[serde(skip_serializing_if = "Option::is_none")]
	params: Option<Value>,
	id: Value,
}

#[derive(Deserialize)]
struct JsonRpcResponse {
	jsonrpc: String,
	#[serde(default)]
	result: Option<Value>,
	#[serde(default)]
	error: Option<JsonRpcErrorObj>,
	id: Value,
}

#[derive(Deserialize)]
struct JsonRpcErrorObj {
	code: i64,
	message: String,
}

/// `initialize` の正常結果。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct InitializeAck {
	pub protocol_version: String,
	pub plugin_id: String,
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::Path;
	use std::thread;
	use std::time::Instant;

	#[test]
	fn default_rpc_read_timeout_is_sane() {
		assert!(DEFAULT_RPC_READ_TIMEOUT.as_secs() >= 1);
	}

	#[cfg(unix)]
	#[test]
	fn handshake_times_out_when_child_never_writes_line() {
		let mut cmd = Command::new("/bin/sh");
		cmd.args(["-c", "sleep 60"]);
		let mut child = PluginChild::from_command_with_session_wall(cmd, Duration::from_millis(200), Duration::from_millis(200), None)
			.expect("spawn sh sleep");
		let started = Instant::now();
		let err = child.handshake().expect_err("should timeout");
		assert!(matches!(err, HandshakeError::ReadTimeout), "got {err:?}");
		assert!(started.elapsed() < Duration::from_secs(3));
	}

	/// PowerShell の `Start-Sleep` は stdout に JSON-RPC 行を出さない。
	#[cfg(windows)]
	#[test]
	fn handshake_times_out_when_child_never_writes_line() {
		let mut cmd = Command::new("powershell.exe");
		cmd.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 120"]);
		let mut child = PluginChild::from_command_with_session_wall(cmd, Duration::from_millis(400), Duration::from_millis(400), None)
			.expect("spawn powershell sleep");
		let started = Instant::now();
		let err = child.handshake().expect_err("should timeout");
		assert!(matches!(err, HandshakeError::ReadTimeout), "got {err:?}");
		assert!(started.elapsed() < Duration::from_secs(4));
	}

	fn workspace_sample_plugin_exe() -> std::path::PathBuf {
		let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
		let name = if cfg!(windows) {
			"sample-io-plugin.exe"
		} else {
			"sample-io-plugin"
		};
		Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target").join(profile).join(name)
	}

	#[test]
	fn session_wall_expired_triggers_before_second_rpc() {
		let exe = workspace_sample_plugin_exe();
		assert!(exe.is_file(), "sample-io-plugin binary missing at {:?}; build workspace first", exe);
		let mut p = PluginChild::from_command_with_session_wall(
			Command::new(&exe),
			Duration::from_secs(30),
			Duration::from_secs(30),
			Some(Duration::from_secs(2)),
		)
		.expect("spawn sample-io-plugin");
		p.handshake().expect("handshake");
		thread::sleep(Duration::from_millis(2500));
		let err = p.rpc_import_path(Path::new("probe.exampleavatar")).expect_err("session wall");
		assert!(matches!(err, HandshakeError::SessionWallTimeout), "{err:?}");
	}

	#[cfg(unix)]
	#[test]
	fn session_wall_caps_handshake_timeout() {
		let mut cmd = Command::new("/bin/sh");
		cmd.args(["-c", "sleep 60"]);
		let mut child = PluginChild::from_command_with_session_wall(
			cmd,
			Duration::from_secs(60),
			Duration::from_secs(60),
			Some(Duration::from_millis(350)),
		)
		.expect("spawn sh sleep");
		let started = Instant::now();
		let err = child.handshake().expect_err("should timeout");
		assert!(matches!(err, HandshakeError::ReadTimeout), "got {err:?}");
		assert!(started.elapsed() < Duration::from_secs(2));
	}

	#[cfg(windows)]
	#[test]
	fn session_wall_caps_handshake_timeout() {
		let mut cmd = Command::new("powershell.exe");
		cmd.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 120"]);
		let mut child = PluginChild::from_command_with_session_wall(
			cmd,
			Duration::from_secs(120),
			Duration::from_secs(120),
			Some(Duration::from_millis(450)),
		)
		.expect("spawn powershell sleep");
		let started = Instant::now();
		let err = child.handshake().expect_err("should timeout");
		assert!(matches!(err, HandshakeError::ReadTimeout), "got {err:?}");
		assert!(started.elapsed() < Duration::from_secs(4));
	}
}
