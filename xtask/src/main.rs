//! UN Avatar workspace 用 xtask。`cargo xtask ci` 等を拡張する。

use std::{
	collections::BTreeMap,
	env, fs,
	io::{BufReader, BufWriter, Read, Write},
	path::{Path, PathBuf},
	process::{self, Command, Stdio},
	time::{Duration, Instant},
};

use glam::{EulerRot, Mat4, Quat, Vec3};
use un_avatar_core::UnaDocument;
use un_avatar_io::{AvatarImporter, ImportContext, ImportInput, ImportOptions};
use un_avatar_skeleton::{apply_un_motion_frame_to_document_with_rest, ApplyUnMotionFrameOpts};
use un_avatar_types::HumanoidProfile;
use un_avatar_zenoh::UnAvatarZenohReceiver;
use un_motion_frame::{
	BodyMotion, BoneSample, CoordinateSpace, Finger, FingerPose, HandMotion, HumanoidBone, HumanoidPose, Quatf, SampleState, TrackingState,
	TransformSample, UNMotionFrame,
};
use un_motion_frame_zenoh::ZenohTopicStrategy;
use zip::{write::SimpleFileOptions, CompressionMethod};

const SPOUT2_REPO_URL: &str = "https://github.com/leadedge/Spout2.git";
const DEFAULT_SPOUT2_REF: &str = "2.007.017";
const COPY_BUFFER_SIZE: usize = 64 * 1024;
const UNITY_EXPORTER_PACKAGE: &str = "un-avatar-unity-exporter";

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
			eprintln!("copy: mkdir {}: {e}", parent.display());
			return false;
		}
	}
	match fs::copy(src, dst) {
		Ok(_) => true,
		Err(e) => {
			eprintln!("copy: {} -> {}: {e}", src.display(), dst.display());
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

fn unity_exporter_source_dir(repo: &Path) -> PathBuf {
	repo.join("unity").join(UNITY_EXPORTER_PACKAGE)
}

fn unity_fpng_source_dir(repo: &Path) -> PathBuf {
	unity_exporter_source_dir(repo).join("Native").join("unavatar_fpng")
}

fn unity_fpng_build_dir(repo: &Path) -> PathBuf {
	repo.join("target").join("unity-fpng").join("build")
}

fn unity_fpng_output_dir(repo: &Path) -> PathBuf {
	repo.join("target").join("unity-fpng").join("plugin")
}

fn unity_fpng_library_file_name() -> &'static str {
	if cfg!(windows) {
		"unavatar_fpng.dll"
	} else if cfg!(target_os = "macos") {
		"libunavatar_fpng.dylib"
	} else {
		"libunavatar_fpng.so"
	}
}

fn unity_fpng_meta_template(repo: &Path) -> PathBuf {
	unity_fpng_source_dir(repo).join("unavatar_fpng.plugin.meta.template")
}

fn build_unity_fpng(repo: &Path) -> bool {
	let source = unity_fpng_source_dir(repo);
	let build = unity_fpng_build_dir(repo);
	let output = unity_fpng_output_dir(repo);
	if !source.join("CMakeLists.txt").is_file() {
		eprintln!("unity-fpng: source missing CMakeLists.txt: {}", source.display());
		return false;
	}
	if let Err(e) = fs::create_dir_all(&build) {
		eprintln!("unity-fpng: mkdir {}: {e}", build.display());
		return false;
	}
	if let Err(e) = fs::create_dir_all(&output) {
		eprintln!("unity-fpng: mkdir {}: {e}", output.display());
		return false;
	}
	println!("unity-fpng: cmake configure -> {}", build.display());
	let output_arg = format!("-DUNAVATAR_FPNG_OUTPUT_DIR={}", output.display());
	let mut configure_args = vec!["-S", path_str(&source), "-B", path_str(&build)];
	let generator = env::var("UN_AVATAR_CMAKE_GENERATOR").ok().or_else(|| {
		if cfg!(windows) {
			Some("Visual Studio 17 2022".to_string())
		} else {
			None
		}
	});
	if let Some(generator) = generator.as_deref() {
		configure_args.extend(["-G", generator]);
	}
	let arch = env::var("UN_AVATAR_CMAKE_ARCH")
		.ok()
		.or_else(|| if cfg!(windows) { Some("x64".to_string()) } else { None });
	if let Some(arch) = arch.as_deref() {
		configure_args.extend(["-A", arch]);
	}
	configure_args.push(&output_arg);
	let configured = run_tool(repo, "cmake", &configure_args).success();
	if !configured {
		return false;
	}
	println!("unity-fpng: cmake build Release");
	run_tool(repo, "cmake", &["--build", path_str(&build), "--config", "Release"]).success()
}

fn copy_unity_fpng_to_package(repo: &Path, library: &Path, unity_package_dir: &Path) -> bool {
	if !library.is_file() {
		eprintln!("unity-fpng: native library not found: {}", library.display());
		return false;
	}
	let file_name = unity_fpng_library_file_name();
	let plugin_dir = unity_package_dir.join("Editor").join("Plugins").join("x86_64");
	let plugin = plugin_dir.join(file_name);
	let meta = plugin_dir.join(format!("{file_name}.meta"));
	if !copy_file_to(library, &plugin) {
		return false;
	}
	if let Some(parent) = meta.parent() {
		if let Err(e) = fs::create_dir_all(parent) {
			eprintln!("unity-fpng: mkdir {}: {e}", parent.display());
			return false;
		}
	}
	let meta_template = unity_fpng_meta_template(repo);
	if !copy_file_to(&meta_template, &meta) {
		return false;
	}
	println!("unity-fpng: staged {}", plugin.display());
	true
}

fn stage_unity_fpng(repo: &Path, unity_package_dir: &Path) -> bool {
	let library = unity_fpng_output_dir(repo).join(unity_fpng_library_file_name());
	copy_unity_fpng_to_package(repo, &library, unity_package_dir)
}

fn stage_unity_fpng_for_development(repo: &Path, required: bool) -> bool {
	let library = unity_fpng_output_dir(repo).join(unity_fpng_library_file_name());
	let source_package = unity_exporter_source_dir(repo);
	if copy_unity_fpng_to_package(repo, &library, &source_package) {
		return true;
	}
	if required {
		return false;
	}
	eprintln!(
		"unity-fpng: development package copy failed; continuing because staged package output is still valid. Close Unity Editor and run `cargo xtask unity-fpng` to refresh the development copy."
	);
	true
}

fn run_unity_fpng(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	for arg in args {
		match arg.as_str() {
			"help" | "--help" | "-h" => {
				print_unity_fpng_usage();
				return true;
			}
			other => {
				eprintln!("unity-fpng: 不明な option: {other}");
				print_unity_fpng_usage();
				return false;
			}
		}
	}
	build_unity_fpng(repo) && stage_unity_fpng_for_development(repo, true)
}

fn stage_unity_exporter_package(repo: &Path, dst: &Path) -> bool {
	let source = unity_exporter_source_dir(repo);
	if !source.is_dir() {
		eprintln!("unity-exporter-package: source directory not found: {}", source.display());
		return false;
	}
	if !build_unity_fpng(repo) {
		return false;
	}
	if dst.exists() {
		if let Err(err) = fs::remove_dir_all(dst) {
			eprintln!("unity-exporter-package: remove {}: {err}", dst.display());
			return false;
		}
	}
	if !copy_unity_exporter_source_package(&source, dst) {
		return false;
	}
	if !stage_unity_fpng(repo, dst) || !stage_unity_fpng_for_development(repo, false) {
		return false;
	}
	println!("unity-exporter-package: staged {}", dst.display());
	true
}

fn run_unity_exporter_package(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut output_dir = None;
	let mut iter = args.peekable();
	while let Some(arg) = iter.next() {
		match arg.as_str() {
			"--output-dir" => {
				let Some(value) = iter.next() else {
					eprintln!("unity-exporter-package: --output-dir には path が必要です");
					return false;
				};
				output_dir = Some(PathBuf::from(value));
			}
			"help" | "--help" | "-h" => {
				print_unity_exporter_package_usage();
				return true;
			}
			other => {
				eprintln!("unity-exporter-package: 不明な option: {other}");
				print_unity_exporter_package_usage();
				return false;
			}
		}
	}
	let dst = output_dir.unwrap_or_else(|| repo.join("target").join("unity").join(UNITY_EXPORTER_PACKAGE));
	stage_unity_exporter_package(repo, &dst)
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

fn copy_unity_exporter_source_package(src: &Path, dst: &Path) -> bool {
	fn should_skip_unity_exporter_file(package_root: &Path, path: &Path) -> bool {
		let is_fpng_plugin = path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
			name == "unavatar_fpng.dll"
				|| name == "unavatar_fpng.dll.meta"
				|| name == "libunavatar_fpng.so"
				|| name == "libunavatar_fpng.so.meta"
				|| name == "libunavatar_fpng.dylib"
				|| name == "libunavatar_fpng.dylib.meta"
		});
		if !is_fpng_plugin {
			return false;
		}
		let Some(parent) = path.parent() else {
			return false;
		};
		parent.strip_prefix(package_root).ok().is_some_and(|relative_parent| {
			let components = relative_parent
				.components()
				.map(|component| component.as_os_str().to_string_lossy())
				.collect::<Vec<_>>();
			components.as_slice() == ["Editor", "Plugins", "x86_64"]
		})
	}

	fn visit(package_root: &Path, src: &Path, dst: &Path) -> bool {
		let entries = match fs::read_dir(src) {
			Ok(entries) => entries,
			Err(err) => {
				eprintln!("unity-exporter-package: read {}: {err}", src.display());
				return false;
			}
		};
		if let Err(err) = fs::create_dir_all(dst) {
			eprintln!("unity-exporter-package: mkdir {}: {err}", dst.display());
			return false;
		}
		for entry in entries.flatten() {
			let path = entry.path();
			if should_skip_unity_exporter_file(package_root, &path) {
				continue;
			}
			let target = dst.join(entry.file_name());
			if path.is_dir() {
				if !visit(package_root, &path, &target) {
					return false;
				}
			} else if path.is_file() && !copy_file_to(&path, &target) {
				return false;
			}
		}
		true
	}

	visit(src, src, dst)
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
		&& stage_package_docs(repo)
		&& stage_unity_exporter_package(repo, &package.join("unity").join(UNITY_EXPORTER_PACKAGE));
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
	Unity exporter package は target/package/un-avatar/unity/ に同梱する。\n\
	既定では cargo xtask spout2 も実行し、spout-sdk feature付きでrendererをビルドする。"
	);
}

