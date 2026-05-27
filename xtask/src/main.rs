//! UN Avatar workspace 用 xtask。`cargo xtask ci` 等を拡張する。

use std::{
	env, fs,
	io::{BufReader, BufWriter, Read, Write},
	path::{Path, PathBuf},
	process::{self, Command, Stdio},
};

use zip::{write::SimpleFileOptions, CompressionMethod};

const SPOUT2_REPO_URL: &str = "https://github.com/leadedge/Spout2.git";
const DEFAULT_SPOUT2_REF: &str = "2.007.017";
const COPY_BUFFER_SIZE: usize = 64 * 1024;

fn repo_root() -> &'static Path {
	Path::new(env!("CARGO_MANIFEST_DIR"))
		.parent()
		.expect("xtask はリポジトリ直下の xtask/ に置く")
}

fn run_cargo(repo: &Path, args: &[&str]) -> process::ExitStatus {
	Command::new("cargo")
		.args(args)
		.current_dir(repo)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.expect("cargo を実行できない（PATH に cargo があるか確認）")
}

fn run_cargo_with_env(repo: &Path, args: &[&str], envs: &[(&str, &str)]) -> process::ExitStatus {
	let mut command = Command::new("cargo");
	command
		.args(args)
		.current_dir(repo)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit());
	for (key, value) in envs {
		command.env(key, value);
	}
	command.status().expect("cargo を実行できない（PATH に cargo があるか確認）")
}

fn run_tool(repo: &Path, program: &str, args: &[&str]) -> process::ExitStatus {
	Command::new(program)
		.args(args)
		.current_dir(repo)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.unwrap_or_else(|e| panic!("{program} を実行できない: {e}"))
}

fn run_renderer(repo: &Path, mut args: impl Iterator<Item = String>) -> bool {
	let mut profile: Option<String> = None;
	let mut manifest: Option<PathBuf> = None;
	let mut release = false;
	let mut renderer_args: Vec<String> = Vec::new();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--profile" | "-p" => {
				let Some(value) = args.next() else {
					print_run_renderer_usage();
					return false;
				};
				profile = Some(value);
			}
			"--manifest" | "-m" => {
				let Some(value) = args.next() else {
					print_run_renderer_usage();
					return false;
				};
				manifest = Some(path_from_arg(repo, value));
			}
			"--release" => release = true,
			"--" => {
				renderer_args.extend(args);
				break;
			}
			"--help" | "-h" => {
				print_run_renderer_usage();
				return true;
			}
			other => {
				eprintln!("run-renderer: unknown argument: {other}");
				print_run_renderer_usage();
				return false;
			}
		}
	}

	if profile.is_some() && manifest.is_some() {
		eprintln!("run-renderer: use either --profile or --manifest, not both");
		return false;
	}
	let manifest = if let Some(path) = manifest {
		path
	} else if let Some(profile) = profile {
		match resolve_renderer_profile_manifest(repo, &profile) {
			Ok(path) => path,
			Err(e) => {
				eprintln!("{e}");
				return false;
			}
		}
	} else {
		eprintln!("run-renderer: --profile <name> or --manifest <path> is required");
		print_run_renderer_usage();
		return false;
	};
	if !manifest.is_file() {
		eprintln!("run-renderer: manifest not found: {}", manifest.display());
		return false;
	}

	let mut command = Command::new("cargo");
	command
		.arg("run")
		.arg("-p")
		.arg("un-avatar-render-wgpu")
		.arg("--bin")
		.arg("un-avatar-renderer");
	if release {
		command.arg("--release");
	}
	command.arg("--").arg("--manifest").arg(&manifest).args(renderer_args);
	eprintln!("run-renderer: manifest {}", manifest.display());
	command
		.current_dir(repo)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.map(|status| status.success())
		.unwrap_or_else(|e| {
			eprintln!("run-renderer: cargo run failed to start: {e}");
			false
		})
}

fn path_from_arg(repo: &Path, value: String) -> PathBuf {
	let path = PathBuf::from(value);
	if path.is_absolute() {
		path
	} else {
		repo.join(path)
	}
}

fn resolve_renderer_profile_manifest(repo: &Path, profile: &str) -> Result<PathBuf, String> {
	let wanted = normalize_profile_key(profile);
	if wanted.is_empty() {
		return Err("run-renderer: profile name is empty".to_string());
	}
	let mut matches = Vec::new();
	for dir in [user_profiles_dir(repo), repo.join("profiles")] {
		let Ok(entries) = fs::read_dir(&dir) else { continue };
		for entry in entries.flatten() {
			let path = entry.path();
			if path
				.extension()
				.and_then(|e| e.to_str())
				.is_some_and(|e| e.eq_ignore_ascii_case("toml"))
				&& profile_manifest_matches(&path, &wanted)
			{
				matches.push(path);
			}
		}
		if !matches.is_empty() {
			break;
		}
	}
	match matches.len() {
		0 => Err(format!(
			"run-renderer: profile `{profile}` not found in {} or {}",
			user_profiles_dir(repo).display(),
			repo.join("profiles").display()
		)),
		1 => Ok(matches.remove(0)),
		_ => Err(format!(
			"run-renderer: profile `{profile}` is ambiguous:\n{}",
			matches
				.iter()
				.map(|path| format!("  - {}", path.display()))
				.collect::<Vec<_>>()
				.join("\n")
		)),
	}
}

fn user_profiles_dir(repo: &Path) -> PathBuf {
	if let Some(path) = env::var_os("APPDATA") {
		return PathBuf::from(path).join("UN Avatar").join("profiles");
	}
	if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
		return PathBuf::from(path).join("un-avatar").join("profiles");
	}
	if let Some(path) = env::var_os("HOME") {
		return PathBuf::from(path).join(".config").join("un-avatar").join("profiles");
	}
	repo.join("target").join("tmp").join("un-avatar-config").join("profiles")
}

fn profile_manifest_matches(path: &Path, wanted: &str) -> bool {
	let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
	if normalize_profile_key(stem) == wanted || normalize_profile_key(strip_timestamp_prefix(stem)) == wanted {
		return true;
	}
	let Ok(text) = fs::read_to_string(path) else { return false };
	let Ok(value) = toml::from_str::<toml::Value>(&text) else {
		return false;
	};
	for candidate in [
		value.get("title").and_then(toml::Value::as_str),
		value
			.get("profile")
			.and_then(toml::Value::as_table)
			.and_then(|profile| profile.get("id"))
			.and_then(toml::Value::as_str),
		value
			.get("profile")
			.and_then(toml::Value::as_table)
			.and_then(|profile| profile.get("display_name"))
			.and_then(toml::Value::as_str),
	] {
		if candidate.is_some_and(|candidate| normalize_profile_key(candidate) == wanted) {
			return true;
		}
	}
	false
}

