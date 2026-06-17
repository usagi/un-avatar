use std::{collections::BTreeMap, path::Path, sync::Arc, time::Instant};

use un_avatar_core::{
	ImportReport, ReportSeverity, UnaDocument, UnaRuntimeAction, UnaRuntimeActionEffect, UnaRuntimeActionSet, UnaRuntimeActionTrigger,
};
use un_avatar_io::{AvatarImporter, ImportContext, ImportInput, ImportOptions};
use un_avatar_io_gltf::{apply_unavatar_wardrobe_set, GltfImporter, WardrobeApplyReport};
use un_avatar_io_vrm::{
	gltf_root_json_from_bytes, gltf_root_json_from_path, has_vrm_extension, import_vrm_bytes, import_vrm_bytes_profiled,
};

pub(crate) fn normalize_wardrobe_set_id(wardrobe_set: Option<&str>) -> Option<&str> {
	wardrobe_set.map(str::trim).filter(|set_id| !set_id.is_empty())
}

pub(crate) fn require_wardrobe_set_id(wardrobe_set: &str) -> Result<&str, String> {
	normalize_wardrobe_set_id(Some(wardrobe_set)).ok_or_else(|| "wardrobe set id required".to_string())
}

pub(crate) fn base_wardrobe_set_id(document: &UnaDocument) -> Option<String> {
	let wardrobe = document.unavatar.as_ref()?.source.get("wardrobe")?.as_object()?;
	let explicit_base = wardrobe.get("baseSet").and_then(serde_json::Value::as_str).map(str::trim);
	let sets = wardrobe.get("sets").and_then(serde_json::Value::as_array)?;
	if let Some(base_id) = explicit_base {
		if sets
			.iter()
			.any(|set| set.get("id").and_then(serde_json::Value::as_str).map(str::trim) == Some(base_id))
		{
			return Some(base_id.to_string());
		}
	}
	sets.iter()
		.find(|set| {
			set.get("default").and_then(serde_json::Value::as_bool).unwrap_or(false)
				|| set.get("id").and_then(serde_json::Value::as_str).map(str::trim) == Some("")
		})
		.and_then(|set| set.get("id").and_then(serde_json::Value::as_str))
		.map(str::to_string)
}

fn wardrobe_apply_report_summary(set_id: &str, report: &WardrobeApplyReport) -> String {
	format!(
		"un-avatar-renderer: .unavatar wardrobe set `{set_id}` applied: visibility_applied={} visibility_missing={} blendshape_applied={} blendshape_missing={} dynamics_applied={} dynamics_missing={} material_applied={} material_missing={} material_slot_applied={} material_slot_missing={} active_asset_groups={:?} scoped_active_groups={} scoped_missing_groups={:?} scoped_resident=mesh:{} material:{} image:{} dynamics:{}",
		report.visibility_applied,
		report.visibility_missing,
		report.blendshape_applied,
		report.blendshape_missing,
		report.dynamics_applied,
		report.dynamics_missing,
		report.material_applied,
		report.material_missing,
		report.material_slot_applied,
		report.material_slot_missing,
		report.active_asset_groups,
		report.scoped_active_asset_group_count,
		report.scoped_missing_active_asset_groups,
		report.scoped_resident_mesh_primitive_count,
		report.scoped_resident_material_count,
		report.scoped_resident_image_count,
		report.scoped_resident_dynamics_count
	)
}

fn log_wardrobe_apply_report(set_id: &str, report: &WardrobeApplyReport) {
	eprintln!("{}", wardrobe_apply_report_summary(set_id, report));
	if !report.missing_visibility_paths.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing visibility paths: {:?}",
			report.missing_visibility_paths
		);
	}
	if !report.missing_blendshapes.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing blendshapes: {:?}",
			report.missing_blendshapes
		);
	}
	if !report.missing_dynamics_ids.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing dynamics ids: {:?}",
			report.missing_dynamics_ids
		);
	}
	if !report.missing_materials.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing material targets: {:?}",
			report.missing_materials
		);
	}
	if !report.missing_material_slots.is_empty() {
		eprintln!(
			"un-avatar-renderer: .unavatar wardrobe set `{set_id}` missing material slot targets: {:?}",
			report.missing_material_slots
		);
	}
}

