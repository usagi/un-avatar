//! UN Avatar workspace 用 xtask。`cargo xtask ci` 等を拡張する。

use std::{
	collections::{BTreeMap, BTreeSet},
	env, fs,
	io::{BufReader, BufWriter, Read, Write},
	path::{Path, PathBuf},
	process::{self, Command, Stdio},
	time::{Duration, Instant, SystemTime},
};

use glam::{EulerRot, Mat4, Quat, Vec3};
use sha2::{Digest, Sha256};
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
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive};

const SPOUT2_REPO_URL: &str = "https://github.com/leadedge/Spout2.git";
const DEFAULT_SPOUT2_REF: &str = "2.007.017";
const COPY_BUFFER_SIZE: usize = 64 * 1024;
const UNITY_EXPORTER_PACKAGE: &str = "un-avatar-unity-exporter";
const UNITY_EXPORTER_PACKAGE_ID: &str = "network.usagi.un-avatar.unity-exporter";
const VCC_REPOSITORY_NAME: &str = "U.N. Avatar Exporter";
const VCC_REPOSITORY_AUTHOR: &str = "USAGI.NETWORK";

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

fn run_release_guard(repo: &Path) -> bool {
	let commands: &[(&str, &[&str])] = &[
		("xtask unit tests", &["test", "-p", "xtask", "--", "--nocapture"]),
		(
			"renderer tray surface",
			&["test", "-p", "un-avatar-render-wgpu", "--lib", "renderer_tray", "--", "--nocapture"],
		),
		(
			"renderer startup / wardrobe transition roles",
			&["test", "-p", "un-avatar-render-wgpu", "--lib", "startup_progress"],
		),
		(
			"renderer wardrobe transition action isolation",
			&[
				"test",
				"-p",
				"un-avatar-render-wgpu",
				"--lib",
				"wardrobe_transition_frame_skips_runtime_action_evaluation",
			],
		),
		(
			"renderer frame role exclusivity",
			&[
				"test",
				"-p",
				"un-avatar-render-wgpu",
				"--lib",
				"rendered_frame_role_keeps_startup_and_wardrobe_mutually_exclusive",
			],
		),
		(
			"renderer UNAnimator filtering",
			&[
				"test",
				"-p",
				"un-avatar-render-wgpu",
				"--lib",
				"unanimator_excludes_metadata_only_and_tracking_controls",
			],
		),
		(
			"renderer manifest parsing",
			&["test", "-p", "un-avatar-render-wgpu", "--lib", "manifest", "--", "--nocapture"],
		),
		(
			"standalone runtime bus key",
			&[
				"test",
				"-p",
				"un-avatar-render-wgpu",
				"--lib",
				"standalone_runtime_bus_key",
				"--",
				"--nocapture",
			],
		),
		(
			"runtime status compatibility",
			&[
				"test",
				"-p",
				"un-avatar-render-wgpu",
				"--lib",
				"runtime_status_server_keeps_one_shot_compatibility",
				"--",
				"--nocapture",
			],
		),
		(
			"supervisor dev IPC literals",
			&[
				"test",
				"-p",
				"un-avatar-supervisor",
				"--lib",
				"dev_ipc_mock_covers_literal_frontend_invokes",
			],
		),
		(
			"supervisor standalone renderer handoff",
			&[
				"test",
				"-p",
				"un-avatar-supervisor",
				"--lib",
				"open_profile_manifest_registers_standalone_renderer_without_child_process",
				"--",
				"--nocapture",
			],
		),
		(
			"supervisor renderer tray argv shape",
			&[
				"test",
				"-p",
				"un-avatar-supervisor",
				"--lib",
				"startup_open_profile_manifest_arg_accepts_renderer_tray_argv_shape",
				"--",
				"--nocapture",
			],
		),
		(
			"supervisor renderer action boundaries",
			&[
				"test",
				"-p",
				"un-avatar-supervisor",
				"--lib",
				"static_renderer_animator_actions_keep_wardrobe_and_parameter_boundaries",
			],
		),
		(
			"supervisor wardrobe menu active state",
			&[
				"test",
				"-p",
				"un-avatar-supervisor",
				"--lib",
				"static_renderer_wardrobe_menu_resolves_base_active_state",
			],
		),
	];

	for (label, args) in commands {
		eprintln!("release-guard: {label}");
		if !run_cargo(repo, args).success() {
			eprintln!("release-guard: failed at {label}");
			return false;
		}
	}
	eprintln!("release-guard: ok");
	true
}

