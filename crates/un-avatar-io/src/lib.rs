//! UN Avatar — IO プラグイン境界（Phase 0.3 / crate-io §6）。
//!
//! `AvatarImporter` / `AvatarExporter` および関連型の bootstrap。

#![forbid(unsafe_code)]

use std::{
	path::{Path, PathBuf},
	sync::Arc,
};

use serde::{Deserialize, Serialize};
pub use un_avatar_core::{
	Approximation, ExportReport, ImportReport, LostFeature, PreservedExtension, ReportMessage, ReportSeverity, ReportStatus, UnaDocument,
};
pub use un_avatar_types::FormatId;

/// 形式の入出力方向（§6.4 `FormatDirection`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatDirection {
	Import,
	Export,
	ImportExport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginStability {
	Stable,
	Experimental,
}

/// フィーチャ単位の扱い（§6.5）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
	Unsupported,
	ImportOnly,
	ExportOnly,
	ImportExport,
	Approximate,
	PreserveOnly,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FormatCapabilities {
	pub mesh: Capability,
	pub skeleton: Capability,
	pub skinning: Capability,
	pub animation: Capability,
	pub expression: Capability,
	pub material: Capability,
	pub physics: Capability,
	pub cameras: Capability,
	pub lights: Capability,
	pub custom_extensions: Capability,
}

