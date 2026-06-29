//! アバター材料名と UNToon 互換プロファイルからレンダー用の役割を決める純計算。

#[cfg(test)]
use un_avatar_core::{UnaAlphaMode, UnaMtoonMaterial, UnaMtoonOutlineWidthMode};
use un_avatar_core::{UnaMaterialPbr, UnaSceneSnapshot};

use crate::debug_dump::eye_area_material_name;
use crate::liltoon_features;
#[cfg(test)]
use crate::mesh_pass::{AvatarOutlinePolicy, SceneMeshLoadOpts};
use crate::texture_pipeline::TextureRole;

#[cfg(test)]
pub(crate) const DEFAULT_AVATAR_OUTLINE_WIDTH_METERS: f32 = 0.003;

fn texture_role_priority(role: TextureRole) -> u8 {
	match role {
		TextureRole::Face | TextureRole::Eyes => 7,
		TextureRole::Data => 6,
		TextureRole::Normal => 5,
		TextureRole::Occlusion => 4,
		TextureRole::Emissive => 3,
		TextureRole::Clothing => 2,
		TextureRole::GenericColor => 1,
	}
}

fn merge_texture_role(current: TextureRole, next: TextureRole) -> TextureRole {
	if texture_role_priority(next) > texture_role_priority(current) {
		next
	} else {
		current
	}
}

pub(crate) fn material_base_color_role(mat: &UnaMaterialPbr) -> TextureRole {
	let name = mat.name.as_deref().unwrap_or("").to_ascii_lowercase();
	if eye_area_material_name(mat.name.as_deref()) {
		TextureRole::Eyes
	} else if material_name_is_face_skin_or_mouth(&name) {
		TextureRole::Face
	} else if name.contains("occlusion") || name.contains("ambient_occlusion") || name == "ao" {
		TextureRole::Occlusion
	} else if name.contains("cloth")
		|| name.contains("shirt")
		|| name.contains("dress")
		|| name.contains("skirt")
		|| name.contains("coat")
		|| name.contains("pants")
		|| name.contains("shoe")
	{
		TextureRole::Clothing
	} else {
		TextureRole::GenericColor
	}
}

pub(crate) fn material_name_is_face_skin_or_mouth(name: &str) -> bool {
	name.contains("face")
		|| name.contains("skin")
		|| name.contains("mouth")
		|| name.contains("lip")
		|| name.contains("tooth")
		|| name.contains("teeth")
		|| name.contains("tongue")
		|| name.contains("gum")
		|| name.contains("脸")
		|| name.contains("顔")
		|| name.contains("面")
		|| name.contains("肌")
		|| name.contains("皮膚")
		|| name.contains("口")
		|| name.contains("唇")
		|| name.contains("歯")
		|| name.contains("牙")
		|| name.contains("齿")
		|| name.contains("舌")
}

#[cfg(test)]
pub(crate) fn material_rim_strength_multiplier(mat: &UnaMaterialPbr) -> f32 {
	let name = mat.name.as_deref().unwrap_or("").to_ascii_lowercase();
	if eye_area_material_name(mat.name.as_deref()) || material_name_is_face_skin_or_mouth(&name) {
		return 0.0;
	}
	match mat.alpha_mode {
		UnaAlphaMode::Opaque => 1.0,
		UnaAlphaMode::Mask => 0.35,
		UnaAlphaMode::Blend => 0.0,
	}
}

fn mark_texture_role(roles: &mut [TextureRole], index: Option<usize>, role: TextureRole) {
	let Some(index) = index else { return };
	let Some(slot) = roles.get_mut(index) else { return };
	*slot = merge_texture_role(*slot, role);
}

fn lil_enabled(value: f32) -> bool {
	liltoon_features::enabled(value)
}