fn run_renderer(repo: &Path, mut args: impl Iterator<Item = String>) -> bool {
	let mut profile: Option<String> = None;
	let mut manifest: Option<PathBuf> = None;
	let mut wardrobe_set: Option<String> = None;
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
			"--wardrobe-set" => {
				let Some(value) = args.next() else {
					print_run_renderer_usage();
					return false;
				};
				wardrobe_set = Some(value);
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

	if !run_renderer_build(repo, release) {
		return false;
	}
	let exe = target_exe(repo, release, "un-avatar-renderer");
	if !exe.is_file() {
		eprintln!("run-renderer: renderer executable not found after build: {}", exe.display());
		return false;
	}
	let mut command = Command::new(&exe);
	command.arg("--manifest").arg(&manifest);
	if let Some(set_id) = wardrobe_set {
		command.arg("--wardrobe-set").arg(set_id);
	}
	command.args(renderer_args);
	add_spout_runtime_env_if_available(repo, &mut command);
	eprintln!("run-renderer: manifest {}", manifest.display());
	command
		.current_dir(repo)
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status()
		.map(|status| status.success())
		.unwrap_or_else(|e| {
			eprintln!("run-renderer: renderer failed to start: {e}");
			false
		})
}

#[derive(Default)]
struct RendererLogSummary {
	path: PathBuf,
	import_ms: Option<String>,
	pre_scene_import_ms: Option<String>,
	file_read_ms: Option<String>,
	gltf_import_slice_ms: Option<String>,
	gltf_parse_ms: Option<String>,
	gltf_buffers_ms: Option<String>,
	gltf_image_decode_ms: Option<String>,
	scene_snapshot_ms: Option<String>,
	read_meshes_ms: Option<String>,
	read_meshes_stage: Option<String>,
	prewarm_total_ms: Option<String>,
	texture_total_ms: Option<String>,
	mesh_total_ms: Option<String>,
	cache_read_ms: Option<String>,
	upload_ms: Option<String>,
	processed_cache: Option<String>,
	compressed_cache: Option<String>,
	fps_avg: Option<String>,
	cpu_no_surface_ms: Option<String>,
	gpu_ms: Option<String>,
	frame_dynamics_ms: Option<String>,
	frame_dynamics_steps: Option<String>,
	frame_dynamics_world_ms: Option<String>,
	frame_dynamics_collider_ms: Option<String>,
	frame_dynamics_solve_ms: Option<String>,
	frame_dynamics_solve_collision_ms: Option<String>,
	frame_dynamics_solve_propagate_ms: Option<String>,
	frame_draw_state_ms: Option<String>,
	frame_scene_world_ms: Option<String>,
	frame_skin_palette_ms: Option<String>,
	frame_skin_write_ms: Option<String>,
	frame_submit_ms: Option<String>,
	pipeline_load_mb: Option<String>,
	pipeline_store_mb: Option<String>,
	top_texture_ms: f64,
	top_texture: Option<String>,
	texture_roles: Option<String>,
}

fn run_summarize_renderer_log(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut paths = Vec::new();
	for arg in args {
		if matches!(arg.as_str(), "--help" | "-h") {
			print_summarize_renderer_log_usage();
			return true;
		}
		let path = path_from_arg(repo, arg);
		if path.is_dir() {
			let Ok(entries) = fs::read_dir(&path) else {
				eprintln!("summarize-renderer-log: cannot read dir {}", path.display());
				return false;
			};
			for entry in entries.flatten() {
				let path = entry.path();
				if path
					.extension()
					.and_then(|ext| ext.to_str())
					.is_some_and(|ext| ext.eq_ignore_ascii_case("log"))
				{
					paths.push(path);
				}
			}
		} else {
			paths.push(path);
		}
	}
	if paths.is_empty() {
		print_summarize_renderer_log_usage();
		return false;
	}
	paths.sort();
	let summaries = paths
		.iter()
		.filter_map(|path| match summarize_renderer_log(path) {
			Ok(summary) => Some(summary),
			Err(e) => {
				eprintln!("summarize-renderer-log: {}: {e}", path.display());
				None
			}
		})
		.collect::<Vec<_>>();
	println!(
		"file\timport_ms\tpre_scene_import_ms\tfile_read_ms\tgltf_import_slice_ms\tgltf_parse_ms\tgltf_buffers_ms\tgltf_image_decode_ms\tscene_snapshot_ms\tread_meshes_ms\tread_meshes_stage\tprewarm_total_ms\ttexture_ms\tmesh_ms\tcache_read_ms\tupload_ms\tprocessed_cache\tcompressed_cache\tfps\tcpu_no_surface_ms\tgpu_ms\tframe_dynamics_ms\tframe_dynamics_steps\tframe_dynamics_world_ms\tframe_dynamics_collider_ms\tframe_dynamics_solve_ms\tframe_dynamics_solve_collision_ms\tframe_dynamics_solve_propagate_ms\tframe_draw_state_ms\tframe_scene_world_ms\tframe_skin_palette_ms\tframe_skin_write_ms\tframe_submit_ms\tpipeline_load_mb\tpipeline_store_mb\ttop_texture\ttexture_roles"
	);
	for summary in summaries {
		println!(
			"{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
			summary.path.display(),
			summary.import_ms.as_deref().unwrap_or("-"),
			summary.pre_scene_import_ms.as_deref().unwrap_or("-"),
			summary.file_read_ms.as_deref().unwrap_or("-"),
			summary.gltf_import_slice_ms.as_deref().unwrap_or("-"),
			summary.gltf_parse_ms.as_deref().unwrap_or("-"),
			summary.gltf_buffers_ms.as_deref().unwrap_or("-"),
			summary.gltf_image_decode_ms.as_deref().unwrap_or("-"),
			summary.scene_snapshot_ms.as_deref().unwrap_or("-"),
			summary.read_meshes_ms.as_deref().unwrap_or("-"),
			summary.read_meshes_stage.as_deref().unwrap_or("-"),
			summary.prewarm_total_ms.as_deref().unwrap_or("-"),
			summary.texture_total_ms.as_deref().unwrap_or("-"),
			summary.mesh_total_ms.as_deref().unwrap_or("-"),
			summary.cache_read_ms.as_deref().unwrap_or("-"),
			summary.upload_ms.as_deref().unwrap_or("-"),
			summary.processed_cache.as_deref().unwrap_or("-"),
			summary.compressed_cache.as_deref().unwrap_or("-"),
			summary.fps_avg.as_deref().unwrap_or("-"),
			summary.cpu_no_surface_ms.as_deref().unwrap_or("-"),
			summary.gpu_ms.as_deref().unwrap_or("-"),
			summary.frame_dynamics_ms.as_deref().unwrap_or("-"),
			summary.frame_dynamics_steps.as_deref().unwrap_or("-"),
			summary.frame_dynamics_world_ms.as_deref().unwrap_or("-"),
			summary.frame_dynamics_collider_ms.as_deref().unwrap_or("-"),
			summary.frame_dynamics_solve_ms.as_deref().unwrap_or("-"),
			summary.frame_dynamics_solve_collision_ms.as_deref().unwrap_or("-"),
			summary.frame_dynamics_solve_propagate_ms.as_deref().unwrap_or("-"),
			summary.frame_draw_state_ms.as_deref().unwrap_or("-"),
			summary.frame_scene_world_ms.as_deref().unwrap_or("-"),
			summary.frame_skin_palette_ms.as_deref().unwrap_or("-"),
			summary.frame_skin_write_ms.as_deref().unwrap_or("-"),
			summary.frame_submit_ms.as_deref().unwrap_or("-"),
			summary.pipeline_load_mb.as_deref().unwrap_or("-"),
			summary.pipeline_store_mb.as_deref().unwrap_or("-"),
			summary.top_texture.as_deref().unwrap_or("-"),
			summary.texture_roles.as_deref().unwrap_or("-")
		);
	}
	true
}

fn summarize_renderer_log(path: &Path) -> Result<RendererLogSummary, String> {
	let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
	let mut summary = RendererLogSummary {
		path: path.to_path_buf(),
		..Default::default()
	};
	summarize_renderer_log_text(&text, &mut summary);
	Ok(summary)
}

fn summarize_renderer_log_text(text: &str, summary: &mut RendererLogSummary) {
	for line in text.lines() {
		if line.contains("model import profile") && line.contains("step=import_gltf_path") {
			summary.import_ms = metric_token(line, "elapsed=").map(strip_ms);
		} else if summary.import_ms.is_none()
			&& (line.contains("gpu scene benchmark import") || line.contains("scene cache prewarm import"))
		{
			summary.import_ms = metric_token(line, "elapsed=").map(strip_ms);
		} else if line.contains("glTF import profile: pre_scene_import_ms=") {
			summary.pre_scene_import_ms = metric_token(line, "pre_scene_import_ms=");
		} else if line.contains("glTF import profile: file_read_bytes=") {
			summary.file_read_ms = metric_token(line, "file_read_ms=");
		} else if line.contains("glTF import profile: gltf_import_slice_ms=") {
			summary.gltf_import_slice_ms = metric_token(line, "gltf_import_slice_ms=");
		} else if line.contains("glTF import profile: gltf_import_slice.parse_ms=") {
			summary.gltf_parse_ms = metric_token(line, "gltf_import_slice.parse_ms=");
			summary.gltf_buffers_ms = metric_token(line, "buffers_ms=");
			summary.gltf_image_decode_ms = metric_token(line, "image_decode_ms=");
		} else if line.contains("glTF import profile: scene_snapshot_ms=") {
			summary.scene_snapshot_ms = metric_token(line, "scene_snapshot_ms=");
		} else if line.contains("glTF scene profile: read_meshes_ms=") {
			summary.read_meshes_ms = metric_token(line, "read_meshes_ms=");
		} else if let Some(stage) = line.strip_prefix("un-avatar-renderer: glTF scene profile: read_meshes.stage_ms ") {
			summary.read_meshes_stage = Some(stage.to_string());
		} else if line.contains("scene cache prewarm scene") {
			summary.prewarm_total_ms = metric_token(line, "total=").map(strip_ms);
		} else if line.contains("gpu scene texture prepare summary") {
			summary.texture_total_ms = metric_token(line, "total=").map(strip_ms);
			summary.cache_read_ms = metric_token(line, "cache_read=").map(strip_ms);
			summary.upload_ms = metric_token(line, "upload=").map(strip_ms);
			summary.processed_cache = metric_token(line, "processed_cache=");
			summary.compressed_cache = metric_token(line, "compressed_cache=");
		} else if line.contains("gpu scene mesh prepare summary") {
			summary.mesh_total_ms = metric_token(line, "total=").map(strip_ms);
		} else if line.contains("frame bench frames=") {
			summary.fps_avg = metric_token(line, "fps_avg=");
			summary.cpu_no_surface_ms = metric_token(line, "cpu_no_surface_avg=").map(strip_ms);
			summary.gpu_ms = metric_token(line, "gpu_avg=").map(strip_ms);
		} else if line.contains("frame bench detail ") {
			summary.frame_dynamics_ms = frame_detail_avg(line, "dynamics=");
			summary.frame_dynamics_steps = frame_detail_avg(line, "dyn_steps=");
			summary.frame_dynamics_world_ms = frame_detail_avg(line, "dyn_world=");
			summary.frame_dynamics_collider_ms = frame_detail_avg(line, "dyn_colliders=");
			summary.frame_dynamics_solve_ms = frame_detail_avg(line, "dyn_solve=");
			summary.frame_dynamics_solve_collision_ms = frame_detail_avg(line, "dyn_solve_collision=");
			summary.frame_dynamics_solve_propagate_ms = frame_detail_avg(line, "dyn_solve_propagate=");
			summary.frame_draw_state_ms = frame_detail_avg(line, "draw_state=");
			summary.frame_scene_world_ms = frame_detail_avg(line, "scene_world=");
			summary.frame_skin_palette_ms = frame_detail_avg(line, "skin_palette=");
			summary.frame_skin_write_ms = frame_detail_avg(line, "skin_write=");
			summary.frame_submit_ms = frame_detail_avg(line, "submit=");
		} else if line.contains("Vulkan pipeline cache load") {
			summary.pipeline_load_mb = metric_token(line, "bytes=").and_then(bytes_to_mb);
		} else if line.contains("Vulkan pipeline cache store") {
			summary.pipeline_store_mb = metric_token(line, "bytes=").and_then(bytes_to_mb);
		} else if line.contains("gpu scene texture image=") {
			record_top_texture_line(line, summary);
		} else if let Some(roles) = line.strip_prefix("un-avatar-renderer: gpu scene texture prepare roles: ") {
			summary.texture_roles = Some(roles.to_string());
		}
	}
}

fn frame_detail_avg(line: &str, key: &str) -> Option<String> {
	let token = metric_token(line, key)?;
	let (avg, _) = token.split_once('/')?;
	Some(avg.strip_suffix("ms").unwrap_or(avg).to_string())
}

fn metric_token(line: &str, key: &str) -> Option<String> {
	line.split_whitespace()
		.find_map(|token| token.strip_prefix(key))
		.map(|value| value.trim_end_matches([',', ';', ':']).to_string())
}

fn strip_ms(value: String) -> String {
	value.strip_suffix("ms").unwrap_or(&value).to_string()
}

fn bytes_to_mb(value: String) -> Option<String> {
	let bytes = value.parse::<f64>().ok()?;
	Some(format!("{:.1}", bytes / (1024.0 * 1024.0)))
}

fn record_top_texture_line(line: &str, summary: &mut RendererLogSummary) {
	let Some(ms) = texture_line_elapsed_ms(line) else {
		return;
	};
	if ms <= summary.top_texture_ms {
		return;
	}
	summary.top_texture_ms = ms;
	let image = metric_token(line, "image=").unwrap_or_else(|| "?".to_string());
	let role = metric_token(line, "role=").unwrap_or_else(|| "?".to_string());
	let name = quoted_field(line, "name=").unwrap_or_else(|| "?".to_string());
	let read_mb = metric_token(line, "read_mb=").unwrap_or_else(|| "-".to_string());
	let cache_read_ms = metric_token(line, "cache_read=").map(strip_ms).unwrap_or_else(|| "-".to_string());
	let upload_ms = metric_token(line, "upload=").map(strip_ms).unwrap_or_else(|| "-".to_string());
	summary.top_texture = Some(format!(
		"image={image} role={role} name={name} total_ms={ms:.1} cache_read_ms={cache_read_ms} upload_ms={upload_ms} read_mb={read_mb}"
	));
}

fn texture_line_elapsed_ms(line: &str) -> Option<f64> {
	let (_, after_role) = line.split_once(" role=")?;
	let (_, after_colon) = after_role.split_once(": ")?;
	after_colon.split_whitespace().next()?.strip_suffix("ms")?.parse().ok()
}

fn quoted_field(line: &str, key: &str) -> Option<String> {
	let start = line.find(key)? + key.len();
	let rest = line.get(start..)?;
	let rest = rest.strip_prefix('"')?;
	let end = rest.find('"')?;
	Some(rest[..end].to_string())
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
		let mut exact_file_matches = Vec::new();
		let mut metadata_matches = Vec::new();
		for entry in entries.flatten() {
			let path = entry.path();
			if !path
				.extension()
				.and_then(|e| e.to_str())
				.is_some_and(|e| e.eq_ignore_ascii_case("toml"))
			{
				continue;
			}
			if profile_manifest_filename_matches(&path, &wanted) {
				exact_file_matches.push(path);
			} else if profile_manifest_metadata_matches(&path, &wanted) {
				metadata_matches.push(path);
			}
		}
		if !exact_file_matches.is_empty() {
			matches = exact_file_matches;
			break;
		}
		if !metadata_matches.is_empty() {
			matches = metadata_matches;
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

fn profile_manifest_filename_matches(path: &Path, wanted: &str) -> bool {
	let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
	normalize_profile_key(stem) == wanted || normalize_profile_key(strip_timestamp_prefix(stem)) == wanted
}

fn profile_manifest_metadata_matches(path: &Path, wanted: &str) -> bool {
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

fn newest_mtime(path: &Path, skip_dir_names: &[&str]) -> Option<SystemTime> {
	let metadata = fs::metadata(path).ok()?;
	if metadata.is_file() {
		return metadata.modified().ok();
	}
	if !metadata.is_dir() {
		return None;
	}
	let mut newest = metadata.modified().ok();
	let entries = fs::read_dir(path).ok()?;
	for entry in entries.flatten() {
		let child = entry.path();
		if child.is_dir() {
			if child
				.file_name()
				.and_then(|name| name.to_str())
				.is_some_and(|name| skip_dir_names.iter().any(|skip| *skip == name))
			{
				continue;
			}
		}
		if let Some(time) = newest_mtime(&child, skip_dir_names) {
			if newest.is_none_or(|current| time > current) {
				newest = Some(time);
			}
		}
	}
	newest
}

fn supervisor_frontend_needs_build(frontend_dir: &Path) -> bool {
	let dist = frontend_dir.join("dist");
	let Some(dist_mtime) = newest_mtime(&dist, &[]) else {
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
		.filter_map(|input| newest_mtime(&frontend_dir.join(input), &["node_modules", "dist"]))
		.any(|input_mtime| input_mtime > dist_mtime)
}

fn run_supervisor_frontend_build(repo: &Path) -> bool {
	let frontend_dir = repo.join("apps").join("un-avatar-supervisor");
	if !supervisor_frontend_needs_build(&frontend_dir) {
		eprintln!("frontend: dist is fresh; skip npm run build");
		return true;
	}
	Command::new(npm_exe())
		.args(["run", "build"])
		.current_dir(frontend_dir)
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

fn supervisor_frontend_has_check_tool(frontend_dir: &Path) -> bool {
	let bin = frontend_dir.join("node_modules").join(".bin");
	bin.join(if cfg!(windows) { "svelte-check.cmd" } else { "svelte-check" }).is_file()
}

fn run_supervisor_frontend_ci(repo: &Path) -> bool {
	let frontend_dir = repo.join("apps").join("un-avatar-supervisor");
	if !supervisor_frontend_has_check_tool(&frontend_dir) {
		let (install_command, install_args): (&str, &[&str]) = if frontend_dir.join("node_modules").is_dir() {
			("install", &["install", "--package-lock=false"])
		} else {
			("ci", &["ci"])
		};
		let npm_install = Command::new(npm_exe())
			.args(install_args)
			.current_dir(&frontend_dir)
			.stdin(Stdio::inherit())
			.stdout(Stdio::inherit())
			.stderr(Stdio::inherit())
			.status()
			.map(|status| status.success())
			.unwrap_or_else(|e| {
				eprintln!("ci: npm {install_command} failed to start: {e}");
				false
			});
		if !npm_install {
			return false;
		}
	}
	run_supervisor_frontend_check(repo)
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
		(
			manifests.join("click-through.toml"),
			acceptance_renderer_manifest(
				"Acceptance Click-through Window",
				"target/tmp/model1.vrm",
				"smaa",
				"off",
				"source",
				true,
				true,
				false,
			),
		),
		(
			manifests.join("texture-balanced.toml"),
			acceptance_renderer_manifest(
				"Acceptance Texture Balanced",
				"target/tmp/model1.vrm",
				"fxaa",
				"2k",
				"balanced",
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

fn renderer_build_args(release: bool) -> Vec<&'static str> {
	let mut args = vec!["build", "--locked", "-p", "un-avatar-render-wgpu", "--bin", "un-avatar-renderer"];
	if release {
		args.insert(1, "--release");
	}
	args
}

fn run_renderer_build(repo: &Path, release: bool) -> bool {
	let mut renderer_args = renderer_build_args(release);
	if spout2_dev_available(repo) {
		renderer_args.extend(["--features", "spout-sdk"]);
		run_cargo_with_spout_env(repo, &renderer_args, &[])
	} else {
		eprintln!(
			"renderer build: Spout2 SDK/runtime not staged; renderer will be built without Spout2. Run `cargo xtask spout2` to enable it."
		);
		run_cargo_with_env(repo, &renderer_args, &[]).success()
	}
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

	run_renderer_build(repo, release)
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
- `manifests/click-through.toml`: launch-time Click-through pass.\n\
- `manifests/texture-balanced.toml`: texture summary/fallback diagnostics pass.\n\
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

fn add_spout_runtime_env_if_available(repo: &Path, command: &mut Command) {
	if !spout2_dev_available(repo) {
		return;
	}
	let runtime_dir = spout2_package_dir(repo);
	let old_path = env::var_os("PATH").unwrap_or_default();
	let mut paths = vec![runtime_dir];
	paths.extend(env::split_paths(&old_path));
	let path_value = env::join_paths(paths).expect("PATH join failed");
	command.env("PATH", path_value);
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

/// CLI formats / sample plugin / convert 経路を確認する（CI と同趣旨）。
fn run_smoke(repo: &Path) -> bool {
	let mut dir = env::temp_dir();
	dir.push(format!("un-avatar-xtask-smoke-{}", process::id()));
	if fs::create_dir_all(&dir).is_err() {
		return false;
	}
	let in_plugin = dir.join("in.exampleavatar");
	let out_plugin = dir.join("out.exampleavatar");
	if fs::write(&in_plugin, b"sample plugin input\n").is_err() {
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
		Ok(o) if o.status.success() => {
			let s = String::from_utf8_lossy(&o.stdout);
			s.contains("io.un-avatar.vrm") && s.contains("io.un-avatar.gltf")
		}
		_ => false,
	};
	if !list_ok {
		eprintln!("smoke: formats list --json missing built-in importer");
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

	let validate_in = run_un_avatar(
		repo,
		&[
			"--plugin-dir",
			plugins_dir.to_str().expect("plugins path utf-8"),
			"validate",
			in_plugin.to_str().expect("utf8 path"),
		],
	)
	.success();
	if !validate_in {
		eprintln!("smoke: validate in.exampleavatar failed");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let plugin_convert_ok = run_un_avatar(
		repo,
		&[
			"--plugin-dir",
			plugins_dir.to_str().expect("plugins path utf-8"),
			"convert",
			in_plugin.to_str().expect("utf8"),
			out_plugin.to_str().expect("utf8"),
			"--input-format",
			"io.un-avatar.example.avatar",
			"--output-format",
			"io.un-avatar.example.avatar",
		],
	)
	.success();
	if !plugin_convert_ok {
		eprintln!("smoke: convert in.exampleavatar -> out.exampleavatar (stdio importer/exporter) failed");
		let _ = fs::remove_dir_all(&dir);
		return false;
	}

	let _ = fs::remove_dir_all(&dir);
	true
}

fn write_render_smoke_fixture(dir: &Path) -> Option<PathBuf> {
	if let Err(e) = fs::create_dir_all(dir) {
		eprintln!("render-smoke: mkdir {}: {e}", dir.display());
		return None;
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
		return None;
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
		return None;
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
		return None;
	}
	Some(manifest)
}

fn renderer_validate_startup(mut command: Command, manifest: &Path, wardrobe_set: Option<&str>, label: &str) -> bool {
	let status = command
		.arg("--manifest")
		.arg(manifest)
		.args(
			wardrobe_set
				.filter(|set_id| !set_id.trim().is_empty())
				.into_iter()
				.flat_map(|set_id| ["--wardrobe-set", set_id.trim()]),
		)
		.arg("--validate-startup")
		.stdin(Stdio::inherit())
		.stdout(Stdio::inherit())
		.stderr(Stdio::inherit())
		.status();
	let ok = status.is_ok_and(|status| status.success());
	if !ok {
		eprintln!("{label}: renderer manifest/model startup validation failed");
	}
	ok
}

const UNPHYSICS_AUDIT_DEFAULT_AVATARS: [&str; 3] = ["usagi.unavatar", "blanca.unavatar", "mizuki.unavatar"];

fn resolve_unphysics_audit_paths(repo: &Path, paths: Vec<PathBuf>, label: &str) -> (Vec<PathBuf>, bool) {
	let paths = if paths.is_empty() {
		UNPHYSICS_AUDIT_DEFAULT_AVATARS
			.into_iter()
			.map(|name| repo.join("target").join("tmp").join(name))
			.collect()
	} else {
		paths
	};
	let mut ok = true;
	let mut resolved = Vec::with_capacity(paths.len());
	for path in paths {
		let path = if path.is_absolute() { path } else { repo.join(path) };
		if path.exists() {
			resolved.push(path);
		} else {
			eprintln!("{label}: missing .unavatar: {}", path.display());
			ok = false;
		}
	}
	(resolved, ok)
}

fn un_avatar_cli_bin(repo: &Path) -> PathBuf {
	let bin_name = if cfg!(windows) { "un-avatar.exe" } else { "un-avatar" };
	repo.join("target").join("debug").join(bin_name)
}

fn ensure_un_avatar_cli_built(repo: &Path, label: &str) -> bool {
	let status = Command::new("cargo")
		.args(["build", "--locked", "-q", "-p", "un-avatar-cli", "--bin", "un-avatar"])
		.current_dir(repo)
		.status();
	match status {
		Ok(status) if status.success() => true,
		Ok(status) => {
			eprintln!("{label}: failed to build un-avatar CLI: {status}");
			false
		}
		Err(err) => {
			eprintln!("{label}: failed to launch cargo build for un-avatar CLI: {err}");
			false
		}
	}
}

fn run_un_avatar_json(cli_bin: &Path, path: &Path, label: &str, args: &[&str]) -> Option<(process::Output, serde_json::Value)> {
	let output = match Command::new(cli_bin).args(args).output() {
		Ok(output) => output,
		Err(err) => {
			eprintln!("{label}: failed to run un-avatar for {}: {err}", path.display());
			return None;
		}
	};
	let report = match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
		Ok(report) => report,
		Err(err) => {
			eprintln!("{label}: invalid JSON from {}: {err}", path.display());
			print_child_stderr(&output);
			return None;
		}
	};
	Some((output, report))
}

fn print_child_stderr(output: &process::Output) {
	if !output.stderr.is_empty() {
		eprint!("{}", String::from_utf8_lossy(&output.stderr));
	}
}

fn run_unphysics_exporter_audit(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut paths: Vec<PathBuf> = Vec::new();
	for arg in args {
		match arg.as_str() {
			"--help" | "-h" => {
				print_unphysics_exporter_audit_usage();
				return true;
			}
			other if other.starts_with('-') => {
				eprintln!("unphysics-exporter-audit: unknown option: {other}");
				return false;
			}
			_ => paths.push(PathBuf::from(arg)),
		}
	}

	let (paths, mut ok) = resolve_unphysics_audit_paths(repo, paths, "unphysics-exporter-audit");
	if !ensure_un_avatar_cli_built(repo, "unphysics-exporter-audit") {
		return false;
	}
	let cli_bin = un_avatar_cli_bin(repo);
	for path in paths {
		ok &= run_unphysics_exporter_audit_one(&cli_bin, &path);
	}
	if ok {
		println!("unphysics-exporter-audit: ok");
	}
	ok
}

fn run_unphysics_exporter_audit_one(cli_bin: &Path, path: &Path) -> bool {
	let Some((output, report)) = run_un_avatar_json(
		cli_bin,
		path,
		"unphysics-exporter-audit",
		&["dynamics-scan", path_str(path), "--require-current-exporter", "--json"],
	) else {
		return false;
	};
	let source_params = report.get("source_params_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let missing = json_array(&report, "missing_required_source_params");
	let file_mb = report
		.get("file_bytes")
		.and_then(serde_json::Value::as_u64)
		.map(|bytes| bytes as f64 / 1024.0 / 1024.0)
		.unwrap_or(0.0);
	println!(
		"unphysics-exporter-audit: {} sourceParams={} missing={} file={file_mb:.1}MiB",
		path.display(),
		source_params,
		missing.len()
	);
	if !output.status.success() {
		if !missing.is_empty() {
			let labels = missing.iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>().join(", ");
			eprintln!("unphysics-exporter-audit: missing required sourceParams: {labels}");
		}
		print_child_stderr(&output);
		return false;
	}
	true
}

fn run_unphysics_importer_audit(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut paths: Vec<PathBuf> = Vec::new();
	let mut wardrobe_set: Option<String> = None;
	let mut require_node_constraints = false;
	let mut require_parent_node_constraints = false;
	let mut require_mesh_cloth_assist_candidates = false;
	let mut args = args.peekable();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--help" | "-h" => {
				print_unphysics_importer_audit_usage();
				return true;
			}
			"--wardrobe-set" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-importer-audit: --wardrobe-set requires a value");
					return false;
				};
				wardrobe_set = Some(value);
			}
			other if other.starts_with("--wardrobe-set=") => {
				wardrobe_set = Some(other.trim_start_matches("--wardrobe-set=").to_string());
			}
			"--require-node-constraints" => {
				require_node_constraints = true;
			}
			"--require-parent-node-constraints" => {
				require_parent_node_constraints = true;
			}
			"--require-mesh-cloth-assist-candidates" => {
				require_mesh_cloth_assist_candidates = true;
			}
			"--require-runtime-evidence" => {
				// Kept for CLI parity. xtask always forwards this gate to `dynamics-import-audit`.
			}
			other if other.starts_with('-') => {
				eprintln!("unphysics-importer-audit: unknown option: {other}");
				return false;
			}
			_ => paths.push(PathBuf::from(arg)),
		}
	}

	if wardrobe_set_requires_explicit_paths(wardrobe_set.as_deref(), &paths, "unphysics-importer-audit") {
		return false;
	}
	let (paths, mut ok) = resolve_unphysics_audit_paths(repo, paths, "unphysics-importer-audit");
	if !ensure_un_avatar_cli_built(repo, "unphysics-importer-audit") {
		return false;
	}
	let cli_bin = un_avatar_cli_bin(repo);
	for path in paths {
		ok &= run_unphysics_importer_audit_one(
			&cli_bin,
			&path,
			wardrobe_set.as_deref(),
			require_node_constraints,
			require_parent_node_constraints,
			require_mesh_cloth_assist_candidates,
		);
	}
	if ok {
		println!("unphysics-importer-audit: ok");
	}
	ok
}

fn run_unphysics_importer_audit_one(
	cli_bin: &Path,
	path: &Path,
	wardrobe_set: Option<&str>,
	require_node_constraints: bool,
	require_parent_node_constraints: bool,
	require_mesh_cloth_assist_candidates: bool,
) -> bool {
	let mut args = vec!["dynamics-import-audit", path_str(path), "--require-runtime-evidence", "--json"];
	if let Some(wardrobe_set) = wardrobe_set {
		args.push("--wardrobe-set");
		args.push(wardrobe_set);
	}
	let Some((output, report)) = run_un_avatar_json(cli_bin, path, "unphysics-importer-audit", &args) else {
		return false;
	};
	let source_params = report.get("source_params_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let groups = report.get("group_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let enabled = report.get("enabled_group_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let joints = report.get("chain_joint_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let response_groups = report.get("response_group_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let source_angle_groups = report
		.get("source_angle_limit_group_count")
		.and_then(serde_json::Value::as_u64)
		.unwrap_or(0);
	let active_angle_groups = report
		.get("active_angle_limit_group_count")
		.and_then(serde_json::Value::as_u64)
		.unwrap_or(0);
	let hard_angle_constraints = report
		.get("hard_angle_constraint_group_count")
		.and_then(serde_json::Value::as_u64)
		.unwrap_or(0);
	let cloth_angle_metadata_only = report
		.get("cloth_angle_limit_metadata_only_count")
		.and_then(serde_json::Value::as_u64)
		.unwrap_or(0);
	let missing = json_array(&report, "missing_runtime_evidence");
	let node_constraint_count = json_u64(&report, "node_constraint_count");
	let parent_constraint_count = json_u64(&report, "parent_node_constraint_count");
	let parent_constraint_sources = json_u64(&report, "parent_node_constraint_source_count");
	let parent_constraint_multi_source = json_u64(&report, "parent_node_constraint_multi_source_count");
	let mesh_cloth_assist_samples = json_array(&report, "mesh_cloth_assist_samples");
	let mesh_cloth_assist_candidates = sum_json_u64(mesh_cloth_assist_samples, "candidate_count");
	let mesh_cloth_assist_seed_candidates = sum_json_u64(mesh_cloth_assist_samples, "seed_candidate_count");
	let top_mesh_cloth_assist = top_mesh_cloth_assist_sample_label(mesh_cloth_assist_samples);
	println!(
		"unphysics-importer-audit: {} wardrobeSet={} sourceParams={} groups={} enabled={} joints={} responseGroups={} sourceAngleGroups={} activeAngleGroups={} hardAngleConstraints={} clothAngleMetadataOnly={} nodeConstraints={} parentConstraints={} parentSources={} parentMultiSource={} meshAssistSamples={} meshAssistCandidates={} meshAssistSeedCandidates={} topMeshAssist={} missing={}",
		path.display(),
		wardrobe_set.unwrap_or("-"),
		source_params,
		groups,
		enabled,
		joints,
		response_groups,
		source_angle_groups,
		active_angle_groups,
		hard_angle_constraints,
		cloth_angle_metadata_only,
		node_constraint_count,
		parent_constraint_count,
		parent_constraint_sources,
		parent_constraint_multi_source,
		mesh_cloth_assist_samples.len(),
		mesh_cloth_assist_candidates,
		mesh_cloth_assist_seed_candidates,
		top_mesh_cloth_assist,
		missing.len()
	);
	if !output.status.success() {
		if !missing.is_empty() {
			let labels = missing.iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>().join(", ");
			eprintln!("unphysics-importer-audit: missing runtime evidence: {labels}");
		}
		print_child_stderr(&output);
		return false;
	}
	if importer_constraint_gate_failed(require_node_constraints, node_constraint_count) {
		eprintln!(
			"unphysics-importer-audit: no node constraints were observed for {} wardrobeSet={}",
			path.display(),
			wardrobe_set.unwrap_or("-")
		);
		return false;
	}
	if importer_constraint_gate_failed(require_parent_node_constraints, parent_constraint_count) {
		eprintln!(
			"unphysics-importer-audit: no parent node constraints were observed for {} wardrobeSet={}",
			path.display(),
			wardrobe_set.unwrap_or("-")
		);
		return false;
	}
	if importer_mesh_cloth_assist_candidate_gate_failed(require_mesh_cloth_assist_candidates, mesh_cloth_assist_candidates) {
		eprintln!(
			"unphysics-importer-audit: no mesh cloth assist candidates were observed for {} wardrobeSet={}",
			path.display(),
			wardrobe_set.unwrap_or("-")
		);
		return false;
	}
	true
}

fn importer_constraint_gate_failed(require_constraints: bool, constraint_count: u64) -> bool {
	gate_required_count_missing(require_constraints, constraint_count)
}

fn importer_mesh_cloth_assist_candidate_gate_failed(require_candidates: bool, candidate_count: u64) -> bool {
	gate_required_count_missing(require_candidates, candidate_count)
}

fn wardrobe_set_requires_explicit_paths(wardrobe_set: Option<&str>, paths: &[PathBuf], command: &str) -> bool {
	if wardrobe_set.is_none() || !paths.is_empty() {
		return false;
	}
	eprintln!("{command}: --wardrobe-set requires explicit avatar path(s); default regression avatars do not share wardrobe ids");
	true
}

fn run_unphysics_response_audit(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut paths: Vec<PathBuf> = Vec::new();
	let mut wardrobe_set: Option<String> = None;
	let mut require_visual_response_evidence = false;
	let mut args = args.peekable();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--help" | "-h" => {
				print_unphysics_response_audit_usage();
				return true;
			}
			"--require-visual-response-evidence" => {
				require_visual_response_evidence = true;
			}
			"--wardrobe-set" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-response-audit: --wardrobe-set requires a value");
					return false;
				};
				wardrobe_set = Some(value);
			}
			other if other.starts_with("--wardrobe-set=") => {
				wardrobe_set = Some(other.trim_start_matches("--wardrobe-set=").to_string());
			}
			other if other.starts_with('-') => {
				eprintln!("unphysics-response-audit: unknown option: {other}");
				return false;
			}
			_ => paths.push(PathBuf::from(arg)),
		}
	}

	if wardrobe_set_requires_explicit_paths(wardrobe_set.as_deref(), &paths, "unphysics-response-audit") {
		return false;
	}
	let (paths, mut ok) = resolve_unphysics_audit_paths(repo, paths, "unphysics-response-audit");
	if !ensure_un_avatar_cli_built(repo, "unphysics-response-audit") {
		return false;
	}
	let cli_bin = un_avatar_cli_bin(repo);
	for path in paths {
		ok &= run_unphysics_response_audit_one(&cli_bin, &path, wardrobe_set.as_deref(), require_visual_response_evidence);
	}
	if ok {
		println!("unphysics-response-audit: ok");
	}
	ok
}

fn run_unphysics_response_audit_one(
	cli_bin: &Path,
	path: &Path,
	wardrobe_set: Option<&str>,
	require_visual_response_evidence: bool,
) -> bool {
	let mut args = vec!["dynamics-response-audit", path_str(path), "--require-override-effect", "--json"];
	if let Some(wardrobe_set) = wardrobe_set {
		args.push("--wardrobe-set");
		args.push(wardrobe_set);
	}
	let Some((output, report)) = run_un_avatar_json(cli_bin, path, "unphysics-response-audit", &args) else {
		return false;
	};
	let groups = report.get("group_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let joints = report.get("joint_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let missing = json_array(&report, "missing_response_evidence");
	let modes = json_array(&report, "modes");
	let visual_response_evidence = response_audit_visual_response_evidence_count(&modes);
	let mode_label = |name: &str| {
		report
			.get("modes")
			.and_then(serde_json::Value::as_array)
			.and_then(|modes| {
				modes.iter().find(|mode| {
					mode.get("name")
						.and_then(serde_json::Value::as_str)
						.is_some_and(|value| value == name)
				})
			})
			.map(|mode| {
				let rest = mode.get("average_rest_response").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
				let shape = mode
					.get("average_shape_preservation")
					.and_then(serde_json::Value::as_f64)
					.unwrap_or(0.0);
				let motion = mode
					.get("average_parent_motion_follow")
					.and_then(serde_json::Value::as_f64)
					.unwrap_or(0.0);
				let bounce = mode
					.get("average_bounce_response")
					.and_then(serde_json::Value::as_f64)
					.unwrap_or(0.0);
				let damping = mode
					.get("average_damping_half_life_ms")
					.and_then(serde_json::Value::as_f64)
					.map(|value| format!("{value:.1}"))
					.unwrap_or_else(|| "-".to_string());
				let categories = json_array(mode, "categories");
				let groups = json_array(mode, "groups");
				let category_count = mode.get("category_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
				let top_category = response_audit_top_category_label(&categories);
				let top_visual_category = response_audit_top_visual_category_label(&categories);
				let top_nonvisual_category = response_audit_top_nonvisual_category_label(&categories);
				let top_group = response_audit_top_group_label(groups);
				let top_visual_group = response_audit_top_visual_group_label(groups);
				let top_nonvisual_group = response_audit_top_nonvisual_group_label(groups);
				format!("{name}:categories={category_count},rest={rest:.3},shape={shape:.3},bounce={bounce:.3},damp={damping},motion={motion:.3},top={top_category},top_visual={top_visual_category},top_nonvisual={top_nonvisual_category},top_group={top_group},top_visual_group={top_visual_group},top_nonvisual_group={top_nonvisual_group}")
			})
			.unwrap_or_else(|| format!("{name}:missing"))
	};
	println!(
		"unphysics-response-audit: {} wardrobeSet={} groups={} joints={} visualResponseEvidence={} {} {} missing={}",
		path.display(),
		wardrobe_set.unwrap_or("-"),
		groups,
		joints,
		visual_response_evidence,
		mode_label("soft_override"),
		mode_label("firm_override"),
		missing.len()
	);
	if !output.status.success() {
		if !missing.is_empty() {
			let labels = missing.iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>().join(", ");
			eprintln!("unphysics-response-audit: missing response evidence: {labels}");
		}
		print_child_stderr(&output);
		return false;
	}
	if response_audit_visual_response_evidence_gate_failed(require_visual_response_evidence, groups, visual_response_evidence) {
		eprintln!(
			"unphysics-response-audit: missing visual response evidence: groups={groups} visualResponseEvidence={visual_response_evidence}"
		);
		return false;
	}
	true
}

fn response_audit_top_category_label(categories: &[serde_json::Value]) -> String {
	categories
		.iter()
		.max_by(|left, right| {
			json_u64(left, "joint_count")
				.cmp(&json_u64(right, "joint_count"))
				.then_with(|| json_str(right, "category", "").cmp(json_str(left, "category", "")))
		})
		.map(response_audit_category_label)
		.unwrap_or_else(|| "none".to_string())
}

fn response_audit_top_visual_category_label(categories: &[serde_json::Value]) -> String {
	categories
		.iter()
		.filter(|category| response_audit_category_visual_response_evidence_count(category) > 0)
		.max_by(|left, right| {
			json_u64(left, "visible_skinned_joint_count")
				.cmp(&json_u64(right, "visible_skinned_joint_count"))
				.then_with(|| json_u64(left, "visible_mesh_subtree_node_count").cmp(&json_u64(right, "visible_mesh_subtree_node_count")))
				.then_with(|| json_u64(left, "visual_target_group_count").cmp(&json_u64(right, "visual_target_group_count")))
				.then_with(|| json_u64(left, "joint_count").cmp(&json_u64(right, "joint_count")))
				.then_with(|| json_str(right, "category", "").cmp(json_str(left, "category", "")))
		})
		.map(response_audit_category_label)
		.unwrap_or_else(|| "none".to_string())
}

fn response_audit_top_nonvisual_category_label(categories: &[serde_json::Value]) -> String {
	categories
		.iter()
		.filter(|category| json_u64(category, "nonvisual_group_count") > 0)
		.max_by(|left, right| {
			json_u64(left, "nonvisual_group_count")
				.cmp(&json_u64(right, "nonvisual_group_count"))
				.then_with(|| json_u64(left, "joint_count").cmp(&json_u64(right, "joint_count")))
				.then_with(|| json_str(right, "category", "").cmp(json_str(left, "category", "")))
		})
		.map(response_audit_category_label)
		.unwrap_or_else(|| "none".to_string())
}

fn response_audit_group_order(left: &serde_json::Value, right: &serde_json::Value) -> std::cmp::Ordering {
	json_u64(left, "joint_count")
		.cmp(&json_u64(right, "joint_count"))
		.then_with(|| json_str(right, "source_id", "").cmp(json_str(left, "source_id", "")))
}

fn response_audit_group_label(group: &serde_json::Value) -> String {
	let source_id = json_str(group, "source_id", "unknown");
	let category = json_str(group, "category", "unknown");
	let visual = motion_trace_bool_label(group, "visual_target");
	let joints = json_u64(group, "joint_count");
	let visible_joints = json_u64(group, "skinned_joint_count");
	let visible_mesh_subtrees = json_u64(group, "mesh_subtree_node_count");
	let rest = json_f64(group, "average_rest_response");
	let shape = json_f64(group, "average_shape_preservation");
	let bounce = json_f64(group, "average_bounce_response");
	let follow = json_f64(group, "average_parent_motion_follow");
	let orient = json_f64(group, "average_orientation_follow");
	let stretch = json_f64(group, "average_max_stretch_response");
	let stretch_motion = json_f64(group, "average_stretch_motion_response");
	format!("{source_id}:category={category},visual={visual},joints={joints},visibleJoints={visible_joints},visibleMeshSubtrees={visible_mesh_subtrees},rest={rest:.3},shape={shape:.3},bounce={bounce:.3},follow={follow:.3},orient={orient:.3},stretch={stretch:.3},stretchMotion={stretch_motion:.3}")
}

fn response_audit_top_group_label(groups: &[serde_json::Value]) -> String {
	groups
		.iter()
		.max_by(|left, right| response_audit_group_order(left, right))
		.map(response_audit_group_label)
		.unwrap_or_else(|| "none".to_string())
}

fn response_audit_top_visual_group_label(groups: &[serde_json::Value]) -> String {
	groups
		.iter()
		.filter(|group| group.get("visual_target").and_then(serde_json::Value::as_bool) == Some(true))
		.max_by(|left, right| response_audit_group_order(left, right))
		.map(response_audit_group_label)
		.unwrap_or_else(|| "none".to_string())
}

fn response_audit_top_nonvisual_group_label(groups: &[serde_json::Value]) -> String {
	groups
		.iter()
		.filter(|group| group.get("visual_target").and_then(serde_json::Value::as_bool) == Some(false))
		.max_by(|left, right| response_audit_group_order(left, right))
		.map(response_audit_group_label)
		.unwrap_or_else(|| "none".to_string())
}

fn response_audit_category_label(category: &serde_json::Value) -> String {
	let name = json_str(category, "category", "unknown");
	let groups = json_u64(category, "group_count");
	let visual_groups = json_u64(category, "visual_target_group_count");
	let nonvisual_groups = json_u64(category, "nonvisual_group_count");
	let joints = json_u64(category, "joint_count");
	let visible_joints = json_u64(category, "visible_skinned_joint_count");
	let visible_mesh_subtrees = json_u64(category, "visible_mesh_subtree_node_count");
	let rest = json_f64(category, "average_rest_response");
	let shape = json_f64(category, "average_shape_preservation");
	let bounce = json_f64(category, "average_bounce_response");
	let follow = json_f64(category, "average_parent_motion_follow");
	let orient = json_f64(category, "average_orientation_follow");
	let stretch = json_f64(category, "average_max_stretch_response");
	let stretch_motion = json_f64(category, "average_stretch_motion_response");
	format!("{name}:groups={groups},visualGroups={visual_groups},nonvisualGroups={nonvisual_groups},joints={joints},visibleJoints={visible_joints},visibleMeshSubtrees={visible_mesh_subtrees},rest={rest:.3},shape={shape:.3},bounce={bounce:.3},follow={follow:.3},orient={orient:.3},stretch={stretch:.3},stretchMotion={stretch_motion:.3}")
}

fn response_audit_visual_response_evidence_count(modes: &[serde_json::Value]) -> u64 {
	modes
		.iter()
		.map(|mode| {
			json_array(mode, "categories")
				.iter()
				.map(response_audit_category_visual_response_evidence_count)
				.sum::<u64>()
		})
		.max()
		.unwrap_or(0)
}

fn response_audit_category_visual_response_evidence_count(category: &serde_json::Value) -> u64 {
	json_u64(category, "visual_target_group_count")
		+ json_u64(category, "visible_skinned_joint_count")
		+ json_u64(category, "visible_mesh_subtree_node_count")
}

fn response_audit_visual_response_evidence_gate_failed(
	require_visual_response_evidence: bool,
	groups: u64,
	visual_response_evidence: u64,
) -> bool {
	require_visual_response_evidence && groups > 0 && visual_response_evidence == 0
}

fn json_f64(value: &serde_json::Value, key: &str) -> f64 {
	value.get(key).and_then(serde_json::Value::as_f64).unwrap_or(0.0)
}

fn json_u64(value: &serde_json::Value, key: &str) -> u64 {
	value.get(key).and_then(serde_json::Value::as_u64).unwrap_or(0)
}

fn sum_json_u64(values: &[serde_json::Value], key: &str) -> u64 {
	values.iter().map(|value| json_u64(value, key)).sum()
}

fn json_str<'a>(value: &'a serde_json::Value, key: &str, fallback: &'a str) -> &'a str {
	value.get(key).and_then(serde_json::Value::as_str).unwrap_or(fallback)
}

fn json_array<'a>(value: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
	value
		.get(key)
		.and_then(serde_json::Value::as_array)
		.map(Vec::as_slice)
		.unwrap_or(&[])
}

fn top_mesh_cloth_assist_sample_label(samples: &[serde_json::Value]) -> String {
	samples
		.iter()
		.max_by(|left, right| mesh_cloth_assist_sample_order(left, right))
		.map(|sample| {
			let path = json_str(sample, "node_path", "");
			let path = if path.is_empty() { "<unknown>" } else { path };
			let region = json_str(sample, "region", "unknown");
			let vertices = json_u64(sample, "vertex_count");
			let candidates = json_u64(sample, "candidate_count");
			let existing = json_u64(sample, "existing_dynamic_candidate_count");
			let bridge = json_u64(sample, "static_cloth_bridge_candidate_count");
			let seed = json_u64(sample, "seed_candidate_count");
			let assist = json_f64(sample, "suggested_assist_weight_sum");
			let seeded_assist = json_f64(sample, "seeded_assist_weight_sum");
			format!("{path}:region={region},vertices={vertices},candidates={candidates},existing={existing},bridge={bridge},seed={seed},assist={assist:.3},seedAssist={seeded_assist:.3}")
		})
		.unwrap_or_else(|| "none".to_string())
}

fn mesh_cloth_assist_sample_order(left: &serde_json::Value, right: &serde_json::Value) -> std::cmp::Ordering {
	json_u64(left, "candidate_count")
		.cmp(&json_u64(right, "candidate_count"))
		.then_with(|| json_u64(left, "seed_candidate_count").cmp(&json_u64(right, "seed_candidate_count")))
		.then_with(|| json_str(right, "node_path", "").cmp(json_str(left, "node_path", "")))
		.then_with(|| json_str(right, "region", "").cmp(json_str(left, "region", "")))
}

fn collider_summaries_with_projection_count(collider_summaries: &[serde_json::Value]) -> usize {
	collider_summaries
		.iter()
		.filter(|summary| json_u64(summary, "projection_count") > 0)
		.count()
}

fn collider_summaries_with_penetration_count(collider_summaries: &[serde_json::Value]) -> usize {
	collider_summaries
		.iter()
		.filter(|summary| json_u64(summary, "penetrating_count") > 0)
		.count()
}

fn top_projecting_collider_summary_label(collider_summaries: &[serde_json::Value]) -> String {
	collider_summaries
		.iter()
		.filter(|summary| json_u64(summary, "projection_count") > 0)
		.max_by(|left, right| projecting_collider_summary_order(left, right))
		.map(collider_summary_label)
		.unwrap_or_else(|| "none".to_string())
}

fn top_candidate_collider_summary_label(collider_summaries: &[serde_json::Value]) -> String {
	collider_summaries
		.iter()
		.filter(|summary| json_u64(summary, "candidate_count") > 0)
		.min_by(|left, right| candidate_collider_summary_order(left, right))
		.map(collider_summary_label)
		.unwrap_or_else(|| "none".to_string())
}

fn projecting_collider_summary_order(left: &serde_json::Value, right: &serde_json::Value) -> std::cmp::Ordering {
	json_u64(left, "projection_count")
		.cmp(&json_u64(right, "projection_count"))
		.then_with(|| json_str(right, "collider_path", "").cmp(json_str(left, "collider_path", "")))
}

fn candidate_collider_summary_order(left: &serde_json::Value, right: &serde_json::Value) -> std::cmp::Ordering {
	json_f64(left, "min_margin")
		.total_cmp(&json_f64(right, "min_margin"))
		.then_with(|| json_u64(right, "penetrating_count").cmp(&json_u64(left, "penetrating_count")))
		.then_with(|| json_u64(right, "candidate_count").cmp(&json_u64(left, "candidate_count")))
		.then_with(|| json_str(left, "collider_path", "").cmp(json_str(right, "collider_path", "")))
}

fn collider_summary_label(summary: &serde_json::Value) -> String {
	let path = json_str(summary, "collider_path", "");
	let path = if path.is_empty() { "<unscoped>" } else { path };
	let shape = json_str(summary, "collider_shape", "unknown");
	let inside_bounds = summary
		.get("inside_bounds")
		.and_then(serde_json::Value::as_bool)
		.map(|value| if value { "true" } else { "false" })
		.unwrap_or("unknown");
	let candidates = json_u64(summary, "candidate_count");
	let penetrating = json_u64(summary, "penetrating_count");
	let projections = json_u64(summary, "projection_count");
	let sources = json_u64(summary, "source_count");
	let min_margin = json_f64(summary, "min_margin");
	format!("{path}:shape={shape},inside={inside_bounds},candidates={candidates},penetrating={penetrating},projections={projections},sources={sources},minMargin={min_margin:.5}")
}

fn json_stable_offset(value: &serde_json::Value) -> f64 {
	value
		.get("stable_offset")
		.or_else(|| value.get("settled_recovery_lag"))
		.and_then(serde_json::Value::as_f64)
		.unwrap_or(0.0)
}

fn json_half_life_label(value: &serde_json::Value) -> String {
	value
		.get("recovery_half_life_frames")
		.and_then(serde_json::Value::as_f64)
		.map(|value| format!("{value:.1}"))
		.unwrap_or_else(|| "none".to_string())
}

fn motion_trace_response_label(value: &serde_json::Value) -> String {
	let rest = json_f64(value, "average_rest_response");
	let shape = json_f64(value, "average_shape_preservation");
	let bounce = json_f64(value, "average_bounce_response");
	let follow = json_f64(value, "average_parent_motion_follow");
	let orient = json_f64(value, "average_orientation_follow");
	let stretch = json_f64(value, "average_max_stretch_response");
	let stretch_motion = json_f64(value, "average_stretch_motion_response");
	format!("resp=rest:{rest:.3},shape:{shape:.3},bounce:{bounce:.3},follow:{follow:.3},orient:{orient:.3},stretch:{stretch:.3},stretchMotion:{stretch_motion:.3}")
}

fn motion_trace_bool_label(value: &serde_json::Value, key: &str) -> &'static str {
	value
		.get(key)
		.and_then(serde_json::Value::as_bool)
		.map(|value| if value { "true" } else { "false" })
		.unwrap_or("unknown")
}

fn motion_trace_category_summary_label(category: &serde_json::Value) -> String {
	let name = json_str(category, "category", "unknown");
	let visual_groups = json_u64(category, "visual_target_group_count");
	let nonvisual_groups = json_u64(category, "nonvisual_group_count");
	let max_lag = json_f64(category, "max_lag");
	let chain_length = json_f64(category, "average_chain_rest_length");
	let max_lag_chain_ratio = json_f64(category, "max_lag_chain_ratio");
	let avg_lag = json_f64(category, "average_lag");
	let recovery = json_f64(category, "recovery_final_lag");
	let initial_stable = json_f64(category, "initial_stable_offset");
	let settled = json_stable_offset(category);
	let stable_ratio = json_f64(category, "stable_offset_ratio");
	let stable_chain_ratio = json_f64(category, "stable_offset_chain_ratio");
	let recovery_state = json_str(category, "recovery_state", "unknown");
	let residual = json_f64(category, "residual_motion");
	let residual_chain_ratio = json_f64(category, "residual_motion_chain_ratio");
	let half_life = json_half_life_label(category);
	let response = motion_trace_response_label(category);
	format!("{name}:visualGroups={visual_groups},nonvisualGroups={nonvisual_groups},chain={chain_length:.3},max={max_lag:.3},maxChain={max_lag_chain_ratio:.3},avg={avg_lag:.3},rec={recovery:.3},initialStable={initial_stable:.3},stable={settled:.3},stableRatio={stable_ratio:.3},stableChain={stable_chain_ratio:.3},state={recovery_state},res={residual:.3},resChain={residual_chain_ratio:.3},half={half_life},{response}")
}

fn motion_trace_top_visual_category_label(categories: &[serde_json::Value], groups: &[serde_json::Value]) -> String {
	let Some(top_group) = groups
		.iter()
		.filter(|group| group.get("visual_target").and_then(serde_json::Value::as_bool) == Some(true))
		.max_by(|left, right| motion_trace_group_order(left, right))
	else {
		return "none".to_string();
	};
	let group_category = json_str(top_group, "category", "");
	categories
		.iter()
		.find(|category| json_str(category, "category", "") == group_category)
		.map(motion_trace_category_summary_label)
		.unwrap_or_else(|| motion_trace_group_summary_label(top_group))
}

fn motion_trace_top_nonvisual_category_label(categories: &[serde_json::Value], groups: &[serde_json::Value]) -> String {
	let Some(top_group) = groups
		.iter()
		.filter(|group| group.get("visual_target").and_then(serde_json::Value::as_bool) == Some(false))
		.max_by(|left, right| motion_trace_group_order(left, right))
	else {
		return "none".to_string();
	};
	let group_category = json_str(top_group, "category", "");
	categories
		.iter()
		.find(|category| json_str(category, "category", "") == group_category)
		.map(motion_trace_category_summary_label)
		.unwrap_or_else(|| motion_trace_group_summary_label(top_group))
}

fn motion_trace_category_order(left: &serde_json::Value, right: &serde_json::Value) -> std::cmp::Ordering {
	json_stable_offset(left)
		.total_cmp(&json_stable_offset(right))
		.then_with(|| json_f64(left, "max_lag").total_cmp(&json_f64(right, "max_lag")))
		.then_with(|| json_str(right, "category", "").cmp(json_str(left, "category", "")))
}

fn motion_trace_finding_top_label(findings: &[serde_json::Value]) -> String {
	motion_trace_finding_top_label_where(findings, |_| true)
}

fn motion_trace_finding_top_label_where(findings: &[serde_json::Value], predicate: impl Fn(&serde_json::Value) -> bool) -> String {
	let labels = findings
		.iter()
		.filter(|finding| predicate(finding))
		.take(3)
		.map(|finding| {
			let kind = json_str(finding, "kind", "unknown");
			let source_id = json_str(finding, "source_id", "");
			if source_id.is_empty() {
				let category = json_str(finding, "category", "");
				if category.is_empty() {
					kind.to_string()
				} else {
					format!("{kind}:{category}")
				}
			} else {
				format!("{kind}:{source_id}")
			}
		})
		.collect::<Vec<_>>();
	if labels.is_empty() {
		"-".to_string()
	} else {
		labels.join(",")
	}
}

fn motion_trace_visual_finding_top_label(findings: &[serde_json::Value]) -> String {
	motion_trace_finding_top_label_where(findings, |finding| {
		finding.get("visual_target").and_then(serde_json::Value::as_bool) == Some(true)
	})
}

fn motion_trace_group_summary_label(group: &serde_json::Value) -> String {
	let source_id = json_str(group, "source_id", "unknown");
	let visual_target = motion_trace_bool_label(group, "visual_target");
	let max_lag = json_f64(group, "max_lag");
	let initial_stable = json_f64(group, "initial_stable_offset");
	let chain_length = json_f64(group, "chain_rest_length");
	let max_lag_chain_ratio = json_f64(group, "max_lag_chain_ratio");
	let settled = json_stable_offset(group);
	let stable_ratio = json_f64(group, "stable_offset_ratio");
	let stable_chain_ratio = json_f64(group, "stable_offset_chain_ratio");
	let recovery_state = json_str(group, "recovery_state", "unknown");
	let residual = json_f64(group, "residual_motion");
	let residual_chain_ratio = json_f64(group, "residual_motion_chain_ratio");
	let half_life = json_half_life_label(group);
	let response = motion_trace_response_label(group);
	format!("{source_id}:visual={visual_target},chain={chain_length:.3},max={max_lag:.3},maxChain={max_lag_chain_ratio:.3},initialStable={initial_stable:.3},stable={settled:.3},stableRatio={stable_ratio:.3},stableChain={stable_chain_ratio:.3},state={recovery_state},res={residual:.3},resChain={residual_chain_ratio:.3},half={half_life},{response}")
}

fn motion_trace_top_group_label(groups: &[serde_json::Value]) -> String {
	groups
		.iter()
		.max_by(|left, right| motion_trace_group_order(left, right))
		.map(motion_trace_group_summary_label)
		.unwrap_or_else(|| "none".to_string())
}

fn motion_trace_top_visual_group_label(groups: &[serde_json::Value]) -> String {
	groups
		.iter()
		.filter(|group| group.get("visual_target").and_then(serde_json::Value::as_bool) == Some(true))
		.max_by(|left, right| motion_trace_group_order(left, right))
		.map(motion_trace_group_summary_label)
		.unwrap_or_else(|| "none".to_string())
}

fn motion_trace_top_nonvisual_group_label(groups: &[serde_json::Value]) -> String {
	groups
		.iter()
		.filter(|group| group.get("visual_target").and_then(serde_json::Value::as_bool) == Some(false))
		.max_by(|left, right| motion_trace_group_order(left, right))
		.map(motion_trace_group_summary_label)
		.unwrap_or_else(|| "none".to_string())
}

fn motion_trace_group_order(left: &serde_json::Value, right: &serde_json::Value) -> std::cmp::Ordering {
	json_stable_offset(left)
		.total_cmp(&json_stable_offset(right))
		.then_with(|| json_f64(left, "max_lag").total_cmp(&json_f64(right, "max_lag")))
		.then_with(|| json_str(right, "source_id", "").cmp(json_str(left, "source_id", "")))
}

fn motion_trace_finding_visibility_counts(findings: &[serde_json::Value]) -> (usize, usize, usize) {
	let mut visual = 0;
	let mut nonvisual = 0;
	let mut unknown = 0;
	for finding in findings {
		match finding.get("visual_target").and_then(serde_json::Value::as_bool) {
			Some(true) => visual += 1,
			Some(false) => nonvisual += 1,
			None => unknown += 1,
		}
	}
	(visual, nonvisual, unknown)
}

fn motion_trace_visible_findings_gate_failed(require_no_visible_findings: bool, visual_findings: usize) -> bool {
	require_no_visible_findings && visual_findings > 0
}

fn motion_trace_unknown_visibility_gate_failed(require_known_finding_visibility: bool, unknown_visibility_findings: usize) -> bool {
	require_known_finding_visibility && unknown_visibility_findings > 0
}

fn motion_trace_recovery_frames_to_run(requested: &str, include_short_recovery: bool) -> Vec<String> {
	let mut frames = vec![requested.to_string()];
	if include_short_recovery {
		for short in ["24", "96"] {
			if !frames.iter().any(|value| value == short) {
				frames.push(short.to_string());
			}
		}
	}
	frames
}

#[derive(Clone, Debug)]
struct UnphysicsVertexProbeAuditOptions {
	wardrobe_set: Option<String>,
	node_contains: String,
	settle_frames: String,
	tuning: String,
	apply_mesh_cloth_assist: bool,
	ignore_authored_colliders: bool,
	require_mesh_cloth_assist_changes: bool,
	require_collision_projections: bool,
	require_probe_collision_projections: bool,
	require_collision_projection_sources: bool,
	require_collision_projection_paths: bool,
	require_collider_summaries: bool,
	require_probe_collider_candidates: bool,
	require_projecting_collider_summaries: bool,
}

impl Default for UnphysicsVertexProbeAuditOptions {
	fn default() -> Self {
		Self {
			wardrobe_set: None,
			node_contains: String::new(),
			settle_frames: "240".to_string(),
			tuning: "authored".to_string(),
			apply_mesh_cloth_assist: false,
			ignore_authored_colliders: false,
			require_mesh_cloth_assist_changes: false,
			require_collision_projections: false,
			require_probe_collision_projections: false,
			require_collision_projection_sources: false,
			require_collision_projection_paths: false,
			require_collider_summaries: false,
			require_probe_collider_candidates: false,
			require_projecting_collider_summaries: false,
		}
	}
}

fn vertex_probe_mesh_cloth_assist_gate_failed(
	require_mesh_cloth_assist_changes: bool,
	apply_mesh_cloth_assist: bool,
	changed_vertices: u64,
) -> bool {
	require_mesh_cloth_assist_changes && (!apply_mesh_cloth_assist || changed_vertices == 0)
}

fn vertex_probe_collision_projection_gate_failed(require_collision_projections: bool, collision_projections: u64) -> bool {
	gate_required_count_missing(require_collision_projections, collision_projections)
}

fn vertex_probe_probe_collision_projection_gate_failed(
	require_probe_collision_projections: bool,
	probe_collision_projections: u64,
) -> bool {
	gate_required_count_missing(require_probe_collision_projections, probe_collision_projections)
}

fn vertex_probe_collision_projection_source_gate_failed(require_collision_projection_sources: bool, projection_source_ids: usize) -> bool {
	gate_required_count_missing(require_collision_projection_sources, projection_source_ids as u64)
}

fn vertex_probe_collision_projection_path_gate_failed(require_collision_projection_paths: bool, projection_collider_paths: usize) -> bool {
	gate_required_count_missing(require_collision_projection_paths, projection_collider_paths as u64)
}

fn vertex_probe_collider_summary_gate_failed(require_collider_summaries: bool, collider_summary_count: usize) -> bool {
	gate_required_count_missing(require_collider_summaries, collider_summary_count as u64)
}

fn vertex_probe_probe_collider_candidate_gate_failed(
	require_probe_collider_candidates: bool,
	probe_collider_summaries: &[serde_json::Value],
) -> bool {
	require_probe_collider_candidates
		&& !probe_collider_summaries
			.iter()
			.any(|summary| json_u64(summary, "candidate_count") > 0)
}

fn vertex_probe_projecting_collider_summary_gate_failed(
	require_projecting_collider_summaries: bool,
	projecting_collider_summary_count: usize,
) -> bool {
	gate_required_count_missing(require_projecting_collider_summaries, projecting_collider_summary_count as u64)
}

fn gate_required_count_missing(require_count: bool, count: u64) -> bool {
	require_count && count == 0
}

fn run_unphysics_motion_audit(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	for arg in args {
		match arg.as_str() {
			"--help" | "-h" => {
				print_unphysics_motion_audit_usage();
				return true;
			}
			other => {
				eprintln!("unphysics-motion-audit: unknown argument: {other}");
				return false;
			}
		}
	}
	let tests: &[(&str, &str)] = &[
		(
			"motion coupling changes head-motion lag",
			"unphysics_motion_coupling_override_changes_head_motion_lag",
		),
		(
			"rest response changes recovery speed",
			"unphysics_rest_response_override_changes_recovery_speed",
		),
		(
			"cloth softness changes solver output",
			"unphysics_cloth_profile_softness_changes_solver_output",
		),
		(
			"ears preset intent changes lag/recovery/residual motion",
			"unphysics_ears_preset_intent_changes_lag_recovery_and_residual_motion",
		),
		("bounce scale changes solver output", "unphysics_bounce_scale_changes_solver_output"),
		(
			"source-authored shape intent does not bypass soft tuning",
			"unphysics_source_stiffness_does_not_bypass_soft_pull_and_motion_coupling",
		),
	];
	for (label, test_name) in tests {
		eprintln!("unphysics-motion-audit: {label}");
		if !run_cargo(repo, &["test", "-p", "un-avatar-skeleton", "--lib", test_name, "--", "--nocapture"]).success() {
			eprintln!("unphysics-motion-audit: failed at {label}");
			return false;
		}
	}
	println!("unphysics-motion-audit: ok");
	true
}

fn run_unphysics_motion_trace_audit(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut paths: Vec<PathBuf> = Vec::new();
	let mut frames = "24".to_string();
	let mut recovery_frames = "240".to_string();
	let mut tuning = "authored".to_string();
	let mut wardrobe_set: Option<String> = None;
	let mut include_short_recovery = false;
	let mut require_no_visible_findings = false;
	let mut require_known_finding_visibility = false;
	let mut args = args.peekable();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--help" | "-h" => {
				print_unphysics_motion_trace_audit_usage();
				return true;
			}
			"--frames" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-motion-trace-audit: --frames requires a value");
					return false;
				};
				frames = value;
			}
			other if other.starts_with("--frames=") => {
				frames = other.trim_start_matches("--frames=").to_string();
			}
			"--recovery-frames" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-motion-trace-audit: --recovery-frames requires a value");
					return false;
				};
				recovery_frames = value;
			}
			other if other.starts_with("--recovery-frames=") => {
				recovery_frames = other.trim_start_matches("--recovery-frames=").to_string();
			}
			"--tuning" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-motion-trace-audit: --tuning requires a value");
					return false;
				};
				tuning = value;
			}
			other if other.starts_with("--tuning=") => {
				tuning = other.trim_start_matches("--tuning=").to_string();
			}
			"--wardrobe-set" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-motion-trace-audit: --wardrobe-set requires a value");
					return false;
				};
				wardrobe_set = Some(value);
			}
			other if other.starts_with("--wardrobe-set=") => {
				wardrobe_set = Some(other.trim_start_matches("--wardrobe-set=").to_string());
			}
			"--include-short-recovery" => {
				include_short_recovery = true;
			}
			"--require-no-visible-findings" => {
				require_no_visible_findings = true;
			}
			"--require-known-finding-visibility" => {
				require_known_finding_visibility = true;
			}
			other if other.starts_with('-') => {
				eprintln!("unphysics-motion-trace-audit: unknown option: {other}");
				return false;
			}
			_ => paths.push(PathBuf::from(arg)),
		}
	}

	if wardrobe_set_requires_explicit_paths(wardrobe_set.as_deref(), &paths, "unphysics-motion-trace-audit") {
		return false;
	}
	let (paths, mut ok) = resolve_unphysics_audit_paths(repo, paths, "unphysics-motion-trace-audit");
	if !ensure_un_avatar_cli_built(repo, "unphysics-motion-trace-audit") {
		return false;
	}
	let cli_bin = un_avatar_cli_bin(repo);
	for path in paths {
		for recovery_frames in motion_trace_recovery_frames_to_run(&recovery_frames, include_short_recovery) {
			ok &= run_unphysics_motion_trace_audit_one(
				&cli_bin,
				&path,
				&frames,
				&recovery_frames,
				&tuning,
				wardrobe_set.as_deref(),
				require_no_visible_findings,
				require_known_finding_visibility,
			);
		}
	}
	if ok {
		println!("unphysics-motion-trace-audit: ok");
	}
	ok
}