fn import_report_warning_lines(report: &ImportReport, sample_limit: usize) -> Vec<String> {
	let mut lines = Vec::new();
	for diagnostic in &report.diagnostics {
		if diagnostic.severity == ReportSeverity::Warning {
			lines.push(format!("import warning: {}", diagnostic.text));
		}
	}
	for lost in &report.lost_features {
		let detail = lost.detail.as_deref().unwrap_or("");
		if detail.is_empty() {
			lines.push(format!("import lost feature: {}", lost.feature));
		} else {
			lines.push(format!("import lost feature: {} ({detail})", lost.feature));
		}
	}
	for approximation in &report.approximations {
		let detail = approximation.detail.as_deref().unwrap_or("");
		if detail.is_empty() {
			lines.push(format!("import approximation: {}", approximation.feature));
		} else {
			lines.push(format!("import approximation: {} ({detail})", approximation.feature));
		}
	}
	lines.truncate(sample_limit);
	lines
}

fn log_import_report_warnings(report: &ImportReport) {
	let total = report
		.diagnostics
		.iter()
		.filter(|diagnostic| diagnostic.severity == ReportSeverity::Warning)
		.count()
		+ report.lost_features.len()
		+ report.approximations.len();
	if total == 0 {
		return;
	}
	eprintln!("un-avatar-renderer: model import reported {total} warning/approximation/lost-feature item(s); showing up to 8");
	for line in import_report_warning_lines(report, 8) {
		eprintln!("un-avatar-renderer: {line}");
	}
}

fn log_import_report_profile(report: &ImportReport) {
	for message in &report.messages {
		if message.starts_with("glTF import profile:")
			|| message.starts_with("glTF scene profile:")
			|| message.starts_with(".unavatar textureAssets:")
		{
			eprintln!("un-avatar-renderer: {message}");
		}
	}
}

pub(crate) fn apply_required_wardrobe_set(document: &mut UnaDocument, wardrobe_set: &str) -> Result<WardrobeApplyReport, String> {
	let resolved_base;
	let set_id = if let Some(set_id) = normalize_wardrobe_set_id(Some(wardrobe_set)) {
		set_id
	} else {
		resolved_base = base_wardrobe_set_id(document).ok_or_else(|| ".unavatar wardrobe base set not found".to_string())?;
		resolved_base.as_str()
	};
	let report =
		apply_unavatar_wardrobe_set(document, set_id).map_err(|e| format!(".unavatar wardrobe set `{set_id}` not applied: {e}"))?;
	log_wardrobe_apply_report(set_id, &report);
	Ok(report)
}

pub(crate) fn apply_requested_wardrobe_set(document: &mut UnaDocument, wardrobe_set: Option<&str>) -> Option<WardrobeApplyReport> {
	let set_id = normalize_wardrobe_set_id(wardrobe_set)?;
	match apply_required_wardrobe_set(document, set_id) {
		Ok(report) => Some(report),
		Err(e) => {
			eprintln!("un-avatar-renderer: {e}");
			None
		}
	}
}

pub(crate) fn load_document(
	path: &Path,
	wardrobe_set: Option<&str>,
	enabled_animator_action_ids: &[String],
	animator_action_values: &std::collections::BTreeMap<String, f32>,
	contact_parameter_emission: bool,
	defer_initial_image_decode: bool,
) -> Result<Arc<UnaDocument>, String> {
	load_document_inner(
		path,
		wardrobe_set,
		enabled_animator_action_ids,
		animator_action_values,
		contact_parameter_emission,
		defer_initial_image_decode,
		false,
	)
}

pub(crate) fn load_document_profiled(
	path: &Path,
	wardrobe_set: Option<&str>,
	enabled_animator_action_ids: &[String],
	animator_action_values: &std::collections::BTreeMap<String, f32>,
	contact_parameter_emission: bool,
	defer_initial_image_decode: bool,
) -> Result<Arc<UnaDocument>, String> {
	load_document_inner(
		path,
		wardrobe_set,
		enabled_animator_action_ids,
		animator_action_values,
		contact_parameter_emission,
		defer_initial_image_decode,
		true,
	)
}