fn print_unity_exporter_package_usage() {
	eprintln!(
		"cargo xtask unity-exporter-package [--output-dir <path>]\n\
	\n\
	unity/un-avatar-unity-exporter を UPM package layout としてコピーし、native fpng plugin をビルドして同梱する。\n\
	ビルド済み fpng plugin は開発用 local package にも配置するが、gitignore 対象とする。\n\
	既定出力先は target/unity/un-avatar-unity-exporter。Unity Editor の compile は実行しない。"
	);
}

fn print_unity_fpng_usage() {
	eprintln!(
		"cargo xtask unity-fpng\n\
	\n\
	Unity Exporter の native fpng plugin をビルドし、開発用 local package の Editor/Plugins/x86_64 に配置する。"
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

#[derive(Clone)]
struct RetargetAuditModel {
	label: &'static str,
	document: UnaDocument,
	rest_nodes: Vec<un_avatar_core::UnaSceneNode>,
}

struct RetargetAxisCase {
	name: &'static str,
	bone: HumanoidBone,
	source_axis: Vec3,
	target_axis: Vec3,
	successor_key: &'static str,
}

struct RetargetFingerAxisCase {
	name: &'static str,
	finger: Finger,
	coordinate_space: CoordinateSpace,
	side_prefix: &'static str,
	rotation: Quat,
	joint_index: usize,
	parent_key: &'static str,
	successor_key: &'static str,
	reference_label: &'static str,
}

fn quatf(q: Quat) -> Quatf {
	Quatf {
		x: q.x,
		y: q.y,
		z: q.z,
		w: q.w,
	}
}

fn identity_transform_sample() -> TransformSample {
	TransformSample {
		translation: None,
		rotation: Some(quatf(Quat::IDENTITY)),
		scale: None,
		linear_velocity: None,
		angular_velocity: None,
	}
}

fn rotation_transform_sample(rotation: Quat) -> TransformSample {
	TransformSample {
		translation: None,
		rotation: Some(quatf(rotation)),
		scale: None,
		linear_velocity: None,
		angular_velocity: None,
	}
}

fn retarget_body_frame(bone: HumanoidBone, rotation: Quat) -> UNMotionFrame {
	let mut frame = UNMotionFrame::new(0);
	frame.header.coordinate_space = CoordinateSpace::UNMotion;
	frame.body = Some(BodyMotion {
		tracking_state: TrackingState::Valid,
		confidence: 1.0,
		humanoid: Some(HumanoidPose {
			root: None,
			bones: vec![BoneSample {
				bone,
				transform: TransformSample {
					translation: None,
					rotation: Some(quatf(rotation)),
					scale: None,
					linear_velocity: None,
					angular_velocity: None,
				},
				confidence: 1.0,
				source_index: Some(0),
				state: SampleState::Valid,
			}],
		}),
	});
	frame
}

fn retarget_finger_frame(
	side_prefix: &str,
	finger: Finger,
	coordinate_space: CoordinateSpace,
	joint_index: usize,
	rotation: Quat,
) -> UNMotionFrame {
	let mut joints = vec![identity_transform_sample(), identity_transform_sample(), identity_transform_sample()];
	if let Some(joint) = joints.get_mut(joint_index) {
		*joint = rotation_transform_sample(rotation);
	}
	let hand = HandMotion {
		tracking_state: TrackingState::Valid,
		confidence: 1.0,
		wrist: None,
		fingers: vec![FingerPose {
			finger,
			joints,
			confidence: 1.0,
		}],
	};
	let mut frame = UNMotionFrame::new(0);
	frame.header.coordinate_space = coordinate_space;
	if side_prefix == "left" {
		frame.left_hand = Some(hand);
	} else {
		frame.right_hand = Some(hand);
	}
	frame
}

fn retarget_thumb_proximal_unmotion_frame(side_prefix: &str, curl: f32) -> UNMotionFrame {
	let yaw = if side_prefix == "left" { -0.44 + curl } else { 0.44 - curl };
	retarget_finger_frame(
		side_prefix,
		Finger::Thumb,
		CoordinateSpace::UNMotion,
		0,
		Quat::from_rotation_y(yaw),
	)
}

fn retarget_thumb_proximal_unmotion_z_curl_frame(side_prefix: &str, curl: f32) -> UNMotionFrame {
	let yaw = if side_prefix == "left" { -0.44 } else { 0.44 };
	let roll = if side_prefix == "left" { curl } else { -curl };
	retarget_finger_frame(
		side_prefix,
		Finger::Thumb,
		CoordinateSpace::UNMotion,
		0,
		Quat::from_euler(EulerRot::XYZ, 0.0, yaw, roll),
	)
}

fn retarget_full_thumb_unmotion_curl_frame(side_prefix: &str, curl: f32) -> UNMotionFrame {
	let yaw = if side_prefix == "left" { -0.44 } else { 0.44 };
	let side = if side_prefix == "left" { 1.0 } else { -1.0 };
	let hand = HandMotion {
		tracking_state: TrackingState::Valid,
		confidence: 1.0,
		wrist: None,
		fingers: vec![FingerPose {
			finger: Finger::Thumb,
			joints: vec![
				rotation_transform_sample(Quat::from_euler(EulerRot::XYZ, 0.0, yaw, side * curl.min(0.61))),
				rotation_transform_sample(Quat::from_rotation_y(side * (curl * 1.57).min(1.57))),
				rotation_transform_sample(Quat::from_rotation_y(side * (curl * 0.70).min(0.70))),
			],
			confidence: 1.0,
		}],
	};
	let mut frame = UNMotionFrame::new(0);
	frame.header.coordinate_space = CoordinateSpace::UNMotion;
	if side_prefix == "left" {
		frame.left_hand = Some(hand);
	} else {
		frame.right_hand = Some(hand);
	}
	frame
}

fn scene_world_matrices(nodes: &[un_avatar_core::UnaSceneNode], roots: &[usize]) -> Vec<Mat4> {
	let mut world = vec![Mat4::IDENTITY; nodes.len().max(1)];
	fn visit(nodes: &[un_avatar_core::UnaSceneNode], idx: usize, parent: Mat4, world: &mut [Mat4]) {
		if idx >= nodes.len() {
			return;
		}
		let local = Mat4::from_cols_array(&nodes[idx].transform);
		let w = parent * local;
		world[idx] = w;
		for &child in &nodes[idx].children {
			visit(nodes, child, w, world);
		}
	}
	for &root in roots {
		visit(nodes, root, Mat4::IDENTITY, &mut world);
	}
	world
}

fn profile_node_index(profile: &HumanoidProfile, key: &str) -> Option<usize> {
	let normalized = normalize_humanoid_profile_key(key);
	profile
		.bone_node_indices
		.iter()
		.find(|(candidate, _)| normalize_humanoid_profile_key(candidate) == normalized)
		.map(|(_, index)| *index)
}

fn normalize_humanoid_profile_key(value: &str) -> String {
	value
		.chars()
		.filter(|ch| ch.is_ascii_alphanumeric())
		.map(|ch| ch.to_ascii_lowercase())
		.collect()
}

fn normalized_world_successor_axis(document: &UnaDocument, parent_key: &str, successor_key: &str) -> Result<Vec3, String> {
	let scene = document.scene.as_ref().ok_or("document has no scene")?;
	let profile = document.humanoid_profile.as_ref().ok_or("document has no humanoid profile")?;
	let parent = profile_node_index(profile, parent_key).ok_or_else(|| format!("missing humanoid bone: {parent_key}"))?;
	let child = profile_node_index(profile, successor_key).ok_or_else(|| format!("missing humanoid successor: {successor_key}"))?;
	let world = scene_world_matrices(&scene.nodes, &scene.roots);
	let from = world[parent].transform_point3(Vec3::ZERO);
	let to = world[child].transform_point3(Vec3::ZERO);
	(to - from)
		.try_normalize()
		.ok_or_else(|| format!("zero-length axis: {parent_key}->{successor_key}"))
}

fn normalized_world_basis(document: &UnaDocument, key: &str) -> Result<[Vec3; 3], String> {
	let scene = document.scene.as_ref().ok_or("document has no scene")?;
	let profile = document.humanoid_profile.as_ref().ok_or("document has no humanoid profile")?;
	let node = profile_node_index(profile, key).ok_or_else(|| format!("missing humanoid bone: {key}"))?;
	let world = scene_world_matrices(&scene.nodes, &scene.roots);
	let (_, rotation, _) = world[node].to_scale_rotation_translation();
	Ok([
		(rotation * Vec3::X).normalize_or_zero(),
		(rotation * Vec3::Y).normalize_or_zero(),
		(rotation * Vec3::Z).normalize_or_zero(),
	])
}

fn world_point(document: &UnaDocument, key: &str) -> Result<Vec3, String> {
	let scene = document.scene.as_ref().ok_or("document has no scene")?;
	let profile = document.humanoid_profile.as_ref().ok_or("document has no humanoid profile")?;
	let node = profile_node_index(profile, key).ok_or_else(|| format!("missing humanoid bone: {key}"))?;
	let world = scene_world_matrices(&scene.nodes, &scene.roots);
	Ok(world[node].transform_point3(Vec3::ZERO))
}

fn print_thumb_index_distance_delta(
	name: &str,
	side_prefix: &str,
	model: &RetargetAuditModel,
	open_document: &UnaDocument,
	curled_document: &UnaDocument,
) {
	let thumb_key = format!("{side_prefix}thumbdistal");
	let index_key = format!("{side_prefix}indexproximal");
	let Ok(open_thumb) = world_point(open_document, &thumb_key) else {
		return;
	};
	let Ok(open_index) = world_point(open_document, &index_key) else {
		return;
	};
	let Ok(curled_thumb) = world_point(curled_document, &thumb_key) else {
		return;
	};
	let Ok(curled_index) = world_point(curled_document, &index_key) else {
		return;
	};
	let open_distance = open_thumb.distance(open_index);
	let curled_distance = curled_thumb.distance(curled_index);
	println!(
		"  {:24} {name}_thumb_index_dist open={open_distance:.5} curled={curled_distance:.5} delta={:+.5}",
		model.label,
		curled_distance - open_distance
	);
}

fn compare_delta_direction(
	ok: &mut bool,
	name: &str,
	model_deltas: &BTreeMap<&'static str, Vec3>,
	reference_label: &'static str,
	max_unavatar_delta_deg: f32,
) {
	if let Some(reference) = model_deltas.get(reference_label).copied() {
		for (label, delta) in model_deltas {
			let angle = reference.angle_between(*delta).to_degrees();
			println!("  {:24} {name}_delta_to_{}={angle:.3}deg", label, reference_label);
			if label.starts_with("unavatar:") && angle > max_unavatar_delta_deg {
				eprintln!("retarget-audit: {} {name} differs from {} by {angle:.3}deg", label, reference_label);
				*ok = false;
			}
		}
	}
}

fn print_thumb_rest_chain(model: &RetargetAuditModel) {
	let Some(scene) = model.document.scene.as_ref() else {
		return;
	};
	let Some(profile) = model.document.humanoid_profile.as_ref() else {
		return;
	};
	for key in [
		"leftthumbproximal",
		"leftthumbintermediate",
		"leftthumbdistal",
		"rightthumbproximal",
		"rightthumbintermediate",
		"rightthumbdistal",
	] {
		let Some(index) = profile_node_index(profile, key) else {
			continue;
		};
		let Some(node) = scene.nodes.get(index) else {
			continue;
		};
		let (_, rotation, translation) = Mat4::from_cols_array(&node.transform).to_scale_rotation_translation();
		println!(
			"  {:24} rest {key} node={} name={} t=({:+.4},{:+.4},{:+.4}) children={:?} local_y=({:+.3},{:+.3},{:+.3})",
			model.label,
			index,
			node.name.as_deref().unwrap_or(""),
			translation.x,
			translation.y,
			translation.z,
			node.children,
			(rotation * Vec3::Y).x,
			(rotation * Vec3::Y).y,
			(rotation * Vec3::Y).z,
		);
	}
}

fn print_thumb_hinge_candidates(model: &RetargetAuditModel) {
	let Some(scene) = model.document.scene.as_ref() else {
		return;
	};
	let Some(profile) = model.document.humanoid_profile.as_ref() else {
		return;
	};
	let world = scene_world_matrices(&scene.nodes, &scene.roots);
	for side in ["left", "right"] {
		let thumb_key = format!("{side}thumbproximal");
		let successor_key = format!("{side}thumbintermediate");
		let Some(thumb_index) = profile_node_index(profile, &thumb_key) else {
			continue;
		};
		let Some(successor_index) = profile_node_index(profile, &successor_key) else {
			continue;
		};
		let thumb_axis = (world[successor_index].transform_point3(Vec3::ZERO) - world[thumb_index].transform_point3(Vec3::ZERO))
			.normalize_or_zero();
		let (_, thumb_rot, _) = world[thumb_index].to_scale_rotation_translation();
		let candidates = [
			("thumb_basis_x", thumb_rot * Vec3::X),
			("thumb_basis_y", thumb_rot * Vec3::Y),
			("thumb_basis_z", thumb_rot * Vec3::Z),
		];
		let sign = if side == "left" { 1.0 } else { -1.0 };
		for (name, axis) in candidates {
			let curled = Quat::from_axis_angle(axis.normalize_or_zero(), sign * 0.8) * thumb_axis;
			println!(
				"  {:24} {side} {name} hinge=({:+.4},{:+.4},{:+.4}) curled_axis=({:+.4},{:+.4},{:+.4})",
				model.label, axis.x, axis.y, axis.z, curled.x, curled.y, curled.z
			);
		}
	}
}

fn import_vrm_model(path: &Path) -> Result<UnaDocument, String> {
	let importer = un_avatar_io_vrm::VrmImporter;
	let mut ctx = ImportContext::dummy();
	importer
		.import(&mut ctx, ImportInput::Path(path.to_path_buf()), ImportOptions)
		.map(|result| result.document)
		.map_err(|e| format!("{}: {e}", path.display()))
}

fn import_unavatar_model(path: &Path) -> Result<UnaDocument, String> {
	let importer = un_avatar_io_gltf::GltfImporter;
	let mut ctx = ImportContext::dummy();
	importer
		.import(&mut ctx, ImportInput::Path(path.to_path_buf()), ImportOptions)
		.map(|result| result.document)
		.map_err(|e| format!("{}: {e}", path.display()))
}

fn retarget_audit_model(label: &'static str, document: UnaDocument) -> Result<RetargetAuditModel, String> {
	let scene = document.scene.as_ref().ok_or_else(|| format!("{label}: missing scene"))?;
	let rest_nodes = scene.nodes.clone();
	if document.humanoid_profile.is_none() {
		return Err(format!("{label}: missing humanoid profile"));
	}
	if scene.roots.is_empty() {
		return Err(format!("{label}: missing scene roots"));
	}
	Ok(RetargetAuditModel {
		label,
		document,
		rest_nodes,
	})
}

fn find_vrm1_fixture(repo: &Path) -> Option<PathBuf> {
	fs::read_dir(repo.join("target/tmp"))
		.ok()?
		.filter_map(Result::ok)
		.map(|entry| entry.path())
		.find(|path| {
			path.extension()
				.and_then(|ext| ext.to_str())
				.is_some_and(|ext| ext.eq_ignore_ascii_case("vrm"))
				&& path
					.file_name()
					.and_then(|name| name.to_str())
					.is_some_and(|name| name.contains("vrm1.0-optimized"))
		})
}

fn run_retarget_audit(repo: &Path) -> bool {
	let model1 = repo.join("target/tmp/model1.vrm");
	let vrm1 = match find_vrm1_fixture(repo) {
		Some(path) => path,
		None => {
			eprintln!("retarget-audit: VRM1 fixture not found under target/tmp");
			return false;
		}
	};
	let mizuki = repo.join("target/tmp/mizuki-split.unavatar");
	for path in [&model1, &vrm1, &mizuki] {
		if !path.is_file() {
			eprintln!("retarget-audit: fixture not found: {}", path.display());
			return false;
		}
	}

	let models = match (
		import_vrm_model(&model1).and_then(|doc| retarget_audit_model("vrm0:model1", doc)),
		import_vrm_model(&vrm1).and_then(|doc| retarget_audit_model("vrm1:usagi", doc)),
		import_unavatar_model(&mizuki).and_then(|doc| retarget_audit_model("unavatar:mizuki-split", doc)),
	) {
		(Ok(a), Ok(b), Ok(c)) => vec![a, b, c],
		(a, b, c) => {
			for result in [a, b, c] {
				if let Err(error) = result {
					eprintln!("retarget-audit: {error}");
				}
			}
			return false;
		}
	};

	for model in &models {
		print_thumb_rest_chain(model);
		print_thumb_hinge_candidates(model);
	}

	let cases = [
		RetargetAxisCase {
			name: "left upper arm raise",
			bone: HumanoidBone::LeftUpperArm,
			source_axis: -Vec3::X,
			target_axis: Vec3::Y,
			successor_key: "leftlowerarm",
		},
		RetargetAxisCase {
			name: "right upper arm raise",
			bone: HumanoidBone::RightUpperArm,
			source_axis: Vec3::X,
			target_axis: Vec3::Y,
			successor_key: "rightlowerarm",
		},
		RetargetAxisCase {
			name: "left lower arm forward",
			bone: HumanoidBone::LeftLowerArm,
			source_axis: -Vec3::X,
			target_axis: Vec3::Z,
			successor_key: "lefthand",
		},
		RetargetAxisCase {
			name: "right lower arm forward",
			bone: HumanoidBone::RightLowerArm,
			source_axis: Vec3::X,
			target_axis: Vec3::Z,
			successor_key: "righthand",
		},
		RetargetAxisCase {
			name: "left upper leg down",
			bone: HumanoidBone::LeftUpperLeg,
			source_axis: -Vec3::Y,
			target_axis: -Vec3::Y,
			successor_key: "leftlowerleg",
		},
		RetargetAxisCase {
			name: "right upper leg down",
			bone: HumanoidBone::RightUpperLeg,
			source_axis: -Vec3::Y,
			target_axis: -Vec3::Y,
			successor_key: "rightlowerleg",
		},
	];

	let mut ok = true;
	for case in cases {
		println!("retarget-audit: {}", case.name);
		let mut axes = BTreeMap::new();
		let rotation = Quat::from_rotation_arc(case.source_axis, case.target_axis);
		for model in &models {
			let mut document = model.document.clone();
			let frame = retarget_body_frame(case.bone, rotation);
			apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&model.rest_nodes));
			let parent_key = un_avatar_skeleton::humanoid_bone_profile_key(case.bone);
			match normalized_world_successor_axis(&document, parent_key, case.successor_key) {
				Ok(axis) => {
					println!("  {:24} axis=({:+.4}, {:+.4}, {:+.4})", model.label, axis.x, axis.y, axis.z);
					axes.insert(model.label, axis);
				}
				Err(error) => {
					eprintln!("  {:24} ERROR {error}", model.label);
					ok = false;
				}
			}
		}
		if let Some(reference) = axes.get("vrm1:usagi").copied() {
			for (label, axis) in &axes {
				let angle = reference.angle_between(*axis).to_degrees();
				println!("  {:24} delta_to_vrm1={angle:.3}deg", label);
				if angle > 8.0 {
					eprintln!("retarget-audit: {} differs from vrm1 by {angle:.3}deg in {}", label, case.name);
					ok = false;
				}
			}
		}
	}

	let finger_cases = [
		RetargetFingerAxisCase {
			name: "left index intermediate curl",
			finger: Finger::Index,
			coordinate_space: CoordinateSpace::UNMotion,
			side_prefix: "left",
			rotation: Quat::from_rotation_z(0.5),
			joint_index: 1,
			parent_key: "leftindexintermediate",
			successor_key: "leftindexdistal",
			reference_label: "vrm0:model1",
		},
		RetargetFingerAxisCase {
			name: "right index intermediate curl",
			finger: Finger::Index,
			coordinate_space: CoordinateSpace::UNMotion,
			side_prefix: "right",
			rotation: Quat::from_rotation_z(-0.5),
			joint_index: 1,
			parent_key: "rightindexintermediate",
			successor_key: "rightindexdistal",
			reference_label: "vrm0:model1",
		},
		RetargetFingerAxisCase {
			name: "left thumb proximal curl",
			finger: Finger::Thumb,
			coordinate_space: CoordinateSpace::UNMotion,
			side_prefix: "left",
			rotation: Quat::from_rotation_y(-0.44 + 0.5),
			joint_index: 0,
			parent_key: "leftthumbproximal",
			successor_key: "leftthumbintermediate",
			reference_label: "vrm0:model1",
		},
		RetargetFingerAxisCase {
			name: "right thumb proximal curl",
			finger: Finger::Thumb,
			coordinate_space: CoordinateSpace::UNMotion,
			side_prefix: "right",
			rotation: Quat::from_rotation_y(0.44 - 0.5),
			joint_index: 0,
			parent_key: "rightthumbproximal",
			successor_key: "rightthumbintermediate",
			reference_label: "vrm0:model1",
		},
		RetargetFingerAxisCase {
			name: "left thumb intermediate curl",
			finger: Finger::Thumb,
			coordinate_space: CoordinateSpace::UNMotion,
			side_prefix: "left",
			rotation: Quat::from_rotation_y(0.5),
			joint_index: 1,
			parent_key: "leftthumbintermediate",
			successor_key: "leftthumbdistal",
			reference_label: "vrm0:model1",
		},
		RetargetFingerAxisCase {
			name: "right thumb intermediate curl",
			finger: Finger::Thumb,
			coordinate_space: CoordinateSpace::UNMotion,
			side_prefix: "right",
			rotation: Quat::from_rotation_y(-0.5),
			joint_index: 1,
			parent_key: "rightthumbintermediate",
			successor_key: "rightthumbdistal",
			reference_label: "vrm0:model1",
		},
	];

	for case in finger_cases {
		println!("retarget-audit: {}", case.name);
		let mut axes = BTreeMap::new();
		for model in &models {
			let mut document = model.document.clone();
			let frame = retarget_finger_frame(case.side_prefix, case.finger, case.coordinate_space, case.joint_index, case.rotation);
			apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&model.rest_nodes));
			match normalized_world_successor_axis(&document, case.parent_key, case.successor_key) {
				Ok(axis) => {
					println!("  {:24} axis=({:+.4}, {:+.4}, {:+.4})", model.label, axis.x, axis.y, axis.z);
					axes.insert(model.label, axis);
				}
				Err(error) => {
					eprintln!("  {:24} ERROR {error}", model.label);
					if model.label == case.reference_label {
						ok = false;
					}
				}
			}
		}
		if let Some(reference) = axes.get(case.reference_label).copied() {
			for (label, axis) in &axes {
				let angle = reference.angle_between(*axis).to_degrees();
				println!("  {:24} delta_to_{}={angle:.3}deg", label, case.reference_label);
				if angle > 8.0 {
					if label.starts_with("unavatar:") {
						eprintln!(
							"retarget-audit: {} differs from {} by {angle:.3}deg in {}",
							label, case.reference_label, case.name
						);
						ok = false;
					} else {
						eprintln!(
							"retarget-audit: note: {} differs from {} by {angle:.3}deg in {}",
							label, case.reference_label, case.name
						);
					}
				}
			}
		}
		if case.finger == Finger::Thumb {
			let mut bases = BTreeMap::new();
			for model in &models {
				let mut document = model.document.clone();
				let frame = retarget_finger_frame(case.side_prefix, case.finger, case.coordinate_space, case.joint_index, case.rotation);
				apply_un_motion_frame_to_document_with_rest(&mut document, &frame, ApplyUnMotionFrameOpts::default(), Some(&model.rest_nodes));
				if let Ok(basis) = normalized_world_basis(&document, case.parent_key) {
					println!(
						"  {:24} basis x=({:+.3},{:+.3},{:+.3}) y=({:+.3},{:+.3},{:+.3}) z=({:+.3},{:+.3},{:+.3})",
						model.label,
						basis[0].x,
						basis[0].y,
						basis[0].z,
						basis[1].x,
						basis[1].y,
						basis[1].z,
						basis[2].x,
						basis[2].y,
						basis[2].z
					);
					bases.insert(model.label, basis);
				}
			}
			if let Some(reference) = bases.get(case.reference_label) {
				for (label, basis) in &bases {
					for (axis_name, (a, b)) in ["x", "y", "z"].into_iter().zip(reference.iter().zip(basis.iter())) {
						println!(
							"  {:24} basis_{}_delta_to_{}={:.3}deg",
							label,
							axis_name,
							case.reference_label,
							a.angle_between(*b).to_degrees()
						);
					}
				}
			}
		}
	}

	for (name, side_prefix, parent_key, successor_key) in [
		(
			"left thumb proximal curl direction",
			"left",
			"leftthumbproximal",
			"leftthumbintermediate",
		),
		(
			"right thumb proximal curl direction",
			"right",
			"rightthumbproximal",
			"rightthumbintermediate",
		),
	] {
		println!("retarget-audit: {name}");
		let mut deltas = BTreeMap::new();
		let mut basis_x_deltas = BTreeMap::new();
		let mut basis_y_deltas = BTreeMap::new();
		let mut basis_z_deltas = BTreeMap::new();
		for model in &models {
			let mut open_document = model.document.clone();
			let mut curled_document = model.document.clone();
			let open_frame = retarget_thumb_proximal_unmotion_frame(side_prefix, 0.0);
			let curled_frame = retarget_thumb_proximal_unmotion_frame(side_prefix, 0.8);
			apply_un_motion_frame_to_document_with_rest(
				&mut open_document,
				&open_frame,
				ApplyUnMotionFrameOpts::default(),
				Some(&model.rest_nodes),
			);
			apply_un_motion_frame_to_document_with_rest(
				&mut curled_document,
				&curled_frame,
				ApplyUnMotionFrameOpts::default(),
				Some(&model.rest_nodes),
			);
			print_thumb_index_distance_delta("y_curl", side_prefix, model, &open_document, &curled_document);
			let open_axis = normalized_world_successor_axis(&open_document, parent_key, successor_key);
			let curled_axis = normalized_world_successor_axis(&curled_document, parent_key, successor_key);
			let open_basis = normalized_world_basis(&open_document, parent_key);
			let curled_basis = normalized_world_basis(&curled_document, parent_key);
			match (open_axis, curled_axis) {
				(Ok(open_axis), Ok(curled_axis)) => {
					let delta = curled_axis - open_axis;
					println!(
						"  {:24} open=({:+.4}, {:+.4}, {:+.4}) curled=({:+.4}, {:+.4}, {:+.4}) delta=({:+.4}, {:+.4}, {:+.4})",
						model.label,
						open_axis.x,
						open_axis.y,
						open_axis.z,
						curled_axis.x,
						curled_axis.y,
						curled_axis.z,
						delta.x,
						delta.y,
						delta.z
					);
					if delta.length_squared() > 1e-8 {
						deltas.insert(model.label, delta.normalize());
					}
				}
				(Err(error), _) | (_, Err(error)) => {
					eprintln!("  {:24} ERROR {error}", model.label);
					if model.label == "vrm0:model1" {
						ok = false;
					}
				}
			}
			match (open_basis, curled_basis) {
				(Ok(open_basis), Ok(curled_basis)) => {
					for (axis_name, open_axis, curled_axis, out) in [
						("basis_x", open_basis[0], curled_basis[0], &mut basis_x_deltas),
						("basis_y", open_basis[1], curled_basis[1], &mut basis_y_deltas),
						("basis_z", open_basis[2], curled_basis[2], &mut basis_z_deltas),
					] {
						let delta = curled_axis - open_axis;
						println!(
							"  {:24} {axis_name}_open=({:+.4}, {:+.4}, {:+.4}) {axis_name}_curled=({:+.4}, {:+.4}, {:+.4}) {axis_name}_delta=({:+.4}, {:+.4}, {:+.4})",
							model.label,
							open_axis.x,
							open_axis.y,
							open_axis.z,
							curled_axis.x,
							curled_axis.y,
							curled_axis.z,
							delta.x,
							delta.y,
							delta.z
						);
						if delta.length_squared() > 1e-8 {
							out.insert(model.label, delta.normalize());
						}
					}
				}
				(Err(error), _) | (_, Err(error)) => {
					eprintln!("  {:24} ERROR {error}", model.label);
					if model.label == "vrm0:model1" {
						ok = false;
					}
				}
			}
		}
		compare_delta_direction(&mut ok, "successor_axis_curl", &deltas, "vrm0:model1", 20.0);
		compare_delta_direction(&mut ok, "basis_x_curl", &basis_x_deltas, "vrm0:model1", 20.0);
		compare_delta_direction(&mut ok, "basis_y_curl", &basis_y_deltas, "vrm0:model1", 20.0);
		compare_delta_direction(&mut ok, "basis_z_curl", &basis_z_deltas, "vrm0:model1", 20.0);
	}
	for (name, side_prefix, parent_key, successor_key) in [
		(
			"left thumb proximal z-curl direction",
			"left",
			"leftthumbproximal",
			"leftthumbintermediate",
		),
		(
			"right thumb proximal z-curl direction",
			"right",
			"rightthumbproximal",
			"rightthumbintermediate",
		),
	] {
		println!("retarget-audit: {name}");
		let mut deltas = BTreeMap::new();
		for model in &models {
			let mut open_document = model.document.clone();
			let mut curled_document = model.document.clone();
			let open_frame = retarget_thumb_proximal_unmotion_z_curl_frame(side_prefix, 0.0);
			let curled_frame = retarget_thumb_proximal_unmotion_z_curl_frame(side_prefix, 0.8);
			apply_un_motion_frame_to_document_with_rest(
				&mut open_document,
				&open_frame,
				ApplyUnMotionFrameOpts::default(),
				Some(&model.rest_nodes),
			);
			apply_un_motion_frame_to_document_with_rest(
				&mut curled_document,
				&curled_frame,
				ApplyUnMotionFrameOpts::default(),
				Some(&model.rest_nodes),
			);
			print_thumb_index_distance_delta("z_curl", side_prefix, model, &open_document, &curled_document);
			match (
				normalized_world_successor_axis(&open_document, parent_key, successor_key),
				normalized_world_successor_axis(&curled_document, parent_key, successor_key),
			) {
				(Ok(open_axis), Ok(curled_axis)) => {
					let delta = curled_axis - open_axis;
					println!(
						"  {:24} open=({:+.4}, {:+.4}, {:+.4}) curled=({:+.4}, {:+.4}, {:+.4}) delta=({:+.4}, {:+.4}, {:+.4})",
						model.label,
						open_axis.x,
						open_axis.y,
						open_axis.z,
						curled_axis.x,
						curled_axis.y,
						curled_axis.z,
						delta.x,
						delta.y,
						delta.z
					);
					if delta.length_squared() > 1e-8 {
						deltas.insert(model.label, delta.normalize());
					}
				}
				(Err(error), _) | (_, Err(error)) => {
					eprintln!("  {:24} ERROR {error}", model.label);
					if model.label == "vrm0:model1" {
						ok = false;
					}
				}
			}
		}
		compare_delta_direction(&mut ok, "successor_axis_z_curl", &deltas, "vrm0:model1", 20.0);
	}
	for (name, side_prefix) in [("left full thumb curl", "left"), ("right full thumb curl", "right")] {
		println!("retarget-audit: {name}");
		for model in &models {
			let mut open_document = model.document.clone();
			let mut curled_document = model.document.clone();
			let open_frame = retarget_full_thumb_unmotion_curl_frame(side_prefix, 0.0);
			let curled_frame = retarget_full_thumb_unmotion_curl_frame(side_prefix, 1.0);
			apply_un_motion_frame_to_document_with_rest(
				&mut open_document,
				&open_frame,
				ApplyUnMotionFrameOpts::default(),
				Some(&model.rest_nodes),
			);
			apply_un_motion_frame_to_document_with_rest(
				&mut curled_document,
				&curled_frame,
				ApplyUnMotionFrameOpts::default(),
				Some(&model.rest_nodes),
			);
			print_thumb_index_distance_delta("full_curl", side_prefix, model, &open_document, &curled_document);
		}
	}
	ok
}

