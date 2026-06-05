//! UNA 形式 v0 bootstrap（単一 TOML）。
//!
//! - **`.una`**: v0 では UTF-8 TOML 本文と同一形式。
//! - **`.una.d/`**: ディレクトリ内 [`UNA_DIR_MANIFEST`]（既定 `manifest.toml`）。
//!
//! 設計の正本: `docs/crate-io-plugin-plan.md` §4.17

#![forbid(unsafe_code)]

use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use un_avatar_core::UnaDocument;
use un_avatar_io::{
	AvatarExporter, AvatarImporter, Capability, ExportCapability, ExportContext, ExportError, ExportOptions, ExportOutput, ExportReport,
	ExportResult, FormatCapabilities, FormatDescriptor, FormatDirection, FormatId, ImportContext, ImportError, ImportInput, ImportOptions,
	ImportProbe, ImportProbeResult, ImportReport, ImportResult, IoRegistry, PluginStability, ReportStatus,
};

/// 現行 bootstrap スキーマの `format_version`。
pub const UNA_FORMAT_VERSION_V0: u32 = 1;

/// `.una.d/` バンドル内のマニフェスト相対パス名。
pub const UNA_DIR_MANIFEST: &str = "manifest.toml";

/// v0 ファイル全体（TOML ルート）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnaFileV0 {
	pub format_version: u32,
	pub scene: UnaSceneV0,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnaSceneV0 {
	/// v0 では `true` のみサポート（空シーン）。
	#[serde(default = "scene_empty_default")]
	pub empty: bool,
}

fn scene_empty_default() -> bool {
	true
}

impl Default for UnaSceneV0 {
	fn default() -> Self {
		Self { empty: true }
	}
}

impl Default for UnaFileV0 {
	fn default() -> Self {
		Self {
			format_version: UNA_FORMAT_VERSION_V0,
			scene: UnaSceneV0::default(),
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnaIoError {
	UnsupportedVersion { expected: u32, got: u32 },
	Unsupported(&'static str),
	Toml(String),
	Io(String),
}

impl std::fmt::Display for UnaIoError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			UnaIoError::UnsupportedVersion { expected, got } => {
				write!(f, "UNA format_version: expected {expected}, got {got}")
			}
			UnaIoError::Unsupported(s) => write!(f, "unsupported: {s}"),
			UnaIoError::Toml(s) => write!(f, "toml: {s}"),
			UnaIoError::Io(s) => write!(f, "io: {s}"),
		}
	}
}

impl std::error::Error for UnaIoError {}

impl UnaFileV0 {
	pub fn validate_format_version(&self) -> Result<(), UnaIoError> {
		if self.format_version != UNA_FORMAT_VERSION_V0 {
			return Err(UnaIoError::UnsupportedVersion {
				expected: UNA_FORMAT_VERSION_V0,
				got: self.format_version,
			});
		}
		Ok(())
	}

	/// 現在の [`UnaDocument`] から v0 ファイルを生成（中身は常に空シーン相当）。
	pub fn from_una_document(_doc: &UnaDocument) -> Self {
		Self::default()
	}