fn log_import_profile_step(path: &Path, step: &str, started: Instant) {
	eprintln!(
		"un-avatar-renderer: model import profile path={} step={step} elapsed={:.1}ms",
		path.display(),
		started.elapsed().as_secs_f64() * 1000.0
	);
}

fn load_document_inner(
	path: &Path,
	wardrobe_set: Option<&str>,
	enabled_animator_action_ids: &[String],
	animator_action_values: &std::collections::BTreeMap<String, f32>,
	contact_parameter_emission: bool,
	defer_initial_image_decode: bool,
	profile: bool,
) -> Result<Arc<UnaDocument>, String> {
	let mut ctx = ImportContext {
		asset_root: path.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
		temp_dir: std::env::temp_dir(),
		initial_wardrobe_set: normalize_wardrobe_set_id(wardrobe_set).map(str::to_string),
		enabled_animator_action_ids: enabled_animator_action_ids.to_vec(),
		animator_action_values: animator_action_values.clone(),
		defer_initial_image_decode,
		profile,
	};
	let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
	let result = if ext.eq_ignore_ascii_case("vrm") {
		let step_started = Instant::now();
		let bytes = match std::fs::read(path) {
			Ok(bytes) => Arc::<[u8]>::from(bytes),
			Err(e) => return Err(format!("model read: {e}")),
		};
		if profile {
			log_import_profile_step(path, "read_bytes", step_started);
		}
		let step_started = Instant::now();
		let root = if bytes.len() <= 128 * 1024 * 1024 {
			gltf_root_json_from_bytes(bytes.as_ref()).ok()
		} else {
			None
		};
		if profile {
			log_import_profile_step(path, "parse_root_json", step_started);
		}
		let step_started = Instant::now();
		let result = if profile {
			import_vrm_bytes_profiled(Some(path), bytes.as_ref(), root)
		} else {
			import_vrm_bytes(Some(path), bytes.as_ref(), root)
		};
		if profile {
			log_import_profile_step(path, "import_vrm_bytes", step_started);
		}
		result
	} else if ext.eq_ignore_ascii_case("glb") {
		let step_started = Instant::now();
		let root = gltf_root_json_from_path(path).ok();
		if profile {
			log_import_profile_step(path, "parse_glb_root_json", step_started);
		}
		if root.as_ref().is_some_and(has_vrm_extension) {
			let step_started = Instant::now();
			let bytes = match std::fs::read(path) {
				Ok(bytes) => Arc::<[u8]>::from(bytes),
				Err(e) => return Err(format!("model read: {e}")),
			};
			if profile {
				log_import_profile_step(path, "read_vrm_glb_bytes", step_started);
			}
			let step_started = Instant::now();
			let result = if profile {
				import_vrm_bytes_profiled(Some(path), bytes.as_ref(), root)
			} else {
				import_vrm_bytes(Some(path), bytes.as_ref(), root)
			};
			if profile {
				log_import_profile_step(path, "import_vrm_bytes", step_started);
			}
			result
		} else {
			let step_started = Instant::now();
			let result = GltfImporter.import(&mut ctx, ImportInput::Path(path.to_path_buf()), ImportOptions);
			if profile {
				log_import_profile_step(path, "import_gltf_path", step_started);
			}
			result
		}
	} else if ext.eq_ignore_ascii_case("unavatar") {
		let step_started = Instant::now();
		let result = GltfImporter.import(&mut ctx, ImportInput::Path(path.to_path_buf()), ImportOptions);
		if profile {
			log_import_profile_step(path, "import_gltf_path", step_started);
		}
		result
	} else {
		let step_started = Instant::now();
		let result = GltfImporter.import(&mut ctx, ImportInput::Path(path.to_path_buf()), ImportOptions);
		if profile {
			log_import_profile_step(path, "import_gltf_path", step_started);
		}
		result
	};
	match result {
		Ok(res) => {
			let step_started = Instant::now();
			log_import_report_warnings(&res.report);
			if profile {
				log_import_report_profile(&res.report);
			}
			if profile {
				log_import_profile_step(path, "report_warnings", step_started);
			}
			let step_started = Instant::now();
			let mut document = res.document;
			add_enabled_expression_runtime_actions(&mut document, enabled_animator_action_ids, animator_action_values);
			apply_requested_wardrobe_set(&mut document, wardrobe_set);
			if profile {
				log_import_profile_step(path, "apply_wardrobe_set", step_started);
			}
			let step_started = Instant::now();
			if contact_parameter_emission {
				document.enable_contact_parameter_emission_runtime_override();
			}
			if profile {
				log_import_profile_step(path, "contact_parameter_emission", step_started);
			}
			Ok(Arc::new(document))
		}
		Err(e) => Err(format!("model import: {e}")),
	}
}

