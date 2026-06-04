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
		validate_wgsl("compute_fur_cards.wgsl", include_str!("../shaders/compute_fur_cards.wgsl"));
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
			!lines
				.contains(&"let matcap_blend_mask = textureSample(matcap_blend_mask_tex, matcap_blend_mask_samp, matcap_blend_mask_uv).r;"),
			"MatCap blend mask must not collapse to the red channel"
		);
		assert!(
			!lines.contains(
				&"let matcap2_blend_mask = textureSample(matcap2_blend_mask_tex, matcap_blend_mask_samp, matcap2_blend_mask_uv).r;"
			),
			"MatCap 2nd blend mask must not collapse to the red channel"
		);
	}

	#[test]
	fn liltoon_matcap_custom_normal_uses_bump_maps() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("@group(1) @binding(73) var matcap_bump_tex: texture_2d<f32>;")
				&& mesh.contains("@group(1) @binding(74) var matcap2_bump_tex: texture_2d<f32>;"),
			"MatCap custom normal maps must be bound in FullOnePass"
		);
		assert!(
			mesh.contains("let map_uv = uv * uv_offset_scale.zw + uv_offset_scale.xy;")
				&& mesh.contains("lil_unpack_normal_scale(packed, scale)")
				&& mesh.contains("textureSample(matcap_bump_tex, normal_samp, map_uv)")
				&& mesh.contains("textureSample(matcap2_bump_tex, normal_samp, map_uv)"),
			"MatCap custom normal maps must use their texture slot transform"
		);
		assert!(
			mesh.contains("if (drawu.matcap_bump_params.x > 0.5)")
				&& mesh.contains("if (drawu.matcap2_bump_params.x > 0.5)")
				&& mesh.contains("drawu.matcap_bump_params.y")
				&& mesh.contains("drawu.matcap2_bump_params.y"),
			"MatCap custom normal flags and bump scales must reach the MatCap normal path"
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
	fn liltoon_tooning_scale_uses_derivative_antialiasing() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fwidth(value) * aa_scale"),
			"lilToon tooning boundaries should include derivative AA like lilTooningScale"
		);
		assert!(
			mesh.contains("border_max - border_min + fwidth(value) * aa_scale"),
			"shadow border range tooning should widen by fwidth(value) and _AAStrength"
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
			mesh.contains("let unity_time_x = frame.time_params.x * 0.05")
				&& mesh.contains("let time_seed = unity_time_x * drawu.glitter_params2.x"),
			"lilToon Glitter speed uses Unity _Time.x, which is t/20 rather than raw seconds"
		);
		assert!(
			mesh.contains("let factor = fract(sin(dot(random_cell, vec2<f32>(12.9898, 78.233))) * 46203.4357) + 0.5;")
				&& mesh.contains("let factor2 = floor(dd + vec2<f32>(factor * 0.5, factor * 0.5));"),
			"lilToon Glitter mipmap cell factor must keep the upstream randomized density rule"
		);
		assert!(
			mesh.contains("glitter_color_tex") && mesh.contains("drawu.glitter_color.rgb * glitter_color_texel.rgb"),
			"_GlitterColorTex must mask/tint Glitter color like upstream lilToon"
		);
		assert!(
			mesh.contains("drawu.glitter_color.a * glitter_color_texel.a"),
			"_GlitterColorTex alpha must constrain Glitter to authored mask regions"
		);
		assert!(
			mesh.contains("glitter_shape_tex") && mesh.contains("drawu.glitter_ext3.y > 0.5"),
			"_GlitterShapeTex and _GlitterApplyShape must gate shaped glitter particles"
		);
		assert!(
			mesh.contains("drawu.glitter_ext3.z > 0.5") && mesh.contains("nearest.z * 785.238"),
			"_GlitterAngleRandomize must rotate shaped glitter like upstream lilToon"
		);
		assert!(
			mesh.contains("drawu.glitter_atlas.xy") && mesh.contains("floor(nearest.xy * atlas)"),
			"_GlitterAtras must atlas-select shape texture cells"
		);
		assert!(
			mesh.contains("lil_select_uv(drawu.glitter_ext2.z") && mesh.contains("lil_select_uv(drawu.glitter_ext2.w"),
			"_GlitterUVMode and _GlitterColorTex_UVMode must select authored UV sets"
		);
		assert!(
			mesh.contains("let glitter_color_uv_raw = lil_select_uv(drawu.glitter_ext2.w, uv, i.uv1, i.uv2, i.uv3);"),
			"_GlitterColorTex_UVMode 0 must use fd.uvMain-equivalent UV, not raw uv0"
		);
		assert!(
			mesh.contains("let glitter_uv = lil_select_uv(drawu.glitter_ext2.z, uv, i.uv1, uv, uv);"),
			"_GlitterUVMode 0 must use parallax-adjusted fd.uv0-equivalent UV"
		);
		assert!(
			mesh.contains("drawu.glitter_ext3.x") && mesh.contains("glitter_view"),
			"_GlitterVRParallaxStrength must blend Glitter view/camera directions"
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

	#[test]
	fn liltoon_reflection_perceptual_roughness_has_no_floor() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let base_perceptual_roughness = clamp(1.0 - smoothness, 0.0, 1.0);"),
			"lilToon derives perceptual roughness from smoothness without a nonzero floor before reflection sampling"
		);
		assert!(
			mesh.contains("let aniso_perceptual_roughness = clamp(1.2 - abs(anisotropy_basis.amount), 0.0, 1.0);"),
			"lilToon anisotropy perceptual roughness is saturate(1.2 - abs(anisotropy))"
		);
		assert!(
			!mesh.contains("max(1.0 - smoothness, 0.02)") && !mesh.contains("max(1.2 - abs(anisotropy_basis.amount), 0.02)"),
			"do not add a perceptual roughness floor before environment reflection"
		);
		assert!(
			mesh.contains("let roughness2 = max(roughness, 0.002);"),
			"the GGX specular branch should keep lilToon's roughness lower bound inside lilCalcSpecular"
		);
	}

	#[test]
	fn liltoon_anisotropy_uses_upstream_normal_and_ggx_shape() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let aniso_direction = select(aniso_t, aniso_b, anisotropy > 0.0);")
				&& mesh.contains("let aniso_direction_ortho = lil_ortho_normalize(aniso_direction, v);")
				&& mesh.contains("let aniso_n = normalize(mix(n, aniso_direction_ortho, clamp(abs(anisotropy), 0.0, 1.0)));"),
			"lilToon lilGetAnisotropyNormalWS chooses bitangent for positive anisotropy and ortho-normalizes against the view direction"
		);
		assert!(
			mesh.contains("let roughness_t = max(roughness * (1.0 + anisotropy_basis.amount), 0.002);")
				&& mesh.contains("let roughness_b = max(roughness * (1.0 - anisotropy_basis.amount), 0.002);")
				&& mesh.contains("let ggx = r1 * w1 * w1 * drawu.anisotropy_ext_params.w + r2 * w2 * w2 * drawu.anisotropy2_params.z;"),
			"lilToon anisotropic specular should use the upstream tangent/bitangent GGX formula"
		);
		assert!(
			mesh.contains("if (drawu.specular_toon_params.x > 0.5 && !is_anisotropy_specular)")
				&& mesh.contains("specular_reflect = vec3<f32>(lil_tooning_scale(specular_term, 0.5, 0.0));"),
			"when anisotropy is active, lilToon applies toon scaling after anisotropic GGX rather than using nh power"
		);
	}

	#[test]
	fn liltoon_environment_reflection_fresnel_uses_base_nv() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let reflection_dir = normalize(reflect(-v, reflection_n));"),
			"environment lookup should still use the reflection normal"
		);
		assert!(
			mesh.contains("fresnel_lerp(specular_color, grazing_term, max(dot(n, v), 0.0))"),
			"lilToon environment reflection Fresnel uses fd.nv, not the reflection-normal dot"
		);
		assert!(
			!mesh.contains("fresnel_lerp(specular_color, grazing_term, max(dot(reflection_n, v), 0.0))"),
			"do not reuse reflection normal strength for the Fresnel nv term"
		);
	}

	#[test]
	fn liltoon_dissolve_separates_noise_and_non_noise_directional_uv() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn lil_rotate_uv"),
			"lilToon non-noise UV directional dissolve uses lilRotateUV"
		);
		assert!(
			mesh.contains("let has_noise = drawu.dissolve_ext.z > 0.5;"),
			"Dissolve must branch by whether a noise mask is present"
		);
		assert!(
			mesh.contains("select(lil_rotate_uv(uv, drawu.dissolve_pos.w).x, dot(uv, normalize_or2(drawu.dissolve_pos.xy"),
			"Base dissolve UV directional mode must match lilCalcDissolve versus lilCalcDissolveWithNoise"
		);
		assert!(
			mesh.contains("select(lil_rotate_uv(uv, pos.w).x, dot(uv, normalize_or2(pos.xy"),
			"Main2nd/Main3rd dissolve UV directional mode must match layer-specific lilToon dissolve"
		);
	}

	#[test]
	fn liltoon_main_layer_texture_uv_modes_use_authored_uv_sets() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let layer_uv_raw = lil_select_layer_uv(drawu.main2nd_ext.x, uv, uv1, uv2, uv3, uv_mat);"),
			"_Main2ndTex_UVMode must select fd.uv0/1/2/3/uvMat, with mode 0 using parallax-adjusted uv0"
		);
		assert!(
			mesh.contains("let layer_uv_raw = lil_select_layer_uv(drawu.main3rd_ext.x, uv, uv1, uv2, uv3, uv_mat);"),
			"_Main3rdTex_UVMode must select fd.uv0/1/2/3/uvMat, with mode 0 using parallax-adjusted uv0"
		);
		assert!(
			mesh.contains("fn lil_select_layer_uv") && mesh.contains("return uv_mat;"),
			"Main layer UV mode 4 must use MatCap UV instead of falling through to UV3"
		);
		assert!(
			mesh.contains("let mask_uv = uv * drawu.main2nd_blend_mask_uv_offset_scale.zw")
				&& mesh.contains("let mask_uv = uv * drawu.main3rd_blend_mask_uv_offset_scale.zw"),
			"Main layer blend masks must follow fd.uvMain like upstream lilToon"
		);
		assert!(
			mesh.contains("apply_lil_layer_distance_fade(layer_alpha, drawu.main2nd_distance_fade")
				&& mesh.contains("apply_lil_layer_distance_fade(layer_alpha, drawu.main3rd_distance_fade"),
			"Main layer distance fade must affect layer alpha"
		);
		assert!(
			mesh.contains("apply_lil_layer_cull(layer_alpha, drawu.main2nd_ext.y")
				&& mesh.contains("apply_lil_layer_cull(layer_alpha, drawu.main3rd_ext.y"),
			"Main layer cull mode must affect layer alpha"
		);
		assert!(
			mesh.contains("second_unlit = vec4<f32>(layer.rgb, layer_alpha * (1.0 - clamp(drawu.main2nd_params.y")
				&& mesh.contains("third_unlit = vec4<f32>(layer.rgb, layer_alpha * (1.0 - clamp(drawu.main3rd_params.y"),
			"Main layer unlit contribution must be preserved for post-shadow blending"
		);
		assert!(
			mesh.contains("lit = lil_blend_color(lit, main_layers.second_unlit.rgb, main_layers.second_unlit.a, drawu.main2nd_params.w);")
				&& mesh.contains(
					"lit = lil_blend_color(lit, main_layers.third_unlit.rgb, main_layers.third_unlit.a, drawu.main3rd_params.w);"
				),
			"Main layer unlit contribution must be restored after shadow like upstream lilToon"
		);
		assert!(
			mesh.contains("fn lil_calc_decal_uv")
				&& mesh.contains("fn lil_layer_sub_tex_uv")
				&& mesh.contains("fn lil_calc_atlas_animation_uv")
				&& mesh.contains("floor(frame.time_params.x * decal_animation.w) % decal_animation.z"),
			"Main2nd/Main3rd decal UV must keep lilToon decal/copy/flip/animated atlas semantics"
		);
		assert!(
			mesh.contains("drawu.main2nd_decal_flags")
				&& mesh.contains("drawu.main3rd_decal_flags")
				&& mesh.contains("layer.a * layer_uv.alpha_mask"),
			"Main2nd/Main3rd decal alpha masking must affect sampled layer alpha"
		);
	}

	#[test]
	fn liltoon_emission_uv_modes_use_authored_uv_sets_and_rim_uv() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn lil_select_emission_uv") && mesh.contains("return uv_rim;"),
			"Emission UV mode 4 must use lilToon fd.uvRim semantics"
		);
		assert!(
			mesh.contains("let uv_rim = vec2<f32>(abs(dot(n, v)));"),
			"Emission rim UV must match lilToon float2(fd.nvabs, fd.nvabs)"
		);
		assert!(
			mesh.contains("lil_select_emission_uv(drawu.emission_uv_anim_params.w, uv, i.uv1, i.uv2, i.uv3, uv_rim)")
				&& mesh.contains("lil_select_emission_uv(drawu.emission2nd_uv_anim_params.w, uv, i.uv1, i.uv2, i.uv3, uv_rim)"),
			"Emission and Emission2nd maps must use their authored _UVMode values"
		);
	}

	#[test]
	fn liltoon_second_normal_uv_mode_uses_authored_uv_sets() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let normal2nd_base_uv = lil_select_uv(drawu.normal2nd_params.z, uv, uv1, uv2, uv3);"),
			"_Bump2ndMap_UVMode must select fd.uv0/1/2/3 before _Bump2ndMap_ST"
		);
		assert!(
			mesh.contains(
				"let normal2nd_uv = normal2nd_base_uv * drawu.normal2nd_uv_offset_scale.zw + drawu.normal2nd_uv_offset_scale.xy;"
			),
			"_Bump2ndMap_ST must be applied after the authored UV mode selection"
		);
		assert!(
			mesh.contains("let scale_mask = textureSample(normal2nd_scale_mask_tex, base_samp, uv).r;")
				&& mesh.contains(
					"lil_unpack_normal_scale(textureSample(normal2nd_tex, normal_samp, normal2nd_uv), drawu.normal2nd_params.y * scale_mask)"
				),
			"_Bump2ndScaleMask must multiply _BumpScale2nd using fd.uvMain"
		);
	}

	#[test]
	fn liltoon_normal_unpack_reconstructs_z() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn lil_unpack_normal_scale"),
			"normal maps should share the lilToon normal unpack path"
		);
		assert!(
			mesh.contains("packed.a * packed.r") && mesh.contains("sqrt(1.0 - clamp(dot(tn.xy, tn.xy), 0.0, 1.0))"),
			"lilToon reconstructs normal z from unpacked xy"
		);
	}

	#[test]
	fn liltoon_backlight_color_texture_uses_slot_transform() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains(
				"let backlight_color_uv = uv * drawu.backlight_color_uv_offset_scale.zw + drawu.backlight_color_uv_offset_scale.xy;"
			) && mesh.contains("textureSample(backlight_color_tex, base_samp, backlight_color_uv)"),
			"_BacklightColorTex must use its authored _ST on fd.uvMain"
		);
	}

	#[test]
	fn liltoon_light_color_includes_environment_proxy() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn liltoon_light_color() -> vec3<f32>")
				&& mesh.contains("let main_light = frame.light_color.rgb * frame.light_color.w;")
				&& mesh.contains("let sh_proxy = frame.ambient_color.rgb * frame.ambient_color.w;")
				&& mesh.contains("return lil_correct_light_color(main_light + sh_proxy);"),
			"lilToon fd.lightColor must include an SH/environment proxy, matching lilToon main light + SH semantics"
		);
		assert!(
			mesh.contains("let effect_light_color = select(raw_light_color, lil_light_color, is_liltoon);"),
			"Only the lilToon-like path should replace direct light with the lilToon lightColor approximation"
		);
	}

	#[test]
	fn liltoon_light_color_clamps_before_monochrome() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		let limited = mesh.find("let limited = clamp(raw").unwrap();
		let luminance = mesh.find("let luminance = dot(limited").unwrap();
		let monochrome = mesh.find("return mix(limited").unwrap();
		assert!(
			limited < luminance && luminance < monochrome,
			"lilToon _LightMinLimit/_LightMaxLimit should apply before _MonochromeLighting"
		);
	}

	#[test]
	fn liltoon_effects_and_specular_use_raw_shadowmix() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("var lil_shadowmix = 1.0;")
				&& mesh.contains("lil_shadowmix = lil_shadow;")
				&& mesh.contains("let lil_effect_shadowmix = select(lil_shadowmix, clamp(dot(n, l), 0.0, 1.0), is_liltoon_gem);"),
			"lilToon effect masks must use fd.shadowmix before _ShadowStrength is applied"
		);
		assert!(
			mesh.contains("specular_reflect = specular_reflect * select(1.0, lil_shadowmix, is_liltoon);"),
			"lilToon screen-shadow specular path attenuates specular by fd.shadowmix"
		);
	}

	#[test]
	fn liltoon_reflection_uses_geometric_specular_antialiasing() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn lil_gsaa_smoothness")
				&& mesh.contains("let dx = abs(dpdx(normal_ws));")
				&& mesh.contains("let dy = abs(dpdy(normal_ws));")
				&& mesh.contains("dxy / (dxy * 5.0 + 0.002)"),
			"lilToon reflection must match GSAAForSmoothness"
		);
		assert!(
			mesh.contains("smoothness = lil_gsaa_smoothness(smoothness, n, drawu.rendering_ext_params.x);"),
			"_GSAAStrength must reduce reflection smoothness before roughness is derived"
		);
	}

	#[test]
	fn liltoon_reflection_without_source_cube_uses_environment_fallback() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn liltoon_environment_reflection")
				&& mesh.contains("let fallback_env = liltoon_environment_reflection(perceptual_roughness);")
				&& mesh.contains("let uses_source_cube = drawu.rendering_ext_params.y > 0.5;"),
			"lilToon reflection without _ReflectionCubeOverride must use environment reflection instead of black cube"
		);
		assert!(
			mesh.contains("let env = select(fallback_env, source_cube_env * cube_tint * reflection_lighting, uses_source_cube);"),
			"_ReflectionCubeColor and _ReflectionCubeEnableLighting apply only to source-cube reflection"
		);
	}

	#[test]
	fn liltoon_transparent_premultiplies_before_reflection_effects() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		let premultiply = mesh
			.find("let lil_premultiplied_before_effects = is_liltoon && alpha_kind > 1.5 && !is_liltoon_additive_blend;")
			.expect("lilToon transparent premultiply flag");
		let reflection = premultiply
			+ mesh[premultiply..]
				.find("let reflection_color_uv = uv * drawu.reflection_color_uv_offset_scale.zw + drawu.reflection_color_uv_offset_scale.xy;")
				.expect("lilToon reflection blend block");
		assert!(premultiply < reflection, "lilToon LIL_PREMULTIPLY runs before reflection");
		assert!(
			mesh.contains("!is_liltoon_additive_blend && !lil_premultiplied_before_effects"),
			"final blend premultiply must not multiply lilToon transparent output twice"
		);
	}

	#[test]
	fn liltoon_transparent_emission_blend_uses_alpha() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let emission_transparency = select(1.0, out_a, alpha_kind > 1.5);"),
			"lilToon transparent emission blend must use fd.col.a"
		);
		assert!(
			mesh.contains("drawu.emission_color.a * emission_tex_color.a * emission_transparency")
				&& mesh.contains("emission2nd_blink * emission2nd_sample.a * emission_transparency"),
			"both lilToon emission layers must apply transparent alpha to their blend"
		);
	}

	#[test]
	fn liltoon_backface_color_runs_before_distance_fade() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		let dissolve_add = mesh.find("lit = lit + drawu.dissolve_color.rgb * dissolve_result.y").unwrap();
		let backface = mesh.find("drawu.backface_color.rgb * effect_light_color").unwrap();
		let distance_fade = mesh.find("let distance_faded = select").unwrap();
		assert!(
			dissolve_add < backface && backface < distance_fade,
			"lilToon _BackfaceColor should run after emission/dissolve and before distance fade"
		);
	}

	#[test]
	fn liltoon_rim_shade_runs_before_reflection_and_matcap() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		let rim_shade = mesh
			.find("lil_apply_rim_shade(lit, geometry_n, n, v, uv), is_liltoon && !is_liltoon_gem && !is_fur_pass")
			.expect("lilToon rim shade application");
		let backlight = mesh
			.find("if (is_liltoon && !is_liltoon_gem && drawu.backlight_params.x > 0.5)")
			.expect("lilToon backlight block");
		let reflection = backlight
			+ mesh[backlight..]
				.find("let reflection_color_uv = uv * drawu.reflection_color_uv_offset_scale.zw + drawu.reflection_color_uv_offset_scale.xy;")
				.expect("lilToon reflection blend block");
		let matcap = reflection
			+ mesh[reflection..]
				.find("if (drawu.matcap_params.x > 0.0)")
				.expect("lilToon matcap block");
		assert!(
			rim_shade < backlight && backlight < reflection && reflection < matcap,
			"lilToon pass order is RimShade -> Backlight -> Reflection -> MatCap -> RimLight"
		);
	}
}