	/// v0 TOML から [`UnaDocument`] へ。空シーン以外は未実装。
	pub fn to_una_document(&self) -> Result<UnaDocument, UnaIoError> {
		self.validate_format_version()?;
		if !self.scene.empty {
			return Err(UnaIoError::Unsupported("scene.empty = false は未実装"));
		}
		Ok(UnaDocument::default())
	}
}

pub fn parse_una_toml_str(s: &str) -> Result<UnaFileV0, UnaIoError> {
	let v: UnaFileV0 = toml::from_str(s).map_err(|e| UnaIoError::Toml(e.to_string()))?;
	v.validate_format_version()?;
	Ok(v)
}

pub fn serialize_una_toml(file: &UnaFileV0) -> Result<String, UnaIoError> {
	toml::to_string_pretty(file).map_err(|e| UnaIoError::Toml(e.to_string()))
}

/// UTF-8 `.una` など、TOML 1 ファイルを読む。
pub fn read_una_path(path: &Path) -> Result<UnaFileV0, UnaIoError> {
	let text = fs::read_to_string(path).map_err(|e| UnaIoError::Io(e.to_string()))?;
	parse_una_toml_str(&text)
}

pub fn write_una_path(path: &Path, file: &UnaFileV0) -> Result<(), UnaIoError> {
	let s = serialize_una_toml(file)?;
	fs::write(path, s).map_err(|e| UnaIoError::Io(e.to_string()))
}

/// `.una.d/` — ディレクトリ内のマニフェストを読む。
pub fn read_una_dir(dir: &Path) -> Result<UnaFileV0, UnaIoError> {
	read_una_path(&dir.join(UNA_DIR_MANIFEST))
}

/// `.una.d/` — ディレクトリと `manifest.toml` を書く。
pub fn write_una_dir(dir: &Path, file: &UnaFileV0) -> Result<(), UnaIoError> {
	fs::create_dir_all(dir).map_err(|e| UnaIoError::Io(e.to_string()))?;
	write_una_path(&dir.join(UNA_DIR_MANIFEST), file)
}

/// パスが `.una.d` で終わるか（バンドル出力先のヒント）。
pub fn path_looks_una_bundle(path: &Path) -> bool {
	path.as_os_str().to_string_lossy().ends_with(".una.d")
}

/// パスが `.una` で終わるか（単一ファイル）。
pub fn path_looks_una_file(path: &Path) -> bool {
	let s = path.as_os_str().to_string_lossy();
	s.ends_with(".una") && !s.ends_with(".una.d")
}

/// ディレクトリならマニフェスト、そうでなければ単一ファイルとして読む。
pub fn read_una_any(path: &Path) -> Result<UnaFileV0, UnaIoError> {
	if path.is_dir() {
		read_una_dir(path)
	} else {
		read_una_path(path)
	}
}

/// 出力先が既存ディレクトリか `.una.d` で終わるならバンドル、それ以外は単一 `.una` ファイル。
pub fn write_una_output(path: &Path, file: &UnaFileV0) -> Result<(), UnaIoError> {
	if path.is_dir() || path_looks_una_bundle(path) {
		return write_una_dir(path, file);
	}
	if let Some(parent) = path.parent() {
		if !parent.as_os_str().is_empty() {
			fs::create_dir_all(parent).map_err(|e| UnaIoError::Io(e.to_string()))?;
		}
	}
	write_una_path(path, file)
}

fn probe_una_path_hint(path_hint: Option<&Path>) -> ImportProbeResult {
	let Some(p) = path_hint else {
		return ImportProbeResult { confidence: 0 };
	};
	if p.is_file() {
		return if path_looks_una_file(p) {
			ImportProbeResult { confidence: 255 }
		} else {
			ImportProbeResult { confidence: 0 }
		};
	}
	if p.is_dir() {
		let manifest = p.join(UNA_DIR_MANIFEST);
		return if path_looks_una_bundle(p) || manifest.is_file() {
			ImportProbeResult { confidence: 250 }
		} else {
			ImportProbeResult { confidence: 0 }
		};
	}
	if path_looks_una_file(p) || path_looks_una_bundle(p) {
		return ImportProbeResult { confidence: 128 };
	}
	ImportProbeResult { confidence: 0 }
}

fn una_io_to_import(e: UnaIoError) -> ImportError {
	ImportError::Message(e.to_string())
}

fn una_io_to_export(e: UnaIoError) -> ExportError {
	ExportError::Message(e.to_string())
}

fn una_format_id() -> FormatId {
	FormatId::new("io.un-avatar.una")
}

fn una_descriptor(direction: FormatDirection) -> FormatDescriptor {
	FormatDescriptor {
		id: una_format_id(),
		display_name: "UNA (v0)".to_owned(),
		extensions: vec!["una".to_owned(), "una.d".to_owned()],
		media_types: Vec::new(),
		direction,
		capabilities: FormatCapabilities {
			mesh: Capability::Unsupported,
			skeleton: Capability::Unsupported,
			skinning: Capability::Unsupported,
			animation: Capability::Unsupported,
			expression: Capability::Unsupported,
			material: Capability::Unsupported,
			physics: Capability::Unsupported,
			cameras: Capability::Unsupported,
			lights: Capability::Unsupported,
			custom_extensions: Capability::Unsupported,
		},
		stability: PluginStability::Experimental,
		provider_plugin_id: None,
	}
}

fn push_una_v0_export_loss_warnings(report: &mut ExportReport, document: &UnaDocument) {
	if document.scene.is_some() {
		report.push_info("注意: UnaDocument にシーンデータがありますが UNA v0 では空シーン TOML のみ出力します（シーンは失われます）");
	}
	if document.vrm.is_some() {
		report.push_info("注意: VRM 拡張メタデータは UNA v0 には書き出されません");
	}
	if document.humanoid_profile.is_some() {
		report.push_info("注意: Humanoid プロファイルは UNA v0 には書き出されません");
	}
	if document.expression_catalog.is_some() || document.expression_weights.is_some() {
		report.push_info("注意: 表情カタログ／ウェイトは UNA v0 には書き出されません");
	}
}

/// UNA v0 を読み込む built-in [`AvatarImporter`]。
#[derive(Clone, Copy, Debug, Default)]
pub struct UnaFormatImporter;

impl AvatarImporter for UnaFormatImporter {
	fn descriptor(&self) -> FormatDescriptor {
		una_descriptor(FormatDirection::Import)
	}