fn strip_timestamp_prefix(stem: &str) -> &str {
	let bytes = stem.as_bytes();
	if bytes.len() > 17
		&& bytes[0..8].iter().all(u8::is_ascii_digit)
		&& bytes[8] == b'T'
		&& bytes[9..15].iter().all(u8::is_ascii_digit)
		&& bytes[15] == b'Z'
		&& bytes[16] == b'-'
	{
		&stem[17..]
	} else {
		stem
	}
}

fn normalize_profile_key(value: &str) -> String {
	value.trim().to_ascii_lowercase().replace([' ', '_'], "-")
}

fn run_supervisor_frontend_build(repo: &Path) -> bool {
	Command::new(npm_exe())
		.args(["run", "build"])
		.current_dir(repo.join("apps").join("un-avatar-supervisor"))
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.map(|status| status.success())
		.unwrap_or_else(|e| {
			eprintln!("package: npm run build failed to start: {e}");
			false
		})
}

fn run_supervisor_frontend_check(repo: &Path) -> bool {
	Command::new(npm_exe())
		.args(["run", "check"])
		.current_dir(repo.join("apps").join("un-avatar-supervisor"))
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.map(|status| status.success())
		.unwrap_or_else(|e| {
			eprintln!("acceptance-preflight: npm run check failed to start: {e}");
			false
		})
}

fn run_acceptance_preflight(repo: &Path) -> bool {
	run_cargo(repo, &["fmt", "--all", "--", "--check"]).success()
		&& run_render_smoke(repo)
		&& run_cargo(repo, &["test", "-p", "un-avatar-render-wgpu"]).success()
		&& run_cargo(repo, &["test", "-p", "un-avatar-supervisor"]).success()
		&& run_supervisor_frontend_check(repo)
}

fn run_acceptance_prepare(repo: &Path) -> bool {
	let dir = repo.join("target").join("tmp").join("acceptance");
	let manifests = dir.join("manifests");
	if let Err(e) = fs::create_dir_all(&manifests) {
		eprintln!("acceptance-prepare: mkdir {}: {e}", manifests.display());
		return false;
	}

	let fixtures = ["target/tmp/model1.vrm", "target/tmp/model2.vrm", "target/tmp/vrm1.vrm"];
	let missing: Vec<&str> = fixtures.iter().copied().filter(|fixture| !repo.join(fixture).is_file()).collect();
	if !missing.is_empty() {
		eprintln!("acceptance-prepare: missing fixture(s): {}", missing.join(", "));
		return false;
	}

	let clickthrough_manifest = acceptance_renderer_manifest(
		"Acceptance Click-through Window",
		"target/tmp/model1.vrm",
		"smaa",
		"off",
		"source",
		true,
		true,
		false,
	);

	let files = [
		(
			manifests.join("model1-front.toml"),
			acceptance_renderer_manifest(
				"Acceptance Model1 Front",
				"target/tmp/model1.vrm",
				"off",
				"off",
				"source",
				false,
				false,
				false,
			),
		),
		(
			manifests.join("model2-front.toml"),
			acceptance_renderer_manifest(
				"Acceptance Model2 Front",
				"target/tmp/model2.vrm",
				"off",
				"off",
				"source",
				false,
				false,
				false,
			),
		),
		(
			manifests.join("vrm1-front.toml"),
			acceptance_renderer_manifest(
				"Acceptance VRM1 Front",
				"target/tmp/vrm1.vrm",
				"off",
				"off",
				"source",
				false,
				false,
				false,
			),
		),
		(
			manifests.join("transparent-window.toml"),
			acceptance_renderer_manifest(
				"Acceptance Transparent Window",
				"target/tmp/model1.vrm",
				"smaa",
				"off",
				"source",
				true,
				false,
				false,
			),
		),
		(manifests.join("click-through.toml"), clickthrough_manifest.clone()),
		(manifests.join("clickthrough-window.toml"), clickthrough_manifest),
		(
			manifests.join("texture-auto.toml"),
			acceptance_renderer_manifest(
				"Acceptance Texture Auto",
				"target/tmp/model1.vrm",
				"fxaa",
				"2k",
				"auto",
				false,
				false,
				false,
			),
		),
		(
			manifests.join("spout-1080p.toml"),
			acceptance_renderer_manifest(
				"Acceptance Spout 1080p",
				"target/tmp/model1.vrm",
				"fxaa",
				"off",
				"source",
				true,
				false,
				true,
			),
		),
		(dir.join("notes-template.md"), acceptance_notes_template()),
		(dir.join("README.md"), acceptance_readme()),
	];

	for (path, text) in files {
		if let Err(e) = fs::write(&path, text) {
			eprintln!("acceptance-prepare: write {}: {e}", path.display());
			return false;
		}
		println!("acceptance-prepare: wrote {}", path.display());
	}
	println!("ACCEPTANCE_DIR={}", dir.display());
	true
}

fn target_profile_dir(repo: &Path, release: bool) -> PathBuf {
	repo.join("target").join(if release { "release" } else { "debug" })
}

fn target_exe(repo: &Path, release: bool, name: &str) -> PathBuf {
	target_profile_dir(repo, release).join(exe_name(name))
}

fn run_build(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut release = false;
	for arg in args {
		match arg.as_str() {
			"--release" => release = true,
			"help" | "--help" | "-h" => {
				print_build_usage();
				return true;
			}
			other => {
				eprintln!("build: unknown argument: {other}");
				print_build_usage();
				return false;
			}
		}
	}

	if !run_supervisor_frontend_build(repo) {
		return false;
	}

	let profile_arg = release.then_some("--release");
	let mut supervisor_args = vec!["build", "--locked", "-p", "un-avatar-supervisor"];
	if let Some(profile_arg) = profile_arg {
		supervisor_args.insert(1, profile_arg);
	}
	if !run_cargo_with_env(repo, &supervisor_args, &[("UN_AVATAR_FRONTEND_PREBUILT", "1")]).success() {
		return false;
	}

	let mut renderer_args = vec!["build", "--locked", "-p", "un-avatar-render-wgpu"];
	if let Some(profile_arg) = profile_arg {
		renderer_args.insert(1, profile_arg);
	}
	if spout2_dev_available(repo) {
		renderer_args.extend(["--features", "spout-sdk"]);
		run_cargo_with_spout_env(repo, &renderer_args, &[])
	} else {
		eprintln!("build: Spout2 SDK/runtime not staged; renderer will be built without Spout2. Run `cargo xtask spout2` to enable it.");
		run_cargo_with_env(repo, &renderer_args, &[]).success()
	}
}

