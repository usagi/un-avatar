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
		validate_wgsl(
			"startup_progress_overlay.wgsl",
			include_str!("../shaders/startup_progress_overlay.wgsl"),
		);
		validate_wgsl("wardrobe_billboard.wgsl", include_str!("../shaders/wardrobe_billboard.wgsl"));
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
	fn startup_progress_overlay_does_not_embed_wardrobe_transition_art() {
		let startup = include_str!("../shaders/startup_progress_overlay.wgsl");
		let wardrobe = include_str!("../shaders/wardrobe_billboard.wgsl");
		for forbidden in ["wardrobe", "hanger", "garment", "changing"] {
			assert!(
				!startup.contains(forbidden),
				"startup progress overlay must not carry wardrobe transition art token `{forbidden}`"
			);
		}
		for expected in ["hanger", "garment"] {
			assert!(
				wardrobe.contains(expected),
				"wardrobe transition shader should own wardrobe art token `{expected}`"
			);
		}
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
		assert!(
			mesh.contains("let rim_indirect_color = drawu.rim_indirect_color.rgb * rim_tex_color.rgb;")
				&& mesh.contains(
					"let rim_light_mul = mix(vec3<f32>(1.0, 1.0, 1.0), effect_light_color, clamp(drawu.rim_control.z, 0.0, 1.0));"
				) && mesh.contains("drawu.rim_params.w * drawu.rim_indirect_color.a * rim_tex_color.a"),
			"lilToon direction rim must apply RimColorTex, RimEnableLighting, and RimIndirColor alpha to the indirect rim"
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
			"MatCap custom normal maps must be bound in the high-capability mesh shader variant"
		);
		assert!(
			mesh.contains("let map_uv = uv * uv_offset_scale.zw + uv_offset_scale.xy;")
				&& mesh.contains("lil_unpack_normal_scale(packed, scale)")
				&& mesh.contains("textureSample(matcap_bump_tex, normal_samp, map_uv)")
				&& mesh.contains("textureSample(matcap2_bump_tex, normal_samp, map_uv)"),
			"MatCap custom normal maps must use their texture slot transform"
		);
		assert!(
			mesh.contains("if (UNTOON_FEATURE_MATCAP_CUSTOM_NORMAL > 0.5 && drawu.matcap_bump_params.x > 0.5)")
				&& mesh.contains("if (UNTOON_FEATURE_MATCAP_CUSTOM_NORMAL > 0.5 && drawu.matcap2_bump_params.x > 0.5)")
				&& mesh.contains("drawu.matcap_bump_params.y")
				&& mesh.contains("drawu.matcap2_bump_params.y"),
			"MatCap custom normal flags and bump scales must reach the MatCap normal path"
		);
	}

	#[test]
	fn liltoon_matcap_uv_matches_source_controls() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn toon_matcap_uv(")
				&& mesh.contains("clamp(uv1, vec2<f32>(0.0), vec2<f32>(1.0)) * 2.0 - vec2<f32>(1.0)")
				&& mesh.contains("uv_mat = uv_mat * uv_offset_scale.zw + uv_offset_scale.xy;"),
			"lilToon MatCap UV must apply _MatCapBlendUV1 and _MatCapTex_ST like lilCalcMatCapUV"
		);
		assert!(
			mesh.contains("vec3<f32>(frame.view[0][2], frame.view[1][2], frame.view[2][2])")
				&& mesh.contains("select(camera_dir, v, perspective >= 0.5)")
				&& mesh.contains("vec3<f32>(frame.view[0][1], frame.view[1][1], frame.view[2][1])")
				&& mesh.contains("select(camera_up, vec3<f32>(0.0, 1.0, 0.0), z_rot_cancel >= 0.5)"),
			"lilToon MatCap UV must use view-matrix camera direction/up and boolean perspective/z-rotation controls"
		);
		assert!(
			mesh.contains("drawu.matcap_tex_uv_offset_scale, drawu.matcap_uv_ext_params.xy")
				&& mesh.contains("drawu.matcap2_tex_uv_offset_scale, drawu.matcap_uv_ext_params.zw"),
			"both MatCap slots must route their own ST and BlendUV1 controls"
		);
	}

	#[test]
	fn liltoon_tbn_applies_object_negative_scale() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn object_negative_scale_sign() -> f32")
				&& mesh.contains("determinant(mat3_upper(drawt.model)) < 0.0")
				&& mesh.contains("let tangent_sign = select(1.0, -1.0, v.tangent.w < 0.0) * object_negative_scale_sign();"),
			"lilToon TBN bitangent must include LIL_NEGATIVE_SCALE semantics"
		);
	}

	#[test]
	fn liltoon_normals_use_inverse_transpose_model_matrix() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn normal_matrix(m: mat4x4<f32>) -> mat3x3<f32>")
				&& mesh.contains("return mat3x3<f32>(cross(c1, c2), cross(c2, c0), cross(c0, c1)) * (1.0 / det);")
				&& mesh.contains("let mn = normal_matrix(drawt.model) * norm;")
				&& mesh.contains("let wn = normalize(normal_matrix(drawt.model) * local_n);"),
			"lilToon normalWS must match TransformObjectToWorldNormal semantics under non-uniform scale"
		);
	}

	#[test]
	fn liltoon_shadow_masks_preserve_rgb_channels() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("textureSample(shadow_border_mask_tex, shadow_border_mask_samp, uv).rgb"),
			"lilToon uses _ShadowBorderMask rgb for first, second, and third shadow AO"
		);
		assert!(
			mesh.contains("textureSample(shadow_blur_mask_tex, shadow_blur_mask_samp, uv).rgb"),
			"lilToon uses _ShadowBlurMask rgb for first, second, and third shadow blur"
		);
		assert!(
			!mesh.contains("let shadow_border_mask_uv") && !mesh.contains("let shadow_blur_mask_uv"),
			"lilToon shadow border/blur masks sample fd.uvMain directly without slot ST"
		);
		assert!(
			mesh.contains("let shadow_strength_mask = textureSample(shading_shift_tex, shading_shift_samp, uv);")
				&& !mesh.contains("let shadow_strength_mask_uv"),
			"lilToon _ShadowStrengthMask also samples fd.uvMain directly"
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
	fn liltoon_transparent_premultiply_uses_alpha_boost_without_boosting_output_alpha() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn fragment_out_alpha(alpha_kind: f32, a: f32, base_color_a: f32) -> f32 {\n\tif alpha_kind > 1.5 {\n\t\treturn clamp(a, 0.0, 1.0);"),
			"lilToon transparent output alpha remains fd.col.a; _AlphaBoostFA only affects premultiplied rgb"
		);
		assert!(
			mesh.contains("return rgb * clamp(out_a * max(drawu.alpha_mask_params.w, 0.0), 0.0, 1.0);"),
			"lilToon LIL_PREMULTIPLY multiplies transparent rgb by saturate(alpha * _AlphaBoostFA)"
		);
		assert!(
			mesh.contains("let lil_premultiply_alpha_boost = clamp(out_a * max(drawu.alpha_mask_params.w, 0.0), 0.0, 1.0);")
				&& mesh.contains("lit = mix(lit, lit * lil_premultiply_alpha_boost, select(0.0, 1.0, lil_premultiplied_before_effects));"),
			"main toon transparent path must use _AlphaBoostFA when it premultiplies before lighting effects"
		);
	}

	#[test]
	fn liltoon_shadow_color_textures_use_main_uv_without_slot_transform() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let shade_texel = textureSample(shade_tex, shade_samp, uv);")
				&& mesh.contains("let shadow2_color_texel = textureSample(shadow2_color_tex, shade_samp, uv);")
				&& mesh.contains("let shadow3_color_texel = textureSample(shadow3_color_tex, shade_samp, uv);"),
			"lilToon shadow color textures must sample fd.uvMain directly"
		);
		assert!(
			!mesh.contains("let shade_uv = uv * drawu.shade_uv_offset_scale"),
			"_ShadowColorTex/_Shadow2ndColorTex/_Shadow3rdColorTex must not apply slot ST"
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
			mesh.contains("lil_shadow_apply_pre_ao")
				&& mesh.contains("lil_shadow_apply_post_ao")
				&& mesh.contains("fn lil_tooning_no_saturate_scale_range")
				&& mesh.contains("clamp(lil_shadow_apply_post_ao(lil_shadow_raw, shadow_border_mask.r, shadow_post_ao), 0.0, 1.0)"),
			"shadow border AO must match lilToon's no-saturate tooning, then final lns saturate"
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
		assert!(
			!mesh.contains("let aa_blur = blur * aa_scale"),
			"lilToon _AAStrength scales derivative AA, not the authored _ShadowBlur width"
		);
	}

	#[test]
	fn liltoon_shadow_border_range_only_affects_border_color_gradation() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert_eq!(
			mesh.matches("lil_tooning_no_saturate_scale(").count(),
			4,
			"the helper plus primary, second, and third shadows must use the four-argument lilToon overload"
		);
		assert_eq!(
			mesh.matches("lil_tooning_no_saturate_scale_range(").count(),
			2,
			"_ShadowBorderRange belongs only to its helper and lns.w border-color gradation"
		);
		let border_mix = mesh.find("let border_mix_raw = lil_tooning_no_saturate_scale_range(").unwrap();
		assert!(
			mesh[border_mix..].contains("clamp(drawu.shadow_ext_params.x, 0.0, 1.0)"),
			"lilToon _ShadowBorderRange must reach only the border-color gradation"
		);
	}

	#[test]
	fn liltoon_sdf_shadow_matches_authored_mask_channels_and_face_axes() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let shadow_mask_type = drawu.shadow_ao_params.y;")
				&& mesh.contains("dot(l.xz, face_right.xz) < 0.0")
				&& mesh.contains("select(shadow_strength_mask.r, shadow_strength_mask.g")
				&& mesh.contains("mix(sdf_value, primary_value, shadow_strength_mask.b)")
				&& mesh.contains("primary_strength_mask = shadow_strength_mask.a")
				&& mesh.contains("shadow_aa_scale = select(max(drawu.alpha_ext_params.y, 0.0), 0.0, shadow_mask_type >= 1.5)"),
			"lilToon _ShadowMaskType=2 must use RG for mirrored SDF, B for normal/SDF blend, A for strength, and disable derivative AA"
		);
	}

	#[test]
	fn liltoon_flat_shadow_uses_authored_flat_axis_and_strength_mask() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("shadow_mask_type >= 0.5 && shadow_mask_type < 1.5")
				&& mesh.contains("drawt.model * vec4<f32>(0.0, 0.25, 1.0, 0.0)")
				&& mesh.contains("(dot(flat_normal, l) + drawu.shadow_ao_params.z) / max(drawu.shadow_ao_params.w, 0.000001)")
				&& mesh.contains("lil_shadow = mix(flat_shadow, lil_shadow, shadow_strength_mask.r)")
				&& mesh.contains("primary_strength_mask = 1.0"),
			"lilToon _ShadowMaskType=1 must mix its flat shadow into all lns channels before applying unmasked _ShadowStrength"
		);
	}

	#[test]
	fn liltoon_shadow_extra_colors_mix_before_light_color() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		let start = mesh
			.find("var indirect_col = shade_term;")
			.expect("lilToon first shadow color starts before lightColor multiplication");
		let shadow2 = mesh[start..]
			.find("indirect_col = mix(indirect_col, shadow2_color, shadow2_strength);")
			.expect("second shadow color should mix before lightColor");
		let shadow3 = mesh[start..]
			.find("indirect_col = mix(indirect_col, shadow3_color, shadow3_strength);")
			.expect("third shadow color should mix before lightColor");
		let light = mesh[start..]
			.find("indirect_col = indirect_col * lil_light_color;")
			.expect("lightColor should be applied once after shadow colors are resolved");
		assert!(
			shadow2 < light && shadow3 < light,
			"lilToon resolves Shadow2nd/Shadow3rd colors before multiplying indirectCol by fd.lightColor"
		);
		assert!(
			!mesh.contains("shadow2_color * lil_light_color") && !mesh.contains("shadow3_color * lil_light_color"),
			"extra shadow colors must not receive lightColor before they are mixed into indirectCol"
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
	fn liltoon_gem_refraction_runs_even_when_strength_is_zero() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains(
				"let refract_r = textureSample(screen_tex, screen_samp, clamp(base_screen_uv + screen_offset * refraction_strength"
			),
			"lilToon Gem must sample refraction with the authored strength"
		);
		assert!(
			mesh.contains("let refract_g = textureSample(screen_tex, screen_samp, clamp(base_screen_uv + screen_offset * (refraction_strength + chroma)"),
			"lilToon Gem chromatic aberration must still affect G when refraction strength is zero"
		);
		assert!(
			!mesh.contains("if (abs(refraction_strength) > 0.00001) {\n\t\t\t\tlet refraction_fresnel"),
			"lilToon Gem refraction is not gated by _RefractionStrength in upstream lilToon"
		);
	}

	#[test]
	fn liltoon_refraction_uses_material_alpha_for_refracted_mix() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("lit = mix(refract_color, lit, clamp(a, 0.0, 1.0));"),
			"lilToon Refraction must use fd.col.a equivalent for background refraction blending"
		);
		assert!(
			!mesh.contains("lit = mix(refract_color, lit, clamp(out_a, 0.0, 1.0));"),
			"opaque render output alpha must not replace the material alpha used by lilToon Refraction"
		);
	}

	#[test]
	fn liltoon_reflection_apply_transparency_is_transparent_only() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let liltoon_apply_effect_transparency = alpha_kind > 1.5 && !is_untoon_refraction_profile;"),
			"lilToon effect transparency flags apply only in transparent render mode and not in refraction"
		);
		assert!(
			mesh.contains("let reflection_apply_transparency = select(clamp(drawu.transparency_params.w, 0.0, 1.0), 0.0, !liltoon_apply_effect_transparency);")
				&& mesh.contains("let matcap_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.x, 0.0, 1.0), 0.0, !liltoon_apply_effect_transparency));")
				&& mesh.contains("let matcap2_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.y, 0.0, 1.0), 0.0, !liltoon_apply_effect_transparency));")
				&& mesh.contains("let rim_transparency = mix(1.0, a, select(clamp(drawu.transparency_params.z, 0.0, 1.0), 0.0, !liltoon_apply_effect_transparency));")
				&& mesh.contains("glitter_alpha = mix(glitter_alpha, glitter_alpha * out_a, select(clamp(drawu.glitter_ext.w, 0.0, 1.0), 0.0, !liltoon_apply_effect_transparency));")
				&&
			mesh.contains("let reflection_transparency = mix(1.0, a, reflection_apply_transparency);"),
			"MatCap, Rim, Glitter, and Reflection transparency should use the gated transparent-only factor"
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
			mesh.contains("textureSample(main2nd_blend_mask_tex, base_samp, uv).r")
				&& mesh.contains("textureSample(main3rd_blend_mask_tex, base_samp, uv).r")
				&& !mesh.contains("main2nd_blend_mask_tex, base_samp, mask_uv")
				&& !mesh.contains("main3rd_blend_mask_tex, base_samp, mask_uv"),
			"Main layer blend masks must sample fd.uvMain without slot ST like upstream lilToon"
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
	fn liltoon_main_color_adjust_mask_limits_hsvg() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn apply_main_color_adjustments(color: vec3<f32>, uv: vec2<f32>) -> vec3<f32>")
				&& mesh.contains("let main_color_adjust_mask = textureSample(main_color_adjust_mask_tex, base_samp, uv).r;")
				&& mesh.contains("let adjusted = apply_main_gradation(apply_main_hsvg(color));")
				&& mesh.contains("return mix(color, adjusted, clamp(main_color_adjust_mask, 0.0, 1.0));"),
			"_MainColorAdjustMask must limit lilToon main HSV/Gamma and gradation adjustments using fd.uvMain"
		);
		assert!(
			mesh.contains("let c = linear_to_srgb(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)));")
				&& mesh.contains("let mapped = srgb_to_linear(mapped_srgb);"),
			"Main gradation map must use lilToon linear-space sRGB lookup and output conversion"
		);
		assert!(
			mesh.contains("var hsv = rgb_to_hsv(pow(abs(color), vec3<f32>(p.w)));")
				&& mesh.contains("hsv.x = hsv.x + p.x;")
				&& mesh.contains("hsv.z = clamp(hsv.z * p.z, 0.0, 1.0);"),
			"lilToon main tone correction applies gamma before HSV shift and saturates value"
		);
		assert!(
			mesh.contains("apply_main_color_adjustments(samp_tex.rgb, uv)"),
			"main texture HSV/Gamma adjustment should receive fd.uvMain for its mask"
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
			mesh.contains(
				"let normal2nd_scale_mask_uv = uv * drawu.normal2nd_scale_mask_uv_offset_scale.zw + drawu.normal2nd_scale_mask_uv_offset_scale.xy;"
			)
				&& mesh.contains(
					"let scale_mask = textureSample(normal2nd_scale_mask_tex, base_samp, normal2nd_scale_mask_uv).r;"
				)
				&& mesh.contains(
					"lil_unpack_normal_scale(textureSample(normal2nd_tex, normal_samp, normal2nd_uv), drawu.normal2nd_params.y * scale_mask)"
				),
			"_Bump2ndScaleMask must use fd.uvMain with its slot ST and multiply _BumpScale2nd"
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
	fn liltoon_light_color_matches_brp_sh_direct_and_indirect() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn liltoon_raw_light_color(light_dir_un: vec3<f32>) -> vec3<f32>")
				&& mesh.contains("let main_light = frame.light_color.rgb * frame.light_color.w;")
				&& mesh.contains("unity_openlit_sh_direct(light_dir_un)")
				&& mesh.contains("return main_light + sh_direct_proxy;")
				&& mesh.contains("fn liltoon_light_color(light_dir_un: vec3<f32>) -> vec3<f32>")
				&& mesh.contains("return lil_correct_light_color(liltoon_raw_light_color(light_dir_un));"),
			"lilToon/OpenLit BRP direct light is directional plus OpenLit SH direct; scene SH must be used when exported"
		);
		assert!(
			mesh.contains("fn liltoon_indirect_light_color(light_dir_un: vec3<f32>) -> vec3<f32>")
				&& mesh.contains("fn unity_openlit_sh_indirect(n_un: vec3<f32>) -> vec3<f32>")
				&& mesh.contains("let sh_dir_unity = safe_normalize_or(frame.sh_ar.xyz + frame.sh_ag.xyz + frame.sh_ab.xyz, n_unity);")
				&& mesh.contains("return unity_sh_l0_l2(n_unity) + unity_openlit_sh_l1(indirect_dir_unity);")
				&& mesh.contains("let indirect = select(fallback, unity_openlit_sh_indirect(light_dir_un), unity_sh_available());")
				&& mesh.contains("return clamp(indirect, vec3<f32>(0.0), vec3<f32>(1.0));")
				&& mesh.contains("let lil_light_color = liltoon_light_color(l);")
				&& mesh.contains("let lil_indirect_light_color = liltoon_indirect_light_color(l);")
				&& mesh.contains("clamp(lil_indirect_light_color * drawu.shadow_ext_params.z, vec3<f32>(0.0), vec3<f32>(1.0))"),
			"lilToon BRP shadow Environment Light must use OpenLit fd.indLightColor = saturate(indirectLight), not the non-BRP res-l1 path"
		);
		assert!(
			mesh.contains("fn untoon_dir_to_unity_sh_dir(n: vec3<f32>) -> vec3<f32>")
				&& mesh.contains("return vec3<f32>(-n.x, n.y, n.z);")
				&& mesh.contains("fn unity_sh_l0_l2(n_unity: vec3<f32>) -> vec3<f32>")
				&& mesh.contains("let v_b = n_unity.xyzz * n_unity.yzzx;"),
			"exported raw Unity SH coefficients must be evaluated in Unity axes with lilToon/OpenLit basis terms"
		);
		assert!(
			mesh.contains("let direct_color = mix(shade_term, base, shading) * lil_light_color;\n\t\tlit = direct_color * authored_occlusion(uv, dbg);"),
			"lilToon no-shadow path should be fd.col.rgb *= fd.lightColor, without a second ambient add"
		);
		assert!(
			mesh.contains("lit = min(lit, base * max(drawu.lighting_ext_params.y, drawu.lighting_ext_params.x));")
				&& mesh
					.find("lit = min(lit, base * max(drawu.lighting_ext_params.y, drawu.lighting_ext_params.x));")
					.unwrap() < mesh
					.find("if (UNTOON_FEATURE_MAIN_LAYERS > 0.5 && !is_untoon_gem_profile)")
					.unwrap(),
			"lilToon forward pass clamps lit body color to fd.albedo * _LightMaxLimit before unlit main layers and effects"
		);
		assert!(
			mesh.contains("let effect_light_color = lil_light_color;"),
			"UNToon runtime should use the lilToon lightColor approximation without a source-profile branch"
		);
	}

	#[test]
	fn liltoon_light_direction_uses_material_override() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn liltoon_light_direction(base_dir: vec3<f32>) -> vec3<f32>")
				&& mesh.contains("let main_dir = base_dir * lil_light_direction_luminance(main_light);")
				&& mesh.contains("let sh9_dir_unity = (frame.sh_ar.xyz + frame.sh_ag.xyz + frame.sh_ab.xyz) * 0.333333;")
				&& mesh.contains("let sh9_dir_abs_unity = vec3<f32>(sh9_dir_unity.x, abs(sh9_dir_unity.y), sh9_dir_unity.z);")
				&& mesh.contains("unity_sh_dir_to_untoon_dir(sh9_dir_abs_unity)")
				&& mesh.contains("let custom_dir = unity_sh_dir_to_untoon_dir(drawu.light_direction_override.xyz);")
				&& mesh.contains("let l = liltoon_light_direction(base_l);"),
			"lilToon/OpenLit BRP shadow lighting must match OpenLit ComputeLightDirection by folding the SH y component upward before material _LightDirectionOverride"
		);
	}

	#[test]
	fn liltoon_light_color_clamps_before_monochrome() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		let limited = mesh.find("let limited = clamp(raw").unwrap();
		let gray = mesh.find("let gray = dot(limited").unwrap();
		let monochrome = mesh.find("let monochrome = mix(limited").unwrap();
		let as_unlit = mesh.find("return mix(monochrome, vec3<f32>(1.0)").unwrap();
		assert!(
			limited < gray && gray < monochrome && monochrome < as_unlit,
			"lilToon _LightMinLimit/_LightMaxLimit should apply before _MonochromeLighting and _AsUnlit"
		);
	}

	#[test]
	fn liltoon_effects_and_specular_use_raw_shadowmix() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("var lil_shadowmix = 1.0;")
				&& mesh.contains("lil_shadowmix = raw_lil_shadow;")
				&& mesh.contains("let lil_effect_shadowmix = select(lil_shadowmix, clamp(dot(n, l), 0.0, 1.0), is_untoon_gem_profile);"),
			"lilToon effect masks must use fd.shadowmix before _ShadowStrength is applied"
		);
		assert!(
			mesh.contains("specular_reflect = specular_reflect * lil_shadowmix;"),
			"lilToon screen-shadow specular path attenuates specular by fd.shadowmix"
		);
	}

	#[test]
	fn liltoon_backface_force_shadow_applies_to_all_shadow_channels() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let backface_shadow = select(1.0 - clamp(drawu.material_ext_params.y, 0.0, 1.0), 1.0, front_facing);")
				&& mesh.contains("shadow_post_ao), 0.0, 1.0) * backface_shadow;"),
			"lilToon _BackfaceForceShadow must attenuate the primary shadow before fd.shadowmix is copied"
		);
		assert_eq!(
			mesh.matches("* backface_shadow;").count(),
			4,
			"lilToon _BackfaceForceShadow must attenuate primary, second, third, and border shadow channels"
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
				&& mesh.contains("let uses_source_cube = drawu.rendering_ext_params.y > 0.5 && UNTOON_FEATURE_REFLECTION_CUBE > 0.5;"),
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
			.find("let lil_premultiplied_before_effects = alpha_kind > 1.5 && !is_untoon_additive_blend;")
			.expect("lilToon transparent premultiply flag");
		let reflection = premultiply
			+ mesh[premultiply..]
				.find(
					"let reflection_color_uv = uv * drawu.reflection_color_uv_offset_scale.zw + drawu.reflection_color_uv_offset_scale.xy;",
				)
				.expect("lilToon reflection blend block");
		assert!(premultiply < reflection, "lilToon LIL_PREMULTIPLY runs before reflection");
		assert!(
			mesh.contains("!is_untoon_additive_blend && !lil_premultiplied_before_effects"),
			"final blend premultiply must not multiply lilToon transparent output twice"
		);
	}

	#[test]
	fn liltoon_transparent_emission_blend_uses_alpha() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let emission_transparency = select(1.0, out_a, liltoon_apply_effect_transparency);"),
			"lilToon transparent non-refraction emission blend must use fd.col.a"
		);
		assert!(
			mesh.contains("drawu.emission_color.a * emission_tex_color.a * emission_audio * emission_transparency",)
				&& mesh.contains("emission2nd_blink * emission2nd_sample.a * emission2nd_audio * emission_transparency"),
			"both lilToon emission layers must apply transparent alpha to their blend"
		);
	}

	#[test]
	fn liltoon_audio_link_drives_emission_like_upstream() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("fn lil_calc_audio_link_value(nv: f32, uv0: vec2<f32>, uv1: vec2<f32>, uv2: vec2<f32>, uv3: vec2<f32>, op: vec3<f32>) -> f32")
				&& mesh.contains("if (drawu.audio_link_params.x <= 0.5) {\n\t\treturn 1.0;"),
			"AudioLink disabled path must preserve lilToon fd.audioLinkValue default"
		);
		assert!(
			mesh.contains("audio_link_value * drawu.audio_link_params.w")
				&& mesh.contains("audio_link_value * drawu.audio_link_ext.y")
				&& mesh.contains("mix(1.0, audio_link_value, clamp(drawu.audio_link_params.z, 0.0, 1.0))")
				&& mesh.contains("mix(1.0, audio_link_value, clamp(drawu.audio_link_ext.x, 0.0, 1.0))"),
			"AudioLink must affect emission alpha and gradation offsets in the lilToon order"
		);
		assert!(
			mesh.contains("audio_link_mask = textureSample(audio_link_mask_tex, base_samp, uv_mask);")
				&& mesh.contains("value = textureSample(audio_link_local_map_tex, base_samp, vec2<f32>(local_x, audio_link_y)).r;")
				&& mesh.contains("let offset_os = norm * drawu.audio_link_vertex_strength.w + drawu.audio_link_vertex_strength.xyz;"),
			"AudioLink mask, local map, and vertex moving vector must follow lilToon source semantics"
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
			.find(
				"lil_apply_rim_shade(lit, geometry_n, n, v, uv), UNTOON_FEATURE_RIM_SHADE > 0.5 && !is_untoon_gem_profile && !is_fur_pass",
			)
			.expect("lilToon rim shade application");
		let backlight = mesh
			.find("if (UNTOON_FEATURE_BACKLIGHT > 0.5 && !is_untoon_gem_profile && drawu.backlight_params.x > 0.5)")
			.expect("lilToon backlight block");
		let reflection = backlight
			+ mesh[backlight..]
				.find(
					"let reflection_color_uv = uv * drawu.reflection_color_uv_offset_scale.zw + drawu.reflection_color_uv_offset_scale.xy;",
				)
				.expect("lilToon reflection blend block");
		let matcap = reflection
			+ mesh[reflection..]
				.find("if (UNTOON_FEATURE_MATCAP > 0.5 && drawu.matcap_params.x > 0.0)")
				.expect("lilToon matcap block");
		assert!(
			rim_shade < backlight && backlight < reflection && reflection < matcap,
			"lilToon pass order is RimShade -> Backlight -> Reflection -> MatCap -> RimLight"
		);
	}

	#[test]
	fn liltoon_effect_normal_strength_keeps_lerp_length() {
		let mesh = include_str!("../shaders/mesh.wgsl");
		assert!(
			mesh.contains("let specular_base_n = mix(geometry_n, n, clamp(drawu.specular_toon_params.w, 0.0, 1.0));")
				&& mesh.contains("let reflection_base_n = mix(geometry_n, n, clamp(drawu.reflection_params.w, 0.0, 1.0));"),
			"lilToon specular/reflection normal strength uses raw lerp before anisotropy"
		);
		assert!(
			mesh.contains("let rim_shade_n = mix(geometry_n, n, clamp(drawu.rim_ext_params.w, 0.0, 1.0));")
				&& mesh.contains("let backlight_n = mix(geometry_n, n, clamp(drawu.backlight_params.z, 0.0, 1.0));"),
			"lilToon rim shade/backlight normal strength should not renormalize the lerped normal"
		);
		assert!(
			mesh.contains("let matcap2_n = select(matcap2_base_n, normalize(mix(matcap2_base_n, anisotropy_n, matcap2_anisotropy)), matcap2_anisotropy > 0.0);"),
			"lilToon MatCap2nd keeps raw normal-strength lerp when anisotropy is disabled"
		);
	}
}