pub(crate) fn add_enabled_expression_runtime_actions(
	document: &mut UnaDocument,
	enabled_animator_action_ids: &[String],
	animator_action_values: &BTreeMap<String, f32>,
) {
	let Some(catalog) = document.expression_catalog.as_ref() else {
		return;
	};
	let enabled = enabled_animator_action_ids
		.iter()
		.map(|id| id.trim())
		.filter(|id| id.starts_with("expression:"))
		.collect::<std::collections::BTreeSet<_>>();
	if enabled.is_empty() {
		return;
	}
	let mut actions = document.runtime_actions.take().unwrap_or_default().actions;
	for preset in &catalog.presets {
		let id = format!("expression:{}", stable_identifier(&preset.name));
		if !enabled.contains(id.as_str()) {
			continue;
		}
		let weight = animator_action_values.get(&id).copied().unwrap_or(1.0).clamp(0.0, 1.0);
		if let Some(action) = actions.iter_mut().find(|action| action.id == id) {
			for effect in &mut action.effects {
				if let UnaRuntimeActionEffect::ExpressionWeight { name, weight: existing } = effect {
					if name == &preset.name {
						*existing = weight;
					}
				}
			}
			continue;
		}
		actions.push(UnaRuntimeAction {
			id: id.clone(),
			label: format!("Expression / {}", preset.name),
			triggers: vec![
				UnaRuntimeActionTrigger::SupervisorCommand { command: id },
				UnaRuntimeActionTrigger::ExpressionMenu {
					path: format!("Expressions/{}", preset.name),
				},
			],
			conditions: Vec::new(),
			effects: vec![UnaRuntimeActionEffect::ExpressionWeight {
				name: preset.name.clone(),
				weight,
			}],
		});
	}
	if !actions.is_empty() {
		document.runtime_actions = Some(UnaRuntimeActionSet { actions });
	}
}

