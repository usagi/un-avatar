//! UN Avatar — 外部プラグイン子プロセスと **stdio 上の JSON-RPC（1 行 1 メッセージ）** で会話するホスト。
//!
//! 設計: `docs/crate-io-plugin-plan.md` Commit 2.4 / §9.3

#![forbid(unsafe_code)]

mod manifest;
mod stdio_exporter;
mod stdio_importer;
mod stdio_rpc;

pub use manifest::{ManifestError, PluginManifest, PluginManifestFormat, discover_manifests_in_dir, load_manifest};
pub use stdio_exporter::{
	StdioExporterError, StdioJsonRpcExporter, register_stdio_exporters_from_manifest_dir, register_stdio_exporters_from_plugin_root,
};
pub use stdio_importer::{
	StdioImporterError, StdioJsonRpcImporter, plugin_child_uses_bundle_cwd_from_env, plugin_discovery_max_depth_from_env,
	register_stdio_importers_from_manifest_dir, register_stdio_importers_from_plugin_root,
};
pub use stdio_rpc::{
	DEFAULT_RPC_READ_TIMEOUT, HandshakeError, InitializeAck, PROTOCOL_VERSION, PluginChild, RpcError, rpc_export_timeout_from_env,
	rpc_handshake_timeout_from_env, rpc_import_timeout_from_env, rpc_read_timeout_from_env, rpc_session_wall_from_env,
};
