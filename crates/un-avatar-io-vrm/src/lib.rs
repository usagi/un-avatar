//! VRM 0.x / VRM 1.0 インポート（glTF 本体は [`un_avatar_io_gltf::scene_snapshot_from_gltf`]、拡張は JSON から完全抽出）。
//!
//! 設計正本: `docs/development-plan.md` Commit 1.5

#![forbid(unsafe_code)]

use std::{collections::BTreeMap, path::Path};

use glam::Mat4;
use serde_json::Value;
use un_avatar_core::{
	ReportStatus, UnaAlphaMode, UnaCullMode, UnaDocument, UnaDynamicsSourceKind, UnaExpressionCatalog, UnaExpressionPreset,
	UnaExpressionWeights, UnaImageRgba, UnaMorphTargetBind, UnaMtoonMaterial, UnaMtoonOutlineWidthMode, UnaNodeConstraint,
	UnaNodeConstraintAimAxis, UnaNodeConstraintAxis, UnaNodeConstraintKind, UnaSceneSnapshot, UnaShadingModel, UnaSpringBoneGroup,
	UnaSpringBoneSettings, UnaVrm0MtoonMaterialEntry, UnaVrmExtension,
};
use un_avatar_io::{
	AvatarImporter, Capability, FormatCapabilities, FormatDescriptor, FormatDirection, FormatId, ImportContext, ImportError, ImportInput,
	ImportOptions, ImportProbe, ImportProbeResult, ImportReport, ImportResult, IoRegistry, PluginStability,
};
use un_avatar_io_gltf::scene_snapshot_from_gltf;
use un_avatar_types::HumanoidProfile;

pub fn gltf_root_json_from_bytes(bytes: &[u8]) -> Result<Value, ImportError> {
	if bytes.starts_with(b"glTF") {
		let glb = gltf::Glb::from_slice(bytes).map_err(|e| ImportError::Message(format!("GLB 解析: {e}")))?;
		serde_json::from_slice(glb.json.as_ref()).map_err(|e| ImportError::Message(format!("GLB JSON: {e}")))
	} else {
		serde_json::from_slice(bytes).map_err(|e| ImportError::Message(format!("glTF JSON: {e}")))
	}
}

fn take_vrm_extension(root: &Value) -> Result<(VrmFlavor, Value), ImportError> {
	let Some(ext) = root.get("extensions").and_then(|e| e.as_object()) else {
		return Err(ImportError::Message("glTF に extensions がありません".into()));
	};
	if let Some(v) = ext.get("VRM") {
		return Ok((VrmFlavor::Vrm0, v.clone()));
	}
	if let Some(v) = ext.get("VRMC_vrm") {
		return Ok((VrmFlavor::Vrm1, v.clone()));
	}
	Err(ImportError::Message("extensions に VRM または VRMC_vrm がありません".into()))
}

pub fn has_vrm_extension(root: &Value) -> bool {
	take_vrm_extension(root).is_ok()
}

#[derive(Clone, Copy, Debug)]
enum VrmFlavor {
	Vrm0,
	Vrm1,
}

fn spec_version_string(vrm: &Value, flavor: VrmFlavor) -> String {
	vrm.get("specVersion")
		.or_else(|| vrm.get("spec_version"))
		.and_then(|v| v.as_str())
		.map(String::from)
		.unwrap_or_else(|| match flavor {
			VrmFlavor::Vrm0 => "0.0".into(),
			VrmFlavor::Vrm1 => "1.0".into(),
		})
}

/// VRM 1 形式の `humanBones` 配列（`{ bone, node }`）をパース。VRM 0 の一部エクスポータも同形で書き出す。
fn humanoid_bones_from_bone_array(arr: &[Value]) -> Result<BTreeMap<String, usize>, ImportError> {
	let mut out = BTreeMap::new();
	for b in arr {
		let bone = b
			.get("bone")
			.and_then(|x| x.as_str())
			.or_else(|| b.get("name").and_then(|x| x.as_str()))
			.ok_or_else(|| ImportError::Message("humanBones[]: bone / name がありません".into()))?;
		let node = b
			.get("node")
			.and_then(|n| n.as_u64())
			.ok_or_else(|| ImportError::Message(format!("humanBones[] bone={bone}: node がありません")))? as usize;
		out.insert(bone.to_ascii_lowercase(), node);
	}
	Ok(out)
}

fn humanoid_bones_from_bone_object(obj: &serde_json::Map<String, Value>) -> Result<BTreeMap<String, usize>, ImportError> {
	let mut out = BTreeMap::new();
	for (k, b) in obj {
		let node = b
			.get("node")
			.and_then(|n| n.as_u64())
			.ok_or_else(|| ImportError::Message(format!("humanBones.{k}: node がありません")))? as usize;
		out.insert(k.to_ascii_lowercase(), node);
	}
	Ok(out)
}

fn humanoid_vrm0(vrm: &Value) -> Result<BTreeMap<String, usize>, ImportError> {
	let Some(hb) = vrm.get("humanoid").and_then(|h| h.get("humanBones")) else {
		return Ok(BTreeMap::new());
	};
	if hb.is_null() {
		return Ok(BTreeMap::new());
	}
	if let Some(obj) = hb.as_object() {
		humanoid_bones_from_bone_object(obj)
	} else if let Some(arr) = hb.as_array() {
		humanoid_bones_from_bone_array(arr)
	} else {
		Err(ImportError::Message(
			"VRM 0 humanoid.humanBones はオブジェクトまたは配列である必要があります".into(),
		))
	}
}

fn humanoid_vrm1(vrm: &Value) -> Result<BTreeMap<String, usize>, ImportError> {
	let Some(hb) = vrm.get("humanoid").and_then(|h| h.get("humanBones")) else {
		return Ok(BTreeMap::new());
	};
	if hb.is_null() {
		return Ok(BTreeMap::new());
	}
	if let Some(obj) = hb.as_object() {
		humanoid_bones_from_bone_object(obj)
	} else if let Some(arr) = hb.as_array() {
		humanoid_bones_from_bone_array(arr)
	} else {
		Err(ImportError::Message(
			"VRM 1 humanoid.humanBones はオブジェクトまたは配列である必要があります".into(),
		))
	}
}

fn mtoon_materials_v0(vrm: &Value) -> Vec<UnaVrm0MtoonMaterialEntry> {
	let Some(arr) = vrm.get("materialProperties").and_then(|x| x.as_array()) else {
		return Vec::new();
	};
	let mut out = Vec::with_capacity(arr.len());
	for (i, item) in arr.iter().enumerate() {
		// 旧 UniVRM: `material` 整数。省かれている書き出しは **materialProperties の並び = materials[] と同順** とみなす。
		let mi = item.get("material").and_then(|x| x.as_u64()).map(|x| x as usize).unwrap_or(i);
		let shader_name = item.get("shader").and_then(|s| s.as_str()).unwrap_or("").to_string();
		out.push(UnaVrm0MtoonMaterialEntry {
			material_index: mi,
			shader_name,
			raw: item.clone(),
		});
	}
	out
}

fn mtoon_material_indices_v1(root: &Value) -> Vec<usize> {
	let Some(mats) = root.get("materials").and_then(|x| x.as_array()) else {
		return Vec::new();
	};
	let mut indices = Vec::with_capacity(mats.len());
	indices.extend(
		mats.iter()
			.enumerate()
			.filter_map(|(i, m)| m.get("extensions").and_then(|e| e.get("VRMC_materials_mtoon")).map(|_| i)),
	);
	indices
}

fn normalize_scene_basis_for_vrm(scene: &mut UnaSceneSnapshot, flavor: VrmFlavor) {
	match flavor {
		VrmFlavor::Vrm0 => rotate_scene_roots_y_pi(scene),
		VrmFlavor::Vrm1 => {}
	}
}

fn rotate_scene_roots_y_pi(scene: &mut UnaSceneSnapshot) {
	let basis = Mat4::from_rotation_y(std::f32::consts::PI);
	for &root in &scene.roots {
		let Some(node) = scene.nodes.get_mut(root) else {
			continue;
		};
		node.transform = (basis * Mat4::from_cols_array(&node.transform)).to_cols_array();
	}
}

fn parse_constraint_axis(s: &str) -> Option<UnaNodeConstraintAxis> {
	match s {
		"X" => Some(UnaNodeConstraintAxis::X),
		"Y" => Some(UnaNodeConstraintAxis::Y),
		"Z" => Some(UnaNodeConstraintAxis::Z),
		_ => None,
	}
}

fn parse_constraint_aim_axis(s: &str) -> Option<UnaNodeConstraintAimAxis> {
	match s {
		"PositiveX" => Some(UnaNodeConstraintAimAxis::PositiveX),
		"NegativeX" => Some(UnaNodeConstraintAimAxis::NegativeX),
		"PositiveY" => Some(UnaNodeConstraintAimAxis::PositiveY),
		"NegativeY" => Some(UnaNodeConstraintAimAxis::NegativeY),
		"PositiveZ" => Some(UnaNodeConstraintAimAxis::PositiveZ),
		"NegativeZ" => Some(UnaNodeConstraintAimAxis::NegativeZ),
		_ => None,
	}
}

fn constraint_weight(v: &Value) -> f32 {
	v.get("weight")
		.and_then(|x| x.as_f64())
		.map(|x| x as f32)
		.unwrap_or(1.0)
		.clamp(0.0, 1.0)
}

fn node_constraints_from_root(root: &Value) -> Vec<UnaNodeConstraint> {
	let Some(nodes) = root.get("nodes").and_then(|x| x.as_array()) else {
		return Vec::new();
	};
	let mut out = Vec::with_capacity(nodes.len());
	for (target_node, node) in nodes.iter().enumerate() {
		let Some(c) = node
			.get("extensions")
			.and_then(|x| x.get("VRMC_node_constraint"))
			.and_then(|x| x.get("constraint"))
		else {
			continue;
		};
		if let Some(v) = c.get("roll") {
			let Some(source_node) = v.get("source").and_then(|x| x.as_u64()).map(|x| x as usize) else {
				continue;
			};
			let Some(axis) = v.get("rollAxis").and_then(|x| x.as_str()).and_then(parse_constraint_axis) else {
				continue;
			};
			out.push(UnaNodeConstraint {
				target_node,
				source_node,
				weight: constraint_weight(v),
				kind: UnaNodeConstraintKind::Roll { axis },
			});
		} else if let Some(v) = c.get("aim") {
			let Some(source_node) = v.get("source").and_then(|x| x.as_u64()).map(|x| x as usize) else {
				continue;
			};
			let Some(axis) = v.get("aimAxis").and_then(|x| x.as_str()).and_then(parse_constraint_aim_axis) else {
				continue;
			};
			out.push(UnaNodeConstraint {
				target_node,
				source_node,
				weight: constraint_weight(v),
				kind: UnaNodeConstraintKind::Aim { axis },
			});
		} else if let Some(v) = c.get("rotation") {
			let Some(source_node) = v.get("source").and_then(|x| x.as_u64()).map(|x| x as usize) else {
				continue;
			};
			out.push(UnaNodeConstraint {
				target_node,
				source_node,
				weight: constraint_weight(v),
				kind: UnaNodeConstraintKind::Rotation,
			});
		}
	}
	out
}