fn run_unphysics_motion_trace_audit_one(
	cli_bin: &Path,
	path: &Path,
	frames: &str,
	recovery_frames: &str,
	tuning: &str,
	wardrobe_set: Option<&str>,
	require_no_visible_findings: bool,
	require_known_finding_visibility: bool,
) -> bool {
	let mut args = vec![
		"dynamics-motion-trace-audit",
		path_str(path),
		"--require-motion-evidence",
		"--frames",
		frames,
	];
	args.push("--recovery-frames");
	args.push(recovery_frames);
	args.push("--tuning");
	args.push(tuning);
	if let Some(wardrobe_set) = wardrobe_set {
		args.push("--wardrobe-set");
		args.push(wardrobe_set);
	}
	args.push("--json");
	let Some((output, report)) = run_un_avatar_json(cli_bin, path, "unphysics-motion-trace-audit", &args) else {
		return false;
	};
	let groups = report.get("group_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let joints = report.get("joint_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let tuning = report.get("tuning").and_then(serde_json::Value::as_str).unwrap_or("unknown");
	let frame_count = report.get("frame_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let recovery_frame_count = report.get("recovery_frame_count").and_then(serde_json::Value::as_u64).unwrap_or(0);
	let categories = json_array(&report, "categories");
	let groups_json = json_array(&report, "groups");
	let missing = json_array(&report, "missing_motion_evidence");
	let findings = json_array(&report, "findings");
	let finding_details = json_array(&report, "finding_details");
	let finding_top = motion_trace_finding_top_label(&finding_details);
	let visual_finding_top = motion_trace_visual_finding_top_label(&finding_details);
	let (visual_findings, nonvisual_findings, unknown_visibility_findings) = motion_trace_finding_visibility_counts(&finding_details);
	let finding_kind_counts = report
		.get("finding_kind_counts")
		.and_then(serde_json::Value::as_object)
		.map(|counts| {
			counts
				.iter()
				.map(|(kind, value)| format!("{kind}:{}", value.as_u64().unwrap_or(0)))
				.collect::<Vec<_>>()
				.join(",")
		})
		.filter(|counts| !counts.is_empty())
		.unwrap_or_else(|| "-".to_string());
	let top = categories
		.iter()
		.max_by(|left, right| motion_trace_category_order(left, right))
		.map(motion_trace_category_summary_label)
		.unwrap_or_else(|| "none".to_string());
	let top_visual = motion_trace_top_visual_category_label(categories, groups_json);
	let top_nonvisual = motion_trace_top_nonvisual_category_label(categories, groups_json);
	let top_group = motion_trace_top_group_label(groups_json);
	let top_visual_group = motion_trace_top_visual_group_label(&groups_json);
	let top_nonvisual_group = motion_trace_top_nonvisual_group_label(&groups_json);
	println!(
		"unphysics-motion-trace-audit: {} wardrobeSet={} tuning={} frames={} recoveryFrames={} groups={} joints={} categories={} findings={} visualFindings={} nonvisualFindings={} unknownVisibilityFindings={} findingKinds={} findingTop={} visualFindingTop={} top={} top_visual={} top_nonvisual={} top_group={} top_visual_group={} top_nonvisual_group={} missing={}",
		path.display(),
		wardrobe_set.unwrap_or("-"),
		tuning,
		frame_count,
		recovery_frame_count,
		groups,
		joints,
		categories.len(),
		findings.len(),
		visual_findings,
		nonvisual_findings,
		unknown_visibility_findings,
		finding_kind_counts,
		finding_top,
		visual_finding_top,
		top,
		top_visual,
		top_nonvisual,
		top_group,
		top_visual_group,
		top_nonvisual_group,
		missing.len()
	);
	if !output.status.success() {
		if !missing.is_empty() {
			let labels = missing.iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>().join(", ");
			eprintln!("unphysics-motion-trace-audit: missing motion evidence: {labels}");
		}
		print_child_stderr(&output);
		return false;
	}
	if motion_trace_visible_findings_gate_failed(require_no_visible_findings, visual_findings) {
		eprintln!(
			"unphysics-motion-trace-audit: visible motion findings are present: visualFindings={} visualFindingTop={}",
			visual_findings, visual_finding_top
		);
		return false;
	}
	if motion_trace_unknown_visibility_gate_failed(require_known_finding_visibility, unknown_visibility_findings) {
		eprintln!(
			"unphysics-motion-trace-audit: motion findings with unknown visibility are present: unknownVisibilityFindings={} findingTop={}",
			unknown_visibility_findings, finding_top
		);
		return false;
	}
	true
}

fn run_unphysics_vertex_probe_audit(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut paths: Vec<PathBuf> = Vec::new();
	let mut opts = UnphysicsVertexProbeAuditOptions::default();
	let mut args = args.peekable();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--help" | "-h" => {
				print_unphysics_vertex_probe_audit_usage();
				return true;
			}
			"--wardrobe-set" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-vertex-probe-audit: --wardrobe-set requires a value");
					return false;
				};
				opts.wardrobe_set = Some(value);
			}
			other if other.starts_with("--wardrobe-set=") => {
				opts.wardrobe_set = Some(other.trim_start_matches("--wardrobe-set=").to_string());
			}
			"--node-contains" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-vertex-probe-audit: --node-contains requires a value");
					return false;
				};
				opts.node_contains = value;
			}
			other if other.starts_with("--node-contains=") => {
				opts.node_contains = other.trim_start_matches("--node-contains=").to_string();
			}
			"--settle-frames" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-vertex-probe-audit: --settle-frames requires a value");
					return false;
				};
				opts.settle_frames = value;
			}
			other if other.starts_with("--settle-frames=") => {
				opts.settle_frames = other.trim_start_matches("--settle-frames=").to_string();
			}
			"--tuning" => {
				let Some(value) = args.next() else {
					eprintln!("unphysics-vertex-probe-audit: --tuning requires a value");
					return false;
				};
				opts.tuning = value;
			}
			other if other.starts_with("--tuning=") => {
				opts.tuning = other.trim_start_matches("--tuning=").to_string();
			}
			"--apply-mesh-cloth-assist" => {
				opts.apply_mesh_cloth_assist = true;
			}
			"--ignore-authored-colliders" => {
				opts.ignore_authored_colliders = true;
			}
			"--require-mesh-cloth-assist-changes" => {
				opts.require_mesh_cloth_assist_changes = true;
			}
			"--require-collision-projections" => {
				opts.require_collision_projections = true;
			}
			"--require-probe-collision-projections" => {
				opts.require_probe_collision_projections = true;
			}
			"--require-collision-projection-sources" => {
				opts.require_collision_projection_sources = true;
			}
			"--require-collision-projection-paths" => {
				opts.require_collision_projection_paths = true;
			}
			"--require-collider-summaries" => {
				opts.require_collider_summaries = true;
			}
			"--require-probe-collider-candidates" => {
				opts.require_probe_collider_candidates = true;
			}
			"--require-projecting-collider-summaries" => {
				opts.require_projecting_collider_summaries = true;
			}
			other if other.starts_with('-') => {
				eprintln!("unphysics-vertex-probe-audit: unknown option: {other}");
				return false;
			}
			_ => paths.push(PathBuf::from(arg)),
		}
	}

	if wardrobe_set_requires_explicit_paths(opts.wardrobe_set.as_deref(), &paths, "unphysics-vertex-probe-audit") {
		return false;
	}
	let (paths, mut ok) = resolve_unphysics_audit_paths(repo, paths, "unphysics-vertex-probe-audit");
	if !ensure_un_avatar_cli_built(repo, "unphysics-vertex-probe-audit") {
		return false;
	}
	let cli_bin = un_avatar_cli_bin(repo);
	for path in paths {
		ok &= run_unphysics_vertex_probe_audit_one(&cli_bin, &path, &opts);
	}
	if ok {
		println!("unphysics-vertex-probe-audit: ok");
	}
	ok
}