fn texture_role_from_source_metadata(source: &un_avatar_core::UnaImageSourceMetadata) -> Option<TextureRole> {
	let texture_type = source.texture_type.as_deref().unwrap_or("").to_ascii_lowercase();
	let color_space = source.color_space.as_deref().unwrap_or("").to_ascii_lowercase();
	let name = source.name.as_deref().unwrap_or("").to_ascii_lowercase();
	if texture_type.contains("normal") || name.contains("normal") || name.contains("_nrm") {
		return Some(TextureRole::Normal);
	}
	if texture_type.contains("mask")
		|| texture_type.contains("singlechannel")
		|| color_space == "data"
		|| name.contains("mask")
		|| name.contains("occlusion")
		|| name == "ao"
	{
		return Some(TextureRole::Data);
	}
	if source.srgb == Some(false) && color_space == "linear" {
		return Some(TextureRole::Data);
	}
	None
}

pub(crate) fn texture_roles_for_scene(scene: &UnaSceneSnapshot) -> Vec<TextureRole> {
	let mut roles = vec![TextureRole::GenericColor; scene.images.len()];
	for mat in &scene.materials {
		mark_texture_role(&mut roles, mat.base_color_texture_index, material_base_color_role(mat));
		mark_texture_role(&mut roles, mat.normal_texture_index, TextureRole::Normal);
		mark_texture_role(&mut roles, mat.occlusion_texture_index, TextureRole::Occlusion);
		mark_texture_role(&mut roles, mat.emissive_texture_index, TextureRole::Emissive);
		if let Some(liltoon_like) = mat.liltoon_like_runtime() {
			if liltoon_features::uses_main_color_adjustment(&liltoon_like.main_color) {
				mark_texture_role(
					&mut roles,
					liltoon_like.main_color.main_color_adjust_mask_texture_index,
					TextureRole::Data,
				);
			}
			if lil_enabled(liltoon_like.main_color.gradation_enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.main_color.gradation_texture_index, TextureRole::Data);
			}
			if lil_enabled(liltoon_like.main_color.second_enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.main_color.second_texture_index, TextureRole::GenericColor);
				mark_texture_role(
					&mut roles,
					liltoon_like.main_color.second_blend_mask_texture_index,
					TextureRole::Data,
				);
				mark_texture_role(
					&mut roles,
					liltoon_like.main_color.second_dissolve.mask_texture_index,
					TextureRole::Data,
				);
				mark_texture_role(
					&mut roles,
					liltoon_like.main_color.second_dissolve.noise_mask_texture_index,
					TextureRole::Data,
				);
			}
			if lil_enabled(liltoon_like.main_color.third_enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.main_color.third_texture_index, TextureRole::GenericColor);
				mark_texture_role(
					&mut roles,
					liltoon_like.main_color.third_blend_mask_texture_index,
					TextureRole::Data,
				);
				mark_texture_role(
					&mut roles,
					liltoon_like.main_color.third_dissolve.mask_texture_index,
					TextureRole::Data,
				);
				mark_texture_role(
					&mut roles,
					liltoon_like.main_color.third_dissolve.noise_mask_texture_index,
					TextureRole::Data,
				);
			}
			if lil_enabled(liltoon_like.normal.second_enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.normal.second_texture_index, TextureRole::Normal);
				mark_texture_role(&mut roles, liltoon_like.normal.second_scale_mask_texture_index, TextureRole::Data);
			}
			if lil_enabled(liltoon_like.shadow.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.shadow.color_texture_index, TextureRole::GenericColor);
				mark_texture_role(
					&mut roles,
					liltoon_like.shadow.second_color_texture_index,
					TextureRole::GenericColor,
				);
				mark_texture_role(&mut roles, liltoon_like.shadow.third_color_texture_index, TextureRole::GenericColor);
				mark_texture_role(&mut roles, liltoon_like.shadow.strength_mask_texture_index, TextureRole::Data);
				mark_texture_role(&mut roles, liltoon_like.shadow.border_mask_texture_index, TextureRole::Data);
				mark_texture_role(&mut roles, liltoon_like.shadow.blur_mask_texture_index, TextureRole::Data);
			}
			if lil_enabled(liltoon_like.matcap.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.matcap.texture_index, TextureRole::GenericColor);
				mark_texture_role(&mut roles, liltoon_like.matcap.blend_mask_texture_index, TextureRole::Data);
				mark_texture_role(&mut roles, liltoon_like.matcap.bump_texture_index, TextureRole::Normal);
			}
			if lil_enabled(liltoon_like.matcap.second_enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.matcap.second_texture_index, TextureRole::GenericColor);
				mark_texture_role(&mut roles, liltoon_like.matcap.second_blend_mask_texture_index, TextureRole::Data);
				mark_texture_role(&mut roles, liltoon_like.matcap.second_bump_texture_index, TextureRole::Normal);
			}
			if lil_enabled(liltoon_like.reflection.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.reflection.color_texture_index, TextureRole::GenericColor);
				mark_texture_role(&mut roles, liltoon_like.reflection.smoothness_texture_index, TextureRole::Data);
				mark_texture_role(&mut roles, liltoon_like.reflection.metallic_texture_index, TextureRole::Data);
			}
			if lil_enabled(liltoon_like.reflection.anisotropy_enabled_factor) {
				mark_texture_role(
					&mut roles,
					liltoon_like.reflection.anisotropy_tangent_texture_index,
					TextureRole::Normal,
				);
				mark_texture_role(
					&mut roles,
					liltoon_like.reflection.anisotropy_scale_mask_texture_index,
					TextureRole::Data,
				);
				mark_texture_role(
					&mut roles,
					liltoon_like.reflection.anisotropy_shift_noise_mask_texture_index,
					TextureRole::Data,
				);
			}
			if lil_enabled(liltoon_like.rim.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.rim.texture_index, TextureRole::GenericColor);
			}
			if lil_enabled(liltoon_like.rim.shade_enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.rim.shade_mask_texture_index, TextureRole::Data);
			}
			if lil_enabled(liltoon_like.backlight.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.backlight.texture_index, TextureRole::GenericColor);
			}
			if lil_enabled(liltoon_like.glitter.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.glitter.color_texture_index, TextureRole::GenericColor);
				mark_texture_role(&mut roles, liltoon_like.glitter.shape_texture_index, TextureRole::Data);
			}
			if liltoon_like.dissolve.params_factor[0] > 0.5 {
				mark_texture_role(&mut roles, liltoon_like.dissolve.mask_texture_index, TextureRole::Data);
				mark_texture_role(&mut roles, liltoon_like.dissolve.noise_mask_texture_index, TextureRole::Data);
			}
			if lil_enabled(liltoon_like.parallax.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.parallax.texture_index, TextureRole::Data);
			}
			if lil_enabled(liltoon_like.emission.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.emission.texture_index, TextureRole::Emissive);
				mark_texture_role(&mut roles, liltoon_like.emission.blend_mask_texture_index, TextureRole::Data);
				if lil_enabled(liltoon_like.emission.gradation_enabled_factor) {
					mark_texture_role(&mut roles, liltoon_like.emission.gradation_texture_index, TextureRole::Emissive);
				}
			}
			if lil_enabled(liltoon_like.emission.second_enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.emission.second_texture_index, TextureRole::Emissive);
				mark_texture_role(&mut roles, liltoon_like.emission.second_blend_mask_texture_index, TextureRole::Data);
				if lil_enabled(liltoon_like.emission.second_gradation_enabled_factor) {
					mark_texture_role(
						&mut roles,
						liltoon_like.emission.second_gradation_texture_index,
						TextureRole::Emissive,
					);
				}
			}
			if lil_enabled(liltoon_like.outline.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.outline.texture_index, TextureRole::GenericColor);
				mark_texture_role(&mut roles, liltoon_like.outline.width_mask_texture_index, TextureRole::Data);
			}
			if liltoon_like.alpha_mask.mode_factor > 0.5 {
				mark_texture_role(&mut roles, liltoon_like.alpha_mask.texture_index, TextureRole::Data);
			}
			if lil_enabled(liltoon_like.fur.enabled_factor) {
				mark_texture_role(&mut roles, liltoon_like.fur.vector_texture_index, TextureRole::Normal);
				mark_texture_role(&mut roles, liltoon_like.fur.length_mask_texture_index, TextureRole::Data);
				mark_texture_role(&mut roles, liltoon_like.fur.noise_mask_texture_index, TextureRole::Data);
				mark_texture_role(&mut roles, liltoon_like.fur.mask_texture_index, TextureRole::Data);
			}
		}
	}
	for (index, source) in scene.image_sources.iter().enumerate() {
		let Some(source) = source.as_ref() else { continue };
		if let Some(role) = texture_role_from_source_metadata(source) {
			mark_texture_role(&mut roles, Some(index), role);
		}
	}
	roles
}

