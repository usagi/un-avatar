//! §9.4 manifest ＋ stdio JSON-RPC 子プロセスで [`AvatarExporter`] を満たすアダプタ（`IoRegistry` 接続用）。

use std::{
	collections::VecDeque,
	fmt, fs, io,
	path::{Path, PathBuf},
	process::Command,
};

use un_avatar_io::{
	AvatarExporter, ExportCapability, ExportContext, ExportError, ExportOptions, ExportOutput, ExportResult, FormatCapabilities,
	FormatDescriptor, FormatDirection, FormatId, PluginStability, UnaDocument,
};

use crate::manifest::{load_manifest, PluginManifest};
use crate::stdio_importer::{
	bundle_dir_from_manifest, check_protocol, plugin_child_uses_bundle_cwd_from_env, plugin_discovery_max_depth_from_env,
	registration_origin_label, resolve_plugin_executable, should_skip_plugin_search_dir,
};
use crate::stdio_rpc::{rpc_handshake_timeout_from_env, rpc_import_timeout_from_env, HandshakeError, PluginChild};

#[derive(Debug)]
pub enum StdioExporterError {
	Manifest(crate::manifest::ManifestError),
	Io(io::Error),
	NoExportFormat,
	ExecutableMissing(PathBuf),
	UnsupportedProtocol(String),
}

impl fmt::Display for StdioExporterError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			StdioExporterError::Manifest(e) => write!(f, "{e}"),
			StdioExporterError::Io(e) => write!(f, "{e}"),
			StdioExporterError::NoExportFormat => write!(f, "manifest has no format with can_export"),
			StdioExporterError::ExecutableMissing(p) => write!(f, "plugin executable not found (looked near {p:?})"),
			StdioExporterError::UnsupportedProtocol(p) => write!(f, "unsupported plugin protocol: {p}"),
		}
	}
}

impl std::error::Error for StdioExporterError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			StdioExporterError::Manifest(e) => Some(e),
			StdioExporterError::Io(e) => Some(e),
			_ => None,
		}
	}
}

impl From<crate::manifest::ManifestError> for StdioExporterError {
	fn from(e: crate::manifest::ManifestError) -> Self {
		StdioExporterError::Manifest(e)
	}
}

impl From<io::Error> for StdioExporterError {
	fn from(e: io::Error) -> Self {
		StdioExporterError::Io(e)
	}
}

fn export_descriptor_from_manifest(m: &PluginManifest) -> Result<FormatDescriptor, StdioExporterError> {
	let fmt = m.primary_export_format().ok_or(StdioExporterError::NoExportFormat)?;
	Ok(FormatDescriptor {
		id: FormatId::new(fmt.id.clone()),
		display_name: if m.name.is_empty() { m.id.clone() } else { m.name.clone() },
		extensions: fmt.extensions.clone(),
		media_types: Vec::new(),
		direction: FormatDirection::Export,
		capabilities: FormatCapabilities::default(),
		stability: PluginStability::Experimental,
		provider_plugin_id: if m.id.is_empty() { None } else { Some(m.id.clone()) },
	})
}

/// manifest で指定された stdio 子プロセスに JSON-RPC で `export` を送る exporter。
///
/// 子プロセスの cwd は [`crate::stdio_importer::StdioJsonRpcImporter`] と同じ規則（[`plugin_child_uses_bundle_cwd_from_env`]）。
#[derive(Clone, Debug)]
pub struct StdioJsonRpcExporter {
	exe: PathBuf,
	bundle_dir: PathBuf,
	descriptor: FormatDescriptor,
}

impl StdioJsonRpcExporter {
	pub fn format_descriptor(&self) -> &FormatDescriptor {
		&self.descriptor
	}

