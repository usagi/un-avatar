//! Commit 2.5: 外部プロセスプラグイン — `initialize`、dummy `import` / `export`（JSON-RPC）。
#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufWriter, Write};

use serde::Deserialize;
use serde_json::Value;
use un_avatar_io::{ExportReport, ExportResult, FormatId, ImportReport, ImportResult, ReportStatus, UnaDocument};

const PROTOCOL_VERSION: &str = "0.1";
const PLUGIN_ID: &str = "network.usagi.un_avatar.plugin.sample_io";
const SAMPLE_FORMAT_ID: &str = "io.un-avatar.example.avatar";

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
	#[allow(dead_code)]
	jsonrpc: String,
	method: String,
	#[serde(default)]
	#[allow(dead_code)]
	params: Option<Value>,
	id: Value,
}

fn export_dummy(path: &str, _document: &UnaDocument) -> ExportResult {
	let mut report = ExportReport {
		target_format: Some(FormatId::new(SAMPLE_FORMAT_ID)),
		status: ReportStatus::Success,
		..Default::default()
	};
	report.push_info(format!("sample-io-plugin: dummy export to path {path:?}"));
	ExportResult { report }
}

fn import_dummy(path: &str) -> ImportResult {
	let mut report = ImportReport {
		source_format: Some(FormatId::new(SAMPLE_FORMAT_ID)),
		status: ReportStatus::Success,
		..Default::default()
	};
	let emit_cwd = std::env::var_os("SAMPLE_IO_PLUGIN_EMIT_CWD")
		.map(|v| v.to_string_lossy().trim().to_string())
		.is_some_and(|s| s == "1");
	if emit_cwd {
		let cwd = std::env::current_dir()
			.map(|p| p.display().to_string())
			.unwrap_or_else(|e| format!("(getcwd error: {e})"));
		report.push_info(format!("sample-io-plugin: cwd={cwd}"));
	}
	report.push_info(format!("sample-io-plugin: dummy import from path {path:?}"));
	ImportResult {
		document: UnaDocument::default(),
		report,
	}
}

fn main() -> io::Result<()> {
	let stdin = io::stdin().lock();
	let mut stdout = BufWriter::new(io::stdout());
	for line in stdin.lines() {
		let line = line?;
		let line = line.trim();
		if line.is_empty() {
			continue;
		}
		let req: JsonRpcRequest = match serde_json::from_str(line) {
			Ok(r) => r,
			Err(_) => continue,
		};
		let response = match req.method.as_str() {
			"initialize" => serde_json::json!({
				"jsonrpc": "2.0",
				"id": req.id,
				"result": {
					"protocol_version": PROTOCOL_VERSION,
					"plugin_id": PLUGIN_ID,
				}
			}),
			"export" => {
				let params = req.params.as_ref();
				let path = params.and_then(|p| p.get("path")).and_then(|v| v.as_str()).unwrap_or("");
				let document = params
					.and_then(|p| p.get("document"))
					.and_then(|v| serde_json::from_value::<UnaDocument>(v.clone()).ok())
					.unwrap_or_default();
				match serde_json::to_value(export_dummy(path, &document)) {
					Ok(result) => serde_json::json!({
						"jsonrpc": "2.0",
						"id": req.id,
						"result": result,
					}),
					Err(e) => serde_json::json!({
						"jsonrpc": "2.0",
						"id": req.id,
						"error": { "code": -32603, "message": e.to_string() }
					}),
				}
			}
			"import" => {
				let path = req
					.params
					.as_ref()
					.and_then(|p| p.get("path"))
					.and_then(|v| v.as_str())
					.unwrap_or("");
				match serde_json::to_value(import_dummy(path)) {
					Ok(result) => serde_json::json!({
						"jsonrpc": "2.0",
						"id": req.id,
						"result": result,
					}),
					Err(e) => serde_json::json!({
						"jsonrpc": "2.0",
						"id": req.id,
						"error": { "code": -32603, "message": e.to_string() }
					}),
				}
			}
			other => serde_json::json!({
				"jsonrpc": "2.0",
				"id": req.id,
				"error": { "code": -32601, "message": format!("Method not found: {other}") }
			}),
		};
		writeln!(stdout, "{}", serde_json::to_string(&response).unwrap())?;
		stdout.flush()?;
	}
	Ok(())
}