fn stable_identifier(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	for ch in value.chars() {
		if ch.is_ascii_alphanumeric() {
			out.push(ch.to_ascii_lowercase());
		} else if !out.ends_with('_') {
			out.push('_');
		}
	}
	out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn normalize_wardrobe_set_id_trims_empty_values() {
		assert_eq!(normalize_wardrobe_set_id(None), None);
		assert_eq!(normalize_wardrobe_set_id(Some("")), None);
		assert_eq!(normalize_wardrobe_set_id(Some(" \t ")), None);
		assert_eq!(normalize_wardrobe_set_id(Some(" field_drape ")), Some("field_drape"));
	}

	#[test]
	fn apply_required_wardrobe_set_resolves_empty_values_as_base() {
		let mut document = UnaDocument::default();
		let err = apply_required_wardrobe_set(&mut document, " \t ").expect_err("missing base wardrobe should be reported");
		assert_eq!(err, ".unavatar wardrobe base set not found");
		assert_eq!(require_wardrobe_set_id(" field_drape ").unwrap(), "field_drape");
	}

	#[test]
	fn apply_requested_wardrobe_set_ignores_empty_values() {
		let mut document = UnaDocument::default();
		assert!(apply_requested_wardrobe_set(&mut document, None).is_none());
		assert!(apply_requested_wardrobe_set(&mut document, Some("")).is_none());
	}

	#[test]
	fn enabled_expression_actions_are_added_to_runtime_actions() {
		let mut document = UnaDocument {
			expression_catalog: Some(un_avatar_core::UnaExpressionCatalog {
				presets: vec![
					un_avatar_core::UnaExpressionPreset {
						name: "Happy".to_string(),
						binds: Vec::new(),
					},
					un_avatar_core::UnaExpressionPreset {
						name: "Blink".to_string(),
						binds: Vec::new(),
					},
				],
			}),
			..Default::default()
		};

		let values = BTreeMap::from([("expression:happy".to_string(), 0.45)]);
		add_enabled_expression_runtime_actions(&mut document, &["expression:happy".to_string()], &values);

		let actions = document.runtime_actions.as_ref().expect("runtime actions");
		assert_eq!(actions.actions.len(), 1);
		assert_eq!(actions.actions[0].id, "expression:happy");
		assert_eq!(actions.actions[0].label, "Expression / Happy");
		assert_eq!(
			actions.actions[0].effects,
			vec![UnaRuntimeActionEffect::ExpressionWeight {
				name: "Happy".to_string(),
				weight: 0.45,
			}]
		);
	}

	#[test]
	fn enabled_expression_actions_update_existing_runtime_action_weights() {
		let mut document = UnaDocument {
			expression_catalog: Some(un_avatar_core::UnaExpressionCatalog {
				presets: vec![un_avatar_core::UnaExpressionPreset {
					name: "Happy".to_string(),
					binds: Vec::new(),
				}],
			}),
			runtime_actions: Some(UnaRuntimeActionSet {
				actions: vec![UnaRuntimeAction {
					id: "expression:happy".to_string(),
					label: "Expression / Happy".to_string(),
					triggers: Vec::new(),
					conditions: Vec::new(),
					effects: vec![UnaRuntimeActionEffect::ExpressionWeight {
						name: "Happy".to_string(),
						weight: 1.0,
					}],
				}],
			}),
			..Default::default()
		};

		let values = BTreeMap::from([("expression:happy".to_string(), 0.25)]);
		add_enabled_expression_runtime_actions(&mut document, &["expression:happy".to_string()], &values);

		let actions = document.runtime_actions.as_ref().expect("runtime actions");
		assert_eq!(actions.actions.len(), 1);
		assert_eq!(
			actions.actions[0].effects,
			vec![UnaRuntimeActionEffect::ExpressionWeight {
				name: "Happy".to_string(),
				weight: 0.25,
			}]
		);
	}

	#[test]
	fn import_report_warning_lines_include_approximations_and_lost_features() {
		let mut report = ImportReport::default();
		report
			.diagnostics
			.push(un_avatar_core::ReportMessage::warning("diagnostic warning"));
		report.lost_features.push(un_avatar_core::LostFeature {
			feature: "ModularAvatar.Unsupported".to_string(),
			detail: Some("preserved only".to_string()),
		});
		report.approximations.push(un_avatar_core::Approximation {
			feature: "ModularAvatar.ModularAvatarShapeChanger".to_string(),
			detail: Some("enabled static set/delete payloads only".to_string()),
		});

		let lines = import_report_warning_lines(&report, 8);

		assert_eq!(lines.len(), 3);
		assert_eq!(lines[0], "import warning: diagnostic warning");
		assert!(lines[1].contains("import lost feature: ModularAvatar.Unsupported"));
		assert!(lines[2].contains("import approximation: ModularAvatar.ModularAvatarShapeChanger"));
	}

	#[test]
	fn wardrobe_apply_report_summary_includes_material_counts() {
		let report = WardrobeApplyReport {
			active_asset_groups: vec!["outfit:field".to_string()],
			scoped_active_asset_group_count: 1,
			scoped_missing_active_asset_groups: vec!["outfit:missing".to_string()],
			scoped_resident_mesh_primitive_count: 2,
			scoped_resident_material_count: 3,
			scoped_resident_image_count: 4,
			scoped_resident_dynamics_count: 5,
			visibility_applied: 6,
			visibility_missing: 7,
			blendshape_applied: 8,
			blendshape_missing: 9,
			dynamics_applied: 10,
			dynamics_missing: 11,
			material_applied: 12,
			material_missing: 13,
			material_slot_applied: 14,
			material_slot_missing: 15,
			..Default::default()
		};
		let summary = wardrobe_apply_report_summary("field_drape", &report);
		assert!(summary.contains("material_applied=12 material_missing=13"));
		assert!(summary.contains("material_slot_applied=14 material_slot_missing=15"));
		assert!(summary.contains("scoped_resident=mesh:2 material:3 image:4 dynamics:5"));
		assert!(summary.contains("active_asset_groups=[\"outfit:field\"]"));
		assert!(summary.contains("scoped_missing_groups=[\"outfit:missing\"]"));
	}
}