	fn probe(&self, input: &ImportProbe) -> ImportProbeResult {
		probe_una_path_hint(input.path_hint.as_deref())
	}

	fn import(&self, _ctx: &mut ImportContext, input: ImportInput, _options: ImportOptions) -> Result<ImportResult, ImportError> {
		let path = match input {
			ImportInput::Path(path) => path,
			ImportInput::Bytes { .. } => return Err(ImportError::Message("UNA import requires a filesystem path".into())),
		};
		let file = read_una_any(&path).map_err(una_io_to_import)?;
		let document = file.to_una_document().map_err(una_io_to_import)?;
		let mut report = ImportReport {
			source_format: Some(una_format_id()),
			status: ReportStatus::Success,
			..Default::default()
		};
		report.push_info(format!(
			"UNA v0: format_version={}, scene.empty={} → UnaDocument（bootstrap）",
			file.format_version, file.scene.empty
		));
		report.push_info(format!("source: {}", path.display()));
		Ok(ImportResult { document, report })
	}
}

/// UNA v0 を書き出す built-in [`AvatarExporter`]。
#[derive(Clone, Copy, Debug, Default)]
pub struct UnaFormatExporter;

impl AvatarExporter for UnaFormatExporter {
	fn descriptor(&self) -> FormatDescriptor {
		una_descriptor(FormatDirection::Export)
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
		let mut report = ExportReport::default();
		push_una_v0_export_loss_warnings(&mut report, document);
		let file = UnaFileV0::from_una_document(document);
		write_una_output(&path, &file).map_err(una_io_to_export)?;
		report.target_format = Some(una_format_id());
		report.status = ReportStatus::Success;
		report.push_info(format!(
			"UNA v0: format_version={}, scene.empty={}",
			file.format_version, file.scene.empty
		));
		report.push_info(format!("destination: {}", path.display()));
		Ok(ExportResult { report })
	}
}

/// UNA を built-in として登録した [`IoRegistry`]（GUI/CLI の起点）。
pub fn io_registry_with_una() -> IoRegistry {
	let mut r = IoRegistry::new();
	r.register_importer(Box::new(UnaFormatImporter));
	r.register_exporter(Box::new(UnaFormatExporter));
	r
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::PathBuf;
	use un_avatar_io::{ExportContext, ExportOutput, FormatId, ImportContext, ImportInput, ImportOptions, ImportProbe};

	#[test]
	fn io_registry_selects_una_for_una_path() {
		let r = io_registry_with_una();
		let imp = r
			.best_importer_for(&ImportProbe {
				path_hint: Some(PathBuf::from("model.una")),
				bytes: None,
			})
			.expect("UNA importer");
		assert_eq!(imp.descriptor().id, FormatId::new("io.un-avatar.una"));
		assert_eq!(r.importer_descriptors().len(), 1);
		assert_eq!(r.exporter_descriptors().len(), 1);
	}

	#[test]
	fn importer_exporter_roundtrip_una_file() {
		let tmp = std::env::temp_dir().join(format!("un-avatar-una-io-test-{}.una", std::process::id()));
		let _ = fs::remove_file(&tmp);

		let exp = UnaFormatExporter;
		let mut xctx = ExportContext::dummy();
		let er = exp
			.export(&mut xctx, &UnaDocument::default(), ExportOutput::Path(tmp.clone()), ExportOptions)
			.unwrap();
		assert!(er.report.messages.iter().any(|m| m.contains("UNA v0")), "{:?}", er.report.messages);
		assert!(
			er.report.diagnostics.iter().any(|d| d.text.contains("UNA v0")),
			"{:?}",
			er.report.diagnostics
		);
		assert_eq!(er.report.target_format.as_ref().map(|x| x.0.as_str()), Some("io.un-avatar.una"));
		assert_eq!(er.report.status, ReportStatus::Success);

		let imp = UnaFormatImporter;
		let mut ictx = ImportContext::dummy();
		let got = imp.import(&mut ictx, ImportInput::Path(tmp.clone()), ImportOptions).unwrap();
		assert_eq!(got.document, UnaDocument::default());
		assert!(
			got.report.messages.iter().any(|m| m.contains("UNA v0")),
			"expected import report messages, got {:?}",
			got.report.messages
		);
		assert!(
			got.report.diagnostics.iter().any(|d| d.text.contains("UNA v0")),
			"{:?}",
			got.report.diagnostics
		);
		assert_eq!(got.report.source_format.as_ref().map(|x| x.0.as_str()), Some("io.un-avatar.una"));
		assert_eq!(got.report.status, ReportStatus::Success);

		let _ = fs::remove_file(&tmp);
	}

	#[test]
	fn probe_una_path_hints() {
		let imp = UnaFormatImporter;
		assert_eq!(
			imp.probe(&ImportProbe {
				path_hint: Some(PathBuf::from("model.una")),
				bytes: None,
			})
			.confidence,
			128
		);
		assert_eq!(
			imp.probe(&ImportProbe {
				path_hint: Some(std::env::temp_dir().join("nope.txt")),
				bytes: None,
			})
			.confidence,
			0
		);
	}

	#[test]
	fn toml_roundtrip_default() {
		let f = UnaFileV0::default();
		let s = serialize_una_toml(&f).unwrap();
		let g = parse_una_toml_str(&s).unwrap();
		assert_eq!(f, g);
	}

	#[test]
	fn to_una_document_empty() {
		let f = UnaFileV0::default();
		assert_eq!(f.to_una_document().unwrap(), UnaDocument::default());
	}

	#[test]
	fn una_dir_roundtrip_filesystem() {
		let dir = std::env::temp_dir().join(format!("un-avatar-una-d-test-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let f = UnaFileV0::default();
		write_una_dir(&dir, &f).unwrap();
		let g = read_una_dir(&dir).unwrap();
		assert_eq!(f, g);
		let _ = fs::remove_dir_all(&dir);
	}
}
