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
}