fn run_app(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut release = false;
	let mut app_args = Vec::new();
	let mut iter = args.peekable();
	while let Some(arg) = iter.next() {
		match arg.as_str() {
			"--release" => release = true,
			"--" => {
				app_args.extend(iter);
				break;
			}
			"help" | "--help" | "-h" => {
				print_run_usage();
				return true;
			}
			other => {
				eprintln!("run: unknown argument: {other}");
				print_run_usage();
				return false;
			}
		}
	}

	let build_args = if release { vec!["--release".to_string()] } else { Vec::new() };
	if !run_build(repo, build_args.into_iter()) {
		return false;
	}

	let exe = target_exe(repo, release, "un-avatar-supervisor");
	if !exe.is_file() {
		eprintln!("run: supervisor executable not found after build: {}", exe.display());
		return false;
	}
	Command::new(&exe)
		.args(app_args)
		.current_dir(repo)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.map(|status| status.success())
		.unwrap_or_else(|e| {
			eprintln!("run: failed to start {}: {e}", exe.display());
			false
		})
}

#[allow(clippy::too_many_arguments)]
fn acceptance_renderer_manifest(
	title: &str,
	avatar_path: &str,
	aa: &str,
	texture_resolution_limit: &str,
	texture_compression: &str,
	transparent: bool,
	input_passthrough: bool,
	spout_enabled: bool,
) -> String {
	let clear_color = if transparent {
		"[0.0, 0.0, 0.0, 0.0]"
	} else {
		"[0.12, 0.14, 0.18, 1.0]"
	};
	let decorations = !transparent;
	let spout_width = if spout_enabled { "1920" } else { "1280" };
	let spout_height = if spout_enabled { "1080" } else { "720" };
	format!(
		"title = \"{title}\"\n\
avatar_path = \"{avatar_path}\"\n\
transparent = {transparent}\n\
input_passthrough = {input_passthrough}\n\
decorations = {decorations}\n\
aa = \"{aa}\"\n\
clear_color = {clear_color}\n\
show_fps_in_title = true\n\n\
[render_quality]\n\
aa = \"{aa}\"\n\
texture_resolution_limit = \"{texture_resolution_limit}\"\n\
texture_compression = \"{texture_compression}\"\n\
processed_texture_cache = true\n\n\
[window]\n\
decorations = {decorations}\n\
transparent = {transparent}\n\
input_passthrough = {input_passthrough}\n\
always_on_top = false\n\
width = 800\n\
height = 600\n\
[motion.vmc_udp]\n\
enabled = true\n\
address = \"0.0.0.0:39539\"\n\n\
[output.spout2]\n\
enabled = {spout_enabled}\n\
name = \"UN Avatar Acceptance\"\n\
width = {spout_width}\n\
height = {spout_height}\n\n\
[debug]\n\
scene = false\n\
vmc = false\n\
morph = false\n\n\
[diagnostics]\n\
relax_iris_alpha = false\n"
	)
}

fn acceptance_notes_template() -> String {
	"# MVP Acceptance Notes\n\n\
Date:\n\
Operator:\n\
Build/Commit:\n\n\
## Preflight\n\n\
- Command: `cargo xtask acceptance-preflight`\n\
- Result:\n\n\
## Window / Runtime\n\n\
- Transparent window:\n\
- Click-through:\n\
- Borderless resize:\n\
- Renderer close hotkey:\n\
- Console-exit Stop All:\n\n\
## Renderer Quality\n\n\
- AA mode/status:\n\
- Texture policy/summary:\n\
- Diagnostics export path:\n\
- Texture fallback finding:\n\n\
## Model Visuals\n\n\
- model1 +Z-front screenshot:\n\
- model2 +Z-front screenshot:\n\
- vrm1 +Z-front screenshot:\n\
- MToon eyes/highlights/outline notes:\n\n\
## Live Integrations\n\n\
- VMC source and result:\n\
- OBS Spout2 source and result:\n\
"
	.to_string()
}

fn acceptance_readme() -> String {
	"# UN Avatar MVP Acceptance Artifacts\n\n\
Generated by `cargo xtask acceptance-prepare`. Run commands from the repository root.\n\n\
## Preflight\n\n\
```powershell\n\
cargo xtask acceptance-preflight\n\
```\n\n\
## Renderer Manifests\n\n\
Launch a manifest with:\n\n\
```powershell\n\
cargo run --locked -p un-avatar-render-wgpu --bin un-avatar-renderer -- --manifest target/tmp/acceptance/manifests/model1-front.toml\n\
```\n\n\
Useful manifests:\n\n\
- `manifests/model1-front.toml`: +Z-front model1 visual pass.\n\
- `manifests/model2-front.toml`: +Z-front model2 and eye-area MToon pass.\n\
- `manifests/vrm1-front.toml`: +Z-front VRM1 pass.\n\
- `manifests/transparent-window.toml`: real transparent window pass.\n\
- `manifests/click-through.toml`: launch-time Click-through pass; `clickthrough-window.toml` is also generated as a compatibility alias.\n\
- `manifests/texture-auto.toml`: texture summary/fallback diagnostics pass.\n\
- `manifests/spout-1080p.toml`: OBS Spout2 1080p pass. Use a packaged/release Spout2 build, or a dev build with `--features spout-sdk` plus the Spout2 SDK/DLL environment.\n\n\
Record evidence in `notes-template.md` and keep screenshots/diagnostics bundles next to it.\n"
	.to_string()
}

fn npm_exe() -> &'static str {
	if cfg!(windows) {
		"npm.cmd"
	} else {
		"npm"
	}
}

fn run_cargo_with_spout_env(repo: &Path, args: &[&str], extra_envs: &[(&str, &str)]) -> bool {
	let source = spout2_source_dir(repo);
	let Some(lib_dir) = spout2_link_lib_dir(repo) else {
		eprintln!("spout2: Spout.lib が見つかりません。先に cargo xtask spout2 を実行してください。");
		return false;
	};
	let runtime_dir = spout2_package_dir(repo);
	if !runtime_dir.join("Spout.dll").is_file() {
		eprintln!(
			"spout2: {} が見つかりません。先に cargo xtask spout2 を実行してください。",
			runtime_dir.join("Spout.dll").display()
		);
		return false;
	}
	let old_path = env::var_os("PATH").unwrap_or_default();
	let mut paths = vec![runtime_dir];
	paths.extend(env::split_paths(&old_path));
	let path_value = env::join_paths(paths).expect("PATH join failed");
	Command::new("cargo")
		.args(args)
		.current_dir(repo)
		.env("SPOUT2_SDK_DIR", source.join("SPOUTSDK").join("SpoutGL"))
		.env("SPOUT2_LIB_DIR", lib_dir)
		.env("PATH", path_value)
		.envs(extra_envs.iter().copied())
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.map(|status| status.success())
		.unwrap_or_else(|e| {
			eprintln!("cargo を実行できない（PATH に cargo があるか確認）: {e}");
			false
		})
}