	/// manifest ファイルのパスから構築する。
	pub fn from_manifest_file(manifest_path: &Path) -> Result<Self, StdioExporterError> {
		let m = load_manifest(manifest_path)?;
		check_protocol(&m).map_err(map_importer_err_to_exporter)?;
		let exe = resolve_plugin_executable(manifest_path, &m.entry).map_err(map_importer_err_to_exporter)?;
		let descriptor = export_descriptor_from_manifest(&m)?;
		let bundle_dir = bundle_dir_from_manifest(manifest_path);
		Ok(Self {
			exe,
			bundle_dir,
			descriptor,
		})
	}

	/// 解決済み実行ファイルを明示したい場合（テスト・カスタム配置）。
	pub fn with_executable(manifest_path: &Path, exe: PathBuf) -> Result<Self, StdioExporterError> {
		let m = load_manifest(manifest_path)?;
		check_protocol(&m).map_err(map_importer_err_to_exporter)?;
		let descriptor = export_descriptor_from_manifest(&m)?;
		let bundle_dir = bundle_dir_from_manifest(manifest_path);
		Ok(Self {
			exe,
			bundle_dir,
			descriptor,
		})
	}
}

fn map_importer_err_to_exporter(e: crate::stdio_importer::StdioImporterError) -> StdioExporterError {
	match e {
		crate::stdio_importer::StdioImporterError::Manifest(x) => StdioExporterError::Manifest(x),
		crate::stdio_importer::StdioImporterError::Io(x) => StdioExporterError::Io(x),
		crate::stdio_importer::StdioImporterError::NoImportFormat => StdioExporterError::NoExportFormat,
		crate::stdio_importer::StdioImporterError::ExecutableMissing(p) => StdioExporterError::ExecutableMissing(p),
		crate::stdio_importer::StdioImporterError::UnsupportedProtocol(p) => StdioExporterError::UnsupportedProtocol(p),
	}
}

impl AvatarExporter for StdioJsonRpcExporter {
	fn descriptor(&self) -> FormatDescriptor {
		self.descriptor.clone()
	}

	fn can_export(&self, _document: &UnaDocument, _options: &ExportOptions) -> ExportCapability {
		ExportCapability::Supported
	}

	fn export(
		&self,
		_ctx: &mut ExportContext,
		document: &UnaDocument,
		output: ExportOutput,
		_options: ExportOptions,
	) -> Result<ExportResult, ExportError> {
		let ExportOutput::Path(path) = output;
		let mut cmd = Command::new(&self.exe);
		if plugin_child_uses_bundle_cwd_from_env() {
			cmd.current_dir(&self.bundle_dir);
		}
		let mut child =
			PluginChild::from_command(cmd, rpc_handshake_timeout_from_env(), rpc_import_timeout_from_env()).map_err(export_err_io)?;
		child.handshake().map_err(export_err_handshake)?;
		let result = child.rpc_export_path(&path, document).map_err(export_err_handshake)?;
		let _ = child.kill();
		Ok(result)
	}
}

fn export_err_io(e: io::Error) -> ExportError {
	ExportError::Message(e.to_string())
}

fn export_err_handshake(e: HandshakeError) -> ExportError {
	ExportError::Message(e.to_string())
}

/// ディレクトリ直下の manifest（`discover_manifests_in_dir`＝TOML 優先）ごとに stdio exporter を生成し、レジストリに登録する。
/// 個別 manifest が不正・実行ファイル不在・`can_export` 形式なしなら**黙ってスキップ**する（登録できた件数だけ返す）。
///
/// 既に同じ `FormatId` の exporter がある状態でさらに登録するとき、**stderr に警告**を出す（レジストリの `exporter_by_id` は先に登録された方だけを返す）。
pub fn register_stdio_exporters_from_manifest_dir(reg: &mut un_avatar_io::IoRegistry, dir: &Path) -> io::Result<usize> {
	let mut n = 0;
	for p in crate::manifest::discover_manifests_in_dir(dir)? {
		if let Ok(exp) = StdioJsonRpcExporter::from_manifest_file(&p) {
			let new_desc = exp.format_descriptor();
			if let Some(existing) = reg.exporter_by_id(&new_desc.id) {
				eprintln!(
					"un-avatar-plugin-host: warning: duplicate exporter FormatId `{}` \
					 (already registered from {}; also registering from {} at {}). \
					 `exporter_by_id` / first-match tie-breaks use the earlier registration only.",
					new_desc.id.0,
					registration_origin_label(&existing.descriptor()),
					registration_origin_label(new_desc),
					p.display()
				);
			}
			reg.register_exporter(Box::new(exp));
			n += 1;
		}
	}
	Ok(n)
}