#[cfg(test)]
pub(crate) struct EffectiveOutline {
	pub(crate) mode: UnaMtoonOutlineWidthMode,
	pub(crate) width: f32,
	pub(crate) color: [f32; 3],
	pub(crate) lighting_mix: f32,
}

#[cfg(test)]
const MAX_AUTHORED_GEOMETRY_OUTLINE_WIDTH_METERS: f32 = 0.00025;

#[cfg(test)]
pub(crate) fn effective_legacy_geometry_outline(source: &UnaMtoonMaterial, opts: &SceneMeshLoadOpts) -> Option<EffectiveOutline> {
	if opts.avatar_outline.policy == AvatarOutlinePolicy::Off {
		return None;
	}
	let mut width = match opts.avatar_outline.policy {
		AvatarOutlinePolicy::Authored => source.outline_width_factor,
		AvatarOutlinePolicy::Off => 0.0,
		AvatarOutlinePolicy::Override => opts.avatar_outline.width.unwrap_or(if source.outline_width_factor > 0.0 {
			source.outline_width_factor
		} else {
			DEFAULT_AVATAR_OUTLINE_WIDTH_METERS
		}),
	}
	.max(0.0);
	if opts.avatar_outline.policy == AvatarOutlinePolicy::Authored {
		width = width.min(MAX_AUTHORED_GEOMETRY_OUTLINE_WIDTH_METERS);
	}
	if width <= 0.0 {
		return None;
	}
	let mode = match opts.avatar_outline.policy {
		AvatarOutlinePolicy::Authored => source.outline_width_mode,
		AvatarOutlinePolicy::Off => UnaMtoonOutlineWidthMode::None,
		AvatarOutlinePolicy::Override => {
			if source.outline_width_mode == UnaMtoonOutlineWidthMode::None {
				UnaMtoonOutlineWidthMode::WorldCoordinates
			} else {
				source.outline_width_mode
			}
		}
	};
	if mode == UnaMtoonOutlineWidthMode::None {
		return None;
	}
	let color = match opts.avatar_outline.policy {
		AvatarOutlinePolicy::Override => opts.avatar_outline.color.unwrap_or(source.outline_color_factor),
		AvatarOutlinePolicy::Authored | AvatarOutlinePolicy::Off => source.outline_color_factor,
	};
	let lighting_mix = match opts.avatar_outline.policy {
		AvatarOutlinePolicy::Override => opts.avatar_outline.lighting_mix.unwrap_or(source.outline_lighting_mix_factor),
		AvatarOutlinePolicy::Authored | AvatarOutlinePolicy::Off => source.outline_lighting_mix_factor,
	}
	.clamp(0.0, 1.0);
	Some(EffectiveOutline {
		mode,
		width,
		color,
		lighting_mix,
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::mesh_pass::{AvatarOutlineKind, AvatarOutlineOptions};

	#[test]
	fn material_role_detects_cjk_face_and_eye_names() {
		let mut eye = UnaMaterialPbr {
			name: Some("眼睛高光".to_string()),
			..Default::default()
		};
		assert_eq!(material_base_color_role(&eye), TextureRole::Eyes);

		eye.name = Some("眼睑".to_string());
		assert_eq!(material_base_color_role(&eye), TextureRole::Eyes);

		eye.name = Some("眉毛".to_string());
		assert_eq!(material_base_color_role(&eye), TextureRole::Eyes);

		eye.name = Some("脸".to_string());
		assert_eq!(material_base_color_role(&eye), TextureRole::Face);
	}

	#[test]
	fn rim_strength_suppresses_sensitive_face_eye_and_mouth_materials() {
		let mut mat = UnaMaterialPbr::default();
		mat.name = Some("Face".to_string());
		assert_eq!(material_rim_strength_multiplier(&mat), 0.0);
		mat.name = Some("EyeHighlight".to_string());
		assert_eq!(material_rim_strength_multiplier(&mat), 0.0);
		mat.name = Some("mouth_inner".to_string());
		assert_eq!(material_rim_strength_multiplier(&mat), 0.0);
		mat.name = Some("Jacket".to_string());
		assert_eq!(material_rim_strength_multiplier(&mat), 1.0);
	}

	#[test]
	fn rim_strength_weakens_alpha_cutout_and_disables_alpha_blend() {
		let mut mat = UnaMaterialPbr {
			name: Some("HairAccessory".to_string()),
			alpha_mode: UnaAlphaMode::Mask,
			..Default::default()
		};
		assert_eq!(material_rim_strength_multiplier(&mat), 0.35);

		mat.alpha_mode = UnaAlphaMode::Blend;
		assert_eq!(material_rim_strength_multiplier(&mat), 0.0);
	}

	#[test]
	fn avatar_outline_override_can_create_geometry_outline() {
		let mtoon = UnaMtoonMaterial {
			outline_width_mode: UnaMtoonOutlineWidthMode::None,
			outline_width_factor: 0.0,
			outline_color_factor: [0.4, 0.4, 0.4],
			outline_lighting_mix_factor: 1.0,
			..Default::default()
		};
		let opts = SceneMeshLoadOpts {
			avatar_outline: AvatarOutlineOptions {
				policy: AvatarOutlinePolicy::Override,
				kind: AvatarOutlineKind::Geometry,
				width: Some(0.004),
				color: Some([0.02, 0.01, 0.03]),
				lighting_mix: Some(0.25),
				roundness: None,
			},
			..Default::default()
		};

		let outline = effective_legacy_geometry_outline(&mtoon, &opts).expect("override should create outline");
		assert_eq!(outline.mode, UnaMtoonOutlineWidthMode::WorldCoordinates);
		assert_eq!(outline.width, 0.004);
		assert_eq!(outline.color, [0.02, 0.01, 0.03]);
		assert_eq!(outline.lighting_mix, 0.25);
	}

	#[test]
	fn avatar_outline_override_uses_default_width_when_profile_width_is_unset() {
		let mtoon = UnaMtoonMaterial {
			outline_width_mode: UnaMtoonOutlineWidthMode::None,
			outline_width_factor: 0.0,
			outline_color_factor: [1.0, 1.0, 1.0],
			..Default::default()
		};
		let opts = SceneMeshLoadOpts {
			avatar_outline: AvatarOutlineOptions {
				policy: AvatarOutlinePolicy::Override,
				..Default::default()
			},
			..Default::default()
		};

		let outline = effective_legacy_geometry_outline(&mtoon, &opts).expect("override default should create outline");
		assert_eq!(outline.mode, UnaMtoonOutlineWidthMode::WorldCoordinates);
		assert_eq!(outline.width, DEFAULT_AVATAR_OUTLINE_WIDTH_METERS);
		assert_eq!(outline.color, [1.0, 1.0, 1.0]);
	}

	#[test]
	fn avatar_outline_off_disables_authored_outline() {
		let mtoon = UnaMtoonMaterial {
			outline_width_mode: UnaMtoonOutlineWidthMode::WorldCoordinates,
			outline_width_factor: 0.002,
			..Default::default()
		};
		let opts = SceneMeshLoadOpts {
			avatar_outline: AvatarOutlineOptions {
				policy: AvatarOutlinePolicy::Off,
				..Default::default()
			},
			..Default::default()
		};
		assert!(effective_legacy_geometry_outline(&mtoon, &opts).is_none());
	}

	#[test]
	fn authored_outline_width_is_capped_for_untoon_compatibility() {
		let mtoon = UnaMtoonMaterial {
			outline_width_mode: UnaMtoonOutlineWidthMode::WorldCoordinates,
			outline_width_factor: 0.0008,
			..Default::default()
		};
		let opts = SceneMeshLoadOpts {
			avatar_outline: AvatarOutlineOptions {
				policy: AvatarOutlinePolicy::Authored,
				kind: AvatarOutlineKind::Geometry,
				..Default::default()
			},
			..Default::default()
		};

		let outline = effective_legacy_geometry_outline(&mtoon, &opts).expect("authored outline should exist");
		assert_eq!(outline.width, MAX_AUTHORED_GEOMETRY_OUTLINE_WIDTH_METERS);
	}

	#[test]
	fn texture_roles_use_source_metadata_as_fallback() {
		let image = || un_avatar_core::UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 255],
		};
		let mut scene = UnaSceneSnapshot {
			images: vec![image(), image()],
			image_sources: vec![
				Some(un_avatar_core::UnaImageSourceMetadata {
					name: Some("detail_normal".to_string()),
					texture_type: Some("NormalMap".to_string()),
					byte_length: 1,
					source_hash: 1,
					..Default::default()
				}),
				Some(un_avatar_core::UnaImageSourceMetadata {
					name: Some("mask".to_string()),
					color_space: Some("data".to_string()),
					byte_length: 1,
					source_hash: 2,
					..Default::default()
				}),
			],
			..Default::default()
		};
		assert_eq!(texture_roles_for_scene(&scene)[0], TextureRole::Normal);
		assert_eq!(texture_roles_for_scene(&scene)[1], TextureRole::Data);

		scene.materials.push(UnaMaterialPbr {
			base_color_texture_index: Some(0),
			name: Some("Face".to_string()),
			..Default::default()
		});
		assert_eq!(texture_roles_for_scene(&scene)[0], TextureRole::Face);

		scene.images.push(image());
		scene.image_sources.push(None);
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.normal.second_enabled_factor = 1.0;
		liltoon_like.normal.second_texture_index = Some(2);
		scene.materials.push(UnaMaterialPbr {
			shading: un_avatar_core::UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		});
		assert_eq!(texture_roles_for_scene(&scene)[2], TextureRole::Normal);

		scene.images.extend([image(), image(), image(), image(), image(), image()]);
		scene.image_sources.extend([None, None, None, None, None, None]);
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.shadow.enabled_factor = 1.0;
		liltoon_like.shadow.strength_mask_texture_index = Some(3);
		liltoon_like.emission.enabled_factor = 1.0;
		liltoon_like.emission.gradation_enabled_factor = 1.0;
		liltoon_like.emission.gradation_texture_index = Some(4);
		liltoon_like.reflection.anisotropy_enabled_factor = 1.0;
		liltoon_like.reflection.anisotropy_tangent_texture_index = Some(5);
		liltoon_like.alpha_mask.mode_factor = 1.0;
		liltoon_like.alpha_mask.texture_index = Some(6);
		liltoon_like.main_color.main_color_adjust_mask_texture_index = Some(7);
		liltoon_like.main_color.gradation_enabled_factor = 1.0;
		liltoon_like.main_color.gradation_texture_index = Some(8);
		scene.materials.push(UnaMaterialPbr {
			shading: un_avatar_core::UnaShadingModel::LilToonLike,
			liltoon_like: Some(liltoon_like),
			..Default::default()
		});
		let roles = texture_roles_for_scene(&scene);
		assert_eq!(roles[3], TextureRole::Data);
		assert_eq!(roles[4], TextureRole::Emissive);
		assert_eq!(roles[5], TextureRole::Normal);
		assert_eq!(roles[6], TextureRole::Data);
		assert_eq!(roles[7], TextureRole::Data);
		assert_eq!(roles[8], TextureRole::Data);
	}

	#[test]
	fn texture_roles_skip_disabled_liltoon_feature_slots() {
		let image = || un_avatar_core::UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 255],
		};
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.normal.second_texture_index = Some(0);
		liltoon_like.shadow.enabled_factor = 0.0;
		liltoon_like.shadow.strength_mask_texture_index = Some(1);
		liltoon_like.emission.enabled_factor = 0.0;
		liltoon_like.emission.texture_index = Some(2);
		liltoon_like.alpha_mask.mode_factor = 0.0;
		liltoon_like.alpha_mask.texture_index = Some(3);
		liltoon_like.main_color.main_color_adjust_mask_texture_index = Some(4);
		liltoon_like.parallax.texture_index = Some(5);
		let scene = UnaSceneSnapshot {
			images: vec![image(), image(), image(), image(), image(), image()],
			image_sources: vec![None, None, None, None, None, None],
			materials: vec![UnaMaterialPbr {
				shading: un_avatar_core::UnaShadingModel::LilToonLike,
				liltoon_like: Some(liltoon_like),
				..Default::default()
			}],
			..Default::default()
		};

		assert_eq!(
			texture_roles_for_scene(&scene),
			vec![
				TextureRole::GenericColor,
				TextureRole::GenericColor,
				TextureRole::GenericColor,
				TextureRole::GenericColor,
				TextureRole::GenericColor,
				TextureRole::GenericColor
			]
		);
	}

	#[test]
	fn texture_roles_use_mtoon_marker_untoon_runtime_payload() {
		let image = || un_avatar_core::UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 255],
		};
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.shadow.enabled_factor = 1.0;
		liltoon_like.shadow.strength_mask_texture_index = Some(0);
		let scene = UnaSceneSnapshot {
			images: vec![image()],
			image_sources: vec![None],
			materials: vec![UnaMaterialPbr {
				shading: un_avatar_core::UnaShadingModel::MToonLike,
				liltoon_like: Some(liltoon_like),
				..Default::default()
			}],
			..Default::default()
		};

		assert_eq!(texture_roles_for_scene(&scene), vec![TextureRole::Data]);
	}

	#[test]
	fn texture_roles_ignore_non_toon_stale_untoon_payload() {
		let image = || un_avatar_core::UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 255],
		};
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.shadow.enabled_factor = 1.0;
		liltoon_like.shadow.strength_mask_texture_index = Some(0);
		let scene = UnaSceneSnapshot {
			images: vec![image()],
			image_sources: vec![None],
			materials: vec![UnaMaterialPbr {
				shading: un_avatar_core::UnaShadingModel::LitLambert,
				liltoon_like: Some(liltoon_like),
				..Default::default()
			}],
			..Default::default()
		};

		assert_eq!(texture_roles_for_scene(&scene), vec![TextureRole::GenericColor]);
	}

	#[test]
	fn texture_roles_mark_liltoon_adjust_and_parallax_only_when_enabled() {
		let image = || un_avatar_core::UnaImageRgba {
			width: 1,
			height: 1,
			pixel_format: un_avatar_core::UnaImagePixelFormat::R8G8B8A8,
			pixels: vec![255, 255, 255, 255],
		};
		let mut liltoon_like = un_avatar_core::UnaLilToonLikeMaterial::default();
		liltoon_like.main_color.main_color_adjust_mask_texture_index = Some(0);
		liltoon_like.main_color.main_texture_hsvg_factor = [0.1, 1.0, 1.0, 1.0];
		liltoon_like.parallax.texture_index = Some(1);
		liltoon_like.parallax.enabled_factor = 1.0;
		let scene = UnaSceneSnapshot {
			images: vec![image(), image()],
			image_sources: vec![None, None],
			materials: vec![UnaMaterialPbr {
				shading: un_avatar_core::UnaShadingModel::LilToonLike,
				liltoon_like: Some(liltoon_like),
				..Default::default()
			}],
			..Default::default()
		};

		assert_eq!(texture_roles_for_scene(&scene), vec![TextureRole::Data, TextureRole::Data]);
	}
}
