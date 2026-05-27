//! UN Avatar — 外部プラグイン子プロセスと **stdio 上の JSON-RPC（1 行 1 メッセージ）** で会話するホスト。
//!
//! 設計: `docs/crate-io-plugin-plan.md` Commit 2.4 / §9.3

#![forbid(unsafe_code)]

mod manifest;
mod stdio_exporter;
mod stdio_importer;
mod stdio_rpc;

pub use manifest::{discover_manifests_in_dir, load_manifest, ManifestError, PluginManifest, PluginManifestFormat};
pub use stdio_exporter::{
	register_stdio_exporters_from_manifest_dir, register_stdio_exporters_from_plugin_root, StdioExporterError, StdioJsonRpcExporter,
};
pub use stdio_importer::{
	plugin_child_uses_bundle_cwd_from_env, plugin_discovery_max_depth_from_env, register_stdio_importers_from_manifest_dir,
	register_stdio_importers_from_plugin_root, StdioImporterError, StdioJsonRpcImporter,
};
pub use stdio_rpc::{
	rpc_export_timeout_from_env, rpc_handshake_timeout_from_env, rpc_import_timeout_from_env, rpc_read_timeout_from_env,
	rpc_session_wall_from_env, HandshakeError, InitializeAck, PluginChild, RpcError, DEFAULT_RPC_READ_TIMEOUT, PROTOCOL_VERSION,
};
