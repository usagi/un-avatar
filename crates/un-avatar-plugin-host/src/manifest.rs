//! `docs/crate-io-plugin-plan.md` §9.4 manifest の読み込み（bootstrap）。
//!
//! オンディスクは [`development-guidelines.md`](../../docs/development-guidelines.md) どおり **TOML 優先**。

use std::{
	fs, io,
	path::{Path, PathBuf},
};

use serde::Deserialize;

/// manifest の `formats[]` の 1 要素（§9.4 寄り・bootstrap）。
#[derive(Clone, Debug, Deserialize)]
pub struct PluginManifestFormat {
	pub id: String,
	#[serde(default)]
	pub extensions: Vec<String>,
	#[serde(default)]
	pub can_import: bool,
	#[serde(default)]
	pub can_export: bool,
}

/// プラグイン manifest の論理スキーマ（TOML / JSON 共通）。
#[derive(Clone, Debug, Deserialize)]
pub struct PluginManifest {
	#[serde(default)]
	pub schema_version: String,
	pub id: String,
	#[serde(default)]
	pub name: String,
	#[serde(default)]
	pub version: String,
	#[serde(default)]
	pub vendor: String,
	pub entry: String,
	#[serde(default)]
	pub protocol: String,
	#[serde(default)]
	pub formats: Vec<PluginManifestFormat>,
}

impl PluginManifest {
	/// `can_import: true` の最初の形式（なければ `None`）。
	pub fn primary_import_format(&self) -> Option<&PluginManifestFormat> {
		self.formats.iter().find(|f| f.can_import)
	}

	/// `can_export: true` の最初の形式（なければ `None`）。
	pub fn primary_export_format(&self) -> Option<&PluginManifestFormat> {
		self.formats.iter().find(|f| f.can_export)
	}
}

#[derive(Debug)]
pub enum ManifestError {
	Io(std::io::Error),
	Json(serde_json::Error),
	Toml(toml::de::Error),
}

impl std::fmt::Display for ManifestError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			ManifestError::Io(e) => write!(f, "{e}"),
			ManifestError::Json(e) => write!(f, "{e}"),
			ManifestError::Toml(e) => write!(f, "{e}"),
		}
	}
}

impl std::error::Error for ManifestError {
	fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
		match self {
			ManifestError::Io(e) => Some(e),
			ManifestError::Json(e) => Some(e),
			ManifestError::Toml(e) => Some(e),
		}
	}
}

impl From<std::io::Error> for ManifestError {
	fn from(e: std::io::Error) -> Self {
		ManifestError::Io(e)
	}
}

impl From<serde_json::Error> for ManifestError {
	fn from(e: serde_json::Error) -> Self {
		ManifestError::Json(e)
	}
}

impl From<toml::de::Error> for ManifestError {
	fn from(e: toml::de::Error) -> Self {
		ManifestError::Toml(e)
	}
}

fn load_manifest_from_str(text: &str, path: &Path) -> Result<PluginManifest, ManifestError> {
	let lossy = path.to_string_lossy().to_lowercase();
	if lossy.ends_with(".json") {
		return Ok(serde_json::from_str(text)?);
	}
	if lossy.ends_with(".toml") {
		return Ok(toml::from_str(text)?);
	}
	// 拡張子なし等: TOML を先に試し、失敗したら JSON
	if let Ok(m) = toml::from_str::<PluginManifest>(text) {
		return Ok(m);
	}
	Ok(serde_json::from_str(text)?)
}

/// `path` から manifest を読み、[`PluginManifest`] にパースする（`.toml` / `.json`。その他は TOML 優先の二段試行）。
pub fn load_manifest(path: &Path) -> Result<PluginManifest, ManifestError> {
	let text = fs::read_to_string(path)?;
	load_manifest_from_str(&text, path)
}

/// 子ディレクトリ直下の **`manifest.toml` を優先**。無ければ `manifest.json`（非再帰・1 プラグイン 1 ファイル）。
pub(crate) fn discover_manifest_in_dir(dir: &Path) -> io::Result<Option<PathBuf>> {
	let tom = dir.join("manifest.toml");
	if tom.is_file() {
		return Ok(Some(tom));
	}
	let js = dir.join("manifest.json");
	if js.is_file() {
		return Ok(Some(js));
	}
	Ok(None)
}

/// 子ディレクトリ直下の **`manifest.toml` を優先**。無ければ `manifest.json`（非再帰・1 プラグイン 1 ファイル）。
pub fn discover_manifests_in_dir(dir: &Path) -> io::Result<Vec<PathBuf>> {
	let mut out = Vec::with_capacity(1);
	if let Some(path) = discover_manifest_in_dir(dir)? {
		out.push(path);
	}
	Ok(out)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn load_sample_json_fixture() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
		let m = load_manifest(&dir.join("sample.manifest.json")).unwrap();
		assert_eq!(m.schema_version, "0.1");
		assert!(m.id.contains("sample"));
		assert_eq!(m.protocol, "stdio-json-rpc");
		assert!(!m.entry.is_empty());
		assert_eq!(
			m.primary_import_format().map(|f| f.id.as_str()),
			Some("io.un-avatar.example.avatar")
		);
		assert_eq!(
			m.primary_export_format().map(|f| f.id.as_str()),
			Some("io.un-avatar.example.avatar")
		);
	}

	#[test]
	fn load_sample_toml_fixture() {
		let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
		let m = load_manifest(&dir.join("sample.manifest.toml")).unwrap();
		assert_eq!(
			m.primary_import_format().map(|f| f.id.as_str()),
			Some("io.un-avatar.example.avatar")
		);
		assert_eq!(
			m.primary_export_format().map(|f| f.id.as_str()),
			Some("io.un-avatar.example.avatar")
		);
	}
}
