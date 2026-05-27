fn main() {
	println!("cargo:rerun-if-changed=../src");
	println!("cargo:rerun-if-changed=../package.json");
	println!("cargo:rerun-if-changed=../package-lock.json");
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
		.args(["run", "build"])
		.status()
		.expect("run npm build for supervisor frontend");
	if !status.success() {
		panic!("npm run build failed for supervisor frontend");
	}
}