/// [`crate::stdio_importer::register_stdio_importers_from_plugin_root`] と同じ探索規則で stdio exporter を登録する。
pub fn register_stdio_exporters_from_plugin_root(reg: &mut un_avatar_io::IoRegistry, root: &Path) -> io::Result<usize> {
	let mut n = register_stdio_exporters_from_manifest_dir(reg, root)?;
	if n > 0 {
		return Ok(n);
	}
	if !root.is_dir() {
		return Ok(0);
	}
	let max_depth = plugin_discovery_max_depth_from_env();
	let mut q = VecDeque::new();
	for entry in fs::read_dir(root)? {
		let path = entry?.path();
		if path.is_dir() {
			let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
			if should_skip_plugin_search_dir(name) {
				continue;
			}
			q.push_back((path, 1usize));
		}
	}
	while let Some((dir, depth)) = q.pop_front() {
		if depth > max_depth {
			continue;
		}
		let added = register_stdio_exporters_from_manifest_dir(reg, &dir)?;
		n += added;
		if added > 0 {
			continue;
		}
		for entry in fs::read_dir(&dir)? {
			let path = entry?.path();
			if path.is_dir() {
				let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
				if should_skip_plugin_search_dir(name) {
					continue;
				}
				q.push_back((path, depth + 1));
			}
		}
	}
	Ok(n)
}

#[cfg(test)]
mod tests {
	use super::*;
	use un_avatar_io::IoRegistry;

	#[test]
	fn register_same_manifest_dir_twice_yields_two_exporters_and_first_wins_by_id() {
		let plugins_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin");
		let mut reg = IoRegistry::new();
		assert_eq!(register_stdio_exporters_from_manifest_dir(&mut reg, &plugins_dir).unwrap(), 1);
		assert_eq!(register_stdio_exporters_from_manifest_dir(&mut reg, &plugins_dir).unwrap(), 1);
		assert_eq!(reg.exporters().len(), 2);
		let id = FormatId::new("io.un-avatar.example.avatar");
		let d0 = reg.exporters()[0].descriptor();
		let d1 = reg.exporters()[1].descriptor();
		assert_eq!(d0.id, id);
		assert_eq!(d1.id, id);
		let chosen = reg.exporter_by_id(&id).expect("exporter_by_id");
		assert_eq!(chosen.descriptor().id, id);
		assert_eq!(chosen.descriptor().provider_plugin_id, d0.provider_plugin_id);
	}

	#[test]
	fn stdio_exporter_roundtrip_invokes_plugin_export_rpc() {
		let plugins_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin");
		let manifest = plugins_dir.join("manifest.toml");
		let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
		let name = if cfg!(windows) {
			"sample-io-plugin.exe"
		} else {
			"sample-io-plugin"
		};
		let exe = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target").join(profile).join(name);
		assert!(exe.is_file(), "build sample-io-plugin first: {:?}", exe);
		let exp = StdioJsonRpcExporter::with_executable(&manifest, exe).unwrap();
		let mut ctx = ExportContext::dummy();
		let out = std::env::temp_dir().join(format!("ua-sample-export-{}.exampleavatar", std::process::id()));
		let _ = std::fs::remove_file(&out);
		let r = exp
			.export(&mut ctx, &UnaDocument::default(), ExportOutput::Path(out.clone()), ExportOptions)
			.unwrap();
		assert!(r.report.messages.iter().any(|m| m.contains("dummy export")));
		let _ = std::fs::remove_file(&out);
	}
}
