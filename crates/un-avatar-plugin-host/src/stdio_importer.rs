//! §9.4 manifest ＋ stdio JSON-RPC 子プロセスで [`AvatarImporter`] を満たすアダプタ（`IoRegistry` 接続用）。

use std::{
	collections::VecDeque,
	fmt, fs, io,
	path::{Path, PathBuf},
	process::Command,
};

use un_avatar_io::{
	AvatarImporter, FormatCapabilities, FormatDescriptor, FormatDirection, FormatId, ImportContext, ImportError, ImportInput,
	ImportOptions, ImportProbe, ImportProbeResult, ImportResult, PluginStability,
};

use crate::manifest::{load_manifest, PluginManifest};
use crate::stdio_rpc::{rpc_handshake_timeout_from_env, rpc_import_timeout_from_env, HandshakeError, PluginChild};

#[derive(Debug)]
pub enum StdioImporterError {
	Manifest(crate::manifest::ManifestError),
	Io(io::Error),
	NoImportFormat,
	ExecutableMissing(PathBuf),
	UnsupportedProtocol(String),
}

impl fmt::Display for StdioImporterError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			StdioImporterError::Manifest(e) => write!(f, "{e}"),
			StdioImporterError::Io(e) => write!(f, "{e}"),
			StdioImporterError::NoImportFormat => write!(f, "manifest has no format with can_import"),
			StdioImporterError::ExecutableMissing(p) => write!(f, "plugin executable not found (looked near {p:?})"),
			StdioImporterError::UnsupportedProtocol(p) => write!(f, "unsupported plugin protocol: {p}"),
		}
	}
}

impl std::error::Error for StdioImporterError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			StdioImporterError::Manifest(e) => Some(e),
			StdioImporterError::Io(e) => Some(e),
			_ => None,
		}
	}
}

impl From<crate::manifest::ManifestError> for StdioImporterError {
	fn from(e: crate::manifest::ManifestError) -> Self {
		StdioImporterError::Manifest(e)
	}
}

impl From<io::Error> for StdioImporterError {
	fn from(e: io::Error) -> Self {
		StdioImporterError::Io(e)
	}
}

pub(crate) fn check_protocol(m: &PluginManifest) -> Result<(), StdioImporterError> {
	if m.protocol.is_empty() || m.protocol == "stdio-json-rpc" {
		Ok(())
	} else {
		Err(StdioImporterError::UnsupportedProtocol(m.protocol.clone()))
	}
}

/// オンディスク manifest（`.toml` 優先・`manifest.toml` / `manifest.json`）を読み、`entry` 名の実行ファイルを解決する。
///
/// 1. マニフェストと同じディレクトリ（`*.exe` も）
/// 2. ancestor を上り、`target/{debug|release}/{entry}` を探す（開発時の workspace 配置用）
pub(crate) fn resolve_plugin_executable(manifest_path: &Path, entry: &str) -> Result<PathBuf, StdioImporterError> {
	let dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
	let base = dir.join(entry);
	if base.is_file() {
		return Ok(base);
	}
	#[cfg(windows)]
	{
		let with_exe = base.with_extension("exe");
		if with_exe.is_file() {
			return Ok(with_exe);
		}
	}

	let file_name: PathBuf = {
		#[cfg(windows)]
		{
			format!("{entry}.exe").into()
		}
		#[cfg(not(windows))]
		{
			entry.into()
		}
	};
	let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
	let mut cur = dir;
	loop {
		let cand = cur.join("target").join(profile).join(&file_name);
		if cand.is_file() {
			return Ok(cand);
		}
		match cur.parent() {
			Some(p) => cur = p,
			None => break,
		}
	}

	Err(StdioImporterError::ExecutableMissing(base))
}

fn descriptor_from_manifest(m: &PluginManifest) -> Result<FormatDescriptor, StdioImporterError> {
	let fmt = m.primary_import_format().ok_or(StdioImporterError::NoImportFormat)?;
	Ok(FormatDescriptor {
		id: FormatId::new(fmt.id.clone()),
		display_name: if m.name.is_empty() { m.id.clone() } else { m.name.clone() },
		extensions: fmt.extensions.clone(),
		media_types: Vec::new(),
		direction: FormatDirection::Import,
		capabilities: FormatCapabilities::default(),
		stability: PluginStability::Experimental,
		provider_plugin_id: if m.id.is_empty() { None } else { Some(m.id.clone()) },
	})
}