fn run_unphysics_vertex_probe_audit_one(cli_bin: &Path, path: &Path, opts: &UnphysicsVertexProbeAuditOptions) -> bool {
	let mut args = vec![
		"dynamics-vertex-probe",
		path_str(path),
		"--settle-frames",
		opts.settle_frames.as_str(),
		"--tuning",
		opts.tuning.as_str(),
	];
	if let Some(wardrobe_set) = opts.wardrobe_set.as_deref() {
		args.push("--wardrobe-set");
		args.push(wardrobe_set);
	}
	if !opts.node_contains.is_empty() {
		args.push("--node-contains");
		args.push(opts.node_contains.as_str());
	}
	if opts.apply_mesh_cloth_assist {
		args.push("--apply-mesh-cloth-assist");
	}
	if opts.ignore_authored_colliders {
		args.push("--ignore-authored-colliders");
	}
	args.push("--json");
	let Some((output, report)) = run_un_avatar_json(cli_bin, path, "unphysics-vertex-probe-audit", &args) else {
		return false;
	};
	let node_path = json_str(&report, "node_path", "unknown");
	let runtime_colliders = json_u64(&report, "runtime_collider_count");
	let changed_vertices = json_u64(&report, "mesh_cloth_assist_changed_vertices");
	let collision_projections = json_u64(&report, "solve_collision_projection_count");
	let probe_collision_projections = json_u64(&report, "probe_collision_projection_count");
	let probe_dynamic_source_count = report
		.get("probe_dynamic_source_weight_sums")
		.and_then(serde_json::Value::as_object)
		.map(serde_json::Map::len)
		.unwrap_or(0);
	let probe_projection_source_count = report
		.get("probe_collision_projection_source_counts")
		.and_then(serde_json::Value::as_object)
		.map(serde_json::Map::len)
		.unwrap_or(0);
	let probe_projection_collider_paths = report
		.get("probe_collision_projection_collider_path_counts")
		.and_then(serde_json::Value::as_object)
		.map(serde_json::Map::len)
		.unwrap_or(0);
	let projection_source_count = report
		.get("solve_collision_projection_source_counts")
		.and_then(serde_json::Value::as_object)
		.map(serde_json::Map::len)
		.unwrap_or(0);
	let projection_collider_paths = report
		.get("solve_collision_projection_collider_path_counts")
		.and_then(serde_json::Value::as_object)
		.map(serde_json::Map::len)
		.unwrap_or(0);
	let collider_summaries = json_array(&report, "collider_path_summaries");
	let projecting_collider_summaries = collider_summaries_with_projection_count(collider_summaries);
	let top_projecting_collider = top_projecting_collider_summary_label(collider_summaries);
	let probe_collider_summaries = json_array(&report, "probe_collider_path_summaries");
	let probe_penetrating_collider_summaries = collider_summaries_with_penetration_count(probe_collider_summaries);
	let probe_projecting_collider_summaries = collider_summaries_with_projection_count(probe_collider_summaries);
	let top_probe_collider = top_projecting_collider_summary_label(probe_collider_summaries);
	let top_probe_candidate_collider = top_candidate_collider_summary_label(probe_collider_summaries);
	println!(
		"unphysics-vertex-probe-audit: {} wardrobeSet={} node={} tuning={} meshClothAssist={} changedVertices={} runtimeColliders={} collisionProjections={} probeCollisionProjections={} probeDynamicSources={} probeProjectionSources={} probeProjectionColliderPaths={} projectionSources={} projectionColliderPaths={} colliderSummaries={} projectingColliderSummaries={} probeColliderSummaries={} probePenetratingColliderSummaries={} probeProjectingColliderSummaries={} topProjectingCollider={} topProbeCollider={} topProbeCandidateCollider={}",
		path.display(),
		opts.wardrobe_set.as_deref().unwrap_or("-"),
		node_path,
		opts.tuning,
		opts.apply_mesh_cloth_assist,
		changed_vertices,
		runtime_colliders,
		collision_projections,
		probe_collision_projections,
		probe_dynamic_source_count,
		probe_projection_source_count,
		probe_projection_collider_paths,
		projection_source_count,
		projection_collider_paths,
		collider_summaries.len(),
		projecting_collider_summaries,
		probe_collider_summaries.len(),
		probe_penetrating_collider_summaries,
		probe_projecting_collider_summaries,
		top_projecting_collider,
		top_probe_collider,
		top_probe_candidate_collider
	);
	if !output.status.success() {
		print_child_stderr(&output);
		return false;
	}
	if vertex_probe_mesh_cloth_assist_gate_failed(
		opts.require_mesh_cloth_assist_changes,
		opts.apply_mesh_cloth_assist,
		changed_vertices,
	) {
		if !opts.apply_mesh_cloth_assist {
			eprintln!(
				"unphysics-vertex-probe-audit: mesh cloth assist changes were required for {} node={} but --apply-mesh-cloth-assist was not enabled",
				path.display(),
				node_path
			);
		} else {
			eprintln!(
				"unphysics-vertex-probe-audit: no mesh cloth assist vertex changes were observed for {} node={}",
				path.display(),
				node_path
			);
		}
		return false;
	}
	if vertex_probe_collision_projection_gate_failed(opts.require_collision_projections, collision_projections) {
		eprintln!(
			"unphysics-vertex-probe-audit: no collision projections were observed for {} node={}",
			path.display(),
			node_path
		);
		return false;
	}
	if vertex_probe_probe_collision_projection_gate_failed(opts.require_probe_collision_projections, probe_collision_projections) {
		eprintln!(
			"unphysics-vertex-probe-audit: no probe-source collision projections were observed for {} node={} probeDynamicSources={} globalCollisionProjections={}",
			path.display(),
			node_path,
			probe_dynamic_source_count,
			collision_projections
		);
		return false;
	}
	if vertex_probe_collision_projection_source_gate_failed(opts.require_collision_projection_sources, projection_source_count) {
		eprintln!(
			"unphysics-vertex-probe-audit: no collision projection source ids were observed for {} node={}",
			path.display(),
			node_path
		);
		return false;
	}
	if vertex_probe_collision_projection_path_gate_failed(opts.require_collision_projection_paths, projection_collider_paths) {
		eprintln!(
			"unphysics-vertex-probe-audit: no collision projection collider paths were observed for {} node={}",
			path.display(),
			node_path
		);
		return false;
	}
	if vertex_probe_collider_summary_gate_failed(opts.require_collider_summaries, collider_summaries.len()) {
		eprintln!(
			"unphysics-vertex-probe-audit: no collider path summaries were observed for {} node={}",
			path.display(),
			node_path
		);
		return false;
	}
	if vertex_probe_probe_collider_candidate_gate_failed(opts.require_probe_collider_candidates, probe_collider_summaries) {
		eprintln!(
			"unphysics-vertex-probe-audit: no probe-source collider candidates were observed for {} node={} probeDynamicSources={} globalColliderSummaries={}",
			path.display(),
			node_path,
			probe_dynamic_source_count,
			collider_summaries.len()
		);
		return false;
	}
	if vertex_probe_projecting_collider_summary_gate_failed(opts.require_projecting_collider_summaries, projecting_collider_summaries) {
		eprintln!(
			"unphysics-vertex-probe-audit: no projecting collider path summaries were observed for {} node={}",
			path.display(),
			node_path
		);
		return false;
	}
	true
}

fn run_render_smoke(repo: &Path) -> bool {
	let mut dir = env::temp_dir();
	dir.push(format!("un-avatar-xtask-render-smoke-{}", process::id()));
	let Some(manifest) = write_render_smoke_fixture(&dir) else {
		let _ = fs::remove_dir_all(&dir);
		return false;
	};
	let mut command = Command::new("cargo");
	command
		.args([
			"run",
			"--locked",
			"-q",
			"-p",
			"un-avatar-render-wgpu",
			"--bin",
			"un-avatar-renderer",
			"--",
		])
		.current_dir(repo);
	let ok = renderer_validate_startup(command, &manifest, None, "render-smoke");
	let _ = fs::remove_dir_all(&dir);
	ok
}

#[derive(Default)]
struct PackageRenderSmokeOptions {
	manifest: Option<PathBuf>,
	wardrobe_set: Option<String>,
}