fn path_str(path: &Path) -> &str {
	path.to_str().expect("path is not valid UTF-8")
}

fn run_un_avatar(repo: &Path, args: &[&str]) -> process::ExitStatus {
	Command::new("cargo")
		.args(["run", "--locked", "-q", "-p", "un-avatar-cli", "--bin", "un-avatar", "--"])
		.args(args)
		.current_dir(repo)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.expect("cargo run un-avatar を実行できない")
}

/// 最小 `.una` で validate / formats list / convert ラウンドトリップを確認する（CI と同趣旨）。
fn run_smoke(repo: &Path) -> bool {
	let mut dir = env::temp_dir();
	dir.push(format!("un-avatar-xtask-smoke-{}", process::id()));
	if fs::create_dir_all(&dir).is_err() {
		return false;
	}
	let in_una = dir.join("in.una");
	let out_una = dir.join("out.una");
	let write_ok = (|| {
		let mut f = fs::File::create(&in_una).ok()?;
		writeln!(f, "format_version = 1").ok()?;
		writeln!(f).ok()?;
		writeln!(f, "[scene]").ok()?;
		writeln!(f, "empty = true").ok()?;
		Some(())
	})();
	if write_ok.is_none() {
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let validate_in = run_un_avatar(repo, &["validate", in_una.to_str().expect("utf8 path")]).success();
	if !validate_in {
		eprintln!("smoke: validate in.una failed");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let list_out = Command::new("cargo")
		.args([
			"run",
			"--locked",
			"-q",
			"-p",
			"un-avatar-cli",
			"--bin",
			"un-avatar",
			"--",
			"formats",
			"list",
			"--json",
		])
		.current_dir(repo)
		.output();
	let list_ok = match list_out {
		Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).contains("io.un-avatar.una"),
		_ => false,
	};
	if !list_ok {
		eprintln!("smoke: formats list --json missing io.un-avatar.una");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let plugins_dir = repo.join("plugins");
	if !plugins_dir.is_dir() {
		eprintln!("smoke: repo plugins/ missing");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}
	let plugin_list_out = Command::new("cargo")
		.args([
			"run",
			"--locked",
			"-q",
			"-p",
			"un-avatar-cli",
			"--bin",
			"un-avatar",
			"--",
			"--plugin-dir",
			plugins_dir.to_str().expect("plugins path utf-8"),
			"formats",
			"list",
			"--json",
		])
		.current_dir(repo)
		.output();
	let plugin_list_ok = match plugin_list_out {
		Ok(o) if o.status.success() => {
			let s = String::from_utf8_lossy(&o.stdout);
			s.contains("io.un-avatar.example.avatar")
		}
		_ => false,
	};
	if !plugin_list_ok {
		eprintln!("smoke: formats list --plugin-dir … missing io.un-avatar.example.avatar");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let plugin_probe_out = Command::new("cargo")
		.args([
			"run",
			"--locked",
			"-q",
			"-p",
			"un-avatar-cli",
			"--bin",
			"un-avatar",
			"--",
			"--plugin-dir",
			plugins_dir.to_str().expect("plugins path utf-8"),
			"formats",
			"probe",
			"xtask-smoke.exampleavatar",
			"--json",
		])
		.current_dir(repo)
		.output();
	let plugin_probe_ok = match plugin_probe_out {
		Ok(o) if o.status.success() => {
			let s = String::from_utf8_lossy(&o.stdout);
			s.contains("\"best_importer\": \"io.un-avatar.example.avatar\"")
				&& s.contains("\"best_exporter\": \"io.un-avatar.example.avatar\"")
				&& s.contains("\"best_importer_provider_plugin_id\": \"network.usagi.un_avatar.plugin.sample_io\"")
				&& s.contains("\"best_exporter_provider_plugin_id\": \"network.usagi.un_avatar.plugin.sample_io\"")
		}
		_ => false,
	};
	if !plugin_probe_ok {
		eprintln!("smoke: formats probe --plugin-dir --json (best_importer/best_exporter for sample) failed");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let convert_ok = run_un_avatar(repo, &["convert", in_una.to_str().expect("utf8"), out_una.to_str().expect("utf8")]).success();
	if !convert_ok {
		eprintln!("smoke: convert in.una -> out.una failed");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let validate_out = run_un_avatar(repo, &["validate", out_una.to_str().expect("utf8")]).success();
	if !validate_out {
		eprintln!("smoke: validate out.una after convert failed");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let via_plugin = dir.join("via-plugin.exampleavatar");
	let plugin_convert_ok = run_un_avatar(
		repo,
		&[
			"--plugin-dir",
			plugins_dir.to_str().expect("plugins path utf-8"),
			"convert",
			in_una.to_str().expect("utf8"),
			via_plugin.to_str().expect("utf8"),
			"--output-format",
			"io.un-avatar.example.avatar",
		],
	)
	.success();
	if !plugin_convert_ok {
		eprintln!("smoke: convert in.una -> via-plugin.exampleavatar (stdio exporter) failed");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let _ = fs::remove_dir_all(&dir);
	true
}

fn run_render_smoke(repo: &Path) -> bool {
	let mut dir = env::temp_dir();
	dir.push(format!("un-avatar-xtask-render-smoke-{}", process::id()));
	if let Err(e) = fs::create_dir_all(&dir) {
		eprintln!("render-smoke: mkdir {}: {e}", dir.display());
		return false;
	}

	let manifest = dir.join("renderer.toml");
	let model = dir.join("triangle.gltf");
	let buffer = dir.join("triangle.bin");
	let mut buffer_bytes = Vec::new();
	for value in [-1.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
		buffer_bytes.extend_from_slice(&value.to_le_bytes());
	}
	for value in [0_u32, 1, 2] {
		buffer_bytes.extend_from_slice(&value.to_le_bytes());
	}
	if let Err(e) = fs::write(&buffer, buffer_bytes) {
		eprintln!("render-smoke: write {}: {e}", buffer.display());
		let _ = fs::remove_dir_all(&dir);
		return false;
	}
	let gltf = r#"{
  "asset": { "version": "2.0" },
  "scene": 0,
  "scenes": [{ "nodes": [0] }],
  "nodes": [{ "mesh": 0 }],
  "meshes": [{ "primitives": [{ "attributes": { "POSITION": 0 }, "indices": 1 }] }],
  "buffers": [{ "uri": "triangle.bin", "byteLength": 48 }],
  "bufferViews": [
    { "buffer": 0, "byteOffset": 0, "byteLength": 36, "target": 34962 },
    { "buffer": 0, "byteOffset": 36, "byteLength": 12, "target": 34963 }
  ],
  "accessors": [
    { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3", "max": [1.0, 1.0, 0.0], "min": [-1.0, 0.0, 0.0] },
    { "bufferView": 1, "componentType": 5125, "count": 3, "type": "SCALAR", "max": [2], "min": [0] }
  ]
}
"#;
	if let Err(e) = fs::write(&model, gltf) {
		eprintln!("render-smoke: write {}: {e}", model.display());
		let _ = fs::remove_dir_all(&dir);
		return false;
	}
	let model_literal = model.display().to_string().replace('\'', "''");
	let text = format!(
		"title = \"UN Avatar Render Smoke\"\n\
		 avatar_path = '{model_literal}'\n\
		 show_fps_in_title = false\n\
		 transparent = false\n\
		 input_passthrough = false\n\
		 aa = \"smaa\"\n\n\
		 [render_quality]\n\
		 aa = \"smaa\"\n\
		 texture_resolution_limit = \"2k\"\n\
		 texture_compression = \"auto\"\n\
		 processed_texture_cache = true\n\n\
		 [debug]\n\
		 scene = true\n\n\
		 [diagnostics]\n\
		 relax_iris_alpha = false\n"
	);
	if let Err(e) = fs::write(&manifest, text) {
		eprintln!("render-smoke: write {}: {e}", manifest.display());
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let status = Command::new("cargo")
		.args([
			"run",
			"--locked",
			"-q",
			"-p",
			"un-avatar-render-wgpu",
			"--bin",
			"un-avatar-renderer",
			"--",
			"--manifest",
			path_str(&manifest),
			"--validate-startup",
		])
		.current_dir(repo)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status();
	let ok = status.is_ok_and(|status| status.success());
	if !ok {
		eprintln!("render-smoke: renderer manifest/model startup validation failed");
	}
	let _ = fs::remove_dir_all(&dir);
	ok
}

fn find_file_named(root: &Path, name: &str) -> Option<PathBuf> {
	fn visit(dir: &Path, name: &str) -> Option<PathBuf> {
		let entries = fs::read_dir(dir).ok()?;
		for entry in entries.flatten() {
			let path = entry.path();
			if path.is_file() && path.file_name().is_some_and(|file_name| file_name == name) {
				return Some(path);
			}
			if path.is_dir() {
				if let Some(found) = visit(&path, name) {
					return Some(found);
				}
			}
		}
		None
	}
	visit(root, name)
}

fn copy_file_to(src: &Path, dst: &Path) -> bool {
	if let Some(parent) = dst.parent() {
		if let Err(e) = fs::create_dir_all(parent) {
			eprintln!("spout2: mkdir {}: {e}", parent.display());
			return false;
		}
	}
	match fs::copy(src, dst) {
		Ok(_) => true,
		Err(e) => {
			eprintln!("spout2: copy {} -> {}: {e}", src.display(), dst.display());
			false
		}
	}
}

fn spout2_root(repo: &Path) -> PathBuf {
	repo.join("target").join("spout2")
}

fn spout2_source_dir(repo: &Path) -> PathBuf {
	spout2_root(repo).join("Spout2")
}

fn spout2_build_dir(repo: &Path) -> PathBuf {
	spout2_root(repo).join("build")
}

fn spout2_link_lib_dir(repo: &Path) -> Option<PathBuf> {
	find_file_named(&spout2_build_dir(repo), "Spout.lib").and_then(|lib| lib.parent().map(Path::to_path_buf))
}

fn spout2_dev_available(repo: &Path) -> bool {
	spout2_source_dir(repo)
		.join("SPOUTSDK")
		.join("SpoutGL")
		.join("SpoutSender.h")
		.is_file()
		&& spout2_link_lib_dir(repo).is_some()
		&& spout2_package_dir(repo).join("Spout.dll").is_file()
}

fn spout2_package_dir(repo: &Path) -> PathBuf {
	repo.join("target").join("package").join("un-avatar")
}

fn ensure_spout2_source(repo: &Path, git_ref: &str) -> bool {
	let source = spout2_source_dir(repo);
	if source.join(".git").is_dir() {
		println!("spout2: source exists: {}", source.display());
		return true;
	}
	if let Some(parent) = source.parent() {
		if let Err(e) = fs::create_dir_all(parent) {
			eprintln!("spout2: mkdir {}: {e}", parent.display());
			return false;
		}
	}
	println!("spout2: clone {SPOUT2_REPO_URL} ({git_ref}) -> {}", source.display());
	run_tool(
		repo,
		"git",
		&["clone", "--depth", "1", "--branch", git_ref, SPOUT2_REPO_URL, path_str(&source)],
	)
	.success()
}

fn build_spout2(repo: &Path) -> bool {
	let source = spout2_source_dir(repo);
	let build = spout2_build_dir(repo);
	if !source.join("CMakeLists.txt").is_file() {
		eprintln!("spout2: source missing CMakeLists.txt: {}", source.display());
		return false;
	}
	if let Err(e) = fs::create_dir_all(&build) {
		eprintln!("spout2: mkdir {}: {e}", build.display());
		return false;
	}
	println!("spout2: cmake configure -> {}", build.display());
	let configured = run_tool(
		repo,
		"cmake",
		&["-S", path_str(&source), "-B", path_str(&build), "-DCMAKE_BUILD_TYPE=Release"],
	)
	.success();
	if !configured {
		return false;
	}
	println!("spout2: cmake build Release");
	run_tool(repo, "cmake", &["--build", path_str(&build), "--config", "Release"]).success()
}

fn stage_spout2(repo: &Path) -> bool {
	let source = spout2_source_dir(repo);
	let build = spout2_build_dir(repo);
	let package = spout2_package_dir(repo);
	let Some(dll) = find_file_named(&build, "Spout.dll") else {
		eprintln!("spout2: Spout.dll not found under {}", build.display());
		return false;
	};
	let Some(lib) = find_file_named(&build, "Spout.lib") else {
		eprintln!("spout2: Spout.lib not found under {}", build.display());
		return false;
	};
	let license = source.join("LICENSE");
	if !license.is_file() {
		eprintln!("spout2: LICENSE not found: {}", license.display());
		return false;
	}

	let license_dir = package.join("LICENSES");
	let build_info = license_dir.join("spout2-build-info.txt");
	let ok = copy_file_to(&dll, &package.join("Spout.dll"))
		&& copy_file_to(&license, &license_dir.join("Spout2-BSD-2-Clause.txt"))
		&& write_spout2_build_info(&source, &dll, &lib, &build_info);
	if ok {
		println!("spout2: staged runtime: {}", package.join("Spout.dll").display());
		println!("spout2: staged license: {}", license_dir.join("Spout2-BSD-2-Clause.txt").display());
		println!("spout2: staged build info: {}", build_info.display());
		println!(
			"spout2: build env SPOUT2_SDK_DIR={}",
			source.join("SPOUTSDK").join("SpoutGL").display()
		);
		println!("spout2: build env SPOUT2_LIB_DIR={}", lib.parent().unwrap_or(&build).display());
	}
	ok
}

fn git_output(cwd: &Path, args: &[&str]) -> Option<String> {
	let output = Command::new("git").args(args).current_dir(cwd).output().ok()?;
	if !output.status.success() {
		return None;
	}
	Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn file_sha256(path: &Path) -> Option<String> {
	let mut file = fs::File::open(path).ok()?;
	let mut bytes = Vec::new();
	file.read_to_end(&mut bytes).ok()?;
	let output = Command::new("certutil")
		.args(["-hashfile", path_str(path), "SHA256"])
		.output()
		.ok()?;
	if !output.status.success() {
		return None;
	}
	let text = String::from_utf8_lossy(&output.stdout);
	text.lines()
		.map(str::trim)
		.find(|line| line.len() == 64 && line.chars().all(|c| c.is_ascii_hexdigit()))
		.map(str::to_string)
}

fn write_spout2_build_info(source: &Path, dll: &Path, lib: &Path, dst: &Path) -> bool {
	if let Some(parent) = dst.parent() {
		if let Err(e) = fs::create_dir_all(parent) {
			eprintln!("spout2: mkdir {}: {e}", parent.display());
			return false;
		}
	}
	let head = git_output(source, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_string());
	let describe = git_output(source, &["describe", "--tags", "--always", "--dirty"]).unwrap_or_else(|| "unknown".to_string());
	let dll_hash = file_sha256(dll).unwrap_or_else(|| "unavailable".to_string());
	let lib_hash = file_sha256(lib).unwrap_or_else(|| "unavailable".to_string());
	let text = format!(
		"Spout2 build information\n\nrepository = {SPOUT2_REPO_URL}\nref = {describe}\ncommit = {head}\ndll = {}\ndll_sha256 = {dll_hash}\nlib = {}\nlib_sha256 = {lib_hash}\n",
		dll.display(),
		lib.display()
	);
	match fs::write(dst, text) {
		Ok(()) => true,
		Err(e) => {
			eprintln!("spout2: write {}: {e}", dst.display());
			false
		}
	}
}

fn clean_spout2(repo: &Path) -> bool {
	let root = spout2_root(repo);
	if !root.exists() {
		return true;
	}
	println!("spout2: remove {}", root.display());
	match fs::remove_dir_all(&root) {
		Ok(()) => true,
		Err(e) => {
			eprintln!("spout2: remove {}: {e}", root.display());
			false
		}
	}
}

fn exe_name(name: &str) -> String {
	if cfg!(windows) {
		format!("{name}.exe")
	} else {
		name.to_string()
	}
}

fn release_exe(repo: &Path, name: &str) -> PathBuf {
	repo.join("target").join("release").join(exe_name(name))
}

fn reset_package_dir(repo: &Path) -> bool {
	let package = spout2_package_dir(repo);
	if package.exists() {
		if let Err(e) = fs::remove_dir_all(&package) {
			eprintln!("package: remove {}: {e}", package.display());
			return false;
		}
	}
	if let Err(e) = fs::create_dir_all(&package) {
		eprintln!("package: mkdir {}: {e}", package.display());
		return false;
	}
	true
}

fn stage_exe(repo: &Path, bin_name: &str, dst_dir: &Path, dst_name: &str) -> bool {
	let src = release_exe(repo, bin_name);
	if !src.is_file() {
		eprintln!("package: executable not found: {}", src.display());
		return false;
	}
	copy_file_to(&src, &dst_dir.join(exe_name(dst_name)))
}

fn stage_package_docs(repo: &Path) -> bool {
	let package = spout2_package_dir(repo);
	let mut ok = true;
	for name in ["README.md", "LICENSE"] {
		let src = repo.join(name);
		if src.is_file() {
			ok &= copy_file_to(&src, &package.join(name));
		}
	}
	let third_party = repo.join("docs").join("third-party-licenses.md");
	if third_party.is_file() {
		ok &= copy_file_to(&third_party, &package.join("THIRD_PARTY_NOTICES.md"));
		ok &= copy_file_to(&third_party, &package.join("LICENSES").join("third-party-licenses.md"));
	}
	ok
}

fn default_package_version(repo: &Path) -> Option<String> {
	let manifest_path = repo.join("Cargo.toml");
	let raw = fs::read_to_string(&manifest_path).ok()?;
	let value: toml::Value = toml::from_str(&raw).ok()?;
	value
		.get("workspace")
		.and_then(|workspace| workspace.get("package"))
		.and_then(|package| package.get("version"))
		.and_then(|version| version.as_str())
		.map(str::trim)
		.filter(|version| !version.is_empty())
		.map(str::to_string)
}

fn copy_dir_contents(src: &Path, dst: &Path) -> bool {
	let entries = match fs::read_dir(src) {
		Ok(entries) => entries,
		Err(err) => {
			eprintln!("release-package: read {}: {err}", src.display());
			return false;
		}
	};
	if let Err(err) = fs::create_dir_all(dst) {
		eprintln!("release-package: mkdir {}: {err}", dst.display());
		return false;
	}
	for entry in entries.flatten() {
		let path = entry.path();
		let target = dst.join(entry.file_name());
		if path.is_dir() {
			if !copy_dir_contents(&path, &target) {
				return false;
			}
		} else if path.is_file() && !copy_file_to(&path, &target) {
			return false;
		}
	}
	true
}

fn zip_entry_name(staging_root: &Path, path: &Path) -> Option<String> {
	let rel = path.strip_prefix(staging_root).ok()?;
	Some(
		rel.components()
			.map(|component| component.as_os_str().to_string_lossy().into_owned())
			.collect::<Vec<_>>()
			.join("/"),
	)
}

fn add_zip_entry(writer: &mut zip::ZipWriter<BufWriter<fs::File>>, staging_root: &Path, path: &Path, options: SimpleFileOptions) -> bool {
	let Some(name) = zip_entry_name(staging_root, path) else {
		eprintln!("release-package: cannot compute zip path for {}", path.display());
		return false;
	};
	if path.is_dir() {
		if !name.is_empty() && writer.add_directory(format!("{name}/"), options).is_err() {
			eprintln!("release-package: add zip directory {name}");
			return false;
		}
		let entries = match fs::read_dir(path) {
			Ok(entries) => entries,
			Err(err) => {
				eprintln!("release-package: read {}: {err}", path.display());
				return false;
			}
		};
		for entry in entries.flatten() {
			if !add_zip_entry(writer, staging_root, &entry.path(), options) {
				return false;
			}
		}
		return true;
	}
	if !path.is_file() {
		return true;
	}
	if writer.start_file(&name, options).is_err() {
		eprintln!("release-package: start zip file {name}");
		return false;
	}
	let file = match fs::File::open(path) {
		Ok(file) => file,
		Err(err) => {
			eprintln!("release-package: open {}: {err}", path.display());
			return false;
		}
	};
	let mut reader = BufReader::new(file);
	let mut buffer = [0_u8; COPY_BUFFER_SIZE];
	loop {
		let bytes_read = match reader.read(&mut buffer) {
			Ok(bytes_read) => bytes_read,
			Err(err) => {
				eprintln!("release-package: read {}: {err}", path.display());
				return false;
			}
		};
		if bytes_read == 0 {
			break;
		}
		if let Err(err) = writer.write_all(&buffer[..bytes_read]) {
			eprintln!("release-package: write zip entry {name}: {err}");
			return false;
		}
	}
	true
}

fn create_release_zip(staging_root: &Path, package_name: &str, zip_path: &Path) -> bool {
	if let Some(parent) = zip_path.parent() {
		if let Err(err) = fs::create_dir_all(parent) {
			eprintln!("release-package: mkdir {}: {err}", parent.display());
			return false;
		}
	}
	if zip_path.exists() {
		if let Err(err) = fs::remove_file(zip_path) {
			eprintln!("release-package: remove {}: {err}", zip_path.display());
			return false;
		}
	}
	let file = match fs::File::create(zip_path) {
		Ok(file) => file,
		Err(err) => {
			eprintln!("release-package: create {}: {err}", zip_path.display());
			return false;
		}
	};
	let mut writer = zip::ZipWriter::new(BufWriter::new(file));
	let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
	let package_dir = staging_root.join(package_name);
	if !add_zip_entry(&mut writer, staging_root, &package_dir, options) {
		return false;
	}
	if let Err(err) = writer.finish() {
		eprintln!("release-package: finalize {}: {err}", zip_path.display());
		return false;
	}
	true
}

fn run_spout2(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut git_ref = env::var("UN_AVATAR_SPOUT2_REF").unwrap_or_else(|_| DEFAULT_SPOUT2_REF.to_string());
	let mut clean = false;
	let mut no_fetch = false;
	let mut iter = args.peekable();
	while let Some(arg) = iter.next() {
		match arg.as_str() {
			"--clean" => clean = true,
			"--no-fetch" => no_fetch = true,
			"--ref" => {
				let Some(value) = iter.next() else {
					eprintln!("spout2: --ref には git ref が必要です");
					return false;
				};
				git_ref = value;
			}
			"help" | "--help" | "-h" => {
				print_spout2_usage();
				return true;
			}
			other => {
				eprintln!("spout2: 不明な option: {other}");
				print_spout2_usage();
				return false;
			}
		}
	}
	if clean && !clean_spout2(repo) {
		return false;
	}
	if !no_fetch && !ensure_spout2_source(repo, &git_ref) {
		return false;
	}
	build_spout2(repo) && stage_spout2(repo)
}

fn run_package(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut skip_spout2 = false;
	for arg in args {
		match arg.as_str() {
			"--skip-spout2" => skip_spout2 = true,
			"help" | "--help" | "-h" => {
				print_package_usage();
				return true;
			}
			other => {
				eprintln!("package: 不明な option: {other}");
				print_package_usage();
				return false;
			}
		}
	}
	if !reset_package_dir(repo) {
		return false;
	}
	if !skip_spout2 && !run_spout2(repo, std::iter::empty()) {
		return false;
	}
	if !run_supervisor_frontend_build(repo) {
		return false;
	}

	let build_ok = if skip_spout2 {
		run_cargo_with_env(
			repo,
			&["build", "--release", "-p", "un-avatar-supervisor", "-p", "un-avatar-render-wgpu"],
			&[("UN_AVATAR_FRONTEND_PREBUILT", "1")],
		)
		.success()
	} else {
		run_cargo_with_spout_env(
			repo,
			&["build", "--release", "-p", "un-avatar-supervisor"],
			&[("UN_AVATAR_FRONTEND_PREBUILT", "1")],
		) && run_cargo_with_spout_env(
			repo,
			&["build", "--release", "-p", "un-avatar-render-wgpu", "--features", "spout-sdk"],
			&[],
		)
	};
	if !build_ok {
		return false;
	}

	let package = spout2_package_dir(repo);
	let ok = stage_exe(repo, "un-avatar-supervisor", &package, "un-avatar-supervisor")
		&& stage_exe(repo, "un-avatar-renderer", &package, "un-avatar-renderer")
		&& stage_package_docs(repo);
	if ok {
		println!("package: staged {}", package.display());
	}
	ok
}

fn run_release_package(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut version = None;
	let mut output_dir = None;
	let mut skip_build = false;
	let mut skip_spout2 = false;
	let mut keep_staging = false;
	let mut iter = args.peekable();
	while let Some(arg) = iter.next() {
		match arg.as_str() {
			"--version" => {
				let Some(value) = iter.next() else {
					eprintln!("release-package: --version には version が必要です");
					return false;
				};
				version = Some(value);
			}
			"--output-dir" => {
				let Some(value) = iter.next() else {
					eprintln!("release-package: --output-dir には path が必要です");
					return false;
				};
				output_dir = Some(PathBuf::from(value));
			}
			"--skip-build" => skip_build = true,
			"--skip-spout2" => skip_spout2 = true,
			"--keep-staging" => keep_staging = true,
			"help" | "--help" | "-h" => {
				print_release_package_usage();
				return true;
			}
			other => {
				eprintln!("release-package: 不明な option: {other}");
				print_release_package_usage();
				return false;
			}
		}
	}

	let Some(version) = version.or_else(|| default_package_version(repo)) else {
		eprintln!("release-package: Cargo.toml の workspace.package.version を読めませんでした");
		return false;
	};
	if !skip_build {
		let package_args = if skip_spout2 {
			vec!["--skip-spout2".to_string()]
		} else {
			Vec::new()
		};
		if !run_package(repo, package_args.into_iter()) {
			return false;
		}
	}

	let source_package = spout2_package_dir(repo);
	if !source_package.is_dir() {
		eprintln!(
			"release-package: package staging directory missing: {}\n先に cargo xtask package を実行するか、--skip-build を外してください。",
			source_package.display()
		);
		return false;
	}

	let package_name = format!("un-avatar-{version}");
	let staging_root = repo.join("target").join("release").join("package");
	let staging_dir = staging_root.join(&package_name);
	if staging_dir.exists() {
		if let Err(err) = fs::remove_dir_all(&staging_dir) {
			eprintln!("release-package: remove {}: {err}", staging_dir.display());
			return false;
		}
	}
	if !copy_dir_contents(&source_package, &staging_dir) {
		return false;
	}

	let output_dir = output_dir.unwrap_or_else(|| repo.join("release-packages"));
	let zip_path = output_dir.join(format!("{package_name}.zip"));
	if !create_release_zip(&staging_root, &package_name, &zip_path) {
		return false;
	}

	if !keep_staging {
		if let Err(err) = fs::remove_dir_all(&staging_dir) {
			eprintln!("release-package: remove {}: {err}", staging_dir.display());
			return false;
		}
		if staging_root.is_dir() && fs::read_dir(&staging_root).is_ok_and(|mut entries| entries.next().is_none()) {
			let _ = fs::remove_dir(&staging_root);
		}
	}

	let size = fs::metadata(&zip_path).map(|metadata| metadata.len()).unwrap_or(0);
	println!("release-package: created {}", zip_path.display());
	println!("release-package: size {size} bytes");
	println!("PACKAGE_PATH={}", zip_path.display());
	true
}

fn print_spout2_usage() {
	eprintln!(
		"cargo xtask spout2 [--clean] [--no-fetch] [--ref <git-ref>]\n\
	\n\
	Spout2 を target/spout2/Spout2 に取得し、CMake Release ビルド後、\n\
	target/package/un-avatar/Spout.dll と LICENSES/Spout2-BSD-2-Clause.txt を配置する。\n\
	既定 ref は {DEFAULT_SPOUT2_REF}。UN_AVATAR_SPOUT2_REF でも上書き可能。"
	);
}

fn print_package_usage() {
	eprintln!(
		"cargo xtask package [--skip-spout2]\n\
	\n\
	Releaseビルドを行い、target/package/un-avatar に最小配布レイアウトを作る。\n\
	既定では cargo xtask spout2 も実行し、spout-sdk feature付きでrendererをビルドする。"
	);
}

fn print_release_package_usage() {
	eprintln!(
		"cargo xtask release-package [--version <version>] [--output-dir <path>] [--skip-build] [--skip-spout2] [--keep-staging]\n\
	\n\
	配布ディレクトリを作成し、release-packages/un-avatar-<version>.zip を生成する。\n\
	既定versionは Cargo.toml の workspace.package.version。\n\
	--skip-build は既存の target/package/un-avatar をzip化する。"
	);
}

fn print_build_usage() {
	eprintln!(
		"cargo xtask build [--release]\n\
	\n\
	Supervisor frontend を npm build で更新してから、Supervisor と Renderer を同じ cargo profile でビルドする。\n\
	cargo build の default-members では Renderer が更新されないため、開発中の実行前ビルドはこのコマンドを使う。"
	);
}

fn print_run_usage() {
	eprintln!(
		"cargo xtask run [--release] [-- <supervisor args>]\n\
	\n\
	cargo xtask build と同じ手順で Supervisor/Renderer を更新し、target/{{debug|release}}/un-avatar-supervisor を直接起動する。\n\
	Renderer は Supervisor から同じ target profile の実行ファイルとして起動される。"
	);
}

fn print_run_renderer_usage() {
	eprintln!(
		"cargo xtask run-renderer --profile <name> [--release] [-- <renderer args>]\n\
		cargo xtask run-renderer --manifest <path> [--release] [-- <renderer args>]\n\
	\n\
	UN Avatar の user profile dir (%APPDATA%/UN Avatar/profiles) を優先し、次に repo の profiles/ を探す。\n\
	<name> は file stem、timestamp接頭辞を除いた stem、[profile].id、[profile].display_name、title に一致する。\n\
	例: cargo xtask run-renderer --profile model1\n\
	    cargo xtask run-renderer --profile model2 -- --debug-material-dump"
	);
}

fn print_usage() {
	eprintln!(
		"un-avatar xtask\n\
\n\
使い方: cargo xtask <command>\n\
\n\
commands:\n\
  build        frontend dist を更新し、Supervisor と Renderer を同じ profile でビルド\n\
  run          build 後に target/{{debug|release}}/un-avatar-supervisor を直接起動\n\
  fmt          cargo fmt --all\n\
  check        cargo check --workspace\n\
  test         cargo test --workspace\n\
  smoke        一時 .una で CLI validate / formats list / sample plugin / convert を確認\n\
  render-smoke renderer manifestを生成し、fixture glTFを起動前検証でimportできることを確認（windowは開かない）\n\
  run-renderer  profile名またはmanifest pathからrenderer windowを起動\n\
	acceptance-preflight MVP acceptance の実機確認前に必要な高速preflightを実行\n\
	acceptance-prepare   MVP acceptance の証跡テンプレートと実測用manifestを生成\n\
  spout2       Spout2 を取得・CMake Release ビルドし、配布物へ配置\n\
  package      Releaseビルドし、target/package/un-avatar に最小配布レイアウトを作る\n\
	release-package target/package/un-avatar を release-packages/un-avatar-<version>.zip に固める\n\
  ci           fmt --check → check --workspace → test --workspace → smoke → render-smoke\n"
	);
}

fn main() {
	let repo = repo_root();
	let mut args = env::args().skip(1);
	let Some(cmd) = args.next() else {
		print_usage();
		process::exit(2);
	};

	let ok = match cmd.as_str() {
		"build" => run_build(repo, args),
		"run" => run_app(repo, args),
		"fmt" => run_cargo(repo, &["fmt", "--all"]).success(),
		"check" => run_cargo(repo, &["check", "--workspace"]).success(),
		"test" => run_cargo(repo, &["test", "--workspace"]).success(),
		"smoke" => run_smoke(repo),
		"render-smoke" => run_render_smoke(repo),
		"run-renderer" => run_renderer(repo, args),
		"acceptance-preflight" => run_acceptance_preflight(repo),
		"acceptance-prepare" => run_acceptance_prepare(repo),
		"spout2" => run_spout2(repo, args),
		"package" => run_package(repo, args),
		"release-package" | "make-release-package" => run_release_package(repo, args),
		"ci" => {
			run_cargo(repo, &["fmt", "--all", "--", "--check"]).success()
				&& run_cargo(repo, &["check", "--workspace"]).success()
				&& run_cargo(repo, &["test", "--workspace"]).success()
				&& run_smoke(repo)
				&& run_render_smoke(repo)
		}
		"help" | "--help" | "-h" => {
			print_usage();
			true
		}
		other => {
			eprintln!("不明な command: {other}\n");
			print_usage();
			false
		}
	};

	process::exit(if ok { 0 } else { 1 });
}
