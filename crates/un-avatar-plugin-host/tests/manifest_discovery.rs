//! `plugins/sample-io-plugin` の manifest を捜査できることだけホスト側で確認する。

use std::path::Path;

use un_avatar_plugin_host::{discover_manifests_in_dir, load_manifest};

#[test]
fn discover_finds_workspace_sample_manifest() {
	let plugins = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/sample-io-plugin");
	let list = discover_manifests_in_dir(&plugins).unwrap();
	assert_eq!(list.len(), 1);
	assert!(list[0].file_name().and_then(|n| n.to_str()) == Some("manifest.toml"));
	let m = load_manifest(&list[0]).unwrap();
	assert_eq!(m.id, "network.usagi.un_avatar.plugin.sample_io");
}
