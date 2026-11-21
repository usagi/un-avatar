fn main() {
	// The unit-test harness also links the native notification/dialog code.
	// Without Common Controls v6 its TaskDialogIndirect import cannot load.
	// Set only for cargo test --lib; application binaries already embed Tauri resources.
	println!("cargo:rerun-if-env-changed=UN_AVATAR_LIBTEST_MANIFEST");
	if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var_os("UN_AVATAR_LIBTEST_MANIFEST").is_some() {
		println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
		println!(
			"cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
		);
	}
	println!("cargo:rerun-if-env-changed=UN_AVATAR_FRONTEND_PREBUILT");
	println!("cargo:rerun-if-changed=../src");
	println!("cargo:rerun-if-changed=../public");
	println!("cargo:rerun-if-changed=../index.html");
	println!("cargo:rerun-if-changed=../package.json");
	println!("cargo:rerun-if-changed=../package-lock.json");
	println!("cargo:rerun-if-changed=../svelte.config.js");
	println!("cargo:rerun-if-changed=../tsconfig.json");
	println!("cargo:rerun-if-changed=../vite.config.ts");
	if std::env::var("PROFILE").as_deref() == Ok("release") && std::env::var_os("UN_AVATAR_FRONTEND_PREBUILT").is_none() {
		build_frontend();
	}
	tauri_build::build();
}

fn build_frontend() {
	let manifest_dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
	let frontend_dir = manifest_dir.parent().expect("src-tauri parent is frontend dir");
	let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
	let status = std::process::Command::new(npm)
		.current_dir(frontend_dir)
		.env("VITE_UN_DESIGN_PREVIEW", "0")
		.args(["run", "build"])
		.status()
		.expect("run npm build for supervisor frontend");
	if !status.success() {
		panic!("npm run build failed for supervisor frontend");
	}
}
