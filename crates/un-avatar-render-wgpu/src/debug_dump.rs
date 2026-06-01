//! 切り分け用: 目周りマテリアル・スキン関係を標準エラーへ出す。

use serde_json::Value;
use un_avatar_core::UnaDocument;

fn json_number_f(v: &Value) -> Option<f32> {
	v.as_f64().map(|x| x as f32).or_else(|| v.as_u64().map(|u| u as f32))
}

/// `--debug-material-dump` 用: スキン本数と、目周り候補マテリアルの UNA / VRM 生情報。
pub fn log_material_skin_report(doc: &UnaDocument) {
	let Some(sc) = &doc.scene else {
		eprintln!("[debug-material] scene なし");
		return;
	};
	eprintln!("[debug-material] GPU bone palette 上限 = 512（shader storage と一致）");
	for (i, sk) in sc.skins.iter().enumerate() {
		let n = sk.joint_nodes.len();
		let warn = if n > 512 {
			"  WARNING: joint 数 > 512 は切り詰め"
		} else {
			""
		};
		eprintln!("[debug-material] skin[{i}]: joints.len() = {n}{warn}");
	}

	let mtoon_by_mat: Vec<Option<&serde_json::Map<String, Value>>> = doc
		.vrm
		.as_ref()
		.map(|v| {
			let mut slots: Vec<Option<&serde_json::Map<String, Value>>> = vec![None; sc.materials.len()];
			for e in &v.mtoon_materials_v0 {
				if e.material_index < slots.len() {
					if let Some(obj) = e.raw.as_object() {
						slots[e.material_index] = Some(obj);
					}
				}
			}
			slots
		})
		.unwrap_or_else(|| vec![None; sc.materials.len()]);

	for (mi, mat) in sc.materials.iter().enumerate() {
		eprintln!(
			"[debug-material] materials[{mi}] name={:?} shading={:?} alpha_mode={:?} alpha_cutoff={} cull={:?} double_sided={} base_color_factor.a={}",
			mat.name,
			mat.shading,
			mat.alpha_mode,
			mat.alpha_cutoff,
			mat.cull_mode,
			mat.double_sided,
			mat.base_color_factor[3],
		);
		if let Some(unavatar) = mat.unavatar_material.as_ref().and_then(|v| v.as_object()) {
			let source_shader = unavatar.get("sourceShader").and_then(Value::as_str);
			let family = unavatar.get("family").and_then(Value::as_str);
			let render_queue = unavatar.get("renderQueue").and_then(json_number_f);
			eprintln!(
				"  UN_avatar_material: sourceShader={:?} family={:?} renderQueue={:?}",
				source_shader, family, render_queue
			);
			if let Some(fp) = unavatar.get("floatParams").and_then(Value::as_object) {
				let cutoff = fp.get("_Cutoff").or_else(|| fp.get("_AlphaCutoff")).and_then(json_number_f);
				let cull = fp.get("_Cull").or_else(|| fp.get("_CullMode")).and_then(json_number_f);
				let zwrite = fp.get("_ZWrite").or_else(|| fp.get("_ZWriteMode")).and_then(json_number_f);
				let use_shadow = fp.get("_UseShadow").and_then(json_number_f);
				let use_matcap = fp.get("_UseMatCap").and_then(json_number_f);
				let use_rim = fp.get("_UseRim").and_then(json_number_f);
				let use_emission = fp.get("_UseEmission").and_then(json_number_f);
				let outline_width = fp.get("_OutlineWidth").and_then(json_number_f);
				eprintln!(
					"  UN_avatar floatParams: _Cutoff={:?} _Cull/_CullMode={:?} _ZWrite/_ZWriteMode={:?} _UseShadow={:?} _UseMatCap={:?} _UseRim={:?} _UseEmission={:?} _OutlineWidth={:?}",
					cutoff, cull, zwrite, use_shadow, use_matcap, use_rim, use_emission, outline_width
				);
			}
		}
		if let Some(raw) = mtoon_by_mat.get(mi).copied().flatten() {
			if let Some(fp) = raw.get("floatProperties").and_then(|x| x.as_object()) {
				let bm = fp.get("_BlendMode").and_then(json_number_f);
				let cutoff = fp.get("_Cutoff").or_else(|| fp.get("_AlphaCutoff")).and_then(json_number_f);
				let cull = fp.get("_CullMode").and_then(json_number_f);
				eprintln!(
					"  VRM0 floatProperties: _BlendMode={:?} _Cutoff/_AlphaCutoff={:?} _CullMode={:?}",
					bm, cutoff, cull
				);
			}
			if let Some(km) = raw.get("keywordMap").and_then(|x| x.as_object()) {
				let keys: Vec<&String> = km.keys().collect();
				eprintln!("  VRM0 keywordMap keys: {:?}", keys);
			}
		}
	}
}

pub(crate) fn iris_like_material_name(name: Option<&str>) -> bool {
	let Some(n) = name else {
		return false;
	};
	let l = n.to_ascii_lowercase();
	if l.contains("iris")
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
		|| l.contains("高光")
		|| l.contains("ハイライト")
	{
		return true;
	}
	// 「Eye.*」系は取るが、まつげ・眉毛・アイライン名に紛れ込む "eye" は除外
	if l.contains("eye") && !l.contains("eyelash") && !l.contains("eyeline") && !l.contains("eyebrow") {
		return true;
	}
	false
}

pub(crate) fn eye_area_material_name(name: Option<&str>) -> bool {
	if iris_like_material_name(name) {
		return true;
	}
	let Some(n) = name else {
		return false;
	};
	let l = n.to_ascii_lowercase();
	l.contains("eyelid")
		|| l.contains("eyelash")
		|| l.contains("eyeline")
		|| l.contains("eyeliner")
		|| l.contains("eyebrow")
		|| l.contains("brow")
		|| l.contains("lash")
		|| l.contains("lid")
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

/// 単色デバッグ用 RGBA（アルファ 1）
pub(crate) fn debug_primitive_color_rgba(mesh_index: usize, prim_index: usize) -> [f32; 4] {
	let mut h = (mesh_index.wrapping_mul(7919) ^ prim_index.wrapping_mul(104729)) as f32;
	h = (h % 360.0 + 360.0) % 360.0;
	let s = 0.65_f32;
	let v = 0.92_f32;
	let c = v * s;
	let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
	let m = v - c;
	let (r1, g1, b1) = match (h / 60.0) as i32 {
		0 => (c, x, 0.0),
		1 => (x, c, 0.0),
		2 => (0.0, c, x),
		3 => (0.0, x, c),
		4 => (x, 0.0, c),
		_ => (c, 0.0, x),
	};
	[r1 + m, g1 + m, b1 + m, 1.0]
}
