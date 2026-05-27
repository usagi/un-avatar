//! このパッケージの `sample-io-plugin` バイナリと [`un_avatar_plugin_host::PluginChild`] の結合テスト。

use std::path::{Path, PathBuf};

use un_avatar_io::{ExportResult, ImportResult, ReportStatus, UnaDocument};
use un_avatar_plugin_host::{PluginChild, PROTOCOL_VERSION};

fn sample_exe() -> PathBuf {
	let key = "CARGO_BIN_EXE_sample_io_plugin";
	if let Some(p) = std::env::var_os(key) {
		let pb = PathBuf::from(p);
		if pb.is_file() {
			return pb;
		}
	}
	let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
	let file = if cfg!(windows) {
		"sample-io-plugin.exe"
	} else {
		"sample-io-plugin"
	};
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target").join(profile).join(file)
}

#[test]
fn handshake() {
	let exe = sample_exe();
	assert!(exe.is_file(), "expected binary at {:?}", exe);
	let mut p = PluginChild::spawn(&exe).expect("spawn");
	let ack = p.handshake().expect("handshake");
	assert_eq!(ack.protocol_version, PROTOCOL_VERSION);
	assert_eq!(ack.plugin_id, "network.usagi.un_avatar.plugin.sample_io");
	let _ = p.kill();
}

#[test]
fn import_returns_typed_import_result() {
	let exe = sample_exe();
	assert!(exe.is_file(), "{:?}", exe);
	let mut p = PluginChild::spawn(&exe).unwrap();
	p.handshake().unwrap();
	let tmp = std::env::temp_dir().join("un-avatar-sample-dummy.exampleavatar");
	let got: ImportResult = p.rpc_import_path(&tmp).unwrap();
	assert_eq!(got.document, UnaDocument::default());
	assert_eq!(
		got.report.source_format.as_ref().map(|x| x.0.as_str()),
		Some("io.un-avatar.example.avatar")
	);
	assert_eq!(got.report.status, ReportStatus::Success);
	assert!(got.report.messages.iter().any(|m| m.contains("sample-io-plugin")));
	let _ = p.kill();
}

#[test]
fn export_returns_typed_export_result() {
	let exe = sample_exe();
	assert!(exe.is_file(), "{:?}", exe);
	let mut p = PluginChild::spawn(&exe).unwrap();
	p.handshake().unwrap();
	let tmp = std::env::temp_dir().join(format!("un-avatar-sample-export-{}.exampleavatar", std::process::id()));
	let got: ExportResult = p.rpc_export_path(&tmp, &UnaDocument::default()).unwrap();
	assert_eq!(
		got.report.target_format.as_ref().map(|x| x.0.as_str()),
		Some("io.un-avatar.example.avatar")
	);
	assert_eq!(got.report.status, ReportStatus::Success);
	assert!(got.report.messages.iter().any(|m| m.contains("dummy export")));
	let _ = p.kill();
}