impl Default for FormatCapabilities {
	fn default() -> Self {
		Self {
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
		}
	}
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FormatDescriptor {
	pub id: FormatId,
	pub display_name: String,
	pub extensions: Vec<String>,
	pub media_types: Vec<String>,
	pub direction: FormatDirection,
	pub capabilities: FormatCapabilities,
	pub stability: PluginStability,
	/// manifest のトップレベル `id`（stdio プラグイン由来の形式のとき）。組み込みのみは `None`。JSON では未設定時にキーを省略。
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub provider_plugin_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportCapability {
	Unsupported,
	Supported,
}

#[derive(Default)]
pub struct ImportProbe {
	/// 拡張子・`.una.d` 判定に使うパス（なければ probe は低信頼度）。
	pub path_hint: Option<PathBuf>,
	/// 呼び出し側がすでに読んだ入力bytes。GLB/VRM判定の二重readを避けるために使う。
	pub bytes: Option<Arc<[u8]>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImportProbeResult {
	pub confidence: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportInput {
	Path(PathBuf),
	Bytes { bytes: Arc<[u8]>, path_hint: Option<PathBuf> },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportOptions;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportOptions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportOutput {
	Path(PathBuf),
}

pub fn path_has_format_extension(path: &str, extension: &str) -> bool {
	if extension.is_empty() {
		return path.ends_with('.');
	}
	path.len() > extension.len() && path.as_bytes().get(path.len() - extension.len() - 1) == Some(&b'.') && path.ends_with(extension)
}

pub struct ImportContext {
	pub asset_root: PathBuf,
	pub temp_dir: PathBuf,
	pub initial_wardrobe_set: Option<String>,
	pub defer_initial_image_decode: bool,
}

impl ImportContext {
	pub fn dummy() -> Self {
		Self {
			asset_root: PathBuf::from("."),
			temp_dir: PathBuf::from("."),
			initial_wardrobe_set: None,
			defer_initial_image_decode: false,
		}
	}
}

pub struct ExportContext {
	pub output_root: PathBuf,
	pub temp_dir: PathBuf,
}

impl ExportContext {
	pub fn dummy() -> Self {
		Self {
			output_root: PathBuf::from("."),
			temp_dir: PathBuf::from("."),
		}
	}
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ImportResult {
	pub document: UnaDocument,
	pub report: ImportReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExportResult {
	pub report: ExportReport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportError {
	Message(String),
}

impl std::fmt::Display for ImportError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ImportError::Message(s) => write!(f, "{s}"),
		}
	}
}

impl std::error::Error for ImportError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExportError {
	Message(String),
}

impl std::fmt::Display for ExportError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ExportError::Message(s) => write!(f, "{s}"),
		}
	}
}

impl std::error::Error for ExportError {}

pub trait AvatarImporter {
	fn descriptor(&self) -> FormatDescriptor;
	fn probe(&self, input: &ImportProbe) -> ImportProbeResult;
	fn import(&self, ctx: &mut ImportContext, input: ImportInput, options: ImportOptions) -> Result<ImportResult, ImportError>;
}

pub trait AvatarExporter {
	fn descriptor(&self) -> FormatDescriptor;
	fn can_export(&self, document: &UnaDocument, options: &ExportOptions) -> ExportCapability;
	fn export(
		&self,
		ctx: &mut ExportContext,
		document: &UnaDocument,
		output: ExportOutput,
		options: ExportOptions,
	) -> Result<ExportResult, ExportError>;
}

/// 登録済み built-in / プラグイン IO の最小レジストリ（`crate-io-plugin-plan.md` Phase 2.1 の土台）。
#[derive(Default)]
pub struct IoRegistry {
	importers: Vec<Box<dyn AvatarImporter + Send + Sync>>,
	exporters: Vec<Box<dyn AvatarExporter + Send + Sync>>,
}

impl IoRegistry {
	pub fn new() -> Self {
		Self::default()
	}

	/// 末尾に importer を追加する。同一 [`FormatId`] が複数あっても [`Self::importer_by_id`] / [`Self::best_importer_for`] は**先に登録されたもの**を使う。
	pub fn register_importer(&mut self, importer: Box<dyn AvatarImporter + Send + Sync>) {
		self.importers.push(importer);
	}

	/// 末尾に exporter を追加する。同一 [`FormatId`] が複数あっても [`Self::exporter_by_id`] / [`Self::best_exporter_for`] は**先に登録されたもの**を使う。
	pub fn register_exporter(&mut self, exporter: Box<dyn AvatarExporter + Send + Sync>) {
		self.exporters.push(exporter);
	}

	pub fn importers(&self) -> &[Box<dyn AvatarImporter + Send + Sync>] {
		&self.importers
	}

	pub fn exporters(&self) -> &[Box<dyn AvatarExporter + Send + Sync>] {
		&self.exporters
	}

	pub fn importer_descriptors(&self) -> Vec<FormatDescriptor> {
		let mut descriptors = Vec::with_capacity(self.importers.len());
		descriptors.extend(self.importers.iter().map(|i| i.descriptor()));
		descriptors
	}

	pub fn exporter_descriptors(&self) -> Vec<FormatDescriptor> {
		let mut descriptors = Vec::with_capacity(self.exporters.len());
		descriptors.extend(self.exporters.iter().map(|e| e.descriptor()));
		descriptors
	}
	pub fn probe_importers(&self, probe: &ImportProbe) -> Vec<(FormatId, ImportProbeResult)> {
		let mut results = Vec::with_capacity(self.importers.len());
		results.extend(self.importers.iter().map(|i| {
			let desc = i.descriptor();
			(desc.id, i.probe(probe))
		}));
		results
	}

	/// `confidence` が最大の importer。0 のみなら `None`。同点では**先に登録**されたものを残す。
	pub fn best_importer_for(&self, probe: &ImportProbe) -> Option<&dyn AvatarImporter> {
		let mut best: Option<(&dyn AvatarImporter, u8)> = None;
		for imp in &self.importers {
			let confidence = imp.probe(probe).confidence;
			if confidence == 0 {
				continue;
			}
			match best {
				None => best = Some((imp.as_ref(), confidence)),
				Some((_, c0)) if confidence > c0 => best = Some((imp.as_ref(), confidence)),
				_ => {}
			}
		}
		best.map(|(i, _)| i)
	}

	/// [`FormatId`] が一致する importer。同 ID が複数なら**先に登録**されたもの。
	pub fn importer_by_id(&self, id: &FormatId) -> Option<&dyn AvatarImporter> {
		self.importers
			.iter()
			.find(|i| &i.descriptor().id == id)
			.map(|b| b.as_ref() as &dyn AvatarImporter)
	}

	/// [`FormatId`] が一致する exporter。同 ID が複数なら**先に登録**されたもの。
	pub fn exporter_by_id(&self, id: &FormatId) -> Option<&dyn AvatarExporter> {
		self.exporters
			.iter()
			.find(|e| &e.descriptor().id == id)
			.map(|b| b.as_ref() as &dyn AvatarExporter)
	}

	/// 出力パスに対し `can_export` が [`ExportCapability::Supported`] の exporter のうち、
	/// [`FormatDescriptor`] の extensions がパス末尾と一致するものを優先。なければ**先に登録**された Supported。
	pub fn best_exporter_for<'a>(&'a self, document: &UnaDocument, output: &Path) -> Option<&'a dyn AvatarExporter> {
		let path_str = output.as_os_str().to_string_lossy();
		let mut first_supported: Option<&'a dyn AvatarExporter> = None;
		for e in &self.exporters {
			if e.can_export(document, &ExportOptions) != ExportCapability::Supported {
				continue;
			}
			if first_supported.is_none() {
				first_supported = Some(e.as_ref());
			}
			let desc = e.descriptor();
			for ext in &desc.extensions {
				if path_has_format_extension(&path_str, ext) {
					return Some(e.as_ref());
				}
			}
		}
		first_supported
	}
}