fn build_vrm_extension(root: &Value, flavor: VrmFlavor, vrm_obj: Value) -> Result<UnaVrmExtension, ImportError> {
	let spec_version = spec_version_string(&vrm_obj, flavor);
	let meta = vrm_obj.get("meta").cloned().unwrap_or(Value::Null);

	let humanoid_bones = match flavor {
		VrmFlavor::Vrm0 => humanoid_vrm0(&vrm_obj)?,
		VrmFlavor::Vrm1 => humanoid_vrm1(&vrm_obj)?,
	};

	let mtoon_materials_v0 = match flavor {
		VrmFlavor::Vrm0 => mtoon_materials_v0(&vrm_obj),
		VrmFlavor::Vrm1 => Vec::new(),
	};

	let mtoon_material_indices_v1 = match flavor {
		VrmFlavor::Vrm1 => mtoon_material_indices_v1(root),
		VrmFlavor::Vrm0 => Vec::new(),
	};

	Ok(UnaVrmExtension {
		spec_version,
		meta,
		humanoid_bones,
		mtoon_materials_v0,
		mtoon_material_indices_v1,
		source: vrm_obj,
	})
}

/// VRM 0 MToon `floatProperties._BlendMode`（UniVRM）: 0 = Opaque, 1 = Cutout, 2/3 = Transparent 系。
const VRM0_MTOON_BLENDMODE_CUTOUT: f32 = 1.0;
const VRM0_MTOON_BLENDMODE_TRANSPARENT: f32 = 2.0;
const VRM0_MTOON_BLENDMODE_TRANSPARENT_ZW: f32 = 3.0;
const VRM0_MTOON_OUTLINE_WIDTH_TO_METERS: f32 = 0.01;

/// `materialProperties` に `_BlendMode` があるときだけ Some。
fn vrm0_mtoon_blend_mode_explicit(raw: &Value) -> Option<f32> {
	let v = raw.get("floatProperties")?.as_object()?.get("_BlendMode")?;
	v.as_f64().map(|x| x as f32)
}

fn vrm0_mtoon_has_float_properties(raw: &Value) -> bool {
	raw.get("floatProperties").and_then(|x| x.as_object()).is_some()
}

/// UniVRM の MToon カスタム切断（`_ALPHATEST_ON` 等）。
fn vrm0_mtoon_raw_implies_cutout(raw: &Value) -> bool {
	let Some(km) = raw.get("keywordMap").and_then(|v| v.as_object()) else {
		return false;
	};
	for (k, v) in km {
		if !vrm0_keyword_truthy(v) {
			continue;
		}
		let ku = k.to_ascii_uppercase();
		if ku.contains("ALPHATEST") || ku.contains("ALPHA_TEST") {
			return true;
		}
		if ku.contains("MTOON_RENDERING_CUTOUT") {
			return true;
		}
	}
	false
}

fn vrm0_mtoon_cutoff_optional(raw: &Value) -> f32 {
	let Some(floats) = raw.get("floatProperties").and_then(|x| x.as_object()) else {
		return 0.5;
	};
	floats
		.get("_Cutoff")
		.or_else(|| floats.get("_AlphaCutoff"))
		.and_then(|v| v.as_f64())
		.map(|x| x as f32)
		.unwrap_or(0.5)
		.clamp(0.0, 1.0)
}

fn vrm0_keyword_truthy(v: &Value) -> bool {
	match v {
		Value::Bool(b) => *b,
		Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
		Value::String(s) => !s.is_empty() && s != "0" && s != "false",
		_ => false,
	}
}

fn obj_f32(obj: &serde_json::Map<String, Value>, keys: &[&str], default: f32) -> f32 {
	keys.iter()
		.find_map(|k| obj.get(*k).and_then(|v| v.as_f64()).map(|x| x as f32))
		.unwrap_or(default)
}

fn obj_i32(obj: &serde_json::Map<String, Value>, key: &str, default: i32) -> i32 {
	obj.get(key).and_then(|v| v.as_i64()).map(|x| x as i32).unwrap_or(default)
}