pub(crate) fn bundle_dir_from_manifest(manifest_path: &Path) -> PathBuf {
	manifest_path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

pub(crate) fn registration_origin_label(desc: &FormatDescriptor) -> &str {
	desc.provider_plugin_id.as_deref().unwrap_or("builtin/core registry")
}

/// manifest で指定された stdio 子プロセスに JSON-RPC で `import` を送る importer。
///
/// 子プロセス起動時の **カレントディレクトリ**は、既定で manifest 親ディレクトリ（bundle 根）。環境変数 [`plugin_child_uses_bundle_cwd_from_env`] が偽のとき（**`UN_AVATAR_PLUGIN_CHILD_CWD=host`**）はホストと同じ cwd を継承する。
#[derive(Clone, Debug)]
pub struct StdioJsonRpcImporter {
	exe: PathBuf,
	bundle_dir: PathBuf,
	descriptor: FormatDescriptor,
}

impl StdioJsonRpcImporter {
	/// manifest ファイルのパスから構築する。
	pub fn from_manifest_file(manifest_path: &Path) -> Result<Self, StdioImporterError> {
		let m = load_manifest(manifest_path)?;
		check_protocol(&m)?;
		let exe = resolve_plugin_executable(manifest_path, &m.entry)?;
		let descriptor = descriptor_from_manifest(&m)?;
		let bundle_dir = bundle_dir_from_manifest(manifest_path);
		Ok(Self {
			exe,
			bundle_dir,
			descriptor,
		})
	}

	/// 解決済み実行ファイルを明示したい場合（テスト・カスタム配置）。
	pub fn with_executable(manifest_path: &Path, exe: PathBuf) -> Result<Self, StdioImporterError> {
		let m = load_manifest(manifest_path)?;
		check_protocol(&m)?;
		let descriptor = descriptor_from_manifest(&m)?;
		let bundle_dir = bundle_dir_from_manifest(manifest_path);
		Ok(Self {
			exe,
			bundle_dir,
			descriptor,
		})
	}
}

impl AvatarImporter for StdioJsonRpcImporter {
	fn descriptor(&self) -> FormatDescriptor {
		self.descriptor.clone()
	}

	fn probe(&self, input: &ImportProbe) -> ImportProbeResult {
		let Some(ref p) = input.path_hint else {
			return ImportProbeResult { confidence: 0 };
		};
		let lossy_lower = p.to_string_lossy().to_lowercase();
		for ext in &self.descriptor.extensions {
			let suffix = format!(".{}", ext.to_lowercase());
			if lossy_lower.ends_with(&suffix) {
				return ImportProbeResult { confidence: 120 };
			}
		}
		ImportProbeResult { confidence: 0 }
	}

	fn import(&self, _ctx: &mut ImportContext, input: ImportInput, _options: ImportOptions) -> Result<ImportResult, ImportError> {
		let path = match input {
			ImportInput::Path(path) => path,
			ImportInput::Bytes { .. } => return Err(ImportError::Message("stdio plugin import requires a filesystem path".into())),
		};
		let mut cmd = Command::new(&self.exe);
		if plugin_child_uses_bundle_cwd_from_env() {
			cmd.current_dir(&self.bundle_dir);
		}
		let mut child =
			PluginChild::from_command(cmd, rpc_handshake_timeout_from_env(), rpc_import_timeout_from_env()).map_err(import_err_io)?;
		child.handshake().map_err(import_err_handshake)?;
		let result = child.rpc_import_path(&path).map_err(import_err_handshake)?;
		let _ = child.kill();
		Ok(result)
	}
}

fn import_err_io(e: io::Error) -> ImportError {
	ImportError::Message(e.to_string())
}

fn import_err_handshake(e: HandshakeError) -> ImportError {
	ImportError::Message(e.to_string())
}

/// 環境変数 **`UN_AVATAR_PLUGIN_CHILD_CWD`** が **`host`**（大文字小文字無視）のときだけ、stdio プラグイン起動時に **`current_dir` を設定しない**（ホストと同じ cwd）。未設定・空・**`bundle`**・その他の値は **manifest 所在ディレクトリ**（bundle 根）を cwd にする。
pub fn plugin_child_uses_bundle_cwd_from_env() -> bool {
	plugin_child_cwd_is_bundle_for(std::env::var("UN_AVATAR_PLUGIN_CHILD_CWD").ok().as_deref())
}

fn plugin_child_cwd_is_bundle_for(maybe: Option<&str>) -> bool {
	match maybe.map(str::trim).filter(|s| !s.is_empty()) {
		None => true,
		Some(s) if s.eq_ignore_ascii_case("host") => false,
		Some(_) => true,
	}
}

/// 環境変数 **`UN_AVATAR_PLUGIN_DISCOVERY_MAX_DEPTH`**（正の整数）で、`register_stdio_importers_from_plugin_root` がルート直下に manifest が無いときに潜る **最大相対深さ** を上書きする。無効・0・未設定は **8**。上限 **64** に丸める。
pub fn plugin_discovery_max_depth_from_env() -> usize {
	const DEFAULT: usize = 8;
	const CAP: usize = 64;
	std::env::var_os("UN_AVATAR_PLUGIN_DISCOVERY_MAX_DEPTH")
		.and_then(|raw| raw.to_string_lossy().trim().parse::<usize>().ok())
		.filter(|&d| d > 0)
		.map(|d| d.min(CAP))
		.unwrap_or(DEFAULT)
}

pub(crate) fn should_skip_plugin_search_dir(name: &str) -> bool {
	name.starts_with('.') || matches!(name, "target" | "node_modules" | "build" | "dist" | "out" | "DerivedData")
}

/// ディレクトリ直下の manifest（`discover_manifests_in_dir`＝TOML 優先）ごとに stdio importer を生成し、レジストリに登録する。
/// 個別 manifest が不正・実行ファイル不在なら**黙ってスキップ**する（登録できた件数だけ返す）。
///
/// 既に同じ `FormatId` の importer がいる状態でさらに登録するとき、**stderr に警告**を出す（レジストリの `importer_by_id` は先に登録された方だけを返す）。
pub fn register_stdio_importers_from_manifest_dir(reg: &mut un_avatar_io::IoRegistry, dir: &Path) -> io::Result<usize> {
	let mut n = 0;
	if let Some(p) = crate::manifest::discover_manifest_in_dir(dir)? {
		if let Ok(imp) = StdioJsonRpcImporter::from_manifest_file(&p) {
			let new_desc = imp.descriptor();
			if let Some(existing) = reg.importer_by_id(&new_desc.id) {
				eprintln!(
					"un-avatar-plugin-host: warning: duplicate importer FormatId `{}` \
					 (already registered from {}; also registering from {} at {}). \
					 `importer_by_id` / first-match tie-breaks use the earlier registration only.",
					new_desc.id.0,
					registration_origin_label(&existing.descriptor()),
					registration_origin_label(&new_desc),
					p.display()
				);
			}
			reg.register_importer(Box::new(imp));
			n += 1;
		}
	}
	Ok(n)
}

pub(crate) fn enqueue_plugin_search_children(q: &mut VecDeque<(PathBuf, usize)>, dir: &Path, child_depth: usize) -> io::Result<()> {
	for entry in fs::read_dir(dir)? {
		let path = entry?.path();
		if path.is_dir() {
			let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
			if should_skip_plugin_search_dir(name) {
				continue;
			}
			q.push_back((path, child_depth));
		}
	}
	Ok(())
}

pub(crate) fn register_stdio_plugins_from_root<F>(root: &Path, mut register_dir: F) -> io::Result<usize>
where
	F: FnMut(&Path) -> io::Result<usize>,
{
	let mut n = register_dir(root)?;
	if n > 0 {
		return Ok(n);
	}
	if !root.is_dir() {
		return Ok(0);
	}
	let max_depth = plugin_discovery_max_depth_from_env();
	let mut q = VecDeque::new();
	enqueue_plugin_search_children(&mut q, root, 1)?;
	while let Some((dir, depth)) = q.pop_front() {
		if depth > max_depth {
			continue;
		}
		let added = register_dir(&dir)?;
		n += added;
		if added > 0 {
			continue;
		}
		enqueue_plugin_search_children(&mut q, &dir, depth + 1)?;
	}
	Ok(n)
}

/// 単一 bundle（`dir/manifest.toml`）か、**複数 bundle の親**（`dir/foo/manifest.toml`、`dir/a/b/…`）に対応する。
///
/// まず `root` 直下を [`register_stdio_importers_from_manifest_dir`] と同様に試す。1 件も登録できなければ、`root` 以下を幅優先で歩き、各ディレクトリで manifest を試す。**manifest から importer を登録できたディレクトリはその下に降りない**（bundle の内部を誤って列挙しない）。`target` / `node_modules` 等はスキップ。最大深さは [`plugin_discovery_max_depth_from_env`]。
pub fn register_stdio_importers_from_plugin_root(reg: &mut un_avatar_io::IoRegistry, root: &Path) -> io::Result<usize> {
	register_stdio_plugins_from_root(root, |dir| register_stdio_importers_from_manifest_dir(reg, dir))
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::{
		io,
		path::Path,
		sync::{Mutex, MutexGuard},
	};

	use un_avatar_io::{FormatId, IoRegistry};

	fn cwd_serial_lock() -> MutexGuard<'static, ()> {
		static LOCK: Mutex<()> = Mutex::new(());
		LOCK.lock().unwrap_or_else(|e| e.into_inner())
	}

	struct CwdGuard {
		saved: PathBuf,
	}

	impl CwdGuard {
		fn chdir(dir: &Path) -> io::Result<Self> {
			let saved = std::env::current_dir()?;
			std::env::set_current_dir(dir)?;
			Ok(Self { saved })
		}
	}

	impl Drop for CwdGuard {
		fn drop(&mut self) {
			let _ = std::env::set_current_dir(&self.saved);
		}
	}

	struct VarGuard {
		key: &'static str,
		prev: Option<std::ffi::OsString>,
	}

	impl VarGuard {
		fn remove(key: &'static str) -> Self {
			let prev = std::env::var_os(key);
			std::env::remove_var(key);
			Self { key, prev }
		}

		fn set(key: &'static str, val: &str) -> Self {
			let prev = std::env::var_os(key);
			std::env::set_var(key, val);
			Self { key, prev }
		}
	}

	impl Drop for VarGuard {
		fn drop(&mut self) {
			match &self.prev {
				Some(v) => std::env::set_var(self.key, v),
				None => std::env::remove_var(self.key),
			}
		}
	}

	fn plugin_report_cwd(got: &ImportResult) -> PathBuf {
		let prefix = "sample-io-plugin: cwd=";
		let line = got
			.report
			.messages
			.iter()
			.find(|m| m.starts_with(prefix))
			.unwrap_or_else(|| panic!("expected cwd line in {:?}", got.report.messages));
		PathBuf::from(line.trim_start_matches(prefix))
	}

	#[test]
	fn plugin_child_cwd_pref_parsing() {
		assert!(super::plugin_child_cwd_is_bundle_for(None));
		assert!(super::plugin_child_cwd_is_bundle_for(Some("")));
		assert!(super::plugin_child_cwd_is_bundle_for(Some("bundle")));
		assert!(super::plugin_child_cwd_is_bundle_for(Some("BUNDLE")));
		assert!(!super::plugin_child_cwd_is_bundle_for(Some("host")));
		assert!(!super::plugin_child_cwd_is_bundle_for(Some("  HOST  ")));
		assert!(super::plugin_child_cwd_is_bundle_for(Some("unknown")));
	}

	#[test]
	fn skip_plugin_search_dir_names() {
		assert!(should_skip_plugin_search_dir(".git"));
		assert!(should_skip_plugin_search_dir("target"));
		assert!(should_skip_plugin_search_dir("node_modules"));
		assert!(!should_skip_plugin_search_dir("sample-io-plugin"));
	}

	#[test]
	fn plugin_discovery_max_depth_env_default_is_positive() {
		let d = plugin_discovery_max_depth_from_env();
		assert!((1..=64).contains(&d));
	}

	#[test]
	fn stdio_importer_child_cwd_defaults_to_bundle_dir() {
		let _s = cwd_serial_lock();
		let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin/manifest.toml");
		let bundle_dir = std::fs::canonicalize(manifest.parent().expect("manifest parent")).expect("bundle_dir");

		let unrelated = std::env::temp_dir().join(format!("un_avatar_cwd_unrelated_{}.d", std::process::id()));
		std::fs::create_dir_all(&unrelated).expect("mkdir");
		let _cwd = CwdGuard::chdir(&unrelated).expect("chdir");
		let _clear_host = VarGuard::remove("UN_AVATAR_PLUGIN_CHILD_CWD");
		let _emit = VarGuard::set("SAMPLE_IO_PLUGIN_EMIT_CWD", "1");

		let imp = StdioJsonRpcImporter::from_manifest_file(&manifest).expect("importer");
		let probe = ImportProbe {
			path_hint: Some(unrelated.join("dummy.exampleavatar")),
			bytes: None,
		};
		let mut reg = IoRegistry::new();
		reg.register_importer(Box::new(imp));
		let best = reg.best_importer_for(&probe).expect("importer");
		let mut ctx = ImportContext::dummy();
		let got = best
			.import(&mut ctx, ImportInput::Path(probe.path_hint.clone().unwrap()), ImportOptions)
			.expect("import");
		let got_cwd = std::fs::canonicalize(plugin_report_cwd(&got)).expect("canon cwd from plugin");
		assert_eq!(got_cwd, bundle_dir);
	}

	#[test]
	fn stdio_importer_child_cwd_follows_host_when_env_host() {
		let _s = cwd_serial_lock();
		let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin/manifest.toml");

		let host_cwd = std::env::temp_dir().join(format!("un_avatar_cwd_host_{}.d", std::process::id()));
		std::fs::create_dir_all(&host_cwd).expect("mkdir");
		let expect_host = std::fs::canonicalize(&host_cwd).expect("canon host cwd");

		let _cwd = CwdGuard::chdir(&host_cwd).expect("chdir");
		let _host_pref = VarGuard::set("UN_AVATAR_PLUGIN_CHILD_CWD", "host");
		let _emit = VarGuard::set("SAMPLE_IO_PLUGIN_EMIT_CWD", "1");

		let imp = StdioJsonRpcImporter::from_manifest_file(&manifest).expect("importer");
		let probe = ImportProbe {
			path_hint: Some(host_cwd.join("dummy.exampleavatar")),
			bytes: None,
		};
		let mut reg = IoRegistry::new();
		reg.register_importer(Box::new(imp));
		let best = reg.best_importer_for(&probe).expect("importer");
		let mut ctx = ImportContext::dummy();
		let got = best
			.import(&mut ctx, ImportInput::Path(probe.path_hint.clone().unwrap()), ImportOptions)
			.expect("import");
		let got_cwd = std::fs::canonicalize(plugin_report_cwd(&got)).expect("canon cwd from plugin");
		assert_eq!(got_cwd, expect_host);
	}

	#[test]
	fn stdio_importer_registry_import_smoke() {
		let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin/manifest.toml");
		let imp = StdioJsonRpcImporter::from_manifest_file(&manifest).expect("importer");
		let probe = ImportProbe {
			path_hint: Some(PathBuf::from("dummy.exampleavatar")),
			bytes: None,
		};
		assert!(imp.probe(&probe).confidence > 0);

		let mut reg = IoRegistry::new();
		reg.register_importer(Box::new(imp));
		let best = reg.best_importer_for(&probe).expect("importer");
		let mut ctx = ImportContext::dummy();
		let tmp = std::env::temp_dir().join(format!("plug-int-{}.exampleavatar", std::process::id()));
		let got = best.import(&mut ctx, ImportInput::Path(tmp), ImportOptions).expect("import");
		assert!(
			got.report.messages.iter().any(|m| m.contains("sample-io-plugin")),
			"{:?}",
			got.report.messages
		);
	}

	#[test]
	fn register_plugins_parent_dir_finds_child() {
		let plugins_parent = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins");
		let mut reg = IoRegistry::new();
		let n = register_stdio_importers_from_plugin_root(&mut reg, &plugins_parent).unwrap();
		assert!(n >= 1, "plugins/ 配下の bundle を少なくとも 1 つ拾う");
		let probe = ImportProbe {
			path_hint: Some(PathBuf::from("a.exampleavatar")),
			bytes: None,
		};
		assert!(reg.best_importer_for(&probe).is_some());
		assert!(
			reg.importer_by_id(&FormatId::new("io.un-avatar.example.avatar")).is_some(),
			"sample-io-plugin の形式が登録されている"
		);
	}

	#[test]
	fn register_plugins_dir_adds_importer() {
		let plugins_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin");
		let mut reg = IoRegistry::new();
		let n = register_stdio_importers_from_manifest_dir(&mut reg, &plugins_dir).unwrap();
		assert_eq!(n, 1);
		let probe = ImportProbe {
			path_hint: Some(PathBuf::from("a.exampleavatar")),
			bytes: None,
		};
		assert!(reg.best_importer_for(&probe).is_some());
	}

	#[test]
	fn register_same_manifest_dir_twice_yields_two_importers_and_first_wins_by_id() {
		let plugins_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin");
		let mut reg = IoRegistry::new();
		assert_eq!(register_stdio_importers_from_manifest_dir(&mut reg, &plugins_dir).unwrap(), 1);
		assert_eq!(register_stdio_importers_from_manifest_dir(&mut reg, &plugins_dir).unwrap(), 1);
		assert_eq!(reg.importers().len(), 2);
		let id = FormatId::new("io.un-avatar.example.avatar");
		let d0 = reg.importers()[0].descriptor();
		let d1 = reg.importers()[1].descriptor();
		assert_eq!(d0.id, id);
		assert_eq!(d1.id, id);
		let chosen = reg.importer_by_id(&id).expect("importer_by_id");
		assert_eq!(chosen.descriptor().id, id);
		assert_eq!(chosen.descriptor().provider_plugin_id, d0.provider_plugin_id);
	}
}
