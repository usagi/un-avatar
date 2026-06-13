pub(crate) fn enabled(value: f32) -> bool {
	value > 0.5
}

pub(crate) fn uses_main_color_adjustment(main: &un_avatar_core::UnaLilToonLikeMainColor) -> bool {
	main.main_texture_hsvg_factor
		.iter()
		.zip([0.0, 1.0, 1.0, 1.0])
		.any(|(value, default)| (*value - default).abs() > 0.00001)
		|| enabled(main.gradation_enabled_factor)
}

pub(crate) fn id_mask_has_runtime_controls(id_mask: &un_avatar_core::UnaLilToonLikeIdMask) -> bool {
	id_mask.flags_factor.iter().any(|value| *value > 0.0001)
		|| id_mask.prior_flags_factor.iter().any(|value| *value > 0.0001)
		|| id_mask.controls_dissolve_factor > 0.0001
}

pub(crate) fn uses_id_mask(id_mask: &un_avatar_core::UnaLilToonLikeIdMask) -> bool {
	id_mask.compile_factor > 0.5 || id_mask_has_runtime_controls(id_mask)
}

pub(crate) fn udim_discard_has_runtime_rows(udim: &un_avatar_core::UnaLilToonLikeUdimDiscard) -> bool {
	udim.row0_factor.iter().any(|value| *value > 0.0001)
		|| udim.row1_factor.iter().any(|value| *value > 0.0001)
		|| udim.row2_factor.iter().any(|value| *value > 0.0001)
		|| udim.row3_factor.iter().any(|value| *value > 0.0001)
}

pub(crate) fn uses_udim_discard(udim: &un_avatar_core::UnaLilToonLikeUdimDiscard) -> bool {
	udim.compile_factor > 0.5 || udim_discard_has_runtime_rows(udim)
}
