#[cfg(test)]
mod tests {
	fn validate_wgsl(label: &str, source: &str) {
		let module = naga::front::wgsl::parse_str(source).unwrap_or_else(|err| panic!("{label}: WGSL parse error: {err}"));
		let mut validator = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all());
		validator
			.validate(&module)
			.unwrap_or_else(|err| panic!("{label}: WGSL validation error: {err}"));
	}

	#[test]
	fn bundled_wgsl_shaders_parse_and_validate() {
		validate_wgsl("mesh.wgsl", include_str!("../shaders/mesh.wgsl"));
		validate_wgsl("sky.wgsl", include_str!("../shaders/sky.wgsl"));
		validate_wgsl("axes.wgsl", include_str!("../shaders/axes.wgsl"));
		validate_wgsl("bone_colliders.wgsl", include_str!("../shaders/bone_colliders.wgsl"));
		validate_wgsl("startup_splash.wgsl", include_str!("../shaders/startup_splash.wgsl"));
		validate_wgsl("contact_shadow.wgsl", include_str!("../shaders/contact_shadow.wgsl"));
		validate_wgsl("avatar_outline.wgsl", include_str!("../shaders/avatar_outline.wgsl"));
		validate_wgsl("bloom.wgsl", include_str!("../shaders/bloom.wgsl"));
		validate_wgsl("color_adjust.wgsl", include_str!("../shaders/color_adjust.wgsl"));
		validate_wgsl("fxaa.wgsl", include_str!("../shaders/fxaa.wgsl"));
		validate_wgsl("smaa.wgsl", include_str!("../shaders/smaa.wgsl"));
		validate_wgsl("blit.wgsl", include_str!("../shaders/blit.wgsl"));
		validate_wgsl("csfc_fur.wgsl", include_str!("../shaders/csfc_fur.wgsl"));
	}

	#[test]
	fn liltoon_gem_refraction_offset_matches_view_normal_xy() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("return view_normal.xy;"),
			"lilToon Gem refraction should match mul((float3x3)LIL_MATRIX_V, fd.N).xy"
		);
		assert!(
			!mesh.contains("vec2<f32>(view_normal.x, -view_normal.y)"),
			"do not flip Y for lilToon Gem refraction offset"
		);
	}

	#[test]
	fn liltoon_screen_refraction_uses_fragment_position_uv() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("textureDimensions(screen_tex, 0)"),
			"lilToon BRP GrabPass samples use SV_POSITION.xy divided by the background texture size"
		);
		assert!(
			mesh.contains("fragment_position.xy / dims"),
			"screen refraction UV must be based on fragment framebuffer coordinates"
		);
	}

	#[test]
	fn liltoon_rim_direction_uses_signed_range_formula() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("clamp(drawu.rim_indirect_params.y, -1.0, 1.0)"),
			"_RimDirRange is Range(-1, 1) in lilToon"
		);
		assert!(
			mesh.contains("clamp(drawu.rim_indirect_params.z, -1.0, 1.0)"),
			"_RimIndirRange is Range(-1, 1) in lilToon"
		);
		assert!(
			mesh.contains("(ln_raw + dir_range) / max(1.0 + dir_range, 0.00001)"),
			"lilToon computes lnDir as saturate((lnRaw + _RimDirRange) / (1.0 + _RimDirRange))"
		);
		assert!(
			mesh.contains("(1.0 - ln_raw + indir_range) / max(1.0 + indir_range, 0.00001)"),
			"lilToon computes lnIndir as saturate((1.0-lnRaw + _RimIndirRange) / (1.0 + _RimIndirRange))"
		);
	}

	#[test]
	fn liltoon_matcap_blend_mask_is_rgb() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		let lines: Vec<_> = mesh.lines().map(str::trim).collect();
		assert!(
			mesh.contains("textureSample(matcap_blend_mask_tex, matcap_blend_mask_samp, matcap_blend_mask_uv).rgb"),
			"lilToon samples _MatCapBlendMask as rgb for per-channel blending"
		);
		assert!(
			mesh.contains("textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, matcap2_blend_mask_uv).rgb"),
			"lilToon samples _MatCap2ndBlendMask as rgb for per-channel blending"
		);
		assert!(
			!lines.contains(&"let matcap_blend_mask = textureSample(matcap_blend_mask_tex, matcap_blend_mask_samp, matcap_blend_mask_uv).r;"),
			"MatCap blend mask must not collapse to the red channel"
		);
		assert!(
			!lines.contains(&"let matcap2_blend_mask = textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, matcap2_blend_mask_uv).r;"),
			"MatCap 2nd blend mask must not collapse to the red channel"
		);
	}

	#[test]
	fn liltoon_shadow_masks_preserve_rgb_channels() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("textureSample(shadow_border_mask_tex, shadow_border_mask_samp, shadow_border_mask_uv).rgb"),
			"lilToon uses _ShadowBorderMask rgb for first, second, and third shadow AO"
		);
		assert!(
			mesh.contains("textureSample(shadow_blur_mask_tex, shadow_blur_mask_samp, shadow_blur_mask_uv).rgb"),
			"lilToon uses _ShadowBlurMask rgb for first, second, and third shadow blur"
		);
		assert!(
			mesh.contains("shadow_border_mask.g") && mesh.contains("shadow_border_mask.b"),
			"second and third shadow borders must not collapse to the red channel"
		);
		assert!(
			mesh.contains("shadow_blur_mask.g") && mesh.contains("shadow_blur_mask.b"),
			"second and third shadow blur must not collapse to the red channel"
		);
	}

	#[test]
	fn liltoon_flip_normal_flips_backface_normals() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn lil_flip_backface_normal"),
			"lilToon _FlipNormal needs an explicit backface normal flip"
		);
		assert!(
			mesh.contains("!front_facing && flip_backface_normal > 0.5"),
			"_FlipNormal should only affect backfaces"
		);
		assert!(
			mesh.contains("drawu.material_ext_params.x"),
			"_FlipNormal must be driven from the material uniform"
		);
	}

	#[test]
	fn liltoon_shadow_post_ao_controls_border_mask_order() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn lil_shadow_border_ao_mask"),
			"lilToon shadow border masks must apply _ShadowAOShift/_ShadowAOShift2"
		);
		assert!(
			mesh.contains("drawu.shadow_ao_params.x > 0.5"),
			"_ShadowPostAO must control shadow border mask ordering"
		);
		assert!(
			mesh.contains("lil_shadow_apply_pre_ao") && mesh.contains("lil_shadow_apply_post_ao"),
			"shadow border AO must be applicable before or after tooning"
		);
	}

	#[test]
	fn liltoon_glitter_uses_procedural_liltoon_controls() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn lil_calc_glitter"),
			"lilToon Glitter needs a procedural glitter calculation path"
		);
		assert!(
			mesh.contains("drawu.glitter_control.x > 0.5"),
			"_UseGlitter must gate glitter contribution"
		);
		assert!(
			mesh.contains("drawu.glitter_params1") && mesh.contains("drawu.glitter_params2"),
			"_GlitterParams1/_GlitterParams2 must drive scale, size, contrast, speed, angle, and random color"
		);
		assert!(
			mesh.contains("drawu.glitter_ext.z") && mesh.contains("lil_effect_shadowmix"),
			"_GlitterShadowMask must mix glitter alpha with lilToon shadow mix"
		);
		assert!(
			mesh.contains("drawu.glitter_ext.w") && mesh.contains("drawu.glitter_ext2.x"),
			"_GlitterApplyTransparency and _GlitterBackfaceMask must affect glitter alpha"
		);
	}
}