/// スモーク・テスト用の最小 Importer。
#[derive(Clone, Copy, Debug, Default)]
pub struct DummyImporter;

impl AvatarImporter for DummyImporter {
	fn descriptor(&self) -> FormatDescriptor {
		FormatDescriptor {
			id: FormatId::new("io.un-avatar.dummy"),
			display_name: "Dummy".to_owned(),
			extensions: vec!["dummy".to_owned()],
			media_types: Vec::new(),
			direction: FormatDirection::Import,
			capabilities: FormatCapabilities::default(),
			stability: PluginStability::Experimental,
			provider_plugin_id: None,
		}
	}

	fn probe(&self, _input: &ImportProbe) -> ImportProbeResult {
		ImportProbeResult { confidence: 0 }
	}

	fn import(&self, _ctx: &mut ImportContext, _input: ImportInput, _options: ImportOptions) -> Result<ImportResult, ImportError> {
		Ok(ImportResult {
			document: UnaDocument::default(),
			report: ImportReport::default(),
		})
	}
}

/// スモーク・テスト用の最小 Exporter。
#[derive(Clone, Copy, Debug, Default)]
pub struct DummyExporter;

impl AvatarExporter for DummyExporter {
	fn descriptor(&self) -> FormatDescriptor {
		FormatDescriptor {
			id: FormatId::new("io.un-avatar.dummy"),
			display_name: "Dummy".to_owned(),
			extensions: vec!["dummy".to_owned()],
			media_types: Vec::new(),
			direction: FormatDirection::Export,
			capabilities: FormatCapabilities::default(),
			stability: PluginStability::Experimental,
			provider_plugin_id: None,
		}
	}

	fn can_export(&self, _document: &UnaDocument, _options: &ExportOptions) -> ExportCapability {
		ExportCapability::Supported
	}

	fn export(
		&self,
		_ctx: &mut ExportContext,
		_document: &UnaDocument,
		_output: ExportOutput,
		_options: ExportOptions,
	) -> Result<ExportResult, ExportError> {
		Ok(ExportResult {
			report: ExportReport::default(),
		})
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::path::Path;

	#[test]
	fn importer_and_exporter_by_id_find_dummy() {
		let mut reg = IoRegistry::new();
		reg.register_importer(Box::new(DummyImporter));
		reg.register_exporter(Box::new(DummyExporter));
		let id = FormatId::new("io.un-avatar.dummy");
		assert!(reg.importer_by_id(&id).is_some());
		assert!(reg.exporter_by_id(&id).is_some());
		assert!(reg.importer_by_id(&FormatId::new("io.none")).is_none());
	}

	#[test]
	fn path_extension_match_requires_dot_boundary() {
		assert!(path_has_format_extension("out.dummy", "dummy"));
		assert!(path_has_format_extension("out.una.d", "una.d"));
		assert!(!path_has_format_extension("out.notdummy", "dummy"));
		assert!(!path_has_format_extension("dummy", "dummy"));
	}

	#[test]
	fn best_exporter_prefers_path_extension_match() {
		let mut reg = IoRegistry::new();
		reg.register_exporter(Box::new(DummyExporter));
		let doc = UnaDocument::default();
		let e = reg.best_exporter_for(&doc, Path::new("out.dummy")).expect("exporter");
		assert_eq!(e.descriptor().id.0, "io.un-avatar.dummy");
		assert!(reg.best_exporter_for(&doc, Path::new("out.unknown")).is_some());
	}

	#[test]
	fn io_registry_best_importer_respects_confidence() {
		let mut reg = IoRegistry::new();
		reg.register_importer(Box::new(DummyImporter));
		let none = reg.best_importer_for(&ImportProbe::default());
		assert!(none.is_none());

		let mut reg2 = IoRegistry::new();
		reg2.register_importer(Box::new(DummyImporter));
		reg2.register_importer(Box::new(DummyImporter));
		assert_eq!(reg2.importer_descriptors().len(), 2);
	}

	#[test]
	fn dummy_importer_returns_empty_document() {
		let imp = DummyImporter;
		let mut ctx = ImportContext::dummy();
		let r = imp
			.import(&mut ctx, ImportInput::Path(PathBuf::from("x.dummy")), ImportOptions)
			.unwrap();
		assert_eq!(r.document, UnaDocument::default());
	}

	#[test]
	fn dummy_exporter_succeeds() {
		let exp = DummyExporter;
		let mut ctx = ExportContext::dummy();
		let r = exp
			.export(
				&mut ctx,
				&UnaDocument::default(),
				ExportOutput::Path(PathBuf::from("out.dummy")),
				ExportOptions,
			)
			.unwrap();
		assert_eq!(r.report, ExportReport::default());
	}
}