fn print_thumb_joints(prefix: &str, hand: Option<&HandMotion>) {
	let Some(hand) = hand else {
		return;
	};
	for finger in &hand.fingers {
		if finger.finger != Finger::Thumb {
			continue;
		}
		for (index, joint) in finger.joints.iter().enumerate() {
			let Some(q) = joint.rotation.as_ref() else {
				continue;
			};
			println!(
				"  {prefix}.thumb[{index}] q=({:+.5},{:+.5},{:+.5},{:+.5})",
				q.x, q.y, q.z, q.w
			);
		}
	}
}

fn run_unmotion_thumb_dump(args: &[String]) -> bool {
	let key = args.first().cloned().unwrap_or_else(|| "un-motion/frame".to_string());
	let seconds = args
		.get(1)
		.and_then(|value| value.parse::<f32>().ok())
		.unwrap_or(3.0)
		.max(0.1);
	let strategy = ZenohTopicStrategy::new(key, un_motion_frame_zenoh::TopicMode::Frame);
	let receiver = match UnAvatarZenohReceiver::declare_zenoh_default(strategy) {
		Ok(receiver) => receiver,
		Err(error) => {
			eprintln!("unmotion-thumb-dump: subscribe failed: {error}");
			return false;
		}
	};
	let deadline = Instant::now() + Duration::from_secs_f32(seconds);
	let mut count = 0usize;
	while Instant::now() < deadline {
		if let Some(frame) = receiver.try_recv() {
			count += 1;
			println!(
				"frame seq={} space={:?} stream={:?}",
				frame.header.sequence, frame.header.coordinate_space, frame.header.stream_id
			);
			print_thumb_joints("left", frame.left_hand.as_ref());
			print_thumb_joints("right", frame.right_hand.as_ref());
		} else {
			std::thread::sleep(Duration::from_millis(10));
		}
	}
	if count == 0 {
		eprintln!("unmotion-thumb-dump: no frames received");
		return false;
	}
	true
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
  retarget-audit VRM0/VRM1/.unavatar のCPU Humanoid retarget軸比較\n\
  unmotion-thumb-dump [key] [seconds] UNMotion/Zenoh の thumb joint quaternion を短時間dump\n\
	acceptance-preflight MVP acceptance の実機確認前に必要な高速preflightを実行\n\
	acceptance-prepare   MVP acceptance の証跡テンプレートと実測用manifestを生成\n\
  spout2       Spout2 を取得・CMake Release ビルドし、配布物へ配置\n\
  unity-fpng   Unity Exporter の native fpng plugin をビルドし、開発用 package へ配置\n\
  unity-exporter-package Unity Editor exporter の UPM package layout を作る\n\
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
		"retarget-audit" => run_retarget_audit(repo),
		"unmotion-thumb-dump" => run_unmotion_thumb_dump(&args.collect::<Vec<_>>()),
		"acceptance-preflight" => run_acceptance_preflight(repo),
		"acceptance-prepare" => run_acceptance_prepare(repo),
		"spout2" => run_spout2(repo, args),
		"unity-fpng" => run_unity_fpng(repo, args),
		"unity-exporter-package" => run_unity_exporter_package(repo, args),
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