fn parse_package_render_smoke_options(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<PackageRenderSmokeOptions, ()> {
	let mut opts = PackageRenderSmokeOptions::default();
	while let Some(arg) = args.next() {
		match arg.as_str() {
			"--manifest" | "-m" => {
				let Some(value) = args.next() else {
					print_package_render_smoke_usage();
					return Err(());
				};
				opts.manifest = Some(path_from_arg(repo, value));
			}
			"--wardrobe-set" => {
				let Some(value) = args.next() else {
					print_package_render_smoke_usage();
					return Err(());
				};
				opts.wardrobe_set = Some(value);
			}
			"--help" | "-h" => {
				print_package_render_smoke_usage();
				return Err(());
			}
			other => {
				eprintln!("package-render-smoke: unknown argument: {other}");
				print_package_render_smoke_usage();
				return Err(());
			}
		}
	}
	if opts.wardrobe_set.is_some() && opts.manifest.is_none() {
		eprintln!("package-render-smoke: --wardrobe-set requires --manifest");
		print_package_render_smoke_usage();
		return Err(());
	}
	Ok(opts)
}

fn run_package_render_smoke_with_options(repo: &Path, opts: PackageRenderSmokeOptions) -> bool {
	let exe = repo
		.join("target")
		.join("package")
		.join("un-avatar")
		.join(exe_name("un-avatar-renderer"));
	if !exe.is_file() {
		eprintln!("package-render-smoke: packaged renderer not found: {}", exe.display());
		return false;
	}
	let mut dir = env::temp_dir();
	dir.push(format!("un-avatar-xtask-package-render-smoke-{}", process::id()));
	let (manifest, remove_dir) = if let Some(manifest) = opts.manifest {
		if !manifest.is_file() {
			eprintln!("package-render-smoke: manifest not found: {}", manifest.display());
			return false;
		}
		(manifest, false)
	} else {
		let Some(manifest) = write_render_smoke_fixture(&dir) else {
			let _ = fs::remove_dir_all(&dir);
			return false;
		};
		(manifest, true)
	};
	let mut command = Command::new(&exe);
	command.current_dir(repo);
	let ok = renderer_validate_startup(command, &manifest, opts.wardrobe_set.as_deref(), "package-render-smoke");
	if remove_dir {
		let _ = fs::remove_dir_all(&dir);
	}
	ok
}

fn run_package_render_smoke(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let opts = match parse_package_render_smoke_options(repo, args) {
		Ok(opts) => opts,
		Err(()) => return false,
	};
	run_package_render_smoke_with_options(repo, opts)
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

fn run_unity_exporter_vcc(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut version = None;
	let mut base_url = None;
	let mut package_url = None;
	let mut output_dir = None;
	let mut repo_index = None;
	let mut iter = args.peekable();
	while let Some(arg) = iter.next() {
		match arg.as_str() {
			"--version" => {
				let Some(value) = iter.next() else {
					eprintln!("unity-exporter-vcc: --version には version が必要です");
					return false;
				};
				version = Some(value);
			}
			"--base-url" => {
				let Some(value) = iter.next() else {
					eprintln!("unity-exporter-vcc: --base-url には URL が必要です");
					return false;
				};
				base_url = Some(value);
			}
			"--package-url" => {
				let Some(value) = iter.next() else {
					eprintln!("unity-exporter-vcc: --package-url には URL が必要です");
					return false;
				};
				package_url = Some(value);
			}
			"--output-dir" => {
				let Some(value) = iter.next() else {
					eprintln!("unity-exporter-vcc: --output-dir には path が必要です");
					return false;
				};
				output_dir = Some(PathBuf::from(value));
			}
			"--repo-index" => {
				let Some(value) = iter.next() else {
					eprintln!("unity-exporter-vcc: --repo-index には path が必要です");
					return false;
				};
				repo_index = Some(PathBuf::from(value));
			}
			"help" | "--help" | "-h" => {
				print_unity_exporter_vcc_usage();
				return true;
			}
			other => {
				eprintln!("unity-exporter-vcc: 不明な option: {other}");
				print_unity_exporter_vcc_usage();
				return false;
			}
		}
	}

	let Some(version) = version.or_else(|| default_package_version(repo)) else {
		eprintln!("unity-exporter-vcc: Cargo.toml の workspace.package.version を読めませんでした");
		return false;
	};
	let output_dir = output_dir.unwrap_or_else(|| repo.join("target").join("unity").join("vcc"));
	let repo_index = repo_index.unwrap_or_else(|| repo.join("docs").join("vcc").join("index.json"));
	let staging_dir = repo.join("target").join("unity").join("vcc-staging").join(UNITY_EXPORTER_PACKAGE);
	let package_file_name = format!("{UNITY_EXPORTER_PACKAGE_ID}-{version}.zip");
	let package_url = package_url.unwrap_or_else(|| {
		let base = base_url.unwrap_or_else(|| format!("https://github.com/usagi/un-avatar/releases/download/{version}"));
		format!("{}/{}", base.trim_end_matches('/'), package_file_name)
	});

	if !stage_unity_exporter_package(repo, &staging_dir) {
		return false;
	}
	let Some(manifest) = write_unity_exporter_package_manifest(&staging_dir, &version, Some(&package_url)) else {
		return false;
	};
	let zip_path = output_dir.join(&package_file_name);
	if !create_package_contents_zip(&staging_dir, &zip_path) {
		return false;
	}
	if !verify_vcc_package_zip_entries(&zip_path) {
		return false;
	}
	if !verify_vcc_package_zip_manifest(&zip_path, &version) {
		return false;
	}
	let Some(zip_sha256) = file_sha256(&zip_path) else {
		eprintln!("unity-exporter-vcc: sha256 failed: {}", zip_path.display());
		return false;
	};
	if !write_vcc_repo_index(&repo_index, manifest, &version, &zip_sha256) {
		return false;
	}

	println!("unity-exporter-vcc: package {}", zip_path.display());
	println!("unity-exporter-vcc: package_url {package_url}");
	println!("unity-exporter-vcc: zipSHA256 {zip_sha256}");
	println!("unity-exporter-vcc: repo_index {}", repo_index.display());
	true
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
	let mut hasher = Sha256::new();
	let mut buffer = [0_u8; COPY_BUFFER_SIZE];
	loop {
		let bytes_read = file.read(&mut buffer).ok()?;
		if bytes_read == 0 {
			break;
		}
		hasher.update(&buffer[..bytes_read]);
	}
	Some(format!("{:x}", hasher.finalize()))
}

fn checksum_file_path(path: &Path) -> Option<PathBuf> {
	let parent = path.parent()?;
	let mut file_name = path.file_name()?.to_os_string();
	file_name.push(".sha256.txt");
	Some(parent.join(file_name))
}

fn checksum_file_text(sha256: &str, artifact_name: &str) -> String {
	format!("{}  {}\n", sha256.trim(), artifact_name)
}

fn parse_checksum_file(text: &str) -> Option<(&str, &str)> {
	let mut parts = text.split_whitespace();
	let hash = parts.next()?;
	let artifact = parts.next()?;
	if parts.next().is_some() {
		return None;
	}
	Some((hash, artifact))
}

fn write_sha256_file(artifact_path: &Path) -> Option<PathBuf> {
	let sha256 = file_sha256(artifact_path)?;
	let checksum_path = checksum_file_path(artifact_path)?;
	let artifact_name = artifact_path.file_name()?.to_string_lossy();
	let text = checksum_file_text(&sha256, &artifact_name);
	fs::write(&checksum_path, text).ok()?;
	Some(checksum_path)
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

const RELEASE_PACKAGE_DOCS: &[&str] = &[
	"docs/v2-getting-started.md",
	"docs/README.md",
	"docs/v2-roadmap.md",
	"docs/unity-exporter-v0.1.md",
	"docs/v2-ui-gui-operation-plan.md",
	"docs/development-guidelines.md",
	"docs/third-party-licenses.md",
	"docs/assets/v2-getting-started/exporter-workflow.png",
	"docs/assets/v2-getting-started/exporter-wardrobe-sets.png",
	"docs/assets/v2-getting-started/supervisor-unavatar-confirm.png",
	"docs/assets/v2-getting-started/tray-wardrobe-menu.png",
	"docs/assets/v2-getting-started/obs-spout2-capture.png",
	"docs/assets/v2-getting-started/wardrobe-switch-demo.webm",
	"docs/assets/v2-getting-started/wardrobe-switch-demo.mp4",
];

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
	for relative in RELEASE_PACKAGE_DOCS {
		let source = repo.join(Path::new(relative));
		if source.is_file() {
			ok &= copy_file_to(&source, &package.join(Path::new(relative)));
		} else {
			eprintln!("release-package: missing linked README doc {}", source.display());
			ok = false;
		}
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

fn required_release_zip_entries(package_name: &str, require_spout2: bool) -> Vec<String> {
	let root = package_name.trim_end_matches('/');
	let mut entries = vec![
		format!("{root}/"),
		format!("{root}/LICENSE"),
		format!("{root}/README.md"),
		format!("{root}/THIRD_PARTY_NOTICES.md"),
		format!("{root}/LICENSES/third-party-licenses.md"),
	];
	entries.extend(RELEASE_PACKAGE_DOCS.iter().map(|relative| format!("{root}/{relative}")));
	entries.extend([
		format!("{root}/{}", exe_name("un-avatar-renderer")),
		format!("{root}/{}", exe_name("un-avatar-supervisor")),
		format!("{root}/unity/{UNITY_EXPORTER_PACKAGE}/package.json"),
		format!("{root}/unity/{UNITY_EXPORTER_PACKAGE}/Editor/UNAvatarExporterWindow.cs"),
		format!("{root}/unity/{UNITY_EXPORTER_PACKAGE}/Editor/Plugins/x86_64/unavatar_fpng.dll"),
	]);
	if require_spout2 {
		entries.extend([
			format!("{root}/Spout.dll"),
			format!("{root}/LICENSES/Spout2-BSD-2-Clause.txt"),
			format!("{root}/LICENSES/spout2-build-info.txt"),
		]);
	}
	entries
}

fn release_zip_missing_entries(zip_path: &Path, package_name: &str, require_spout2: bool) -> Result<Vec<String>, String> {
	let file = match fs::File::open(zip_path) {
		Ok(file) => file,
		Err(err) => return Err(format!("open {}: {err}", zip_path.display())),
	};
	let mut archive = match ZipArchive::new(file) {
		Ok(archive) => archive,
		Err(err) => return Err(format!("read zip {}: {err}", zip_path.display())),
	};
	let mut names = BTreeSet::new();
	for index in 0..archive.len() {
		let Ok(file) = archive.by_index(index) else {
			return Err(format!("read zip entry {index} from {}", zip_path.display()));
		};
		names.insert(file.name().to_string());
	}
	Ok(required_release_zip_entries(package_name, require_spout2)
		.into_iter()
		.filter(|entry| !names.contains(entry))
		.collect())
}

fn verify_release_zip_entries(zip_path: &Path, package_name: &str, require_spout2: bool) -> bool {
	let missing = match release_zip_missing_entries(zip_path, package_name, require_spout2) {
		Ok(missing) => missing,
		Err(err) => {
			eprintln!("release-package: {err}");
			return false;
		}
	};
	if !missing.is_empty() {
		eprintln!("release-package: zip missing required entries: {}", missing.join(", "));
		false
	} else {
		true
	}
}

fn release_zip_entry_bytes(zip_path: &Path, entry: &str) -> Result<Vec<u8>, String> {
	let file = fs::File::open(zip_path).map_err(|err| format!("open {}: {err}", zip_path.display()))?;
	let mut archive = ZipArchive::new(file).map_err(|err| format!("read zip {}: {err}", zip_path.display()))?;
	let mut file = archive
		.by_name(entry)
		.map_err(|err| format!("read zip entry {entry} from {}: {err}", zip_path.display()))?;
	let mut bytes = Vec::new();
	file.read_to_end(&mut bytes)
		.map_err(|err| format!("read zip entry bytes {entry} from {}: {err}", zip_path.display()))?;
	Ok(bytes)
}

fn verify_release_zip_entry_matches_file(zip_path: &Path, entry: &str, source: &Path) -> bool {
	let expected = match fs::read(source) {
		Ok(bytes) => bytes,
		Err(err) => {
			eprintln!("release-audit: read {}: {err}", source.display());
			return false;
		}
	};
	let actual = match release_zip_entry_bytes(zip_path, entry) {
		Ok(bytes) => bytes,
		Err(err) => {
			eprintln!("release-audit: {err}");
			return false;
		}
	};
	if actual != expected {
		eprintln!(
			"release-audit: stale release zip entry {entry}: does not match {}",
			source.display()
		);
		return false;
	}
	true
}

fn verify_release_zip_source_docs(zip_path: &Path, package_name: &str, repo: &Path) -> bool {
	let root = package_name.trim_end_matches('/');
	let checks = [
		(format!("{root}/README.md"), repo.join("README.md")),
		(format!("{root}/LICENSE"), repo.join("LICENSE")),
		(
			format!("{root}/THIRD_PARTY_NOTICES.md"),
			repo.join("docs").join("third-party-licenses.md"),
		),
		(
			format!("{root}/LICENSES/third-party-licenses.md"),
			repo.join("docs").join("third-party-licenses.md"),
		),
	]
	.into_iter()
	.chain(
		RELEASE_PACKAGE_DOCS
			.iter()
			.map(|relative| (format!("{root}/{relative}"), repo.join(Path::new(relative)))),
	)
	.collect::<Vec<_>>();
	let ok = checks
		.iter()
		.all(|(entry, source)| verify_release_zip_entry_matches_file(zip_path, entry, source));
	if ok {
		println!("release-audit: release zip source docs ok");
	}
	ok
}

fn verify_release_zip_clean_unpack(zip_path: &Path, package_name: &str, require_spout2: bool) -> bool {
	let unpack_root = env::temp_dir().join(format!("un-avatar-release-audit-unpack-{}-{}", package_name, process::id()));
	let _ = fs::remove_dir_all(&unpack_root);
	if let Err(err) = fs::create_dir_all(&unpack_root) {
		eprintln!("release-audit: create clean unpack dir {}: {err}", unpack_root.display());
		return false;
	}

	let result = (|| -> Result<(), String> {
		let file = fs::File::open(zip_path).map_err(|err| format!("open {}: {err}", zip_path.display()))?;
		let mut archive = ZipArchive::new(file).map_err(|err| format!("read zip {}: {err}", zip_path.display()))?;
		archive
			.extract(&unpack_root)
			.map_err(|err| format!("extract {} to {}: {err}", zip_path.display(), unpack_root.display()))?;
		for entry in required_release_zip_entries(package_name, require_spout2) {
			if entry.ends_with('/') {
				continue;
			}
			let path = unpack_root.join(&entry);
			if !path.is_file() {
				return Err(format!("clean unpack missing required file: {}", path.display()));
			}
		}
		Ok(())
	})();

	let cleanup_result = fs::remove_dir_all(&unpack_root);
	if let Err(err) = cleanup_result {
		eprintln!("release-audit: cleanup clean unpack dir {}: {err}", unpack_root.display());
		return false;
	}
	if let Err(err) = result {
		eprintln!("release-audit: {err}");
		false
	} else {
		println!("release-audit: clean unpack ok");
		true
	}
}

fn required_vcc_package_zip_entries() -> Vec<&'static str> {
	vec![
		"package.json",
		"LICENSE.md",
		"Editor/UNAvatar.UnityExporter.Editor.asmdef",
		"Editor/UNAvatarExporterWindow.cs",
		"Editor/UNAvatarExporterWindow.Export.cs",
		"Editor/UNAvatarExporterWindow.ModularAvatar.cs",
		"Editor/UNAvatarExporterWindow.Wardrobe.cs",
		"Editor/MinimalGltfExporter.cs",
		"Editor/GlbExtensionPatcher.cs",
		"Editor/Plugins/x86_64/unavatar_fpng.dll",
	]
}

fn vcc_package_zip_missing_entries(zip_path: &Path) -> Result<Vec<String>, String> {
	let file = match fs::File::open(zip_path) {
		Ok(file) => file,
		Err(err) => return Err(format!("open {}: {err}", zip_path.display())),
	};
	let mut archive = match ZipArchive::new(file) {
		Ok(archive) => archive,
		Err(err) => return Err(format!("read zip {}: {err}", zip_path.display())),
	};
	let mut names = BTreeSet::new();
	for index in 0..archive.len() {
		let Ok(file) = archive.by_index(index) else {
			return Err(format!("read zip entry {index} from {}", zip_path.display()));
		};
		names.insert(file.name().to_string());
	}
	Ok(required_vcc_package_zip_entries()
		.into_iter()
		.filter(|entry| !names.contains(*entry))
		.map(str::to_string)
		.collect())
}

fn verify_vcc_package_zip_entries(zip_path: &Path) -> bool {
	let missing = match vcc_package_zip_missing_entries(zip_path) {
		Ok(missing) => missing,
		Err(err) => {
			eprintln!("unity-exporter-vcc: {err}");
			return false;
		}
	};
	if !missing.is_empty() {
		eprintln!("unity-exporter-vcc: zip missing required entries: {}", missing.join(", "));
		false
	} else {
		true
	}
}

fn verify_vcc_package_zip_staging_files(zip_path: &Path, staging_dir: &Path) -> bool {
	if !staging_dir.is_dir() {
		eprintln!("release-audit: VCC staging directory not found: {}", staging_dir.display());
		return false;
	}
	let ok = required_vcc_package_zip_entries()
		.into_iter()
		.all(|entry| verify_release_zip_entry_matches_file(zip_path, entry, &staging_dir.join(entry)));
	if ok {
		println!("release-audit: VCC zip staging files ok");
	}
	ok
}

fn vcc_package_zip_manifest(zip_path: &Path) -> Result<serde_json::Value, String> {
	let raw = match release_zip_entry_bytes(zip_path, "package.json") {
		Ok(bytes) => bytes,
		Err(err) => return Err(err),
	};
	serde_json::from_slice(&raw).map_err(|err| format!("parse VCC package.json from {}: {err}", zip_path.display()))
}

fn verify_vcc_package_zip_manifest(zip_path: &Path, version: &str) -> bool {
	let manifest = match vcc_package_zip_manifest(zip_path) {
		Ok(manifest) => manifest,
		Err(err) => {
			eprintln!("release-audit: {err}");
			return false;
		}
	};
	let name = manifest.get("name").and_then(serde_json::Value::as_str);
	if name != Some(UNITY_EXPORTER_PACKAGE_ID) {
		eprintln!(
			"release-audit: VCC package.json name mismatch: manifest={:?} expected={UNITY_EXPORTER_PACKAGE_ID}",
			name
		);
		return false;
	}
	let manifest_version = manifest.get("version").and_then(serde_json::Value::as_str);
	if manifest_version != Some(version) {
		eprintln!(
			"release-audit: VCC package.json version mismatch: manifest={:?} expected={version}",
			manifest_version
		);
		return false;
	}
	let expected_zip_name = format!("{UNITY_EXPORTER_PACKAGE_ID}-{version}.zip");
	let url = manifest.get("url").and_then(serde_json::Value::as_str);
	if !url.is_some_and(|url| url.trim_end_matches('/').ends_with(&format!("/{expected_zip_name}"))) {
		eprintln!(
			"release-audit: VCC package.json url mismatch: manifest={:?} expected suffix=/{expected_zip_name}",
			url
		);
		return false;
	}
	println!("release-audit: VCC package manifest ok");
	true
}

fn verify_vcc_package_manifest_matches_index(repo_index: &Path, zip_path: &Path, version: &str) -> bool {
	let raw = match fs::read_to_string(repo_index) {
		Ok(raw) => raw,
		Err(err) => {
			eprintln!("release-audit: read {}: {err}", repo_index.display());
			return false;
		}
	};
	let index: serde_json::Value = match serde_json::from_str(&raw) {
		Ok(index) => index,
		Err(err) => {
			eprintln!("release-audit: parse {}: {err}", repo_index.display());
			return false;
		}
	};
	let index_url = index
		.get("packages")
		.and_then(|packages| packages.get(UNITY_EXPORTER_PACKAGE_ID))
		.and_then(|package| package.get("versions"))
		.and_then(|versions| versions.get(version))
		.and_then(|entry| entry.get("url"))
		.and_then(serde_json::Value::as_str);
	let manifest = match vcc_package_zip_manifest(zip_path) {
		Ok(manifest) => manifest,
		Err(err) => {
			eprintln!("release-audit: {err}");
			return false;
		}
	};
	let manifest_url = manifest.get("url").and_then(serde_json::Value::as_str);
	if index_url != manifest_url {
		eprintln!(
			"release-audit: VCC package URL mismatch between repo index and package.json: index={:?} manifest={:?}",
			index_url, manifest_url
		);
		return false;
	}
	println!("release-audit: VCC package index URL matches manifest");
	true
}

fn create_package_contents_zip(package_root: &Path, zip_path: &Path) -> bool {
	if let Some(parent) = zip_path.parent() {
		if let Err(err) = fs::create_dir_all(parent) {
			eprintln!("unity-exporter-vcc: mkdir {}: {err}", parent.display());
			return false;
		}
	}
	if zip_path.exists() {
		if let Err(err) = fs::remove_file(zip_path) {
			eprintln!("unity-exporter-vcc: remove {}: {err}", zip_path.display());
			return false;
		}
	}
	let file = match fs::File::create(zip_path) {
		Ok(file) => file,
		Err(err) => {
			eprintln!("unity-exporter-vcc: create {}: {err}", zip_path.display());
			return false;
		}
	};
	let mut writer = zip::ZipWriter::new(BufWriter::new(file));
	let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
	let entries = match fs::read_dir(package_root) {
		Ok(entries) => entries,
		Err(err) => {
			eprintln!("unity-exporter-vcc: read {}: {err}", package_root.display());
			return false;
		}
	};
	for entry in entries.flatten() {
		if !add_zip_entry(&mut writer, package_root, &entry.path(), options) {
			return false;
		}
	}
	if let Err(err) = writer.finish() {
		eprintln!("unity-exporter-vcc: finalize {}: {err}", zip_path.display());
		return false;
	}
	true
}

fn write_unity_exporter_package_manifest(package_dir: &Path, version: &str, package_url: Option<&str>) -> Option<serde_json::Value> {
	let path = package_dir.join("package.json");
	let raw = fs::read_to_string(&path)
		.map_err(|err| eprintln!("unity-exporter-vcc: read {}: {err}", path.display()))
		.ok()?;
	let mut manifest: serde_json::Value = serde_json::from_str(&raw)
		.map_err(|err| eprintln!("unity-exporter-vcc: parse {}: {err}", path.display()))
		.ok()?;
	let object = manifest.as_object_mut()?;
	object.insert("name".to_string(), serde_json::Value::String(UNITY_EXPORTER_PACKAGE_ID.to_string()));
	object.insert("version".to_string(), serde_json::Value::String(version.to_string()));
	object.insert("license".to_string(), serde_json::Value::String("MIT".to_string()));
	if let Some(package_url) = package_url {
		object.insert("url".to_string(), serde_json::Value::String(package_url.to_string()));
	}
	let author = object.entry("author").or_insert_with(|| serde_json::json!({})).as_object_mut()?;
	author
		.entry("name".to_string())
		.or_insert_with(|| serde_json::Value::String("USAGI.NETWORK".to_string()));
	author
		.entry("email".to_string())
		.or_insert_with(|| serde_json::Value::String("contact@usagi.network".to_string()));
	author
		.entry("url".to_string())
		.or_insert_with(|| serde_json::Value::String("https://github.com/usagi/un-avatar".to_string()));
	let serialized = serde_json::to_string_pretty(&manifest).ok()? + "\n";
	fs::write(&path, serialized)
		.map_err(|err| eprintln!("unity-exporter-vcc: write {}: {err}", path.display()))
		.ok()?;
	Some(manifest)
}

fn write_vcc_repo_index(repo_index: &Path, manifest: serde_json::Value, version: &str, zip_sha256: &str) -> bool {
	let mut listing = if repo_index.is_file() {
		match fs::read_to_string(repo_index)
			.ok()
			.and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
		{
			Some(value) => value,
			None => {
				eprintln!(
					"unity-exporter-vcc: existing repo index is not valid JSON: {}",
					repo_index.display()
				);
				return false;
			}
		}
	} else {
		serde_json::json!({
			"name": VCC_REPOSITORY_NAME,
			"id": "network.usagi.un-avatar",
			"url": "https://usagi.github.io/un-avatar/vcc/index.json",
			"author": VCC_REPOSITORY_AUTHOR,
			"packages": {}
		})
	};

	let Some(root) = listing.as_object_mut() else {
		eprintln!("unity-exporter-vcc: repo index root must be an object: {}", repo_index.display());
		return false;
	};
	root.insert("name".to_string(), serde_json::Value::String(VCC_REPOSITORY_NAME.to_string()));
	root.insert("id".to_string(), serde_json::Value::String("network.usagi.un-avatar".to_string()));
	root.insert(
		"url".to_string(),
		serde_json::Value::String("https://usagi.github.io/un-avatar/vcc/index.json".to_string()),
	);
	root.insert("author".to_string(), serde_json::Value::String(VCC_REPOSITORY_AUTHOR.to_string()));
	let packages = root.entry("packages".to_string()).or_insert_with(|| serde_json::json!({}));
	let Some(packages) = packages.as_object_mut() else {
		eprintln!(
			"unity-exporter-vcc: repo index packages must be an object: {}",
			repo_index.display()
		);
		return false;
	};
	let package = packages
		.entry(UNITY_EXPORTER_PACKAGE_ID.to_string())
		.or_insert_with(|| serde_json::json!({ "versions": {} }));
	let Some(package) = package.as_object_mut() else {
		eprintln!("unity-exporter-vcc: package entry must be an object: {}", repo_index.display());
		return false;
	};
	let versions = package.entry("versions".to_string()).or_insert_with(|| serde_json::json!({}));
	let Some(versions) = versions.as_object_mut() else {
		eprintln!("unity-exporter-vcc: package versions must be an object: {}", repo_index.display());
		return false;
	};
	let mut version_manifest = manifest;
	if let Some(object) = version_manifest.as_object_mut() {
		object.insert("zipSHA256".to_string(), serde_json::Value::String(zip_sha256.to_string()));
	}
	versions.insert(version.to_string(), version_manifest);

	if let Some(parent) = repo_index.parent() {
		if let Err(err) = fs::create_dir_all(parent) {
			eprintln!("unity-exporter-vcc: mkdir {}: {err}", parent.display());
			return false;
		}
	}
	let serialized = match serde_json::to_string_pretty(&listing) {
		Ok(value) => value + "\n",
		Err(err) => {
			eprintln!("unity-exporter-vcc: serialize repo index: {err}");
			return false;
		}
	};
	if let Err(err) = fs::write(repo_index, serialized) {
		eprintln!("unity-exporter-vcc: write {}: {err}", repo_index.display());
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
	if !run_package_render_smoke_with_options(repo, PackageRenderSmokeOptions::default()) {
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
	if !verify_release_zip_entries(&zip_path, &package_name, !skip_spout2) {
		return false;
	}
	let Some(checksum_path) = write_sha256_file(&zip_path) else {
		eprintln!("release-package: sha256 failed: {}", zip_path.display());
		return false;
	};

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
	println!("release-package: sha256 {}", checksum_path.display());
	println!("PACKAGE_PATH={}", zip_path.display());
	true
}

fn release_audit_vcc_index_hash(repo_index: &Path, version: &str) -> Result<String, String> {
	let raw = fs::read_to_string(repo_index).map_err(|err| format!("read {}: {err}", repo_index.display()))?;
	let index: serde_json::Value = serde_json::from_str(&raw).map_err(|err| format!("parse {}: {err}", repo_index.display()))?;
	let entry = index
		.get("packages")
		.and_then(|packages| packages.get(UNITY_EXPORTER_PACKAGE_ID))
		.and_then(|package| package.get("versions"))
		.and_then(|versions| versions.get(version))
		.ok_or_else(|| {
			format!(
				"{} missing packages.{UNITY_EXPORTER_PACKAGE_ID}.versions.{version}",
				repo_index.display()
			)
		})?;
	let name = entry.get("name").and_then(serde_json::Value::as_str).ok_or_else(|| {
		format!(
			"{} missing packages.{UNITY_EXPORTER_PACKAGE_ID}.versions.{version}.name",
			repo_index.display()
		)
	})?;
	if name != UNITY_EXPORTER_PACKAGE_ID {
		return Err(format!(
			"{} VCC package name mismatch for {version}: index={} expected={UNITY_EXPORTER_PACKAGE_ID}",
			repo_index.display(),
			name
		));
	}
	let entry_version = entry.get("version").and_then(serde_json::Value::as_str).ok_or_else(|| {
		format!(
			"{} missing packages.{UNITY_EXPORTER_PACKAGE_ID}.versions.{version}.version",
			repo_index.display()
		)
	})?;
	if entry_version != version {
		return Err(format!(
			"{} VCC package version mismatch for {version}: index={entry_version}",
			repo_index.display()
		));
	}
	let url = entry.get("url").and_then(serde_json::Value::as_str).ok_or_else(|| {
		format!(
			"{} missing packages.{UNITY_EXPORTER_PACKAGE_ID}.versions.{version}.url",
			repo_index.display()
		)
	})?;
	let expected_zip_name = format!("{UNITY_EXPORTER_PACKAGE_ID}-{version}.zip");
	if !url.trim_end_matches('/').ends_with(&format!("/{expected_zip_name}")) {
		return Err(format!(
			"{} VCC package URL mismatch for {version}: index={} expected suffix=/{expected_zip_name}",
			repo_index.display(),
			url
		));
	}
	entry
		.get("zipSHA256")
		.and_then(serde_json::Value::as_str)
		.map(str::to_string)
		.ok_or_else(|| {
			format!(
				"{} missing packages.{UNITY_EXPORTER_PACKAGE_ID}.versions.{version}.zipSHA256",
				repo_index.display()
			)
		})
}

fn release_notes_hash(raw: &str, label: &str) -> Option<String> {
	let prefix = format!("- {label}: `");
	raw.lines().find_map(|line| {
		let rest = line.trim().strip_prefix(&prefix)?;
		let (hash, _) = rest.split_once('`')?;
		(!hash.trim().is_empty()).then(|| hash.trim().to_string())
	})
}

fn release_doc_path(repo: &Path, path: &Path) -> Option<String> {
	let rel = path.strip_prefix(repo).ok()?;
	Some(
		rel.components()
			.map(|component| component.as_os_str().to_string_lossy().into_owned())
			.collect::<Vec<_>>()
			.join("/"),
	)
}

fn verify_release_notes_hashes(release_notes: &Path, portable_hash: &str, vcc_hash: &str) -> bool {
	if !release_notes.exists() {
		return true;
	}
	let raw = match fs::read_to_string(release_notes) {
		Ok(raw) => raw,
		Err(err) => {
			eprintln!("release-audit: read {}: {err}", release_notes.display());
			return false;
		}
	};
	let Some(notes_portable_hash) = release_notes_hash(&raw, "zip SHA-256") else {
		eprintln!(
			"release-audit: {} missing `zip SHA-256` release note entry",
			release_notes.display()
		);
		return false;
	};
	if !notes_portable_hash.eq_ignore_ascii_case(portable_hash) {
		eprintln!(
			"release-audit: release notes portable hash mismatch: notes={} actual={} file={}",
			notes_portable_hash,
			portable_hash,
			release_notes.display()
		);
		return false;
	}
	let Some(notes_vcc_hash) = release_notes_hash(&raw, "VCC zip SHA-256") else {
		eprintln!(
			"release-audit: {} missing `VCC zip SHA-256` release note entry",
			release_notes.display()
		);
		return false;
	};
	if !notes_vcc_hash.eq_ignore_ascii_case(vcc_hash) {
		eprintln!(
			"release-audit: release notes VCC hash mismatch: notes={} actual={} file={}",
			notes_vcc_hash,
			vcc_hash,
			release_notes.display()
		);
		return false;
	}
	println!("release-audit: release notes hashes ok");
	true
}

fn verify_release_notes_required_text(release_notes: &Path) -> bool {
	if !release_notes.exists() {
		return true;
	}
	let raw = match fs::read_to_string(release_notes) {
		Ok(raw) => raw,
		Err(err) => {
			eprintln!("release-audit: read {}: {err}", release_notes.display());
			return false;
		}
	};
	let checks = [
		(
			"portable zip source of truth",
			"Portable Windows zip is the v2 distribution source of truth",
		),
		("installer outside v2", "Installer"),
		("auto-update outside v2", "auto-update"),
		("authenticode outside v2", "Authenticode"),
		("portable zip hash", "- zip SHA-256: `"),
		("VCC package hash", "- VCC zip SHA-256: `"),
		("known limitations section", "## Known Limitations"),
		("unsupported areas explicit", "State the unsupported v2 areas explicitly"),
	];
	let missing = checks
		.into_iter()
		.filter_map(|(label, needle)| (!raw.contains(needle)).then_some(label))
		.collect::<Vec<_>>();
	if !missing.is_empty() {
		eprintln!(
			"release-audit: {} missing required release-note text: {}",
			release_notes.display(),
			missing.join(", ")
		);
		return false;
	}
	println!("release-audit: release notes required text ok");
	true
}

fn verify_release_doc_value(raw: &str, path: &Path, label: &str, expected: &str, context: &str, case_insensitive: bool) -> bool {
	let Some(actual) = release_notes_hash(raw, label) else {
		eprintln!("release-audit: {} missing `{label}` {context} entry", path.display());
		return false;
	};
	let matches = if case_insensitive {
		actual.eq_ignore_ascii_case(expected)
	} else {
		actual == expected
	};
	if !matches {
		eprintln!(
			"release-audit: {context} {label} mismatch: doc={} expected={} file={}",
			actual,
			expected,
			path.display()
		);
		return false;
	}
	true
}

fn verify_manual_release_checklist_candidate(
	checklist: &Path,
	version: &str,
	portable_zip: &str,
	portable_hash: &str,
	vcc_zip: &str,
	vcc_hash: &str,
) -> bool {
	if !checklist.exists() {
		return true;
	}
	let raw = match fs::read_to_string(checklist) {
		Ok(raw) => raw,
		Err(err) => {
			eprintln!("release-audit: read {}: {err}", checklist.display());
			return false;
		}
	};
	let context = "manual checklist Candidate Build";
	let required_lines = [
		"`cargo xtask ci` result: passed".to_string(),
		format!("`cargo xtask release-audit --version <version>` result: passed for `{version}`"),
		"`release-audit` confirms release notes hashes: yes".to_string(),
		"`cargo xtask package-render-smoke` result: passed".to_string(),
		"`package-render-smoke` representative wardrobe `field_drape`: passed; missing counts `0`, scoped missing groups `[]`".to_string(),
		"`package-render-smoke` representative wardrobe `noble1`: passed; missing counts `0`, scoped missing groups `[]`".to_string(),
	];
	let missing = required_lines
		.iter()
		.filter(|line| !raw.contains(line.as_str()))
		.cloned()
		.collect::<Vec<_>>();
	if !missing.is_empty() {
		eprintln!(
			"release-audit: {} missing manual checklist Candidate Build evidence: {}",
			checklist.display(),
			missing.join(", ")
		);
		return false;
	}
	let ok = verify_release_doc_value(&raw, checklist, "Version", version, context, false)
		&& verify_release_doc_value(&raw, checklist, "Portable zip", portable_zip, context, false)
		&& verify_release_doc_value(&raw, checklist, "Portable zip SHA-256", portable_hash, context, true)
		&& verify_release_doc_value(&raw, checklist, "VCC package zip", vcc_zip, context, false)
		&& verify_release_doc_value(&raw, checklist, "VCC package SHA-256", vcc_hash, context, true);
	if ok {
		println!("release-audit: manual checklist Candidate Build ok");
		let open_evidence = manual_release_checklist_open_evidence_items(&raw);
		if !open_evidence.is_empty() {
			println!(
				"release-audit: manual checklist has {} open evidence item(s): {}",
				open_evidence.len(),
				open_evidence.join("; ")
			);
		}
	}
	ok
}

fn manual_release_checklist_open_evidence_items(raw: &str) -> Vec<String> {
	let mut section = String::new();
	let mut in_evidence = false;
	let mut items = Vec::new();
	for line in raw.lines() {
		let trimmed = line.trim();
		if trimmed.starts_with("## ") {
			section = trimmed.trim_start_matches("## ").trim().to_string();
			in_evidence = false;
			continue;
		}
		if trimmed == "Evidence:" {
			in_evidence = true;
			continue;
		}
		if !in_evidence || trimmed.is_empty() {
			continue;
		}
		if trimmed.starts_with("- ") && trimmed.ends_with(':') {
			let item = trimmed.trim_start_matches("- ").trim_end_matches(':').trim();
			if section.is_empty() {
				items.push(item.to_string());
			} else {
				items.push(format!("{section} / {item}"));
			}
		}
	}
	items
}

fn verify_release_checksum_sidecar(zip_path: &Path) -> bool {
	let Some(checksum_path) = checksum_file_path(zip_path) else {
		eprintln!("release-audit: cannot compute checksum sidecar path for {}", zip_path.display());
		return false;
	};
	let expected_artifact = match zip_path.file_name().and_then(|name| name.to_str()) {
		Some(name) => name,
		None => {
			eprintln!("release-audit: zip file name is not UTF-8: {}", zip_path.display());
			return false;
		}
	};
	let Some(actual_hash) = file_sha256(zip_path) else {
		eprintln!("release-audit: sha256 failed: {}", zip_path.display());
		return false;
	};
	let raw = match fs::read_to_string(&checksum_path) {
		Ok(raw) => raw,
		Err(err) => {
			eprintln!("release-audit: read {}: {err}", checksum_path.display());
			return false;
		}
	};
	let Some((sidecar_hash, sidecar_artifact)) = parse_checksum_file(&raw) else {
		eprintln!("release-audit: malformed checksum sidecar: {}", checksum_path.display());
		return false;
	};
	if !sidecar_hash.eq_ignore_ascii_case(&actual_hash) {
		eprintln!(
			"release-audit: checksum sidecar hash mismatch for {}: sidecar={} actual={}",
			zip_path.display(),
			sidecar_hash,
			actual_hash
		);
		return false;
	}
	if sidecar_artifact != expected_artifact {
		eprintln!(
			"release-audit: checksum sidecar artifact mismatch for {}: sidecar={} expected={}",
			zip_path.display(),
			sidecar_artifact,
			expected_artifact
		);
		return false;
	}
	println!("release-audit: portable zip sha256 {actual_hash}");
	true
}

fn run_release_audit(repo: &Path, args: impl Iterator<Item = String>) -> bool {
	let mut version = None;
	let mut output_dir = None;
	let mut vcc_dir = None;
	let mut repo_index = None;
	let mut skip_spout2 = false;
	let mut iter = args.peekable();
	while let Some(arg) = iter.next() {
		match arg.as_str() {
			"--version" => {
				let Some(value) = iter.next() else {
					eprintln!("release-audit: --version には version が必要です");
					return false;
				};
				version = Some(value);
			}
			"--output-dir" => {
				let Some(value) = iter.next() else {
					eprintln!("release-audit: --output-dir には path が必要です");
					return false;
				};
				output_dir = Some(path_from_arg(repo, value));
			}
			"--vcc-dir" => {
				let Some(value) = iter.next() else {
					eprintln!("release-audit: --vcc-dir には path が必要です");
					return false;
				};
				vcc_dir = Some(path_from_arg(repo, value));
			}
			"--repo-index" => {
				let Some(value) = iter.next() else {
					eprintln!("release-audit: --repo-index には path が必要です");
					return false;
				};
				repo_index = Some(path_from_arg(repo, value));
			}
			"--skip-spout2" => skip_spout2 = true,
			"help" | "--help" | "-h" => {
				print_release_audit_usage();
				return true;
			}
			other => {
				eprintln!("release-audit: 不明な option: {other}");
				print_release_audit_usage();
				return false;
			}
		}
	}
	let Some(version) = version.or_else(|| default_package_version(repo)) else {
		eprintln!("release-audit: Cargo.toml の workspace.package.version を読めませんでした");
		return false;
	};
	let package_name = format!("un-avatar-{version}");
	let output_dir = output_dir.unwrap_or_else(|| repo.join("release-packages"));
	let zip_path = output_dir.join(format!("{package_name}.zip"));
	if !verify_release_zip_entries(&zip_path, &package_name, !skip_spout2) {
		return false;
	}
	if !verify_release_checksum_sidecar(&zip_path) {
		return false;
	}
	if !verify_release_zip_clean_unpack(&zip_path, &package_name, !skip_spout2) {
		return false;
	}
	if !verify_release_zip_source_docs(&zip_path, &package_name, repo) {
		return false;
	}

	let vcc_dir = vcc_dir.unwrap_or_else(|| repo.join("target").join("unity").join("vcc"));
	let vcc_zip = vcc_dir.join(format!("{UNITY_EXPORTER_PACKAGE_ID}-{version}.zip"));
	if !verify_vcc_package_zip_entries(&vcc_zip) {
		return false;
	}
	let vcc_staging = repo.join("target").join("unity").join("vcc-staging").join(UNITY_EXPORTER_PACKAGE);
	if !verify_vcc_package_zip_staging_files(&vcc_zip, &vcc_staging) {
		return false;
	}
	if !verify_vcc_package_zip_manifest(&vcc_zip, &version) {
		return false;
	}
	let Some(vcc_hash) = file_sha256(&vcc_zip) else {
		eprintln!("release-audit: sha256 failed: {}", vcc_zip.display());
		return false;
	};
	let repo_index = repo_index.unwrap_or_else(|| repo.join("docs").join("vcc").join("index.json"));
	let index_hash = match release_audit_vcc_index_hash(&repo_index, &version) {
		Ok(hash) => hash,
		Err(err) => {
			eprintln!("release-audit: {err}");
			return false;
		}
	};
	if !index_hash.eq_ignore_ascii_case(&vcc_hash) {
		eprintln!(
			"release-audit: VCC zipSHA256 mismatch: index={} actual={} zip={}",
			index_hash,
			vcc_hash,
			vcc_zip.display()
		);
		return false;
	}
	if !verify_vcc_package_manifest_matches_index(&repo_index, &vcc_zip, &version) {
		return false;
	}
	println!("release-audit: VCC zip sha256 {vcc_hash}");
	let Some(portable_hash) = file_sha256(&zip_path) else {
		eprintln!("release-audit: sha256 failed: {}", zip_path.display());
		return false;
	};
	let release_work = repo.join("local").join("release-work");
	if !verify_release_notes_hashes(&release_work.join("v2-release-notes-draft.md"), &portable_hash, &vcc_hash) {
		return false;
	}
	if !verify_release_notes_required_text(&release_work.join("v2-release-notes-draft.md")) {
		return false;
	}
	if let (Some(portable_doc_path), Some(vcc_doc_path)) = (release_doc_path(repo, &zip_path), release_doc_path(repo, &vcc_zip)) {
		if !verify_manual_release_checklist_candidate(
			&release_work.join("v2-manual-release-checklist.md"),
			&version,
			&portable_doc_path,
			&portable_hash,
			&vcc_doc_path,
			&vcc_hash,
		) {
			return false;
		}
	}
	println!("release-audit: ok for {version}");
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

fn print_unity_exporter_vcc_usage() {
	eprintln!(
		"cargo xtask unity-exporter-vcc [--version <version>] [--base-url <url>] [--package-url <url>] [--output-dir <path>] [--repo-index <path>]\n\
	\n\
	VCC Package Manager 用に Unity Exporter package zip と repo listing index.json を生成する。\n\
	既定 version は Cargo.toml の workspace.package.version。\n\
	既定 base-url は https://github.com/usagi/un-avatar/releases/download/<version> 。git tag / release title に v prefix は付けない。\n\
	既定出力先は target/unity/vcc、repo listing は docs/vcc/index.json。"
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
	配布ディレクトリを作成し、release-packages/un-avatar-<version>.zip と .sha256.txt sidecar を生成する。\n\
	packaged Renderer の windowless startup smoke と release zip の必須 entry 検査も実行する。\n\
	既定versionは Cargo.toml の workspace.package.version。\n\
	--skip-build は既存の target/package/un-avatar をzip化する。"
	);
}

fn print_release_audit_usage() {
	eprintln!(
		"cargo xtask release-audit [--version <version>] [--output-dir <path>] [--vcc-dir <path>] [--repo-index <path>] [--skip-spout2]\n\
	\n\
	既存の portable zip / .sha256.txt / VCC package zip / docs/vcc/index.json / local/release-work の任意 release notes / manual checklist を再ビルドせず検査する。\n\
	portable zip 内の README / LICENSE / third-party notices が現行 source と一致し、VCC zip の必須 entry が target/unity/vcc-staging の生成元と一致することも確認する。"
	);
}

fn print_package_render_smoke_usage() {
	eprintln!(
		"cargo xtask package-render-smoke [--manifest <path> [--wardrobe-set <id>]]\n\
	\n\
	target/package/un-avatar の packaged Renderer で windowless startup validation を実行する。\n\
	--manifest 未指定時は tiny fixture glTF manifest を一時生成する。\n\
	--manifest 指定時は実 avatar/profile manifest を検査でき、--wardrobe-set で起動時 wardrobe set を上書きできる。"
	);
}

fn print_unphysics_exporter_audit_usage() {
	eprintln!(
		"cargo xtask unphysics-exporter-audit [avatar.unavatar ...]\n\
	\n\
	current Unity Exporter で出力した .unavatar の UNPhysics / UNDynamics sourceParams を高速検査する。\n\
	引数なしでは target/tmp/usagi.unavatar、target/tmp/blanca.unavatar、target/tmp/mizuki.unavatar を検査する。\n\
	内部では un-avatar dynamics-scan --require-current-exporter --json を実行し、必須 term 欠落時は失敗する。"
	);
}

fn print_unphysics_importer_audit_usage() {
	eprintln!(
		"cargo xtask unphysics-importer-audit [--wardrobe-set <SET_ID>] [--require-runtime-evidence] [--require-node-constraints] [--require-parent-node-constraints] [--require-mesh-cloth-assist-candidates] [avatar.unavatar ...]\n\
	\n\
	current Unity Exporter で出力した .unavatar を Importer/lowering まで通し、UNPhysics / UNDynamics runtime evidence を検査する。\n\
	引数なしでは target/tmp/usagi.unavatar、target/tmp/blanca.unavatar、target/tmp/mizuki.unavatar を検査する。\n\
	--wardrobe-set 指定時は wardrobe id が代表モデル間で共有されるとは限らないため、対象 avatar path を明示する。\n\
	内部では un-avatar dynamics-import-audit --require-runtime-evidence --json を実行し、sourceParams が runtime group / response group へ落ちない場合は失敗する。同じ import report から scene nodeConstraints 件数も表示する。\n\
	--require-node-constraints / --require-parent-node-constraints は constraint metadata が必要な regression profile で 0 件を失敗にする opt-in gate。\n\
	--require-mesh-cloth-assist-candidates は cloth assist が必要な wardrobe/profile で candidate 0 件を失敗にする opt-in gate。"
	);
}

fn print_unphysics_response_audit_usage() {
	eprintln!(
		"cargo xtask unphysics-response-audit [--wardrobe-set <SET_ID>] [--require-visual-response-evidence] [avatar.unavatar ...]\n\
	\n\
	UNPhysics / UNDynamics runtime response が soft/firm profile override で実際に変化するか検査する。\n\
	引数なしでは target/tmp/usagi.unavatar、target/tmp/blanca.unavatar、target/tmp/mizuki.unavatar を検査する。\n\
	--wardrobe-set 指定時は wardrobe id が代表モデル間で共有されるとは限らないため、対象 avatar path を明示する。\n\
	内部では un-avatar dynamics-response-audit --require-override-effect --json を実行し、rest_response / shape_preservation / bounce_response / damping_half_life_ms / motion_coupling が分離しない場合は失敗する。\n\
	summary は mode ごとの category count と joint_count 最大 category を top として出し、wardrobe scope 後の response 偏りを読む。\n\
	--require-visual-response-evidence は active response group があるのに weighted visible skin joint / visible mesh subtree evidence が 0 の場合を失敗にする opt-in gate。"
	);
}

fn print_unphysics_motion_audit_usage() {
	eprintln!(
		"cargo xtask unphysics-motion-audit\n\
	\n\
	UNPhysics / UNDynamics の solver motion trace テストをまとめて実行する。\n\
	soft/firm tuning、cloth、ears preset、bounce、source-authored shape intent と soft response の分離が実際の step 出力を変えることを検査する。"
	);
}

fn print_unphysics_motion_trace_audit_usage() {
	eprintln!(
		"cargo xtask unphysics-motion-trace-audit [--wardrobe-set <SET_ID>] [--include-short-recovery] [--require-no-visible-findings] [--require-known-finding-visibility] [--frames <n>] [--recovery-frames <n>] [--tuning authored|soft|firm|rest-low|rest-high|shape-low|shape-high|bounce-low|bounce-high|follow-low|follow-high|gravity-off|gravity-low|gravity-high|stretch-off|stretch-low|stretch-high|damping-long|damping-short] [avatar.unavatar ...]\n\
	\n\
	current Unity Exporter で出力した .unavatar に簡易root rotation motionを流し、カテゴリ別/group別 lag と回復を検査する。\n\
	--wardrobe-set 指定時は wardrobe id が代表モデル間で共有されるとは限らないため、対象 avatar path を明示する。\n\
	summary は findingKinds / findingTop / visualFindingTop に加え、visualFindings / nonvisualFindings / unknownVisibilityFindings で可視 target ありの候補と制御・interaction 系候補と未分類候補を分けて表示する。\n\
	top / top_group は全体の監査候補、top_visual / top_visual_group は可視 target あり group、top_nonvisual / top_nonvisual_group は可視 target なし group の監査順を基準にした候補を表示する。\n\
	xtask は CLI を --require-motion-evidence 付きで呼び、missing_motion_evidence が空でない場合は visual finding の有無に関係なく失敗する。\n\
	--require-no-visible-findings は visualFindings > 0 を失敗にし、非可視制御 finding は失敗扱いにしない。\n\
	--require-known-finding-visibility は unknownVisibilityFindings > 0 を失敗にし、finding の可視性分類漏れを gate する。\n\
	default は --frames 24 --recovery-frames 240。短期回復を見る場合は --recovery-frames 24 または 96 を明示する。\n\
	--include-short-recovery は指定 recovery に加えて 24 / 96 frame recovery も同じ入力で検査する。\n\
	--tuning は既定 authored。soft / firm と単一term tuning は全カテゴリoverrideを監査時だけ適用する。\n\
	引数なしでは target/tmp/usagi.unavatar、target/tmp/blanca.unavatar、target/tmp/mizuki.unavatar を検査する。"
	);
}

fn print_unphysics_vertex_probe_audit_usage() {
	eprintln!(
		"cargo xtask unphysics-vertex-probe-audit [--wardrobe-set <SET_ID>] [--node-contains <TEXT>] [--settle-frames <n>] [--tuning authored|soft|firm|rest-low|rest-high|shape-low|shape-high|bounce-low|bounce-high|follow-low|follow-high|gravity-off|gravity-low|gravity-high|stretch-off|stretch-low|stretch-high|damping-long|damping-short] [--apply-mesh-cloth-assist] [--ignore-authored-colliders] [--require-mesh-cloth-assist-changes] [--require-collision-projections] [--require-probe-collision-projections] [--require-collision-projection-sources] [--require-collision-projection-paths] [--require-collider-summaries] [--require-probe-collider-candidates] [--require-projecting-collider-summaries] [avatar.unavatar ...]\n\
	\n\
	dynamics-vertex-probe を JSON で実行し、runtime collider count、mesh cloth assist 変更頂点数、collision projection 件数、probe 対象 mesh の dynamic source に限定した projection 件数、probe source 限定 collider summary / penetrating summary / projecting summary 件数、projection source 種類数、projection collider path 数、collider summary 件数、projection 付き collider summary 件数、projection_count 最大 collider path、probe source 限定の最近接 candidate collider path を一行に要約する。\n\
	--wardrobe-set 指定時は wardrobe id が代表モデル間で共有されるとは限らないため、対象 avatar path を明示する。\n\
	--require-mesh-cloth-assist-changes は --apply-mesh-cloth-assist が実際に changedVertices>0 を出すことを失敗条件として確認する。cloth assist が必要な wardrobe/profile 向けの opt-in gate。\n\
	--require-collision-projections は collisionProjections=0 を失敗にする。collider が効いたかを見る opt-in gate であり、通常の幾何変位監査そのものは失敗扱いにしない。\n\
	--require-probe-collision-projections は probeCollisionProjections=0 を失敗にする。全体 simulation ではなく、選択 mesh の weighted dynamic source が実 projection まで届いたかを見る opt-in gate。\n\
	--require-collision-projection-sources は projectionSources=0 を失敗にする。どの dynamics source が実 projection まで届いたかを見る opt-in gate。\n\
	--require-collision-projection-paths は projectionColliderPaths=0 を失敗にする。どの authored/runtime collider path が実 projection まで届いたかを見る opt-in gate。\n\
	--require-collider-summaries は colliderSummaries=0 を失敗にする。candidate/contact/projection summary が JSON に届いたかを見る opt-in gate。\n\
	--require-probe-collider-candidates は probe source 限定 collider summary に candidate_count>0 が無い場合を失敗にする。対象 mesh の weighted dynamic source が collider 距離評価まで届いたかを見る opt-in gate。\n\
	--require-projecting-collider-summaries は projectingColliderSummaries=0 を失敗にする。summary が実 projection count まで持つことを見る opt-in gate。\n\
	topProjectingCollider は projection_count 最大の collider path を表示し、authored collider がどの path で実押し出しへ届いたかを読む。\n\
	topProbeCandidateCollider は probe source 限定 candidate の min_margin 最小 collider path を表示し、probeCollisionProjections=0 の時に候補なしと候補あり非貫通を分ける。\n\
	引数なしでは target/tmp/usagi.unavatar、target/tmp/blanca.unavatar、target/tmp/mizuki.unavatar を検査する。"
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
		"cargo xtask run-renderer --profile <name> [--wardrobe-set <id>] [--release] [-- <renderer args>]\n\
		cargo xtask run-renderer --manifest <path> [--wardrobe-set <id>] [--release] [-- <renderer args>]\n\
	\n\
	Renderer を必要なら cargo build --locked で更新してから target/{{debug|release}}/un-avatar-renderer を直接起動する。\n\
	Spout2 SDK/runtime が staged 済みなら cargo xtask build/run と同じく spout-sdk feature 付きでビルドし、起動 PATH に Spout.dll を追加する。\n\
	UN Avatar の user profile dir (%APPDATA%/UN Avatar/profiles) を優先し、次に repo の profiles/ を探す。\n\
	<name> は file stem、timestamp接頭辞を除いた stem、[profile].id、[profile].display_name、title に一致する。\n\
	--wardrobe-set は profile/manifest を上書き保存せず、この起動だけ renderer の wardrobe set を指定する。\n\
	例: cargo xtask run-renderer --profile model1\n\
	    cargo xtask run-renderer --profile mizuki-split --wardrobe-set field_drape\n\
	    cargo xtask run-renderer --profile mizuki-split --wardrobe-set field_drape -- --prewarm-scene-cache\n\
	    cargo xtask run-renderer --profile model2 -- --debug-material-dump"
	);
}

fn print_summarize_renderer_log_usage() {
	eprintln!(
		"cargo xtask summarize-renderer-log <log-file-or-dir>...\n\
	\n\
	renderer stderr log から import / texture prepare / mesh prepare / frame bench / pipeline cache の主要値をTSVで要約する。\n\
	ディレクトリを渡した場合は直下の .log を対象にする。\n\
	例: cargo xtask summarize-renderer-log target/tmp/mizuki-field-drape-source-hash-default-hot-180.log"
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
	let mut joints = vec![
		identity_transform_sample(),
		identity_transform_sample(),
		identity_transform_sample(),
	];
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
	retarget_finger_frame(side_prefix, Finger::Thumb, CoordinateSpace::UNMotion, 0, Quat::from_rotation_y(yaw))
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
) -> Option<f32> {
	let thumb_key = format!("{side_prefix}thumbdistal");
	let index_key = format!("{side_prefix}indexproximal");
	let Ok(open_thumb) = world_point(open_document, &thumb_key) else {
		return None;
	};
	let Ok(open_index) = world_point(open_document, &index_key) else {
		return None;
	};
	let Ok(curled_thumb) = world_point(curled_document, &thumb_key) else {
		return None;
	};
	let Ok(curled_index) = world_point(curled_document, &index_key) else {
		return None;
	};
	let open_distance = open_thumb.distance(open_index);
	let curled_distance = curled_thumb.distance(curled_index);
	let delta = curled_distance - open_distance;
	println!(
		"  {:24} {name}_thumb_index_dist open={open_distance:.5} curled={curled_distance:.5} delta={:+.5}",
		model.label, delta
	);
	Some(delta)
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

fn compare_signed_distance_delta(ok: &mut bool, name: &str, model_deltas: &BTreeMap<&'static str, f32>, reference_label: &'static str) {
	let Some(reference) = model_deltas.get(reference_label).copied() else {
		return;
	};
	if reference.abs() <= 1e-5 {
		return;
	}
	for (label, delta) in model_deltas {
		println!(
			"  {:24} {name}_signed_delta_to_{}={:+.5}",
			label,
			reference_label,
			delta - reference
		);
		if label.starts_with("unavatar:") && (*delta * reference) <= 0.0 {
			eprintln!("retarget-audit: {} {name} direction is reversed from {}", label, reference_label);
			*ok = false;
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
		let thumb_axis =
			(world[successor_index].transform_point3(Vec3::ZERO) - world[thumb_index].transform_point3(Vec3::ZERO)).normalize_or_zero();
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
			let frame = retarget_finger_frame(
				case.side_prefix,
				case.finger,
				case.coordinate_space,
				case.joint_index,
				case.rotation,
			);
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
				let max_delta_deg = if case.finger == Finger::Thumb { 20.0 } else { 8.0 };
				if angle > max_delta_deg {
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
				let frame = retarget_finger_frame(
					case.side_prefix,
					case.finger,
					case.coordinate_space,
					case.joint_index,
					case.rotation,
				);
				apply_un_motion_frame_to_document_with_rest(
					&mut document,
					&frame,
					ApplyUnMotionFrameOpts::default(),
					Some(&model.rest_nodes),
				);
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
		let mut thumb_index_distance_deltas = BTreeMap::new();
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
			if let Some(delta) = print_thumb_index_distance_delta("y_curl", side_prefix, model, &open_document, &curled_document) {
				thumb_index_distance_deltas.insert(model.label, delta);
			}
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
		compare_signed_distance_delta(&mut ok, "thumb_index_y_curl", &thumb_index_distance_deltas, "vrm0:model1");
		compare_delta_direction(&mut ok, "basis_y_curl", &basis_y_deltas, "vrm0:model1", 20.0);
		for (basis_name, basis_deltas) in [("basis_x_curl", &basis_x_deltas), ("basis_z_curl", &basis_z_deltas)] {
			let mut diagnostic_only = true;
			compare_delta_direction(&mut diagnostic_only, basis_name, basis_deltas, "vrm0:model1", 180.0);
		}
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
		let mut thumb_index_distance_deltas = BTreeMap::new();
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
			if let Some(delta) = print_thumb_index_distance_delta("z_curl", side_prefix, model, &open_document, &curled_document) {
				thumb_index_distance_deltas.insert(model.label, delta);
			}
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
		compare_signed_distance_delta(&mut ok, "thumb_index_z_curl", &thumb_index_distance_deltas, "vrm0:model1");
		let mut diagnostic_only = true;
		compare_delta_direction(&mut diagnostic_only, "successor_axis_z_curl", &deltas, "vrm0:model1", 180.0);
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
			println!("  {prefix}.thumb[{index}] q=({:+.5},{:+.5},{:+.5},{:+.5})", q.x, q.y, q.z, q.w);
		}
	}
}

fn run_unmotion_thumb_dump(args: &[String]) -> bool {
	let key = args.first().cloned().unwrap_or_else(|| "un-motion/frame".to_string());
	let seconds = args.get(1).and_then(|value| value.parse::<f32>().ok()).unwrap_or(3.0).max(0.1);
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
  release-guard v2 release-prep の unit/static regression guard を実行（GUI/package rebuildなし）\n\
  smoke        CLI formats list / sample plugin / convert を確認\n\
  unphysics-exporter-audit current exporter のUNPhysics sourceParams必須termを検査\n\
  unphysics-importer-audit current exporter 出力をImporter/lowering後のruntime evidenceまで検査\n\
  unphysics-response-audit UNPhysics response がsoft/firm overrideで変化することを検査\n\
  unphysics-motion-audit UNPhysics solver motion trace がtuningで変化することを検査\n\
  unphysics-motion-trace-audit current exporter 出力でカテゴリ別motion lagを検査\n\
  unphysics-vertex-probe-audit current exporter 出力でcollider projectionを検査\n\
  render-smoke renderer manifestを生成し、fixture glTFを起動前検証でimportできることを確認（windowは開かない）\n\
  package-render-smoke target/package/un-avatar の renderer で render-smoke と同じ検証を実行（--manifest / --wardrobe-set対応）\n\
  run-renderer  profile名またはmanifest pathからrenderer windowを起動\n\
  summarize-renderer-log renderer stderr log の主要startup/bench値をTSV要約\n\
  retarget-audit VRM0/VRM1/.unavatar のCPU Humanoid retarget軸比較\n\
  unmotion-thumb-dump [key] [seconds] UNMotion/Zenoh の thumb joint quaternion を短時間dump\n\
	acceptance-preflight MVP acceptance の実機確認前に必要な高速preflightを実行\n\
	acceptance-prepare   MVP acceptance の証跡テンプレートと実測用manifestを生成\n\
  spout2       Spout2 を取得・CMake Release ビルドし、配布物へ配置\n\
  unity-fpng   Unity Exporter の native fpng plugin をビルドし、開発用 package へ配置\n\
  unity-exporter-package Unity Editor exporter の UPM package layout を作る\n\
  unity-exporter-vcc Unity Exporter の VCC Package Manager 用zip/repo listingを作る\n\
  package      Releaseビルドし、target/package/un-avatar に最小配布レイアウトを作る\n\
	release-package target/package/un-avatar を release-packages/un-avatar-<version>.zip に固める\n\
	release-audit   既存 release zip / sidecar / VCC zip / repo listing と任意 local release-work 証跡の整合を検査\n\
  ci           supervisor frontend check → fmt --check → check --workspace → test --workspace → smoke → render-smoke\n"
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
		"release-guard" => run_release_guard(repo),
		"smoke" => run_smoke(repo),
		"unphysics-exporter-audit" => run_unphysics_exporter_audit(repo, args),
		"unphysics-importer-audit" => run_unphysics_importer_audit(repo, args),
		"unphysics-response-audit" => run_unphysics_response_audit(repo, args),
		"unphysics-motion-audit" => run_unphysics_motion_audit(repo, args),
		"unphysics-motion-trace-audit" => run_unphysics_motion_trace_audit(repo, args),
		"unphysics-vertex-probe-audit" => run_unphysics_vertex_probe_audit(repo, args),
		"render-smoke" => run_render_smoke(repo),
		"package-render-smoke" => run_package_render_smoke(repo, args),
		"run-renderer" => run_renderer(repo, args),
		"summarize-renderer-log" | "summarize-renderer-logs" => run_summarize_renderer_log(repo, args),
		"retarget-audit" => run_retarget_audit(repo),
		"unmotion-thumb-dump" => run_unmotion_thumb_dump(&args.collect::<Vec<_>>()),
		"acceptance-preflight" => run_acceptance_preflight(repo),
		"acceptance-prepare" => run_acceptance_prepare(repo),
		"spout2" => run_spout2(repo, args),
		"unity-fpng" => run_unity_fpng(repo, args),
		"unity-exporter-package" => run_unity_exporter_package(repo, args),
		"unity-exporter-vcc" => run_unity_exporter_vcc(repo, args),
		"package" => run_package(repo, args),
		"release-package" | "make-release-package" => run_release_package(repo, args),
		"release-audit" => run_release_audit(repo, args),
		"ci" => {
			run_supervisor_frontend_ci(repo)
				&& run_cargo(repo, &["fmt", "--all", "--", "--check"]).success()
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn unphysics_audit_paths_resolve_defaults_and_relative_paths() {
		let repo = env::temp_dir().join(format!("un-avatar-xtask-audit-paths-{}", process::id()));
		let target_tmp = repo.join("target").join("tmp");
		fs::create_dir_all(&target_tmp).expect("target tmp");
		for name in UNPHYSICS_AUDIT_DEFAULT_AVATARS {
			fs::write(target_tmp.join(name), b"fixture").expect("default avatar fixture");
		}

		let (default_paths, default_ok) = resolve_unphysics_audit_paths(&repo, Vec::new(), "test-audit");
		assert!(default_ok);
		assert_eq!(
			default_paths,
			UNPHYSICS_AUDIT_DEFAULT_AVATARS
				.into_iter()
				.map(|name| target_tmp.join(name))
				.collect::<Vec<_>>()
		);

		let custom = repo.join("custom.unavatar");
		fs::write(&custom, b"fixture").expect("custom avatar fixture");
		let (custom_paths, custom_ok) = resolve_unphysics_audit_paths(
			&repo,
			vec![PathBuf::from("custom.unavatar"), PathBuf::from("missing.unavatar")],
			"test-audit",
		);
		assert!(!custom_ok);
		assert_eq!(custom_paths, vec![custom]);

		let _ = fs::remove_dir_all(repo);
	}

	#[test]
	fn motion_trace_visible_findings_gate_ignores_nonvisual_findings() {
		assert!(!motion_trace_visible_findings_gate_failed(false, 1));
		assert!(!motion_trace_visible_findings_gate_failed(true, 0));
		assert!(motion_trace_visible_findings_gate_failed(true, 1));
		assert!(!motion_trace_unknown_visibility_gate_failed(false, 1));
		assert!(!motion_trace_unknown_visibility_gate_failed(true, 0));
		assert!(motion_trace_unknown_visibility_gate_failed(true, 1));

		let findings = vec![
			serde_json::json!({"kind": "nonvisual_control_motion", "visual_target": false}),
			serde_json::json!({"kind": "high_chain_lag", "source_id": "physbone:cloth", "visual_target": true}),
			serde_json::json!({"kind": "category_recovery_state"}),
		];
		assert_eq!(motion_trace_finding_visibility_counts(&findings), (1, 1, 1));
		assert_eq!(motion_trace_visual_finding_top_label(&findings), "high_chain_lag:physbone:cloth");
	}

	#[test]
	fn motion_trace_top_visual_group_skips_nonvisual_groups() {
		let groups = vec![
			serde_json::json!({
				"source_id": "physbone:control",
				"visual_target": false,
				"chain_rest_length": 0.1,
				"max_lag": 0.9,
				"max_lag_chain_ratio": 9.0,
				"initial_stable_offset": 0.2,
				"stable_offset": 0.3,
				"stable_offset_ratio": 0.0,
				"stable_offset_chain_ratio": 0.0,
				"recovery_state": "settled",
				"residual_motion": 0.0,
				"residual_motion_chain_ratio": 0.0
			}),
			serde_json::json!({
				"source_id": "physbone:cloth",
				"visual_target": true,
				"chain_rest_length": 0.5,
				"max_lag": 0.2,
				"max_lag_chain_ratio": 1.5,
				"initial_stable_offset": 0.2,
				"stable_offset": 0.0,
				"stable_offset_ratio": 0.0,
				"stable_offset_chain_ratio": 0.0,
				"recovery_state": "settled",
				"residual_motion": 0.0,
				"residual_motion_chain_ratio": 0.0
			}),
			serde_json::json!({
				"source_id": "physbone:ribbon",
				"visual_target": true,
				"chain_rest_length": 0.3,
				"max_lag": 0.4,
				"max_lag_chain_ratio": 2.5,
				"initial_stable_offset": 0.4,
				"stable_offset": 0.1,
				"stable_offset_ratio": 0.25,
				"stable_offset_chain_ratio": 0.3,
				"recovery_state": "settled_offset",
				"residual_motion": 0.0,
				"residual_motion_chain_ratio": 0.0
			}),
		];

		let top = motion_trace_top_group_label(&groups);
		let label = motion_trace_top_visual_group_label(&groups);
		let nonvisual_label = motion_trace_top_nonvisual_group_label(&groups);

		assert!(top.starts_with("physbone:control:visual=false"));
		assert!(label.starts_with("physbone:ribbon:visual=true"));
		assert!(nonvisual_label.starts_with("physbone:control:visual=false"));
	}

	#[test]
	fn response_audit_top_category_prefers_largest_joint_count() {
		let categories = vec![
			serde_json::json!({
				"category": "control",
				"group_count": 3,
				"visual_target_group_count": 0,
				"nonvisual_group_count": 3,
				"joint_count": 18,
				"visible_skinned_joint_count": 0,
				"visible_mesh_subtree_node_count": 0,
				"average_rest_response": 0.1,
				"average_shape_preservation": 0.2,
				"average_bounce_response": 0.3,
				"average_parent_motion_follow": 0.4,
				"average_orientation_follow": 0.5,
				"average_max_stretch_response": 0.6,
				"average_stretch_motion_response": 0.7
			}),
			serde_json::json!({
				"category": "cloth",
				"group_count": 5,
				"visual_target_group_count": 4,
				"nonvisual_group_count": 1,
				"joint_count": 12,
				"visible_skinned_joint_count": 10,
				"visible_mesh_subtree_node_count": 2,
				"average_rest_response": 0.11,
				"average_shape_preservation": 0.22,
				"average_bounce_response": 0.33,
				"average_parent_motion_follow": 0.44,
				"average_orientation_follow": 0.55,
				"average_max_stretch_response": 0.66,
				"average_stretch_motion_response": 0.77
			}),
		];

		let label = response_audit_top_category_label(&categories);
		let visual_label = response_audit_top_visual_category_label(&categories);
		let nonvisual_label = response_audit_top_nonvisual_category_label(&categories);

		assert!(label.starts_with("control:groups=3,visualGroups=0,nonvisualGroups=3,joints=18,visibleJoints=0,visibleMeshSubtrees=0"));
		assert!(
			visual_label.starts_with("cloth:groups=5,visualGroups=4,nonvisualGroups=1,joints=12,visibleJoints=10,visibleMeshSubtrees=2")
		);
		assert!(nonvisual_label
			.starts_with("control:groups=3,visualGroups=0,nonvisualGroups=3,joints=18,visibleJoints=0,visibleMeshSubtrees=0"));
		assert!(visual_label.contains("stretchMotion=0.770"));
	}

	#[test]
	fn response_audit_top_groups_split_visual_and_nonvisual_targets() {
		let groups = vec![
			serde_json::json!({
				"source_id": "physbone:control",
				"category": "other",
				"visual_target": false,
				"joint_count": 12,
				"skinned_joint_count": 0,
				"mesh_subtree_node_count": 0,
				"average_rest_response": 0.1,
				"average_shape_preservation": 0.2,
				"average_bounce_response": 0.3,
				"average_parent_motion_follow": 0.4,
				"average_orientation_follow": 0.5,
				"average_max_stretch_response": 0.6,
				"average_stretch_motion_response": 0.7
			}),
			serde_json::json!({
				"source_id": "physbone:cloth",
				"category": "cloth",
				"visual_target": true,
				"joint_count": 8,
				"skinned_joint_count": 6,
				"mesh_subtree_node_count": 1,
				"average_rest_response": 0.11,
				"average_shape_preservation": 0.22,
				"average_bounce_response": 0.33,
				"average_parent_motion_follow": 0.44,
				"average_orientation_follow": 0.55,
				"average_max_stretch_response": 0.66,
				"average_stretch_motion_response": 0.77
			}),
		];

		let top = response_audit_top_group_label(&groups);
		let visual = response_audit_top_visual_group_label(&groups);
		let nonvisual = response_audit_top_nonvisual_group_label(&groups);

		assert!(top.starts_with("physbone:control:category=other,visual=false,joints=12"));
		assert!(visual.starts_with("physbone:cloth:category=cloth,visual=true,joints=8,visibleJoints=6"));
		assert!(nonvisual.starts_with("physbone:control:category=other,visual=false,joints=12"));
	}

	#[test]
	fn response_audit_visual_response_evidence_counts_weighted_visibility_fields() {
		let modes = vec![
			serde_json::json!({
				"categories": [
					{
						"category": "control",
						"visual_target_group_count": 0,
						"visible_skinned_joint_count": 0,
						"visible_mesh_subtree_node_count": 0
					}
				]
			}),
			serde_json::json!({
				"categories": [
					{
						"category": "cloth",
						"visual_target_group_count": 0,
						"visible_skinned_joint_count": 2,
						"visible_mesh_subtree_node_count": 0
					},
					{
						"category": "accessory",
						"visual_target_group_count": 0,
						"visible_skinned_joint_count": 0,
						"visible_mesh_subtree_node_count": 3
					}
				]
			}),
		];

		assert_eq!(response_audit_visual_response_evidence_count(&modes), 5);
		assert!(!response_audit_visual_response_evidence_gate_failed(false, 7, 0));
		assert!(!response_audit_visual_response_evidence_gate_failed(true, 0, 0));
		assert!(!response_audit_visual_response_evidence_gate_failed(true, 7, 5));
		assert!(response_audit_visual_response_evidence_gate_failed(true, 7, 0));
	}

	#[test]
	fn importer_constraint_gates_are_opt_in() {
		assert!(!importer_constraint_gate_failed(false, 0));
		assert!(!importer_constraint_gate_failed(false, 4));
		assert!(!importer_constraint_gate_failed(true, 4));
		assert!(importer_constraint_gate_failed(true, 0));
		assert!(!importer_mesh_cloth_assist_candidate_gate_failed(false, 0));
		assert!(!importer_mesh_cloth_assist_candidate_gate_failed(false, 12));
		assert!(!importer_mesh_cloth_assist_candidate_gate_failed(true, 12));
		assert!(importer_mesh_cloth_assist_candidate_gate_failed(true, 0));
	}

	#[test]
	fn wardrobe_scoped_unphysics_audits_require_explicit_paths() {
		assert!(!wardrobe_set_requires_explicit_paths(None, &[], "test-audit"));
		assert!(!wardrobe_set_requires_explicit_paths(
			Some("StarMemoryBG"),
			&[PathBuf::from("target/tmp/usagi.unavatar")],
			"test-audit"
		));
		assert!(wardrobe_set_requires_explicit_paths(Some("StarMemoryBG"), &[], "test-audit"));
	}

	#[test]
	fn importer_audit_summarizes_top_mesh_cloth_assist_sample() {
		let samples = vec![
			serde_json::json!({
				"node_path": "Avatar/ClothA",
				"region": "all",
				"vertex_count": 10,
				"candidate_count": 3,
				"existing_dynamic_candidate_count": 2,
				"static_cloth_bridge_candidate_count": 1,
				"seed_candidate_count": 1,
				"suggested_assist_weight_sum": 0.25,
				"seeded_assist_weight_sum": 0.05
			}),
			serde_json::json!({
				"node_path": "Avatar/ClothB",
				"region": "front",
				"vertex_count": 8,
				"candidate_count": 5,
				"existing_dynamic_candidate_count": 2,
				"static_cloth_bridge_candidate_count": 4,
				"seed_candidate_count": 2,
				"suggested_assist_weight_sum": 0.5,
				"seeded_assist_weight_sum": 0.2
			}),
		];

		assert_eq!(sum_json_u64(&samples, "candidate_count"), 8);
		assert_eq!(sum_json_u64(&samples, "seed_candidate_count"), 3);
		let label = top_mesh_cloth_assist_sample_label(&samples);
		assert!(label.starts_with("Avatar/ClothB:region=front,vertices=8,candidates=5"));
		assert!(label.contains("bridge=4,seed=2"));
		assert!(label.contains("assist=0.500,seedAssist=0.200"));
	}

	#[test]
	fn importer_audit_top_mesh_cloth_assist_sample_uses_stable_tie_breakers() {
		let samples = vec![
			serde_json::json!({
				"node_path": "Avatar/ClothB",
				"region": "front",
				"candidate_count": 5,
				"seed_candidate_count": 2
			}),
			serde_json::json!({
				"node_path": "Avatar/ClothA",
				"region": "front",
				"candidate_count": 5,
				"seed_candidate_count": 2
			}),
		];

		let label = top_mesh_cloth_assist_sample_label(&samples);
		assert!(label.starts_with("Avatar/ClothA:region=front"));
	}

	#[test]
	fn motion_trace_short_recovery_expands_without_duplicates() {
		assert_eq!(motion_trace_recovery_frames_to_run("240", false), vec!["240".to_string()]);
		assert_eq!(
			motion_trace_recovery_frames_to_run("240", true),
			vec!["240".to_string(), "24".to_string(), "96".to_string()]
		);
		assert_eq!(
			motion_trace_recovery_frames_to_run("24", true),
			vec!["24".to_string(), "96".to_string()]
		);
		assert_eq!(
			motion_trace_recovery_frames_to_run("96", true),
			vec!["96".to_string(), "24".to_string()]
		);
	}

	#[test]
	fn motion_trace_top_categories_use_explicit_ordering() {
		let categories = vec![
			serde_json::json!({
				"category": "control",
				"visual_target_group_count": 0,
				"nonvisual_group_count": 2,
				"average_chain_rest_length": 0.1,
				"max_lag": 0.9,
				"max_lag_chain_ratio": 9.0,
				"average_lag": 0.2,
				"recovery_final_lag": 0.0,
				"initial_stable_offset": 0.1,
				"stable_offset": 0.0,
				"stable_offset_ratio": 0.0,
				"stable_offset_chain_ratio": 0.0,
				"recovery_state": "settled",
				"residual_motion": 0.0,
				"residual_motion_chain_ratio": 0.0
			}),
			serde_json::json!({
				"category": "cloth",
				"visual_target_group_count": 4,
				"nonvisual_group_count": 0,
				"average_chain_rest_length": 0.3,
				"max_lag": 0.4,
				"max_lag_chain_ratio": 1.3,
				"average_lag": 0.1,
				"recovery_final_lag": 0.02,
				"initial_stable_offset": 0.12,
				"stable_offset": 0.05,
				"stable_offset_ratio": 0.08,
				"stable_offset_chain_ratio": 0.03,
				"recovery_state": "settled_offset",
				"residual_motion": 0.0,
				"residual_motion_chain_ratio": 0.0
			}),
			serde_json::json!({
				"category": "hair",
				"visual_target_group_count": 2,
				"nonvisual_group_count": 0,
				"average_chain_rest_length": 0.2,
				"max_lag": 0.6,
				"max_lag_chain_ratio": 3.0,
				"average_lag": 0.2,
				"recovery_final_lag": 0.04,
				"initial_stable_offset": 0.18,
				"stable_offset": 0.02,
				"stable_offset_ratio": 0.11,
				"stable_offset_chain_ratio": 0.1,
				"recovery_state": "moving",
				"residual_motion": 0.01,
				"residual_motion_chain_ratio": 0.05
			}),
		];
		let groups = vec![
			serde_json::json!({
				"source_id": "physbone:control",
				"category": "control",
				"visual_target": false,
				"chain_rest_length": 0.1,
				"max_lag": 0.9,
				"max_lag_chain_ratio": 9.0,
				"initial_stable_offset": 0.2,
				"stable_offset": 0.3,
				"stable_offset_ratio": 0.0,
				"stable_offset_chain_ratio": 0.0,
				"recovery_state": "settled",
				"residual_motion": 0.0,
				"residual_motion_chain_ratio": 0.0
			}),
			serde_json::json!({
				"source_id": "physbone:cloth",
				"category": "cloth",
				"visual_target": true,
				"chain_rest_length": 0.3,
				"max_lag": 0.4,
				"max_lag_chain_ratio": 1.3,
				"initial_stable_offset": 0.12,
				"stable_offset": 0.01,
				"stable_offset_ratio": 0.08,
				"stable_offset_chain_ratio": 0.03,
				"recovery_state": "settled_offset",
				"residual_motion": 0.0,
				"residual_motion_chain_ratio": 0.0
			}),
			serde_json::json!({
				"source_id": "physbone:hair",
				"category": "hair",
				"visual_target": true,
				"chain_rest_length": 0.2,
				"max_lag": 0.6,
				"max_lag_chain_ratio": 3.0,
				"initial_stable_offset": 0.18,
				"stable_offset": 0.02,
				"stable_offset_ratio": 0.11,
				"stable_offset_chain_ratio": 0.1,
				"recovery_state": "moving",
				"residual_motion": 0.01,
				"residual_motion_chain_ratio": 0.05
			}),
		];

		let top = categories
			.iter()
			.max_by(|left, right| motion_trace_category_order(left, right))
			.map(motion_trace_category_summary_label)
			.unwrap();
		let label = motion_trace_top_visual_category_label(&categories, &groups);
		let nonvisual_label = motion_trace_top_nonvisual_category_label(&categories, &groups);

		assert!(top.starts_with("cloth:visualGroups=4,nonvisualGroups=0"));
		assert!(label.starts_with("hair:visualGroups=2,nonvisualGroups=0"));
		assert!(nonvisual_label.starts_with("control:visualGroups=0,nonvisualGroups=2"));
		assert!(label.contains("state=moving"));
	}

	#[test]
	fn vertex_probe_collision_projection_gate_is_opt_in() {
		assert!(!vertex_probe_collision_projection_gate_failed(false, 0));
		assert!(!vertex_probe_collision_projection_gate_failed(false, 12));
		assert!(!vertex_probe_collision_projection_gate_failed(true, 12));
		assert!(vertex_probe_collision_projection_gate_failed(true, 0));
		assert!(!vertex_probe_probe_collision_projection_gate_failed(false, 0));
		assert!(!vertex_probe_probe_collision_projection_gate_failed(false, 12));
		assert!(!vertex_probe_probe_collision_projection_gate_failed(true, 12));
		assert!(vertex_probe_probe_collision_projection_gate_failed(true, 0));
		assert!(!vertex_probe_collision_projection_source_gate_failed(false, 0));
		assert!(!vertex_probe_collision_projection_source_gate_failed(true, 1));
		assert!(vertex_probe_collision_projection_source_gate_failed(true, 0));
		assert!(!vertex_probe_collision_projection_path_gate_failed(false, 0));
		assert!(!vertex_probe_collision_projection_path_gate_failed(true, 3));
		assert!(vertex_probe_collision_projection_path_gate_failed(true, 0));
		assert!(!vertex_probe_collider_summary_gate_failed(false, 0));
		assert!(!vertex_probe_collider_summary_gate_failed(true, 2));
		assert!(vertex_probe_collider_summary_gate_failed(true, 0));
		assert!(!vertex_probe_probe_collider_candidate_gate_failed(false, &[]));
		assert!(!vertex_probe_probe_collider_candidate_gate_failed(
			true,
			&[serde_json::json!({"candidate_count": 1})]
		));
		assert!(vertex_probe_probe_collider_candidate_gate_failed(
			true,
			&[serde_json::json!({"candidate_count": 0})]
		));
		assert!(!vertex_probe_projecting_collider_summary_gate_failed(false, 0));
		assert!(!vertex_probe_projecting_collider_summary_gate_failed(true, 1));
		assert!(vertex_probe_projecting_collider_summary_gate_failed(true, 0));
	}

	#[test]
	fn vertex_probe_counts_only_projecting_collider_summaries() {
		let summaries = vec![
			serde_json::json!({
				"collider_path": "A",
				"collider_shape": "sphere",
				"inside_bounds": false,
				"candidate_count": 4,
				"penetrating_count": 2,
				"projection_count": 2,
				"source_count": 1,
				"min_margin": -0.01
			}),
			serde_json::json!({
				"collider_path": "B",
				"collider_shape": "capsule",
				"inside_bounds": true,
				"candidate_count": 8,
				"penetrating_count": 4,
				"projection_count": 5,
				"source_count": 2,
				"min_margin": -0.02
			}),
			serde_json::json!({"collider_path": "C"}),
		];

		assert_eq!(collider_summaries_with_projection_count(&summaries), 2);
		assert_eq!(collider_summaries_with_penetration_count(&summaries), 2);
		let top = top_projecting_collider_summary_label(&summaries);
		assert!(top.starts_with("B:shape=capsule,inside=true"));
		assert!(top.contains("projections=5"));
	}

	#[test]
	fn vertex_probe_top_projecting_collider_uses_stable_tie_breaker() {
		let summaries = vec![
			serde_json::json!({
				"collider_path": "B",
				"collider_shape": "sphere",
				"projection_count": 5
			}),
			serde_json::json!({
				"collider_path": "A",
				"collider_shape": "capsule",
				"projection_count": 5
			}),
		];

		let top = top_projecting_collider_summary_label(&summaries);
		assert!(top.starts_with("A:shape=capsule"));
	}

	#[test]
	fn vertex_probe_top_candidate_collider_reports_non_projecting_probe_candidates() {
		let summaries = vec![
			serde_json::json!({
				"collider_path": "Chest",
				"collider_shape": "capsule",
				"inside_bounds": false,
				"candidate_count": 57,
				"penetrating_count": 0,
				"projection_count": 0,
				"source_count": 19,
				"min_margin": 0.006
			}),
			serde_json::json!({
				"collider_path": "Spine",
				"collider_shape": "capsule",
				"inside_bounds": false,
				"candidate_count": 57,
				"penetrating_count": 0,
				"projection_count": 0,
				"source_count": 19,
				"min_margin": 0.008
			}),
			serde_json::json!({
				"collider_path": "Other",
				"candidate_count": 0,
				"min_margin": -1.0
			}),
		];

		assert_eq!(top_projecting_collider_summary_label(&summaries), "none");
		let top = top_candidate_collider_summary_label(&summaries);
		assert!(top.starts_with("Chest:shape=capsule,inside=false"));
		assert!(top.contains("candidates=57"));
		assert!(top.contains("projections=0"));
	}

	#[test]
	fn vertex_probe_mesh_cloth_assist_gate_requires_actual_changes() {
		assert!(!vertex_probe_mesh_cloth_assist_gate_failed(false, false, 0));
		assert!(!vertex_probe_mesh_cloth_assist_gate_failed(false, true, 0));
		assert!(!vertex_probe_mesh_cloth_assist_gate_failed(true, true, 19));
		assert!(vertex_probe_mesh_cloth_assist_gate_failed(true, false, 19));
		assert!(vertex_probe_mesh_cloth_assist_gate_failed(true, true, 0));
	}

	#[test]
	fn renderer_log_summary_parses_benchmark_and_texture_lines() {
		let mut summary = RendererLogSummary::default();
		summarize_renderer_log_text(
			"un-avatar-renderer: Vulkan pipeline cache load path=x bytes=6025281\n\
glTF import profile: file_read_bytes=779284856 file_read_ms=182\n\
glTF import profile: gltf_import_slice_ms=102\n\
glTF import profile: gltf_import_slice.parse_ms=30 buffers_ms=72 image_decode_ms=0 images=39/231 workers=8\n\
glTF import profile: pre_scene_import_ms=382\n\
glTF scene profile: read_meshes_ms=129\n\
un-avatar-renderer: glTF scene profile: read_meshes.stage_ms cache_clone=6 cache_take=0 positions=5 joints_weights=12 attributes=19 indices=9 morphs=96 defaults=0 cache_insert=42\n\
glTF import profile: scene_snapshot_ms=249\n\
un-avatar-renderer: gpu scene benchmark import path=model elapsed=1120.9ms\n\
un-avatar-renderer: gpu scene texture image=6 name=\"Body_b\" mime=\"image/png\" resident=true role=GenericColor: 72.5ms cube=0.0ms source=0.0ms rgba=0.0ms cache_lookup=0.0ms cache_read=25.9ms processed=0.0ms payload=0.0ms upload=46.5ms read_mb=85.3\n\
un-avatar-renderer: gpu scene texture prepare summary: total=1271.4ms images=231 cache_read=70.4ms upload=387.5ms processed_cache=39/0/0 compressed_cache=21/0/0\n\
un-avatar-renderer: gpu scene texture prepare roles: GenericColor=27/161 read=985.9MB/291.2ms upload=101.6ms cache_hits=15 compressed_hits=12 Data=20/39 read=1239.8MB/350.4ms upload=84.5ms cache_hits=20 compressed_hits=0\n\
un-avatar-renderer: gpu scene mesh prepare summary: total=121.9ms prepared=625\n\
un-avatar-renderer: frame bench frames=180 warmup=5 fps_avg=60.1 cpu_no_surface_avg=1.29ms gpu_avg=0.69ms\n\
un-avatar-renderer: frame bench detail motion=0.07/0.22ms dynamics=0.80/1.29ms globals=0.03/0.09ms surface=14.30/15.26ms draw_state=0.33/0.75ms scene_world=0.15/0.45ms skin_palette=0.17/0.42ms skin_write=0.16/0.40ms submit=0.78/1.44ms\n\
un-avatar-renderer: Vulkan pipeline cache store path=x bytes=6025281\n",
			&mut summary,
		);
		assert_eq!(summary.import_ms.as_deref(), Some("1120.9"));
		assert_eq!(summary.pre_scene_import_ms.as_deref(), Some("382"));
		assert_eq!(summary.file_read_ms.as_deref(), Some("182"));
		assert_eq!(summary.gltf_import_slice_ms.as_deref(), Some("102"));
		assert_eq!(summary.gltf_parse_ms.as_deref(), Some("30"));
		assert_eq!(summary.gltf_buffers_ms.as_deref(), Some("72"));
		assert_eq!(summary.gltf_image_decode_ms.as_deref(), Some("0"));
		assert_eq!(summary.scene_snapshot_ms.as_deref(), Some("249"));
		assert_eq!(summary.read_meshes_ms.as_deref(), Some("129"));
		assert_eq!(
			summary.read_meshes_stage.as_deref(),
			Some("cache_clone=6 cache_take=0 positions=5 joints_weights=12 attributes=19 indices=9 morphs=96 defaults=0 cache_insert=42")
		);
		assert_eq!(summary.texture_total_ms.as_deref(), Some("1271.4"));
		assert_eq!(summary.mesh_total_ms.as_deref(), Some("121.9"));
		assert_eq!(summary.cache_read_ms.as_deref(), Some("70.4"));
		assert_eq!(summary.upload_ms.as_deref(), Some("387.5"));
		assert_eq!(summary.processed_cache.as_deref(), Some("39/0/0"));
		assert_eq!(summary.compressed_cache.as_deref(), Some("21/0/0"));
		assert_eq!(summary.fps_avg.as_deref(), Some("60.1"));
		assert_eq!(summary.cpu_no_surface_ms.as_deref(), Some("1.29"));
		assert_eq!(summary.gpu_ms.as_deref(), Some("0.69"));
		assert_eq!(summary.frame_dynamics_ms.as_deref(), Some("0.80"));
		assert_eq!(summary.frame_draw_state_ms.as_deref(), Some("0.33"));
		assert_eq!(summary.frame_scene_world_ms.as_deref(), Some("0.15"));
		assert_eq!(summary.frame_skin_palette_ms.as_deref(), Some("0.17"));
		assert_eq!(summary.frame_skin_write_ms.as_deref(), Some("0.16"));
		assert_eq!(summary.frame_submit_ms.as_deref(), Some("0.78"));
		assert_eq!(summary.pipeline_load_mb.as_deref(), Some("5.7"));
		assert_eq!(summary.pipeline_store_mb.as_deref(), Some("5.7"));
		assert_eq!(
			summary.top_texture.as_deref(),
			Some("image=6 role=GenericColor name=Body_b total_ms=72.5 cache_read_ms=25.9 upload_ms=46.5 read_mb=85.3")
		);
		assert_eq!(
			summary.texture_roles.as_deref(),
			Some(
				"GenericColor=27/161 read=985.9MB/291.2ms upload=101.6ms cache_hits=15 compressed_hits=12 Data=20/39 read=1239.8MB/350.4ms upload=84.5ms cache_hits=20 compressed_hits=0"
			)
		);
	}

	#[test]
	fn renderer_log_summary_prefers_precise_model_import_profile() {
		let mut summary = RendererLogSummary::default();
		summarize_renderer_log_text(
			"un-avatar-renderer: scene cache prewarm import path=model elapsed=1018.3ms\n\
un-avatar-renderer: model import profile path=model step=import_gltf_path elapsed=978.5ms\n",
			&mut summary,
		);
		assert_eq!(summary.import_ms.as_deref(), Some("978.5"));
	}

	#[test]
	fn vcc_repo_index_updates_package_version_without_dropping_existing_versions() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-vcc-index-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).expect("temp dir");
		let index = dir.join("index.json");
		fs::write(
			&index,
			r#"{
  "name": "Old Name",
  "id": "old.id",
  "url": "https://old.invalid/index.json",
  "author": "old@example.invalid",
  "packages": {
    "network.usagi.un-avatar.unity-exporter": {
      "versions": {
        "2.0.0-beta-1": {
          "name": "network.usagi.un-avatar.unity-exporter",
          "version": "2.0.0-beta-1",
          "zipSHA256": "old-sha"
        }
      }
    }
  }
}
"#,
		)
		.expect("write existing index");
		let manifest = serde_json::json!({
			"name": "network.usagi.un-avatar.unity-exporter",
			"displayName": "U.N. Avatar Unity Exporter",
			"version": "2.0.0-beta-2",
			"url": "https://github.com/usagi/un-avatar/releases/download/2.0.0-beta-2/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip"
		});

		assert!(write_vcc_repo_index(&index, manifest, "2.0.0-beta-2", "new-sha"));

		let written: serde_json::Value =
			serde_json::from_str(&fs::read_to_string(&index).expect("read written index")).expect("parse written index");
		assert_eq!(written.get("name").and_then(serde_json::Value::as_str), Some(VCC_REPOSITORY_NAME));
		assert_eq!(
			written.get("author").and_then(serde_json::Value::as_str),
			Some(VCC_REPOSITORY_AUTHOR)
		);
		assert_eq!(
			written.get("id").and_then(serde_json::Value::as_str),
			Some("network.usagi.un-avatar")
		);
		let versions = written
			.get("packages")
			.and_then(|packages| packages.get(UNITY_EXPORTER_PACKAGE_ID))
			.and_then(|package| package.get("versions"))
			.and_then(serde_json::Value::as_object)
			.expect("versions object");
		assert_eq!(
			versions
				.get("2.0.0-beta-1")
				.and_then(|version| version.get("zipSHA256"))
				.and_then(serde_json::Value::as_str),
			Some("old-sha")
		);
		assert_eq!(
			versions
				.get("2.0.0-beta-2")
				.and_then(|version| version.get("zipSHA256"))
				.and_then(serde_json::Value::as_str),
			Some("new-sha")
		);

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn release_checksum_file_uses_zip_sha256_sidecar_name() {
		let path = Path::new("release-packages").join("un-avatar-2.0.0-beta-2.zip");
		let expected = Path::new("release-packages").join("un-avatar-2.0.0-beta-2.zip.sha256.txt");
		assert_eq!(checksum_file_path(&path), Some(expected));
		assert_eq!(
			checksum_file_text(" ABCD ", "un-avatar-2.0.0-beta-2.zip"),
			"ABCD  un-avatar-2.0.0-beta-2.zip\n"
		);
		assert_eq!(
			parse_checksum_file("ABCD  un-avatar-2.0.0-beta-2.zip\n"),
			Some(("ABCD", "un-avatar-2.0.0-beta-2.zip"))
		);
		assert!(parse_checksum_file("ABCD artifact.zip extra").is_none());
	}

	#[test]
	fn file_sha256_hashes_file_contents_without_platform_tools() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-sha256-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).expect("temp dir");
		let path = dir.join("sample.bin");
		fs::write(&path, b"abc").expect("write sample");

		assert_eq!(
			file_sha256(&path).as_deref(),
			Some("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")
		);

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn package_render_smoke_options_parse_manifest_and_wardrobe() {
		let repo = Path::new("C:/repo");
		let opts = parse_package_render_smoke_options(
			repo,
			[
				"--manifest".to_string(),
				"target/tmp/mizuki.toml".to_string(),
				"--wardrobe-set".to_string(),
				"field_drape".to_string(),
			]
			.into_iter(),
		)
		.expect("parse package render smoke options");
		assert_eq!(opts.manifest.as_deref(), Some(Path::new("C:/repo/target/tmp/mizuki.toml")));
		assert_eq!(opts.wardrobe_set.as_deref(), Some("field_drape"));
	}

	#[test]
	fn package_render_smoke_options_require_manifest_for_wardrobe() {
		assert!(parse_package_render_smoke_options(
			Path::new("."),
			["--wardrobe-set".to_string(), "field_drape".to_string()].into_iter()
		)
		.is_err());
	}

	#[test]
	fn release_zip_verifier_requires_core_and_spout_entries() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-release-zip-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let staging_root = dir.join("staging");
		let package_name = "un-avatar-test";
		let package = staging_root.join(package_name);
		for entry in required_release_zip_entries(package_name, true) {
			if entry.ends_with('/') {
				fs::create_dir_all(staging_root.join(entry)).expect("create dir entry");
			} else {
				let path = staging_root.join(entry);
				fs::create_dir_all(path.parent().expect("entry parent")).expect("create entry parent");
				fs::write(path, b"placeholder").expect("write entry");
			}
		}
		assert!(package.is_dir());
		let zip_path = dir.join("package.zip");

		assert!(create_release_zip(&staging_root, package_name, &zip_path));
		assert!(verify_release_zip_entries(&zip_path, package_name, true));
		assert!(verify_release_zip_entries(&zip_path, package_name, false));
		assert!(verify_release_zip_clean_unpack(&zip_path, package_name, true));

		let no_spout_root = dir.join("staging-no-spout");
		let no_spout_package = no_spout_root.join(package_name);
		copy_dir_contents(&package, &no_spout_package);
		for entry in ["Spout.dll", "LICENSES/Spout2-BSD-2-Clause.txt", "LICENSES/spout2-build-info.txt"] {
			let _ = fs::remove_file(no_spout_package.join(entry));
		}
		let no_spout_zip_path = dir.join("package-no-spout.zip");
		assert!(create_release_zip(&no_spout_root, package_name, &no_spout_zip_path));
		assert_eq!(
			release_zip_missing_entries(&no_spout_zip_path, package_name, true).expect("missing spout entries"),
			vec![
				format!("{package_name}/Spout.dll"),
				format!("{package_name}/LICENSES/Spout2-BSD-2-Clause.txt"),
				format!("{package_name}/LICENSES/spout2-build-info.txt"),
			]
		);
		assert!(verify_release_zip_entries(&no_spout_zip_path, package_name, false));
		assert!(!verify_release_zip_clean_unpack(&no_spout_zip_path, package_name, true));
		assert!(verify_release_zip_clean_unpack(&no_spout_zip_path, package_name, false));

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn release_zip_source_doc_verifier_detects_stale_packaged_readme_and_notices() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-release-docs-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let repo = dir.join("repo");
		let staging_root = dir.join("staging");
		let package_name = "un-avatar-test";
		let package = staging_root.join(package_name);
		fs::create_dir_all(repo.join("docs")).expect("repo docs dir");
		fs::write(repo.join("README.md"), b"fresh readme").expect("repo readme");
		fs::write(repo.join("LICENSE"), b"fresh license").expect("repo license");
		for relative in RELEASE_PACKAGE_DOCS {
			let path = repo.join(Path::new(relative));
			fs::create_dir_all(path.parent().expect("repo doc parent")).expect("repo doc parent");
			fs::write(&path, format!("fresh {relative}")).expect("repo linked doc");
		}
		for entry in required_release_zip_entries(package_name, false) {
			if entry.ends_with('/') {
				fs::create_dir_all(staging_root.join(entry)).expect("create dir entry");
			} else {
				let path = staging_root.join(entry);
				fs::create_dir_all(path.parent().expect("entry parent")).expect("create entry parent");
				fs::write(path, b"placeholder").expect("write placeholder entry");
			}
		}
		fs::write(package.join("README.md"), b"fresh readme").expect("package readme");
		fs::write(package.join("LICENSE"), b"fresh license").expect("package license");
		let notices = fs::read(repo.join("docs").join("third-party-licenses.md")).expect("repo notices");
		fs::write(package.join("THIRD_PARTY_NOTICES.md"), &notices).expect("package notices");
		fs::write(package.join("LICENSES").join("third-party-licenses.md"), &notices).expect("package license notices");
		for relative in RELEASE_PACKAGE_DOCS {
			let source = fs::read(repo.join(Path::new(relative))).expect("repo linked doc bytes");
			let target = package.join(Path::new(relative));
			fs::create_dir_all(target.parent().expect("package doc parent")).expect("package doc parent");
			fs::write(target, source).expect("package linked doc");
		}
		let zip_path = dir.join("package.zip");
		assert!(create_release_zip(&staging_root, package_name, &zip_path));
		assert!(verify_release_zip_source_docs(&zip_path, package_name, &repo));

		fs::write(package.join("README.md"), b"stale readme").expect("stale package readme");
		let stale_zip_path = dir.join("package-stale.zip");
		assert!(create_release_zip(&staging_root, package_name, &stale_zip_path));
		assert!(!verify_release_zip_source_docs(&stale_zip_path, package_name, &repo));

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn vcc_package_zip_verifier_requires_core_unity_exporter_entries() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-vcc-zip-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let package = dir.join("package");
		for entry in required_vcc_package_zip_entries() {
			let path = package.join(entry);
			fs::create_dir_all(path.parent().expect("entry parent")).expect("create entry parent");
			fs::write(path, b"placeholder").expect("write entry");
		}
		let zip_path = dir.join("vcc.zip");

		assert!(create_package_contents_zip(&package, &zip_path));
		assert!(verify_vcc_package_zip_entries(&zip_path));

		let missing_package = dir.join("package-missing");
		copy_dir_contents(&package, &missing_package);
		fs::remove_file(missing_package.join("Editor/Plugins/x86_64/unavatar_fpng.dll")).expect("remove fpng dll");
		let missing_zip_path = dir.join("vcc-missing.zip");
		assert!(create_package_contents_zip(&missing_package, &missing_zip_path));
		assert_eq!(
			vcc_package_zip_missing_entries(&missing_zip_path).expect("missing vcc entries"),
			vec!["Editor/Plugins/x86_64/unavatar_fpng.dll".to_string()]
		);

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn vcc_package_zip_staging_verifier_detects_stale_required_entries() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-vcc-staging-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let package = dir.join("package");
		for entry in required_vcc_package_zip_entries() {
			let path = package.join(entry);
			fs::create_dir_all(path.parent().expect("entry parent")).expect("create entry parent");
			fs::write(path, format!("fresh {entry}")).expect("write entry");
		}
		let zip_path = dir.join("vcc.zip");
		assert!(create_package_contents_zip(&package, &zip_path));
		assert!(verify_vcc_package_zip_staging_files(&zip_path, &package));

		let plugin = package.join("Editor/Plugins/x86_64/unavatar_fpng.dll");
		fs::write(plugin, b"newer dll").expect("update staging dll");
		assert!(!verify_vcc_package_zip_staging_files(&zip_path, &package));

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn vcc_package_zip_manifest_verifier_checks_name_version_and_url() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-vcc-manifest-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let package = dir.join("package");
		for entry in required_vcc_package_zip_entries() {
			let path = package.join(entry);
			fs::create_dir_all(path.parent().expect("entry parent")).expect("create entry parent");
			fs::write(path, b"placeholder").expect("write placeholder");
		}
		fs::write(
			package.join("package.json"),
			serde_json::json!({
				"name": UNITY_EXPORTER_PACKAGE_ID,
				"version": "2.0.0-beta-2",
				"url": "https://example.invalid/releases/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip"
			})
			.to_string(),
		)
		.expect("write manifest");
		let zip_path = dir.join("vcc.zip");
		assert!(create_package_contents_zip(&package, &zip_path));
		assert!(verify_vcc_package_zip_manifest(&zip_path, "2.0.0-beta-2"));

		fs::write(
			package.join("package.json"),
			serde_json::json!({
				"name": UNITY_EXPORTER_PACKAGE_ID,
				"version": "2.0.0-beta-3",
				"url": "https://example.invalid/releases/network.usagi.un-avatar.unity-exporter-2.0.0-beta-3.zip"
			})
			.to_string(),
		)
		.expect("write mismatched manifest");
		let mismatched_zip_path = dir.join("vcc-mismatched.zip");
		assert!(create_package_contents_zip(&package, &mismatched_zip_path));
		assert!(!verify_vcc_package_zip_manifest(&mismatched_zip_path, "2.0.0-beta-2"));

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn vcc_package_manifest_url_must_match_repo_index_url() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-vcc-index-url-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		let package = dir.join("package");
		for entry in required_vcc_package_zip_entries() {
			let path = package.join(entry);
			fs::create_dir_all(path.parent().expect("entry parent")).expect("create entry parent");
			fs::write(path, b"placeholder").expect("write placeholder");
		}
		let matching_url = "https://example.invalid/releases/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip";
		fs::write(
			package.join("package.json"),
			serde_json::json!({
				"name": UNITY_EXPORTER_PACKAGE_ID,
				"version": "2.0.0-beta-2",
				"url": matching_url
			})
			.to_string(),
		)
		.expect("write manifest");
		let zip_path = dir.join("vcc.zip");
		assert!(create_package_contents_zip(&package, &zip_path));
		let index = dir.join("index.json");
		fs::write(
			&index,
			serde_json::json!({
				"packages": {
					UNITY_EXPORTER_PACKAGE_ID: {
						"versions": {
							"2.0.0-beta-2": {
								"url": matching_url
							}
						}
					}
				}
			})
			.to_string(),
		)
		.expect("write index");
		assert!(verify_vcc_package_manifest_matches_index(&index, &zip_path, "2.0.0-beta-2"));

		fs::write(
			&index,
			serde_json::json!({
				"packages": {
					UNITY_EXPORTER_PACKAGE_ID: {
						"versions": {
							"2.0.0-beta-2": {
								"url": "https://example.invalid/releases/other.zip"
							}
						}
					}
				}
			})
			.to_string(),
		)
		.expect("write mismatched index");
		assert!(!verify_vcc_package_manifest_matches_index(&index, &zip_path, "2.0.0-beta-2"));

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn release_audit_reads_vcc_index_hash_for_version() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-release-audit-vcc-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).expect("temp dir");
		let index = dir.join("index.json");
		fs::write(
			&index,
			serde_json::json!({
				"packages": {
					UNITY_EXPORTER_PACKAGE_ID: {
						"versions": {
							"2.0.0-beta-2": {
								"name": UNITY_EXPORTER_PACKAGE_ID,
								"version": "2.0.0-beta-2",
								"url": "https://example.invalid/releases/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip",
								"zipSHA256": "abc123"
							}
						}
					}
				}
			})
			.to_string(),
		)
		.expect("write index");

		assert_eq!(release_audit_vcc_index_hash(&index, "2.0.0-beta-2").as_deref(), Ok("abc123"));
		assert!(release_audit_vcc_index_hash(&index, "2.0.0-beta-3").is_err());
		fs::write(
			&index,
			serde_json::json!({
				"packages": {
					UNITY_EXPORTER_PACKAGE_ID: {
						"versions": {
							"2.0.0-beta-2": {
								"name": "network.usagi.un-avatar.other-package",
								"version": "2.0.0-beta-2",
								"url": "https://example.invalid/releases/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip",
								"zipSHA256": "abc123"
							}
						}
					}
				}
			})
			.to_string(),
		)
		.expect("write mismatched index");
		assert!(release_audit_vcc_index_hash(&index, "2.0.0-beta-2").is_err());
		fs::write(
			&index,
			serde_json::json!({
				"packages": {
					UNITY_EXPORTER_PACKAGE_ID: {
						"versions": {
							"2.0.0-beta-2": {
								"name": UNITY_EXPORTER_PACKAGE_ID,
								"version": "2.0.0-beta-2",
								"url": "https://example.invalid/releases/network.usagi.un-avatar.unity-exporter-2.0.0-beta-1.zip",
								"zipSHA256": "abc123"
							}
						}
					}
				}
			})
			.to_string(),
		)
		.expect("write mismatched URL index");
		assert!(release_audit_vcc_index_hash(&index, "2.0.0-beta-2").is_err());

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn release_audit_checks_release_notes_hashes_when_present() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-release-notes-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).expect("temp dir");
		let notes = dir.join("notes.md");
		fs::write(&notes, "- zip SHA-256: `portable-sha`\n- VCC zip SHA-256: `vcc-sha`\n").expect("write release notes");

		assert_eq!(
			release_notes_hash(&fs::read_to_string(&notes).unwrap(), "zip SHA-256").as_deref(),
			Some("portable-sha")
		);
		assert!(verify_release_notes_hashes(&notes, "portable-sha", "vcc-sha"));
		assert!(!verify_release_notes_hashes(&notes, "other-portable-sha", "vcc-sha"));
		assert!(!verify_release_notes_hashes(&notes, "portable-sha", "other-vcc-sha"));
		assert!(verify_release_notes_hashes(
			&dir.join("missing-notes.md"),
			"portable-sha",
			"vcc-sha"
		));

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn release_audit_checks_release_notes_required_public_text_when_present() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-release-notes-text-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).expect("temp dir");
		let notes = dir.join("notes.md");
		fs::write(
			&notes,
			"Portable Windows zip is the v2 distribution source of truth.\n\
			 Installer, auto-update, and Authenticode signing are outside v2.\n\
			 - zip SHA-256: `portable-sha`\n\
			 - VCC zip SHA-256: `vcc-sha`\n\
			 ## Known Limitations\n\
			 State the unsupported v2 areas explicitly.\n",
		)
		.expect("write release notes");

		assert!(verify_release_notes_required_text(&notes));
		fs::write(&notes, "Portable Windows zip is the v2 distribution source of truth.\n").expect("write incomplete release notes");
		assert!(!verify_release_notes_required_text(&notes));
		assert!(verify_release_notes_required_text(&dir.join("missing-notes.md")));

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn release_audit_checks_manual_release_checklist_candidate_build_when_present() {
		let dir = env::temp_dir().join(format!("un-avatar-xtask-manual-checklist-{}", std::process::id()));
		let _ = fs::remove_dir_all(&dir);
		fs::create_dir_all(&dir).expect("temp dir");
		let checklist = dir.join("checklist.md");
		fs::write(
			&checklist,
			"- Version: `2.0.0-beta-2`\n\
			 - Portable zip: `release-packages/un-avatar-2.0.0-beta-2.zip`\n\
			 - Portable zip SHA-256: `portable-sha`\n\
			 - VCC package zip: `target/unity/vcc/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip`\n\
			 - VCC package SHA-256: `vcc-sha`\n\
			 - `cargo xtask ci` result: passed\n\
			 - `cargo xtask release-audit --version <version>` result: passed for `2.0.0-beta-2`\n\
			 - `release-audit` confirms release notes hashes: yes\n\
			 - `cargo xtask package-render-smoke` result: passed\n\
			 - `package-render-smoke` representative wardrobe `field_drape`: passed; missing counts `0`, scoped missing groups `[]`\n\
			 - `package-render-smoke` representative wardrobe `noble1`: passed; missing counts `0`, scoped missing groups `[]`\n",
		)
		.expect("write checklist");

		assert!(verify_manual_release_checklist_candidate(
			&checklist,
			"2.0.0-beta-2",
			"release-packages/un-avatar-2.0.0-beta-2.zip",
			"PORTABLE-SHA",
			"target/unity/vcc/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip",
			"VCC-SHA",
		));
		assert!(!verify_manual_release_checklist_candidate(
			&checklist,
			"2.0.0-beta-3",
			"release-packages/un-avatar-2.0.0-beta-2.zip",
			"portable-sha",
			"target/unity/vcc/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip",
			"vcc-sha",
		));
		assert!(verify_manual_release_checklist_candidate(
			&dir.join("missing-checklist.md"),
			"2.0.0-beta-2",
			"release-packages/un-avatar-2.0.0-beta-2.zip",
			"portable-sha",
			"target/unity/vcc/network.usagi.un-avatar.unity-exporter-2.0.0-beta-2.zip",
			"vcc-sha",
		));

		let _ = fs::remove_dir_all(&dir);
	}

	#[test]
	fn release_audit_counts_open_manual_evidence_items() {
		let checklist = r#"
## Candidate Build

Evidence:

- Automated line: filled
- Empty field:
- Another empty field:

## Output Modes

Evidence:

- OBS / Spout receiver:
- Runtime status / screenshot: captured locally
"#;
		assert_eq!(
			manual_release_checklist_open_evidence_items(checklist),
			vec![
				"Candidate Build / Empty field".to_string(),
				"Candidate Build / Another empty field".to_string(),
				"Output Modes / OBS / Spout receiver".to_string(),
			]
		);
		assert_eq!(
			manual_release_checklist_open_evidence_items(
				r#"
## Release Text

- This normal checklist bullet:

Evidence:

- Screenshot / notes: done
"#
			),
			Vec::<String>::new()
		);
	}
}