fn obj_bool(obj: &serde_json::Map<String, Value>, key: &str, default: bool) -> bool {
	obj.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

fn obj_vec3(obj: &serde_json::Map<String, Value>, key: &str, default: [f32; 3]) -> [f32; 3] {
	let Some(arr) = obj.get(key).and_then(|v| v.as_array()) else {
		return default;
	};
	[
		arr.first().and_then(|v| v.as_f64()).map(|x| x as f32).unwrap_or(default[0]),
		arr.get(1).and_then(|v| v.as_f64()).map(|x| x as f32).unwrap_or(default[1]),
		arr.get(2).and_then(|v| v.as_f64()).map(|x| x as f32).unwrap_or(default[2]),
	]
}

fn obj_vec3_from_vec4(obj: &serde_json::Map<String, Value>, key: &str, default: [f32; 3]) -> [f32; 3] {
	obj_vec3(obj, key, default)
}

fn srgb_channel_to_linear(c: f32) -> f32 {
	let c = c.clamp(0.0, 1.0);
	if c <= 0.04045 {
		c / 12.92
	} else {
		((c + 0.055) / 1.055).powf(2.4)
	}
}

fn obj_vrm0_rim_color_vec3_from_vec4(obj: &serde_json::Map<String, Value>, key: &str, default: [f32; 3]) -> [f32; 3] {
	let v = obj_vec3_from_vec4(obj, key, default);
	[
		srgb_channel_to_linear(v[0]),
		srgb_channel_to_linear(v[1]),
		srgb_channel_to_linear(v[2]),
	]
}

fn obj_texture_index(obj: &serde_json::Map<String, Value>, key: &str) -> Option<usize> {
	let v = obj.get(key)?;
	if let Some(u) = v.as_u64() {
		return Some(u as usize);
	}
	v.get("index").and_then(|x| x.as_u64()).map(|x| x as usize)
}

fn value_texture_index(v: &Value, key: &str) -> Option<usize> {
	let t = v.get(key)?;
	if let Some(u) = t.as_u64() {
		return Some(u as usize);
	}
	t.get("index").and_then(|x| x.as_u64()).map(|x| x as usize)
}

fn texture_source_index(root: &Value, texture_index: usize) -> Option<usize> {
	root.get("textures")?
		.as_array()?
		.get(texture_index)?
		.get("source")?
		.as_u64()
		.map(|x| x as usize)
}

fn map_texture_to_image_index(root: &Value, texture_index: usize) -> usize {
	texture_source_index(root, texture_index).unwrap_or(texture_index)
}

fn obj_mtoon_texture_index(obj: &serde_json::Map<String, Value>, key: &str, root: &Value) -> Option<usize> {
	obj_texture_index(obj, key).map(|idx| map_texture_to_image_index(root, idx))
}

fn value_mtoon_texture_index(v: &Value, key: &str, root: &Value) -> Option<usize> {
	value_texture_index(v, key).map(|idx| map_texture_to_image_index(root, idx))
}

fn value_texture_scale(v: &Value, key: &str, default: f32) -> f32 {
	v.get(key)
		.and_then(|x| x.get("scale"))
		.and_then(|x| x.as_f64())
		.map(|x| x as f32)
		.unwrap_or(default)
}

fn vrm0_outline_width_mode(mode: f32) -> UnaMtoonOutlineWidthMode {
	if (mode - 1.0).abs() < 0.5 {
		UnaMtoonOutlineWidthMode::WorldCoordinates
	} else if (mode - 2.0).abs() < 0.5 {
		UnaMtoonOutlineWidthMode::ScreenCoordinates
	} else {
		UnaMtoonOutlineWidthMode::None
	}
}

fn vrm1_outline_width_mode(mode: &str) -> UnaMtoonOutlineWidthMode {
	match mode {
		"worldCoordinates" => UnaMtoonOutlineWidthMode::WorldCoordinates,
		"screenCoordinates" => UnaMtoonOutlineWidthMode::ScreenCoordinates,
		_ => UnaMtoonOutlineWidthMode::None,
	}
}

fn vrm0_shading_to_mtoon1_shift_toony(shade_shift: f32, shade_toony: f32) -> (f32, f32) {
	// VRM0/Santarh MToon uses:
	// min = _ShadeShift
	// max = lerp(1, _ShadeShift, _ShadeToony)
	// light = saturate((dotNL - min) / (max - min))
	//
	// The VRMC_materials_mtoon-1.0 style shader path uses:
	// light = linearstep(-1 + toony, 1 - toony, dotNL + shift)
	//
	// Convert the authored VRM0 transition interval into equivalent center/width
	// parameters so VRM0 face materials such as _ShadeShift=-0.8 remain mostly lit.
	let min_edge = shade_shift;
	let max_edge = 1.0 + (shade_shift - 1.0) * shade_toony.clamp(0.0, 1.0);
	let center = (min_edge + max_edge) * 0.5;
	let half_width = ((max_edge - min_edge).abs() * 0.5).clamp(0.0, 1.0);
	(-center, (1.0 - half_width).clamp(0.0, 1.0))
}

fn vrm0_indirect_light_to_mtoon1_gi_equalization(indirect_light_intensity: f32) -> f32 {
	(1.0 - indirect_light_intensity).clamp(0.0, 1.0)
}

fn parse_mtoon_v0(raw: &Value, root: &Value) -> UnaMtoonMaterial {
	let floats = raw.get("floatProperties").and_then(|x| x.as_object()).cloned().unwrap_or_default();
	let vectors = raw.get("vectorProperties").and_then(|x| x.as_object()).cloned().unwrap_or_default();
	let textures = raw
		.get("textureProperties")
		.and_then(|x| x.as_object())
		.cloned()
		.unwrap_or_default();

	let shade_shift = obj_f32(&floats, &["_ShadingShift", "_ShadeShift"], 0.0);
	let shade_toony = obj_f32(&floats, &["_ShadingToony", "_ShadeToony"], 0.9).clamp(0.0, 1.0);
	let (shading_shift_factor, shading_toony_factor) = vrm0_shading_to_mtoon1_shift_toony(shade_shift, shade_toony);
	let mut m = UnaMtoonMaterial {
		shade_color_factor: obj_vec3_from_vec4(&vectors, "_ShadeColor", [0.0, 0.0, 0.0]),
		shade_multiply_texture_index: obj_mtoon_texture_index(&textures, "_ShadeTexture", root),
		shading_shift_factor,
		shading_toony_factor,
		gi_equalization_factor: floats
			.get("_GiEqualization")
			.and_then(|v| v.as_f64().map(|x| x as f32))
			.map(|v| v.clamp(0.0, 1.0))
			.unwrap_or_else(|| vrm0_indirect_light_to_mtoon1_gi_equalization(obj_f32(&floats, &["_IndirectLightIntensity"], 0.1))),
		matcap_factor: [1.0, 1.0, 1.0],
		matcap_texture_index: obj_mtoon_texture_index(&textures, "_SphereAdd", root),
		parametric_rim_color_factor: obj_vrm0_rim_color_vec3_from_vec4(&vectors, "_RimColor", [0.0, 0.0, 0.0]),
		rim_multiply_texture_index: obj_mtoon_texture_index(&textures, "_RimTexture", root),
		rim_lighting_mix_factor: obj_f32(&floats, &["_RimLightingMix"], 1.0).clamp(0.0, 1.0),
		parametric_rim_fresnel_power_factor: obj_f32(&floats, &["_RimFresnelPower"], 5.0).max(0.00001),
		parametric_rim_lift_factor: obj_f32(&floats, &["_RimLift"], 0.0),
		outline_width_mode: vrm0_outline_width_mode(obj_f32(&floats, &["_OutlineWidthMode"], 0.0)),
		outline_width_factor: (obj_f32(&floats, &["_OutlineWidth"], 0.0) * VRM0_MTOON_OUTLINE_WIDTH_TO_METERS).max(0.0),
		outline_width_multiply_texture_index: obj_mtoon_texture_index(&textures, "_OutlineWidthTexture", root),
		outline_color_factor: obj_vec3_from_vec4(&vectors, "_OutlineColor", [0.0, 0.0, 0.0]),
		outline_lighting_mix_factor: obj_f32(&floats, &["_OutlineLightingMix"], 1.0).clamp(0.0, 1.0),
		uv_animation_mask_texture_index: obj_mtoon_texture_index(&textures, "_UvAnimMaskTexture", root),
		uv_animation_scroll_x_speed_factor: obj_f32(&floats, &["_UvAnimScrollX"], 0.0),
		uv_animation_scroll_y_speed_factor: obj_f32(&floats, &["_UvAnimScrollY"], 0.0),
		uv_animation_rotation_speed_factor: obj_f32(&floats, &["_UvAnimRotation"], 0.0),
		..Default::default()
	};
	if m.shade_multiply_texture_index.is_none() && eye_area_material_name(vrm0_mtoon_material_name(raw)) {
		m.shade_multiply_texture_index = obj_mtoon_texture_index(&textures, "_MainTex", root);
	}
	m.transparent_with_z_write =
		vrm0_mtoon_blend_mode_explicit(raw).is_some_and(|bm| (bm - VRM0_MTOON_BLENDMODE_TRANSPARENT_ZW).abs() < 1e-3);
	m
}

fn parse_mtoon_v1(material: &Value, root: &Value) -> Option<UnaMtoonMaterial> {
	let ext = material.get("extensions")?.get("VRMC_materials_mtoon")?;
	let obj = ext.as_object()?;
	Some(UnaMtoonMaterial {
		transparent_with_z_write: obj_bool(obj, "transparentWithZWrite", false),
		render_queue_offset_number: obj_i32(obj, "renderQueueOffsetNumber", 0),
		shade_color_factor: obj_vec3(obj, "shadeColorFactor", [0.0, 0.0, 0.0]),
		shade_multiply_texture_index: value_mtoon_texture_index(ext, "shadeMultiplyTexture", root),
		shading_shift_factor: obj_f32(obj, &["shadingShiftFactor"], 0.0),
		shading_shift_texture_index: value_mtoon_texture_index(ext, "shadingShiftTexture", root),
		shading_shift_texture_scale: value_texture_scale(ext, "shadingShiftTexture", 1.0),
		shading_toony_factor: obj_f32(obj, &["shadingToonyFactor"], 0.9).clamp(0.0, 1.0),
		gi_equalization_factor: obj_f32(obj, &["giEqualizationFactor", "giIntensityFactor"], 0.9).clamp(0.0, 1.0),
		matcap_factor: obj_vec3(obj, "matcapFactor", [1.0, 1.0, 1.0]),
		matcap_texture_index: value_mtoon_texture_index(ext, "matcapTexture", root),
		parametric_rim_color_factor: obj_vec3(obj, "parametricRimColorFactor", [0.0, 0.0, 0.0]),
		rim_multiply_texture_index: value_mtoon_texture_index(ext, "rimMultiplyTexture", root),
		reflection_cube_texture_index: None,
		rim_lighting_mix_factor: obj_f32(obj, &["rimLightingMixFactor"], 1.0).clamp(0.0, 1.0),
		parametric_rim_fresnel_power_factor: obj_f32(obj, &["parametricRimFresnelPowerFactor"], 5.0).max(0.00001),
		parametric_rim_lift_factor: obj_f32(obj, &["parametricRimLiftFactor"], 0.0),
		outline_width_mode: obj
			.get("outlineWidthMode")
			.and_then(|v| v.as_str())
			.map(vrm1_outline_width_mode)
			.unwrap_or_default(),
		outline_width_factor: obj_f32(obj, &["outlineWidthFactor"], 0.0).max(0.0),
		outline_width_multiply_texture_index: value_mtoon_texture_index(ext, "outlineWidthMultiplyTexture", root),
		outline_color_factor: obj_vec3(obj, "outlineColorFactor", [0.0, 0.0, 0.0]),
		outline_lighting_mix_factor: obj_f32(obj, &["outlineLightingMixFactor"], 1.0).clamp(0.0, 1.0),
		uv_animation_mask_texture_index: value_mtoon_texture_index(ext, "uvAnimationMaskTexture", root),
		uv_offset_scale: [0.0, 0.0, 1.0, 1.0],
		uv_animation_scroll_x_speed_factor: obj_f32(obj, &["uvAnimationScrollXSpeedFactor"], 0.0),
		uv_animation_scroll_y_speed_factor: obj_f32(obj, &["uvAnimationScrollYSpeedFactor"], 0.0),
		uv_animation_rotation_speed_factor: obj_f32(obj, &["uvAnimationRotationSpeedFactor"], 0.0),
		..Default::default()
	})
}

fn assign_mtoon_materials(scene: &mut UnaSceneSnapshot, root: &Value, vrm: &UnaVrmExtension) {
	for e in &vrm.mtoon_materials_v0 {
		if let Some(m) = scene.materials.get_mut(e.material_index) {
			m.mtoon = Some(parse_mtoon_v0(&e.raw, root));
		}
	}
	if let Some(mats) = root.get("materials").and_then(|x| x.as_array()) {
		for (i, material_json) in mats.iter().enumerate() {
			if let Some(parsed) = parse_mtoon_v1(material_json, root) {
				if let Some(m) = scene.materials.get_mut(i) {
					m.mtoon = Some(parsed);
				}
			}
		}
	}
}

/// `_BlendMode` が欠落・Opaque のまま書き出されていても、シェーダ名・keywordMap で透明扱いになるケース（瞳・睫など）。
fn vrm0_mtoon_raw_implies_transparent_blend(raw: &Value, shader_name: &str) -> bool {
	let s = shader_name.to_ascii_lowercase();
	if s.contains("transparent") {
		return true;
	}
	let Some(km) = raw.get("keywordMap").and_then(|v| v.as_object()) else {
		return false;
	};
	for (k, v) in km {
		if !vrm0_keyword_truthy(v) {
			continue;
		}
		let ku = k.to_ascii_uppercase();
		if ku.contains("TRANSPARENT") || (ku.contains("ALPHA") && ku.contains("BLEND")) {
			return true;
		}
	}
	false
}

fn vrm0_mtoon_double_sided(raw: &Value) -> Option<bool> {
	vrm0_mtoon_cull_mode(raw).map(|mode| mode == UnaCullMode::Off)
}

fn vrm0_mtoon_cull_mode(raw: &Value) -> Option<UnaCullMode> {
	let mode = raw
		.get("floatProperties")
		.and_then(|x| x.as_object())
		.and_then(|floats| floats.get("_CullMode"))
		.and_then(|x| x.as_f64())?;
	if (mode - 0.0).abs() < 0.5 {
		Some(UnaCullMode::Off)
	} else if (mode - 1.0).abs() < 0.5 {
		Some(UnaCullMode::Front)
	} else if (mode - 2.0).abs() < 0.5 {
		Some(UnaCullMode::Back)
	} else {
		None
	}
}

fn tag_mtoon_materials(scene: &mut UnaSceneSnapshot, vrm: &UnaVrmExtension) {
	for e in &vrm.mtoon_materials_v0 {
		if let Some(m) = scene.materials.get_mut(e.material_index) {
			m.shading = UnaShadingModel::MToonLike;
			if let Some(double_sided) = vrm0_mtoon_double_sided(&e.raw) {
				m.double_sided = double_sided;
			}
			if let Some(cull_mode) = vrm0_mtoon_cull_mode(&e.raw) {
				m.cull_mode = cull_mode;
			}
			if vrm0_mtoon_raw_implies_transparent_blend(&e.raw, &e.shader_name) {
				m.alpha_mode = UnaAlphaMode::Blend;
			} else if vrm0_mtoon_raw_implies_cutout(&e.raw) {
				m.alpha_mode = UnaAlphaMode::Mask;
				m.alpha_cutoff = vrm0_mtoon_cutoff_optional(&e.raw);
			} else if let Some(bm) = vrm0_mtoon_blend_mode_explicit(&e.raw) {
				if bm < 0.5 {
					m.alpha_mode = UnaAlphaMode::Opaque;
				} else if (bm - VRM0_MTOON_BLENDMODE_CUTOUT).abs() < 1e-3 {
					m.alpha_mode = UnaAlphaMode::Mask;
					m.alpha_cutoff = vrm0_mtoon_cutoff_optional(&e.raw);
				} else if (bm - VRM0_MTOON_BLENDMODE_TRANSPARENT).abs() < 1e-3 || (bm - VRM0_MTOON_BLENDMODE_TRANSPARENT_ZW).abs() < 1e-3 {
					m.alpha_mode = UnaAlphaMode::Blend;
				}
			} else if vrm0_mtoon_has_float_properties(&e.raw) {
				// `_BlendMode` 欠落かつカットアウト keyword なし → Unity 既定は Opaque（glTF だけ MASK の瞳が全 discard になるのを防ぐ）
				m.alpha_mode = UnaAlphaMode::Opaque;
			}
		}
	}
	for &i in &vrm.mtoon_material_indices_v1 {
		if let Some(m) = scene.materials.get_mut(i) {
			m.shading = UnaShadingModel::MToonLike;
		}
	}
	relax_mtoon_mask_for_likely_eye_materials(scene);
}

fn vrm0_mtoon_material_name(raw: &Value) -> Option<&str> {
	raw.get("name").and_then(|v| v.as_str())
}

fn eye_area_material_name(name: Option<&str>) -> bool {
	let Some(n) = name else {
		return false;
	};
	let l = n.to_ascii_lowercase();
	l.contains("iris")
		|| l.contains("pupil")
		|| l.contains("eyeball")
		|| l.contains("cornea")
		|| l.contains("sight")
		|| l.contains("eyelid")
		|| l.contains("eyelash")
		|| l.contains("eyeline")
		|| l.contains("eyeliner")
		|| l.contains("eyebrow")
		|| l.contains("brow")
		|| l.contains("lash")
		|| l.contains("lid")
		|| l.contains("瞳")
		|| l.contains("虹彩")
		|| l.contains("虹膜")
		|| l.contains("目玉")
		|| l.contains("眼睛")
		|| l.contains("眼球")
		|| l.contains("眼珠")
		|| l.contains("眼白")
		|| l.contains("高光")
		|| l.contains("ハイライト")
		|| l.contains("瞼")
		|| l.contains("まぶた")
		|| l.contains("まつげ")
		|| l.contains("睫")
		|| l.contains("眉")
		|| l.contains("眼睑")
		|| l.contains("眼瞼")
		|| l.contains("眼皮")
		|| l.contains("アイライン")
		|| l.contains("アイラッシュ")
}

/// glTF 側は **PBR lit → Lambert**、**`KHR_materials_unlit` は Unlit** になる。VRM の MToon は拡張 JSON にしか載らないため、多くの書き出しでは **全マテリアルが Unlit のまま**残る。
/// アバター表示ではトゥーンライティングが既定期待なので、**すでに MToonLike のもの以外**は `MToonLike` に寄せる（意図的な区別は `tag_mtoon_materials` 側の α モード等が優先される）。
fn default_vrm_shading_to_mtoon_like(scene: &mut UnaSceneSnapshot) {
	for m in &mut scene.materials {
		if m.shading != UnaShadingModel::MToonLike {
			m.shading = UnaShadingModel::MToonLike;
		}
	}
}

/// MToon + MASK で material alpha が 0 に近い古い瞳materialだけ、全捨てを避ける。
/// 透明PNGの瞳/ハイライトはMaskのままalphaで抜かないと、透明部分の黒RGBまで表示される。
fn relax_mtoon_mask_for_likely_eye_materials(scene: &mut UnaSceneSnapshot) {
	for m in &mut scene.materials {
		if m.shading != UnaShadingModel::MToonLike || m.alpha_mode != UnaAlphaMode::Mask {
			continue;
		}
		if m.base_color_factor[3] > 0.001 {
			continue;
		}
		let Some(name) = m.name.as_deref() else {
			continue;
		};
		let l = name.to_ascii_lowercase();
		let eye_like = l.contains("iris")
			|| l.contains("pupil")
			|| l.contains("eyeball")
			|| l.contains("cornea")
			|| l.contains("sight")
			|| l.contains("瞳")
			|| l.contains("虹彩")
			|| l.contains("虹膜")
			|| l.contains("目玉")
			|| l.contains("眼睛")
			|| l.contains("眼球")
			|| l.contains("眼珠")
			|| l.contains("眼白")
			|| (l.contains("eye") && !l.contains("eyelash") && !l.contains("eyeline") && !l.contains("eyebrow"));
		let highlight = l.contains("highlight") || l.contains("ハイライト") || l.contains("高光");
		if eye_like || highlight {
			m.alpha_mode = UnaAlphaMode::Opaque;
			m.base_color_factor[3] = 1.0;
		}
	}
}

fn image_is_flat_neutral_normal(image: &UnaImageRgba) -> bool {
	if image.width == 0 || image.height == 0 || image.pixels.is_empty() {
		return false;
	}
	let rgba = image.rgba8_compat_pixels();
	rgba.chunks_exact(4)
		.all(|px| (127..=128).contains(&px[0]) && (127..=128).contains(&px[1]) && px[2] >= 254)
}

fn drop_flat_neutral_normal_textures(scene: &mut UnaSceneSnapshot) {
	let images = &scene.images;
	for mat in &mut scene.materials {
		if mat
			.normal_texture_index
			.and_then(|idx| images.get(idx))
			.is_some_and(image_is_flat_neutral_normal)
		{
			mat.normal_texture_index = None;
			mat.normal_texture_scale = 0.0;
		}
	}
}

fn vrm0_blend_shape_binds(group: &Value) -> Option<&Vec<Value>> {
	group
		.get("values")
		.and_then(|x| x.as_array())
		.or_else(|| group.get("binds").and_then(|x| x.as_array()))
}

fn vrm0_blend_shape_weight_scale(v: &Value) -> f32 {
	let weight = v.get("weight").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
	if weight > 1.0 {
		(weight / 100.0).clamp(0.0, 1.0)
	} else {
		weight.clamp(0.0, 1.0)
	}
}

fn expression_catalog_v0(vrm: &Value) -> UnaExpressionCatalog {
	let groups = vrm
		.get("blendShapeMaster")
		.and_then(|m| m.get("blendShapeGroups"))
		.and_then(|x| x.as_array())
		.or_else(|| vrm.get("blendShapeGroups").and_then(|x| x.as_array()));
	let Some(groups) = groups else {
		return UnaExpressionCatalog::default();
	};
	let mut presets = Vec::with_capacity(groups.len());
	for g in groups {
		// VRM0 BlendShapeGroup の名前選択:
		// - PerfectSync 対応モデルでは ARKit 52 個分の BlendShape が `presetName = "unknown"`、
		//   `name = "MouthSmileLeft"` のような形で登録されている（VRM 0.x の標準慣行）。
		// - 単純に presetName を優先すると 52 件が全部 "unknown" として重複登録されてしまう。
		// - そこで presetName が `unknown` / 空 / 欠落 のときは `name` を採用する。
		// - presetName が `neutral`, `a`, `blink_l`, `lookup` 等の VRM 標準値のときは presetName を使う。
		let preset_name = g.get("presetName").and_then(|x| x.as_str()).map(|s| s.trim()).unwrap_or("");
		let raw_name = g.get("name").and_then(|x| x.as_str()).map(|s| s.trim()).unwrap_or("");
		let name = match preset_name.to_ascii_lowercase().as_str() {
			"" | "unknown" => raw_name.to_string(),
			_ => preset_name.to_string(),
		};
		if name.is_empty() {
			continue;
		}
		let Some(vals) = vrm0_blend_shape_binds(g) else {
			continue;
		};
		let mut binds = Vec::with_capacity(vals.len());
		for v in vals {
			let mesh = v.get("mesh").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
			let idx = v.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
			let wt = vrm0_blend_shape_weight_scale(v);
			binds.push(UnaMorphTargetBind {
				mesh_index: mesh,
				primitive_index: 0,
				morph_target_index: idx,
				weight_scale: wt,
			});
		}
		if !binds.is_empty() {
			presets.push(UnaExpressionPreset { name, binds });
		}
	}
	UnaExpressionCatalog { presets }
}

fn expression_catalog_v1(vrm: &Value, node_mesh: &[Option<usize>]) -> UnaExpressionCatalog {
	let Some(expr_root) = vrm.get("expressions") else {
		return UnaExpressionCatalog::default();
	};
	let preset_count = ["preset", "custom"]
		.iter()
		.filter_map(|cat_key| expr_root.get(cat_key).and_then(|x| x.as_object()))
		.map(|cat| cat.len())
		.sum();
	let mut presets = Vec::with_capacity(preset_count);
	for cat_key in ["preset", "custom"] {
		let Some(cat) = expr_root.get(cat_key).and_then(|x| x.as_object()) else {
			continue;
		};
		for (preset_name, expr_val) in cat {
			let Some(binds_arr) = expr_val.get("morphTargetBinds").and_then(|x| x.as_array()) else {
				continue;
			};
			let mut binds = Vec::with_capacity(binds_arr.len());
			for b in binds_arr {
				let node_idx = b
					.get("node")
					.and_then(|x| x.as_u64())
					.map(|x| x as usize)
					.or_else(|| b.get("nodeIndex").and_then(|x| x.as_u64()).map(|x| x as usize));
				let Some(ni) = node_idx else { continue };
				let Some(mesh_i) = node_mesh.get(ni).copied().flatten() else {
					continue;
				};
				let morph_i = b.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
				let wt = b.get("weight").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
				binds.push(UnaMorphTargetBind {
					mesh_index: mesh_i,
					primitive_index: 0,
					morph_target_index: morph_i,
					weight_scale: wt,
				});
			}
			if !binds.is_empty() {
				presets.push(UnaExpressionPreset {
					name: preset_name.clone(),
					binds,
				});
			}
		}
	}
	UnaExpressionCatalog { presets }
}

fn build_expression_catalog(source: &Value, flavor: VrmFlavor, node_mesh: &[Option<usize>]) -> UnaExpressionCatalog {
	match flavor {
		VrmFlavor::Vrm0 => expression_catalog_v0(source),
		VrmFlavor::Vrm1 => expression_catalog_v1(source, node_mesh),
	}
}

/// VRM の式バインドを、**モーフターゲット数が参照プリミティブと一致する**他プリミティブにだけ複製する。
/// 同じ mesh 内でもプリミティブごとにモーフ集合が異なることがあり、インデックスだけ一致で複製すると
/// 目用モーフが口プリミティブに掛かるなどして形状が破綻するのを防ぐ。
fn expand_expression_binds_per_primitive(scene: &UnaSceneSnapshot, cat: &mut UnaExpressionCatalog) {
	for preset in &mut cat.presets {
		let old = std::mem::take(&mut preset.binds);
		let mut new_binds = Vec::with_capacity(old.len());
		for b in old {
			let Some(mesh_prims) = scene.meshes.get(b.mesh_index) else {
				continue;
			};
			let Some(ref_prim) = mesh_prims.get(b.primitive_index) else {
				continue;
			};
			let ref_n_morphs = ref_prim.morph_targets.len();
			if b.morph_target_index >= ref_n_morphs {
				continue;
			}
			for (prim_i, prim_buf) in mesh_prims.iter().enumerate() {
				if prim_buf.morph_targets.len() == ref_n_morphs && b.morph_target_index < prim_buf.morph_targets.len() {
					new_binds.push(UnaMorphTargetBind {
						mesh_index: b.mesh_index,
						primitive_index: prim_i,
						morph_target_index: b.morph_target_index,
						weight_scale: b.weight_scale,
					});
				}
			}
		}
		preset.binds = new_binds;
	}
}

/// VRM0 `secondaryAnimation.boneGroups[].bones[]` の各エントリは「揺れチェーンのルート」を指す。
/// 実チェーンは gltf `nodes[*].children` をたどって構築する（VRM0 仕様）。
///
/// 旧実装は `bones[]` を 1 本のチェーンとして扱っており、
/// 例えば `bones: [297, 299]` (Bust 左右) を `297 → 299` のチェーンと誤認していた。
/// またチェーン長 < 2 のグループ（`bones: [417]` 単一ルートなど）はスキップされ、
/// 該当ボーンの揺れが完全に止まる/暴れる原因になっていた。
fn collect_chain_from_root(nodes: &[Value], root_idx: usize) -> Vec<usize> {
	let mut chain = vec![root_idx];
	let mut current = root_idx;
	loop {
		let Some(node) = nodes.get(current) else {
			break;
		};
		let Some(children) = node.get("children").and_then(|c| c.as_array()) else {
			break;
		};
		// VRM0 SpringBone のチェーンは原則線形。複数子がある場合は最初の子を採用する
		// （UniVRM 互換）。
		let Some(next) = children.iter().find_map(|v| v.as_u64()).map(|u| u as usize) else {
			break;
		};
		if chain.contains(&next) {
			break;
		}
		chain.push(next);
		current = next;
		if chain.len() > 64 {
			break;
		}
	}
	chain
}

fn spring_bones_from_vrm0(root: &Value, vrm: &Value) -> Option<UnaSpringBoneSettings> {
	let sa = vrm.get("secondaryAnimation")?;
	let groups = sa.get("boneGroups")?.as_array()?;
	let empty: Vec<Value> = Vec::new();
	let nodes = root.get("nodes").and_then(|x| x.as_array()).unwrap_or(&empty);
	let mut out_groups = Vec::with_capacity(groups.len());
	for bg in groups {
		let bone_roots: Vec<usize> = bg
			.get("bones")
			.and_then(|b| b.as_array())
			.map(|a| {
				let mut roots = Vec::with_capacity(a.len());
				roots.extend(a.iter().filter_map(|v| v.as_u64().map(|u| u as usize)));
				roots
			})
			.unwrap_or_default();
		if bone_roots.is_empty() {
			continue;
		}
		let stiffness = bg
			.get("stiffiness")
			.or_else(|| bg.get("stiffness"))
			.and_then(|x| x.as_f64())
			.unwrap_or(1.0) as f32;
		let gravity_power = bg.get("gravityPower").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
		let drag = bg.get("dragForce").and_then(|x| x.as_f64()).unwrap_or(0.4) as f32;
		let gd = bg
			.get("gravityDir")
			.and_then(|o| {
				Some([
					o.get("x")?.as_f64()? as f32,
					o.get("y")?.as_f64()? as f32,
					o.get("z")?.as_f64()? as f32,
				])
			})
			.unwrap_or([0.0, -1.0, 0.0]);
		let center = bg.get("center").and_then(|x| x.as_i64()).unwrap_or(-1);
		let center_node = if center >= 0 { Some(center as usize) } else { None };
		let hit_radius = bg.get("hitRadius").and_then(|x| x.as_f64()).unwrap_or(0.02) as f32;
		let comment = bg.get("comment").and_then(|s| s.as_str()).unwrap_or("").to_string();
		for root_idx in bone_roots {
			let chain = collect_chain_from_root(nodes, root_idx);
			if chain.len() < 2 {
				continue;
			}
			out_groups.push(UnaSpringBoneGroup {
				source_kind: UnaDynamicsSourceKind::VrmSpringBone,
				comment: comment.clone(),
				category: String::new(),
				stiffness,
				gravity_power,
				gravity_dir: gd,
				drag_force: drag,
				center_node,
				hit_radius,
				bone_node_indices: chain,
			});
		}
	}
	if out_groups.is_empty() {
		None
	} else {
		Some(UnaSpringBoneSettings {
			groups: out_groups,
			colliders: Vec::new(),
		})
	}
}

fn spring_bones_from_vrm1_root(root: &Value) -> Option<UnaSpringBoneSettings> {
	let ext = root.get("extensions")?.as_object()?;
	let sb = ext.get("VRMC_springBone")?;
	let springs = sb.get("springs")?.as_array()?;
	let mut out_groups = Vec::with_capacity(springs.len());
	for sp in springs {
		let joints = sp.get("joints")?.as_array()?;
		if joints.len() < 2 {
			continue;
		}
		let mut bones = Vec::with_capacity(joints.len());
		let mut stiffness = 1.0_f32;
		let mut gravity_power = 0.0_f32;
		let mut drag = 0.3_f32;
		let mut gd = [0.0_f32, -1.0, 0.0];
		let mut hit_radius = 0.02_f32;
		for (ji, j) in joints.iter().enumerate() {
			let node = j.get("node").and_then(|x| x.as_u64()).map(|u| u as usize)?;
			bones.push(node);
			if ji == 0 {
				stiffness = j.get("stiffness").and_then(|x| x.as_f64()).unwrap_or(1.0) as f32;
				gravity_power = j.get("gravityPower").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
				drag = j.get("dragForce").and_then(|x| x.as_f64()).unwrap_or(0.3) as f32;
				hit_radius = j.get("hitRadius").and_then(|x| x.as_f64()).unwrap_or(0.02) as f32;
				if let Some(o) = j.get("gravityDir") {
					let x = o.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
					let y = o.get("y").and_then(|v| v.as_f64()).unwrap_or(-1.0) as f32;
					let z = o.get("z").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
					gd = [x, y, z];
				}
			}
		}
		let comment = sp.get("name").and_then(|s| s.as_str()).unwrap_or("").to_string();
		out_groups.push(UnaSpringBoneGroup {
			source_kind: UnaDynamicsSourceKind::VrmSpringBone,
			comment,
			category: String::new(),
			stiffness,
			gravity_power,
			gravity_dir: gd,
			drag_force: drag,
			center_node: None,
			hit_radius,
			bone_node_indices: bones,
		});
	}
	if out_groups.is_empty() {
		None
	} else {
		Some(UnaSpringBoneSettings {
			groups: out_groups,
			colliders: Vec::new(),
		})
	}
}

fn extract_spring_bones(root: &Value, vrm: &Value, flavor: VrmFlavor) -> Option<UnaSpringBoneSettings> {
	match flavor {
		VrmFlavor::Vrm0 => spring_bones_from_vrm0(root, vrm),
		VrmFlavor::Vrm1 => spring_bones_from_vrm1_root(root).or_else(|| spring_bones_from_vrm0(root, vrm)),
	}
}

/// Built-in VRM Importer（`io.un-avatar.vrm`）。`.vrm`（GLB 想定）および拡張付き `.glb`。
#[derive(Clone, Copy, Debug, Default)]
pub struct VrmImporter;

fn import_vrm_from_parts(path_hint: Option<&Path>, bytes: &[u8], root: Option<Value>) -> Result<ImportResult, ImportError> {
	let root = match root {
		Some(root) => root,
		None => gltf_root_json_from_bytes(bytes)?,
	};
	let (flavor, vrm_raw) = take_vrm_extension(&root)?;
	let vrm_ext = build_vrm_extension(&root, flavor, vrm_raw)?;

	let (document, buffers, image_data) = gltf::import_slice(bytes).map_err(|e| ImportError::Message(e.to_string()))?;

	let mut report = ImportReport {
		source_format: Some(VrmImporter.descriptor().id.clone()),
		..Default::default()
	};

	let mut scene = scene_snapshot_from_gltf(&document, &buffers, image_data, &mut report)?;
	normalize_scene_basis_for_vrm(&mut scene, flavor);
	scene.node_constraints = node_constraints_from_root(&root);
	tag_mtoon_materials(&mut scene, &vrm_ext);
	assign_mtoon_materials(&mut scene, &root, &vrm_ext);
	default_vrm_shading_to_mtoon_like(&mut scene);
	// Unlit のままだと relax 対象外だった MASK 瞳を、MToon 化後にもう一度緩和する。
	relax_mtoon_mask_for_likely_eye_materials(&mut scene);
	drop_flat_neutral_normal_textures(&mut scene);
	let node_mesh: Vec<Option<usize>> = scene.nodes.iter().map(|n| n.mesh).collect();
	let mut expr_cat = build_expression_catalog(&vrm_ext.source, flavor, &node_mesh);
	if !expr_cat.presets.is_empty() {
		expand_expression_binds_per_primitive(&scene, &mut expr_cat);
	}
	let (expression_catalog, expression_weights) = if expr_cat.presets.is_empty() {
		(None, None)
	} else {
		(Some(expr_cat), Some(UnaExpressionWeights::default()))
	};

	let humanoid_profile = if vrm_ext.humanoid_bones.is_empty() {
		None
	} else {
		Some(HumanoidProfile {
			bone_node_indices: vrm_ext.humanoid_bones.clone(),
		})
	};

	let spring_bones = extract_spring_bones(&root, &vrm_ext.source, flavor);

	report.status = if report.lost_features.is_empty() && report.approximations.is_empty() {
		ReportStatus::Success
	} else {
		ReportStatus::PartialSuccess
	};
	let n_expr = expression_catalog.as_ref().map(|c| c.presets.len()).unwrap_or(0);
	let n_spring = spring_bones.as_ref().map(|s| s.groups.len()).unwrap_or(0);
	report.push_info(format!(
		"VRM spec={} humanoid bones={} mtoon_hints v0={} v1_indices={} expression_presets={} spring_groups={}",
		vrm_ext.spec_version,
		vrm_ext.humanoid_bones.len(),
		vrm_ext.mtoon_materials_v0.len(),
		vrm_ext.mtoon_material_indices_v1.len(),
		n_expr,
		n_spring
	));
	if let Some(path) = path_hint {
		report.push_info(format!("source: {}", path.display()));
	} else {
		report.push_info("source: in-memory VRM/GLB".to_string());
	}

	Ok(ImportResult {
		document: UnaDocument {
			scene: Some(scene),
			unavatar: None,
			vrm: Some(vrm_ext),
			humanoid_profile,
			expression_catalog,
			expression_weights,
			spring_bones,
		},
		report,
	})
}

pub fn import_vrm_bytes(path_hint: Option<&Path>, bytes: &[u8], root: Option<Value>) -> Result<ImportResult, ImportError> {
	import_vrm_from_parts(path_hint, bytes, root)
}

impl AvatarImporter for VrmImporter {
	fn descriptor(&self) -> FormatDescriptor {
		FormatDescriptor {
			id: FormatId::new("io.un-avatar.vrm"),
			display_name: "VRM 0.x / 1.0".to_owned(),
			extensions: vec!["vrm".to_owned(), "glb".to_owned()],
			media_types: vec!["model/vrm".to_owned(), "model/gltf-binary".to_owned()],
			direction: FormatDirection::Import,
			capabilities: FormatCapabilities {
				mesh: Capability::ImportOnly,
				skeleton: Capability::ImportOnly,
				skinning: Capability::ImportOnly,
				material: Capability::ImportOnly,
				animation: Capability::Unsupported,
				expression: Capability::ImportOnly,
				physics: Capability::ImportOnly,
				cameras: Capability::Unsupported,
				lights: Capability::Unsupported,
				custom_extensions: Capability::ImportOnly,
			},
			stability: PluginStability::Experimental,
			provider_plugin_id: None,
		}
	}

	fn probe(&self, input: &ImportProbe) -> ImportProbeResult {
		let Some(p) = input.path_hint.as_ref() else {
			return ImportProbeResult { confidence: 0 };
		};
		let s = p.as_os_str().to_string_lossy().to_lowercase();
		if s.ends_with(".vrm") {
			return ImportProbeResult { confidence: 255 };
		}
		if s.ends_with(".glb") {
			if let Some(bytes) = input.bytes.as_deref() {
				if bytes.len() > 128 * 1024 * 1024 {
					return ImportProbeResult { confidence: 0 };
				}
				if likely_vrm_binary(bytes) {
					return ImportProbeResult { confidence: 255 };
				}
				return ImportProbeResult { confidence: 0 };
			}
			let Ok(bytes) = std::fs::read(p) else {
				return ImportProbeResult { confidence: 0 };
			};
			if bytes.len() > 128 * 1024 * 1024 {
				return ImportProbeResult { confidence: 0 };
			}
			if likely_vrm_binary(&bytes) {
				return ImportProbeResult { confidence: 255 };
			}
		}
		ImportProbeResult { confidence: 0 }
	}

	fn import(&self, _ctx: &mut ImportContext, input: ImportInput, _options: ImportOptions) -> Result<ImportResult, ImportError> {
		match input {
			ImportInput::Path(path) => {
				let bytes = std::fs::read(&path).map_err(|e| ImportError::Message(format!("読み込み: {e}")))?;
				import_vrm_from_parts(Some(&path), &bytes, None)
			}
			ImportInput::Bytes { bytes, path_hint } => import_vrm_from_parts(path_hint.as_deref(), bytes.as_ref(), None),
		}
	}
}

pub fn register_vrm_importer(registry: &mut IoRegistry) {
	registry.register_importer(Box::new(VrmImporter));
}

/// バイト列から VRM 拡張の有無を軽く判定（`true` なら [`VrmImporter`] が受け持てる可能性が高い）。
pub fn likely_vrm_binary(bytes: &[u8]) -> bool {
	gltf_root_json_from_bytes(bytes)
		.ok()
		.and_then(|root| take_vrm_extension(&root).ok())
		.is_some()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn mtoon_materials_v0_uses_array_index_without_material_field() {
		let vrm = serde_json::json!({
			"materialProperties": [
				{
					"shader": "VRM/MToon",
					"floatProperties": { "_BlendMode": 0.0 }
				},
				{
					"shader": "VRM/MToon",
					"floatProperties": { "_BlendMode": 1.0, "_Cutoff": 0.3 }
				}
			]
		});
		let list = super::mtoon_materials_v0(&vrm);
		assert_eq!(list.len(), 2);
		assert_eq!(list[0].material_index, 0);
		assert_eq!(list[1].material_index, 1);
	}

	#[test]
	fn default_vrm_shading_maps_lit_unlit_and_keeps_mtoon() {
		use un_avatar_core::UnaMaterialPbr;
		let mut scene = UnaSceneSnapshot {
			materials: vec![
				UnaMaterialPbr {
					shading: UnaShadingModel::LitLambert,
					..Default::default()
				},
				UnaMaterialPbr {
					shading: UnaShadingModel::Unlit,
					..Default::default()
				},
				UnaMaterialPbr {
					shading: UnaShadingModel::MToonLike,
					..Default::default()
				},
			],
			..Default::default()
		};
		super::default_vrm_shading_to_mtoon_like(&mut scene);
		assert_eq!(scene.materials[0].shading, UnaShadingModel::MToonLike);
		assert_eq!(scene.materials[1].shading, UnaShadingModel::MToonLike);
		assert_eq!(scene.materials[2].shading, UnaShadingModel::MToonLike);
	}

	#[test]
	fn vrm0_shading_shift_toony_is_converted_to_vrm1_transition_interval() {
		let (shift, toony) = vrm0_shading_to_mtoon1_shift_toony(-0.8, 0.9);
		assert!((shift - 0.71).abs() < 1e-5);
		assert!((toony - 0.91).abs() < 1e-5);

		let (shift, toony) = vrm0_shading_to_mtoon1_shift_toony(0.1, 0.777778);
		assert!((shift + 0.2000001).abs() < 1e-4);
		assert!((toony - 0.9000001).abs() < 1e-4);
	}

	#[test]
	fn vrm0_indirect_light_is_converted_to_vrm1_gi_equalization() {
		assert!((vrm0_indirect_light_to_mtoon1_gi_equalization(0.1) - 0.9).abs() < 1e-6);
		assert_eq!(vrm0_indirect_light_to_mtoon1_gi_equalization(-1.0), 1.0);
		assert_eq!(vrm0_indirect_light_to_mtoon1_gi_equalization(2.0), 0.0);
	}

	#[test]
	fn vrm0_basis_normalization_rotates_roots_to_una_positive_z_front() {
		let mut scene = UnaSceneSnapshot {
			nodes: vec![un_avatar_core::UnaSceneNode {
				source_node_id: None,
				name: None,
				visible: true,
				transform: Mat4::IDENTITY.to_cols_array(),
				children: vec![],
				mesh: None,
				skin: None,
				probe_anchor_node: None,
				local_bounds: None,
			}],
			roots: vec![0],
			..Default::default()
		};

		normalize_scene_basis_for_vrm(&mut scene, VrmFlavor::Vrm0);

		let applied = Mat4::from_cols_array(&scene.nodes[0].transform);
		let expected = Mat4::from_rotation_y(std::f32::consts::PI);
		assert!(applied.abs_diff_eq(expected, 1e-5));
	}

	#[test]
	fn tag_mtoon_vrm0_cutout_sets_alpha_mask() {
		use std::collections::BTreeMap;

		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr};
		let mut scene_materials = vec![UnaMaterialPbr::default()];
		scene_materials[0].alpha_mode = UnaAlphaMode::Opaque;
		let mut scene = UnaSceneSnapshot {
			materials: scene_materials,
			..Default::default()
		};
		let vrm = UnaVrmExtension {
			spec_version: "0.0".into(),
			meta: Value::Null,
			humanoid_bones: BTreeMap::new(),
			mtoon_materials_v0: vec![UnaVrm0MtoonMaterialEntry {
				material_index: 0,
				shader_name: "VRM/MToon".into(),
				raw: serde_json::json!({
					"floatProperties": { "_BlendMode": 1, "_Cutoff": 0.42 }
				}),
			}],
			mtoon_material_indices_v1: vec![],
			source: Value::Null,
		};
		tag_mtoon_materials(&mut scene, &vrm);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Mask);
		assert!((scene.materials[0].alpha_cutoff - 0.42).abs() < 1e-5);
		assert_eq!(scene.materials[0].shading, UnaShadingModel::MToonLike);
	}

	#[test]
	fn tag_mtoon_vrm0_eye_area_cutout_keeps_authored_cutoff() {
		// 履歴: 以前は「眼睑（まぶた）」等の eye-area マテリアルで alpha_cutoff を
		// `EYE_AREA_CUTOUT_ALPHA_CUTOFF_MAX = 0.05` まで強制的に下げる処理が入っていたが、
		// Metasequoia での実測（cutoff=0.5 で正しい描画、cutoff=0.1 で UN Avatar の不具合再現）
		// から、これは VRM/MToon spec から逸脱して逆に肌色寄りの太いリングを生む原因と判明。
		// 撤廃して `_Cutoff` の値（指定なしは 0.5）をそのまま使う。
		use std::collections::BTreeMap;

		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr {
				name: Some("眼睑".into()),
				alpha_mode: UnaAlphaMode::Opaque,
				..Default::default()
			}],
			..Default::default()
		};
		let vrm = UnaVrmExtension {
			spec_version: "0.0".into(),
			meta: Value::Null,
			humanoid_bones: BTreeMap::new(),
			mtoon_materials_v0: vec![UnaVrm0MtoonMaterialEntry {
				material_index: 0,
				shader_name: "VRM/MToon".into(),
				raw: serde_json::json!({
					"name": "眼睑",
					"floatProperties": { "_BlendMode": 1, "_Cutoff": 0.5 }
				}),
			}],
			mtoon_material_indices_v1: vec![],
			source: Value::Null,
		};
		tag_mtoon_materials(&mut scene, &vrm);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Mask);
		assert!(
			(scene.materials[0].alpha_cutoff - 0.5).abs() < 1e-5,
			"alpha_cutoff should keep the authored _Cutoff = 0.5 (got {})",
			scene.materials[0].alpha_cutoff
		);
		assert_eq!(scene.materials[0].shading, UnaShadingModel::MToonLike);
	}

	#[test]
	fn tag_mtoon_vrm0_eye_area_cutout_keeps_authored_cutoff_even_when_material_name_is_garbled() {
		// 文字化け対策: glTF material.name が壊れていても raw の "name" から eye-area を
		// 認識する経路を維持（shade_multiply_texture_index 補完など別目的で利用）。
		// 撤廃した cutoff 引き下げは適用しないので、_Cutoff の値（0.5）がそのまま入る。
		use std::collections::BTreeMap;

		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr {
				name: Some("逵ｼ逹".into()),
				alpha_mode: UnaAlphaMode::Opaque,
				..Default::default()
			}],
			..Default::default()
		};
		let vrm = UnaVrmExtension {
			spec_version: "0.0".into(),
			meta: Value::Null,
			humanoid_bones: BTreeMap::new(),
			mtoon_materials_v0: vec![UnaVrm0MtoonMaterialEntry {
				material_index: 0,
				shader_name: "VRM/MToon".into(),
				raw: serde_json::json!({
					"name": "眼睑",
					"floatProperties": { "_BlendMode": 1, "_Cutoff": 0.5 }
				}),
			}],
			mtoon_material_indices_v1: vec![],
			source: Value::Null,
		};
		tag_mtoon_materials(&mut scene, &vrm);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Mask);
		assert!(
			(scene.materials[0].alpha_cutoff - 0.5).abs() < 1e-5,
			"alpha_cutoff should keep the authored _Cutoff = 0.5 (got {})",
			scene.materials[0].alpha_cutoff
		);
	}

	#[test]
	fn parse_mtoon_vrm0_eye_area_without_shade_texture_uses_main_texture_for_shade() {
		let mtoon = parse_mtoon_v0(
			&serde_json::json!({
				"name": "眼睑",
				"textureProperties": { "_MainTex": 28 },
				"vectorProperties": { "_ShadeColor": [1.0, 1.0, 1.0, 1.0] },
				"floatProperties": { "_BlendMode": 1.0 }
			}),
			&Value::Null,
		);

		assert_eq!(mtoon.shade_multiply_texture_index, Some(28));
	}

	#[test]
	fn parse_mtoon_vrm0_texture_properties_map_texture_indices_to_image_sources() {
		let root = serde_json::json!({
			"textures": [
				{ "source": 10 },
				{ "source": 11 },
				{ "source": 12 }
			]
		});
		let mtoon = parse_mtoon_v0(
			&serde_json::json!({
				"textureProperties": {
					"_ShadeTexture": 0,
					"_SphereAdd": 1,
					"_RimTexture": 2
				}
			}),
			&root,
		);

		assert_eq!(mtoon.shade_multiply_texture_index, Some(10));
		assert_eq!(mtoon.matcap_texture_index, Some(11));
		assert_eq!(mtoon.rim_multiply_texture_index, Some(12));
	}

	#[test]
	fn parse_mtoon_vrm0_authored_rim_color_is_srgb_decoded() {
		let mtoon = parse_mtoon_v0(
			&serde_json::json!({
				"vectorProperties": {
					"_ShadeColor": [0.372549, 0.5, 1.0, 1.0],
					"_RimColor": [0.25, 0.25, 0.25, 1.0],
					"_OutlineColor": [0.5, 0.5, 0.5, 1.0]
				}
			}),
			&Value::Null,
		);

		assert!((mtoon.shade_color_factor[0] - 0.372549).abs() < 0.00001);
		assert!((mtoon.shade_color_factor[1] - 0.5).abs() < 0.00001);
		assert!((mtoon.shade_color_factor[2] - 1.0).abs() < 0.00001);
		assert!((mtoon.parametric_rim_color_factor[0] - 0.050876).abs() < 0.00001);
		assert!((mtoon.outline_color_factor[0] - 0.5).abs() < 0.00001);
	}

	#[test]
	fn tag_mtoon_vrm0_blendmode_zero_overrides_gltf_mask() {
		use std::collections::BTreeMap;

		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr {
				alpha_mode: UnaAlphaMode::Mask,
				alpha_cutoff: 0.5,
				..Default::default()
			}],
			..Default::default()
		};
		let vrm = UnaVrmExtension {
			spec_version: "0.0".into(),
			meta: Value::Null,
			humanoid_bones: BTreeMap::new(),
			mtoon_materials_v0: vec![UnaVrm0MtoonMaterialEntry {
				material_index: 0,
				shader_name: "VRM/MToon".into(),
				raw: serde_json::json!({
					"floatProperties": { "_BlendMode": 0.0 }
				}),
			}],
			mtoon_material_indices_v1: vec![],
			source: Value::Null,
		};
		tag_mtoon_materials(&mut scene, &vrm);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Opaque);
	}

	#[test]
	fn tag_mtoon_vrm0_cull_mode_overrides_gltf_double_sided() {
		use std::collections::BTreeMap;

		use un_avatar_core::UnaMaterialPbr;
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr {
				double_sided: true,
				..Default::default()
			}],
			..Default::default()
		};
		let vrm = UnaVrmExtension {
			spec_version: "0.0".into(),
			meta: Value::Null,
			humanoid_bones: BTreeMap::new(),
			mtoon_materials_v0: vec![UnaVrm0MtoonMaterialEntry {
				material_index: 0,
				shader_name: "VRM/MToon".into(),
				raw: serde_json::json!({
					"floatProperties": { "_CullMode": 2.0 }
				}),
			}],
			mtoon_material_indices_v1: vec![],
			source: Value::Null,
		};
		tag_mtoon_materials(&mut scene, &vrm);
		assert!(!scene.materials[0].double_sided);
		assert_eq!(scene.materials[0].cull_mode, UnaCullMode::Back);
	}

	#[test]
	fn tag_mtoon_vrm0_missing_blendmode_key_defaults_opaque() {
		use std::collections::BTreeMap;

		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr {
				alpha_mode: UnaAlphaMode::Mask,
				..Default::default()
			}],
			..Default::default()
		};
		let vrm = UnaVrmExtension {
			spec_version: "0.0".into(),
			meta: Value::Null,
			humanoid_bones: BTreeMap::new(),
			mtoon_materials_v0: vec![UnaVrm0MtoonMaterialEntry {
				material_index: 0,
				shader_name: "VRM/MToon".into(),
				raw: serde_json::json!({ "floatProperties": {} }),
			}],
			mtoon_material_indices_v1: vec![],
			source: Value::Null,
		};
		tag_mtoon_materials(&mut scene, &vrm);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Opaque);
	}

	#[test]
	fn tag_mtoon_vrm0_transparent_sets_blend() {
		use std::collections::BTreeMap;

		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr::default()],
			..Default::default()
		};
		let vrm = UnaVrmExtension {
			spec_version: "0.0".into(),
			meta: Value::Null,
			humanoid_bones: BTreeMap::new(),
			mtoon_materials_v0: vec![UnaVrm0MtoonMaterialEntry {
				material_index: 0,
				shader_name: "VRM/MToon".into(),
				raw: serde_json::json!({
					"floatProperties": { "_BlendMode": 2 }
				}),
			}],
			mtoon_material_indices_v1: vec![],
			source: Value::Null,
		};
		tag_mtoon_materials(&mut scene, &vrm);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Blend);
	}

	#[test]
	fn relax_eye_mtoon_mask_to_opaque_by_name() {
		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr, UnaShadingModel};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr {
				name: Some("Eye_Iris".into()),
				shading: UnaShadingModel::MToonLike,
				alpha_mode: UnaAlphaMode::Mask,
				alpha_cutoff: 0.5,
				base_color_factor: [1.0, 1.0, 1.0, 0.0],
				..Default::default()
			}],
			..Default::default()
		};
		super::relax_mtoon_mask_for_likely_eye_materials(&mut scene);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Opaque);
		assert!((scene.materials[0].base_color_factor[3] - 1.0).abs() < 1e-5);
	}

	#[test]
	fn relax_chinese_eye_highlight_mtoon_mask_to_opaque_by_name() {
		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr, UnaShadingModel};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr {
				name: Some("眼睛高光".into()),
				shading: UnaShadingModel::MToonLike,
				alpha_mode: UnaAlphaMode::Mask,
				alpha_cutoff: 0.5,
				base_color_factor: [1.0, 1.0, 1.0, 0.0],
				..Default::default()
			}],
			..Default::default()
		};
		super::relax_mtoon_mask_for_likely_eye_materials(&mut scene);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Opaque);
		assert!((scene.materials[0].base_color_factor[3] - 1.0).abs() < 1e-5);
	}

	#[test]
	fn keeps_chinese_eye_highlight_mask_when_material_alpha_is_visible() {
		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr, UnaShadingModel};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr {
				name: Some("眼睛高光".into()),
				shading: UnaShadingModel::MToonLike,
				alpha_mode: UnaAlphaMode::Mask,
				alpha_cutoff: 0.5,
				base_color_factor: [1.0, 1.0, 1.0, 1.0],
				..Default::default()
			}],
			..Default::default()
		};
		super::relax_mtoon_mask_for_likely_eye_materials(&mut scene);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Mask);
	}

	#[test]
	fn keeps_chinese_eyelid_mask_as_mask() {
		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr, UnaShadingModel};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr {
				name: Some("眼睑".into()),
				shading: UnaShadingModel::MToonLike,
				alpha_mode: UnaAlphaMode::Mask,
				..Default::default()
			}],
			..Default::default()
		};
		super::relax_mtoon_mask_for_likely_eye_materials(&mut scene);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Mask);
	}

	#[test]
	fn tag_mtoon_vrm0_keyword_transparent_sets_blend() {
		use std::collections::BTreeMap;

		use un_avatar_core::{UnaAlphaMode, UnaMaterialPbr};
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr::default()],
			..Default::default()
		};
		let vrm = UnaVrmExtension {
			spec_version: "0.0".into(),
			meta: Value::Null,
			humanoid_bones: BTreeMap::new(),
			mtoon_materials_v0: vec![UnaVrm0MtoonMaterialEntry {
				material_index: 0,
				shader_name: "VRM/MToon".into(),
				raw: serde_json::json!({
					"floatProperties": { "_BlendMode": 0 },
					"keywordMap": { "MTOON_RENDERING_TRANSPARENT": true }
				}),
			}],
			mtoon_material_indices_v1: vec![],
			source: Value::Null,
		};
		tag_mtoon_materials(&mut scene, &vrm);
		assert_eq!(scene.materials[0].alpha_mode, UnaAlphaMode::Blend);
	}

	#[test]
	fn parses_vrm0_outline_width_in_renderer_meters() {
		let raw = serde_json::json!({
			"floatProperties": {
				"_OutlineWidthMode": 1,
				"_OutlineWidth": 0.08
			}
		});
		let mtoon = parse_mtoon_v0(&raw, &Value::Null);
		assert_eq!(mtoon.outline_width_mode, UnaMtoonOutlineWidthMode::WorldCoordinates);
		assert!((mtoon.outline_width_factor - 0.0008).abs() < 1e-7);
	}

	#[test]
	fn parses_vrm0_humanoid_object() {
		let vrm = serde_json::json!({
			"specVersion": "0.0",
			"meta": {"title": "t"},
			"humanoid": {
				"humanBones": {
					"hips": { "node": 1 },
					"leftUpperLeg": { "node": 2 }
				}
			}
		});
		let root = serde_json::json!({ "extensions": { "VRM": vrm } });
		let (_f, raw) = take_vrm_extension(&root).unwrap();
		let ext = build_vrm_extension(&root, VrmFlavor::Vrm0, raw).unwrap();
		assert_eq!(ext.humanoid_bones.get("hips"), Some(&1usize));
		assert_eq!(ext.humanoid_bones.get("leftupperleg"), Some(&2usize));
	}

	#[test]
	fn parses_vrm0_humanoid_array_like_vrm1() {
		let vrm = serde_json::json!({
			"specVersion": "0.0",
			"meta": {"title": "t"},
			"humanoid": {
				"humanBones": [
					{ "bone": "hips", "node": 1 },
					{ "name": "leftUpperLeg", "node": 2 }
				]
			}
		});
		let root = serde_json::json!({ "extensions": { "VRM": vrm } });
		let (_f, raw) = take_vrm_extension(&root).unwrap();
		let ext = build_vrm_extension(&root, VrmFlavor::Vrm0, raw).unwrap();
		assert_eq!(ext.humanoid_bones.get("hips"), Some(&1usize));
		assert_eq!(ext.humanoid_bones.get("leftupperleg"), Some(&2usize));
	}

	#[test]
	fn parses_vrm0_humanoid_finger_bones() {
		let vrm = serde_json::json!({
			"specVersion": "0.0",
			"meta": {"title": "t"},
			"humanoid": {
				"humanBones": {
					"rightIndexProximal": { "node": 10 },
					"rightIndexIntermediate": { "node": 11 },
					"rightIndexDistal": { "node": 12 },
					"leftThumbIntermediate": { "node": 21 }
				}
			}
		});
		let root = serde_json::json!({ "extensions": { "VRM": vrm } });
		let (_f, raw) = take_vrm_extension(&root).unwrap();
		let ext = build_vrm_extension(&root, VrmFlavor::Vrm0, raw).unwrap();
		assert_eq!(ext.humanoid_bones.get("rightindexproximal"), Some(&10usize));
		assert_eq!(ext.humanoid_bones.get("rightindexintermediate"), Some(&11usize));
		assert_eq!(ext.humanoid_bones.get("rightindexdistal"), Some(&12usize));
		assert_eq!(ext.humanoid_bones.get("leftthumbintermediate"), Some(&21usize));
	}

	#[test]
	fn parses_vrm0_expression_presets() {
		let vrm = serde_json::json!({
			"blendShapeMaster": {
				"blendShapeGroups": [{
					"name": "blink",
					"presetName": "Blink",
					"values": [{ "mesh": 0, "index": 2, "weight": 1.0 }]
				}]
			}
		});
		let cat = expression_catalog_v0(&vrm);
		assert_eq!(cat.presets.len(), 1);
		assert_eq!(cat.presets[0].name, "Blink");
		assert_eq!(cat.presets[0].binds[0].mesh_index, 0);
		assert_eq!(cat.presets[0].binds[0].morph_target_index, 2);
	}

	#[test]
	fn vrm0_perfect_sync_uses_name_when_presetname_is_unknown() {
		// VRM0 の PerfectSync 対応モデルでは ARKit 52 個ぶんを `presetName = "unknown"`,
		// `name = "MouthSmileLeft"` のような構造で登録するのが慣例。
		// presetName を素朴に優先すると 52 件が全部同名 "unknown" になり VMC マッチも失敗するため、
		// presetName が unknown/空の場合は name にフォールバックする必要がある。
		let vrm = serde_json::json!({
			"blendShapeMaster": {
				"blendShapeGroups": [
					{ "name": "Joy", "presetName": "joy", "binds": [{ "mesh": 0, "index": 1, "weight": 100 }] },
					{ "name": "MouthSmileLeft", "presetName": "unknown", "binds": [{ "mesh": 0, "index": 2, "weight": 100 }] },
					{ "name": "BrowInnerUp", "presetName": "Unknown", "binds": [{ "mesh": 0, "index": 3, "weight": 100 }] },
					{ "name": "EyeBlinkLeft", "binds": [{ "mesh": 0, "index": 4, "weight": 100 }] }
				]
			}
		});
		let cat = expression_catalog_v0(&vrm);
		let names: Vec<_> = cat.presets.iter().map(|p| p.name.as_str()).collect();
		assert!(names.contains(&"joy"));
		assert!(names.contains(&"MouthSmileLeft"));
		assert!(names.contains(&"BrowInnerUp"));
		assert!(names.contains(&"EyeBlinkLeft"));
		// "unknown" が preset 名として残らないことを保証
		assert!(!names.contains(&"unknown"));
	}

	#[test]
	fn parses_vrm0_expression_binds_and_normalizes_percent_weight() {
		let vrm = serde_json::json!({
			"blendShapeMaster": {
				"blendShapeGroups": [{
					"name": "blink",
					"presetName": "blink",
					"binds": [{ "mesh": 8, "index": 88, "weight": 100 }]
				}]
			}
		});
		let cat = expression_catalog_v0(&vrm);
		assert_eq!(cat.presets.len(), 1);
		assert_eq!(cat.presets[0].name, "blink");
		assert_eq!(cat.presets[0].binds[0].mesh_index, 8);
		assert_eq!(cat.presets[0].binds[0].morph_target_index, 88);
		assert!((cat.presets[0].binds[0].weight_scale - 1.0).abs() < 1e-5);
	}

	#[test]
	fn parses_vrm1_humanoid_array() {
		let vrm = serde_json::json!({
			"specVersion": "1.0",
			"meta": { "name": "a" },
			"humanoid": {
				"humanBones": [
					{ "bone": "hips", "node": 5 },
					{ "bone": "head", "node": 7 }
				]
			}
		});
		let root = serde_json::json!({
			"extensions": { "VRMC_vrm": vrm },
			"materials": [
				{},
				{ "extensions": { "VRMC_materials_mtoon": { "shadeMultiplyTexture": null } } }
			]
		});
		let (_f, raw) = take_vrm_extension(&root).unwrap();
		let ext = build_vrm_extension(&root, VrmFlavor::Vrm1, raw).unwrap();
		assert_eq!(ext.humanoid_bones.get("hips"), Some(&5usize));
		assert_eq!(ext.mtoon_material_indices_v1, vec![1]);
	}

	#[test]
	fn parses_vrm1_humanoid_object() {
		let vrm = serde_json::json!({
			"specVersion": "1.0",
			"meta": { "name": "a" },
			"humanoid": {
				"humanBones": {
					"hips": { "node": 5 },
					"leftEye": { "node": 26 }
				}
			}
		});
		let root = serde_json::json!({ "extensions": { "VRMC_vrm": vrm } });
		let (_f, raw) = take_vrm_extension(&root).unwrap();
		let ext = build_vrm_extension(&root, VrmFlavor::Vrm1, raw).unwrap();
		assert_eq!(ext.humanoid_bones.get("hips"), Some(&5usize));
		assert_eq!(ext.humanoid_bones.get("lefteye"), Some(&26usize));
	}

	#[test]
	fn parses_vrm1_humanoid_finger_bones() {
		let vrm = serde_json::json!({
			"specVersion": "1.0",
			"meta": { "name": "a" },
			"humanoid": {
				"humanBones": {
					"rightIndexProximal": { "node": 10 },
					"rightIndexIntermediate": { "node": 11 },
					"rightIndexDistal": { "node": 12 },
					"leftThumbIntermediate": { "node": 21 }
				}
			}
		});
		let root = serde_json::json!({ "extensions": { "VRMC_vrm": vrm } });
		let (_f, raw) = take_vrm_extension(&root).unwrap();
		let ext = build_vrm_extension(&root, VrmFlavor::Vrm1, raw).unwrap();
		assert_eq!(ext.humanoid_bones.get("rightindexproximal"), Some(&10usize));
		assert_eq!(ext.humanoid_bones.get("rightindexintermediate"), Some(&11usize));
		assert_eq!(ext.humanoid_bones.get("rightindexdistal"), Some(&12usize));
		assert_eq!(ext.humanoid_bones.get("leftthumbintermediate"), Some(&21usize));
	}

	#[test]
	fn parses_vrm1_node_constraints() {
		let root = serde_json::json!({
			"nodes": [
				{},
				{},
				{
					"extensions": {
						"VRMC_node_constraint": {
							"specVersion": "1.0",
							"constraint": {
								"roll": { "source": 1, "rollAxis": "X", "weight": 0.5 }
							}
						}
					}
				},
				{
					"extensions": {
						"VRMC_node_constraint": {
							"specVersion": "1.0",
							"constraint": {
								"aim": { "source": 1, "aimAxis": "NegativeY" }
							}
						}
					}
				}
			]
		});
		let constraints = node_constraints_from_root(&root);
		assert_eq!(constraints.len(), 2);
		assert_eq!(constraints[0].target_node, 2);
		assert_eq!(constraints[0].source_node, 1);
		assert!((constraints[0].weight - 0.5).abs() < 1e-5);
		assert!(matches!(
			constraints[0].kind,
			UnaNodeConstraintKind::Roll {
				axis: UnaNodeConstraintAxis::X
			}
		));
		assert!(matches!(
			constraints[1].kind,
			UnaNodeConstraintKind::Aim {
				axis: UnaNodeConstraintAimAxis::NegativeY
			}
		));
	}

	#[test]
	fn tag_mtoon_vrm1_material_index_sets_mtoon_like() {
		use std::collections::BTreeMap;

		use un_avatar_core::UnaMaterialPbr;
		let mut scene = UnaSceneSnapshot {
			materials: vec![UnaMaterialPbr::default(), UnaMaterialPbr::default()],
			..Default::default()
		};
		let vrm = UnaVrmExtension {
			spec_version: "1.0".into(),
			meta: Value::Null,
			humanoid_bones: BTreeMap::new(),
			mtoon_materials_v0: vec![],
			mtoon_material_indices_v1: vec![1],
			source: Value::Null,
		};

		tag_mtoon_materials(&mut scene, &vrm);

		assert_eq!(scene.materials[0].shading, UnaShadingModel::LitLambert);
		assert_eq!(scene.materials[1].shading, UnaShadingModel::MToonLike);
	}

	#[test]
	fn parses_vrm1_expression_morph_binds() {
		let vrm = serde_json::json!({
			"specVersion": "1.0",
			"expressions": {
				"preset": {
					"happy": {
						"morphTargetBinds": [
							{ "node": 1, "index": 0, "weight": 1.0 }
						]
					}
				}
			}
		});
		let mut node_mesh: Vec<Option<usize>> = vec![None; 4];
		node_mesh[1] = Some(3);
		let cat = expression_catalog_v1(&vrm, &node_mesh);
		assert_eq!(cat.presets.len(), 1);
		assert_eq!(cat.presets[0].name, "happy");
		assert_eq!(cat.presets[0].binds[0].mesh_index, 3);
		assert_eq!(cat.presets[0].binds[0].morph_target_index, 0);
	}

	#[test]
	fn parses_vrm0_secondary_animation_expands_each_root_via_children() {
		// 実モデル相当: `bones: [10, 20]` は 2 つの独立したチェーンの root を指す。
		// チェーンは gltf nodes[*].children を辿って構築される。
		let vrm = serde_json::json!({
			"secondaryAnimation": {
				"boneGroups": [{
					"comment": "hair",
					"stiffiness": 1.2,
					"gravityPower": 0.5,
					"gravityDir": { "x": 0.0, "y": -1.0, "z": 0.0 },
					"dragForce": 0.3,
					"center": -1,
					"hitRadius": 0.03,
					"bones": [10, 20]
				}]
			}
		});
		let root = serde_json::json!({
			"nodes": [
				{}, {}, {}, {}, {}, {}, {}, {}, {}, {},
				{ "children": [11] },    // 10 -> 11
				{ "children": [12] },    // 11 -> 12
				{},                       // 12 leaf
				{}, {}, {}, {}, {}, {}, {},
				{ "children": [21] },    // 20 -> 21
				{ "children": [22] },    // 21 -> 22
				{}                        // 22 leaf
			]
		});
		let sb = extract_spring_bones(&root, &vrm, VrmFlavor::Vrm0).expect("spring");
		assert_eq!(sb.groups.len(), 2, "each root expands to its own chain");
		assert_eq!(sb.groups[0].bone_node_indices, vec![10, 11, 12]);
		assert_eq!(sb.groups[1].bone_node_indices, vec![20, 21, 22]);
		assert!((sb.groups[0].stiffness - 1.2).abs() < 1e-5);
		assert!((sb.groups[1].stiffness - 1.2).abs() < 1e-5);
	}

	#[test]
	fn parses_vrm0_single_root_still_expands_chain() {
		// `bones: [10]` の単一 root も子をたどって長さ >= 2 のチェーンになる。
		// 旧実装ではここで `len() < 2` でスキップされ「全く揺れない」原因になっていた。
		let vrm = serde_json::json!({
			"secondaryAnimation": {
				"boneGroups": [{
					"comment": "tail",
					"stiffiness": 0.5,
					"gravityPower": 0.01,
					"gravityDir": { "x": 0.0, "y": -1.0, "z": 0.0 },
					"dragForce": 0.69,
					"center": -1,
					"hitRadius": 0.02,
					"bones": [10]
				}]
			}
		});
		let root = serde_json::json!({
			"nodes": [
				{}, {}, {}, {}, {}, {}, {}, {}, {}, {},
				{ "children": [11] },
				{ "children": [12] },
				{}
			]
		});
		let sb = extract_spring_bones(&root, &vrm, VrmFlavor::Vrm0).expect("spring");
		assert_eq!(sb.groups.len(), 1);
		assert_eq!(sb.groups[0].bone_node_indices, vec![10, 11, 12]);
	}

	#[test]
	fn flat_neutral_normal_textures_are_dropped() {
		use un_avatar_core::UnaMaterialPbr;
		let mut scene = UnaSceneSnapshot {
			meshes: vec![],
			materials: vec![UnaMaterialPbr {
				normal_texture_index: Some(0),
				normal_texture_scale: 1.0,
				..Default::default()
			}],
			images: vec![UnaImageRgba {
				width: 2,
				height: 1,
				pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
				pixels: vec![127, 127, 255, 255, 128, 128, 255, 255],
			}],
			image_sources: vec![],
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
		};
		drop_flat_neutral_normal_textures(&mut scene);
		assert_eq!(scene.materials[0].normal_texture_index, None);
		assert_eq!(scene.materials[0].normal_texture_scale, 0.0);
	}

	#[test]
	fn expands_expression_binds_across_mesh_primitives() {
		use un_avatar_core::{UnaMeshBuffers, UnaMorphTargetDeltas};
		let prim_with_4 = || UnaMeshBuffers {
			name: None,
			positions: vec![],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: None,
			morph_targets: vec![
				UnaMorphTargetDeltas {
					position_deltas: vec![],
					normal_deltas: None,
				};
				4
			],
			morph_target_names: vec![],
			default_morph_weights: vec![],
		};
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![prim_with_4(), prim_with_4()]],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
		};
		let mut cat = UnaExpressionCatalog {
			presets: vec![UnaExpressionPreset {
				name: "a".into(),
				binds: vec![UnaMorphTargetBind {
					mesh_index: 0,
					primitive_index: 0,
					morph_target_index: 2,
					weight_scale: 1.0,
				}],
			}],
		};
		expand_expression_binds_per_primitive(&scene, &mut cat);
		assert_eq!(cat.presets[0].binds.len(), 2);
		assert!(cat.presets[0]
			.binds
			.iter()
			.any(|b| b.primitive_index == 0 && b.morph_target_index == 2));
		assert!(cat.presets[0]
			.binds
			.iter()
			.any(|b| b.primitive_index == 1 && b.morph_target_index == 2));
	}

	#[test]
	fn expands_expression_binds_only_when_morph_target_counts_match() {
		use un_avatar_core::{UnaMeshBuffers, UnaMorphTargetDeltas};
		let prim_n = |n: usize| UnaMeshBuffers {
			name: None,
			positions: vec![],
			normals: None,
			tangents: None,
			tex_coords_0: None,
			tex_coords_1: None,
			tex_coords_2: None,
			tex_coords_3: None,
			colors_0: None,
			joints: None,
			weights: None,
			indices: None,
			material_index: None,
			morph_targets: vec![
				UnaMorphTargetDeltas {
					position_deltas: vec![],
					normal_deltas: None,
				};
				n
			],
			morph_target_names: vec![],
			default_morph_weights: vec![],
		};
		let scene = UnaSceneSnapshot {
			meshes: vec![vec![prim_n(4), prim_n(2)]],
			materials: vec![],
			images: vec![],
			image_sources: vec![],
			skins: vec![],
			nodes: vec![],
			roots: vec![],
			node_constraints: vec![],
		};
		let mut cat = UnaExpressionCatalog {
			presets: vec![UnaExpressionPreset {
				name: "a".into(),
				binds: vec![UnaMorphTargetBind {
					mesh_index: 0,
					primitive_index: 0,
					morph_target_index: 2,
					weight_scale: 1.0,
				}],
			}],
		};
		expand_expression_binds_per_primitive(&scene, &mut cat);
		assert_eq!(cat.presets[0].binds.len(), 1);
		assert_eq!(cat.presets[0].binds[0].primitive_index, 0);
	}
}
