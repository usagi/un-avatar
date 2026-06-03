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
}
