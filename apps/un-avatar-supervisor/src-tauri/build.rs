fn main() {
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
	if !frontend_needs_build(frontend_dir) {
		println!("cargo:warning=frontend dist is fresh; skip npm run build");
		return;
	}
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

fn frontend_needs_build(frontend_dir: &std::path::Path) -> bool {
	let Some(dist_mtime) = newest_mtime(&frontend_dir.join("dist")) else {
		return true;
	};
	let inputs = [
		"src",
		"public",
		"index.html",
		"package.json",
		"package-lock.json",
		"svelte.config.js",
		"tsconfig.json",
		"vite.config.ts",
	];
	inputs
		.iter()
		.filter_map(|input| newest_mtime(&frontend_dir.join(input)))
		.any(|input_mtime| input_mtime > dist_mtime)
}

fn newest_mtime(path: &std::path::Path) -> Option<std::time::SystemTime> {
	let metadata = std::fs::metadata(path).ok()?;
	if metadata.is_file() {
		return metadata.modified().ok();
	}
	if !metadata.is_dir() {
		return None;
	}
	let mut newest = metadata.modified().ok();
	for entry in std::fs::read_dir(path).ok()?.flatten() {
		let child = entry.path();
		if child
			.file_name()
			.and_then(|name| name.to_str())
			.is_some_and(|name| name == "node_modules" || name == "dist")
		{
			continue;
		}
		if let Some(time) = newest_mtime(&child) {
			if newest.is_none_or(|current| time > current) {
				newest = Some(time);
			}
		}
	}
	newest
}
