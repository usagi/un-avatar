export const OUTLINE_COLOR_FALLBACK: [number, number, number] = [0.02, 0.01, 0.03];

export const OUTLINE_RANGE_FIELDS = [
	{
		key: "outline_width",
		labelKey: "profiles.editor.look_outline_width",
		hintKey: "profiles.hints.look.outline_width",
		field: "effects.avatar.outline.width",
		fallback: 0.003,
		scale: 1000,
		rangeMin: 0,
		rangeMax: 50,
		step: 0.5,
		decimals: 1,
	},
	{
		key: "outline_roundness",
		labelKey: "profiles.editor.look_outline_roundness",
		hintKey: "profiles.hints.look.outline_roundness",
		field: "effects.avatar.outline.roundness",
		fallback: 0.5,
		scale: 1,
		rangeMin: 0,
		rangeMax: 1,
		step: 0.01,
		decimals: 2,
	},
] as const;

export const OUTLINE_NUMBER_FIELDS = [
	{
		key: "outline_lighting_mix",
		labelKey: "profiles.editor.look_outline_lighting",
		hintKey: "profiles.hints.look.outline_lighting",
		field: "effects.avatar.outline.lighting_mix",
		fallback: 0,
		min: 0,
		max: 1,
		step: 0.05,
		decimals: 2,
	},
] as const;

export const CONTACT_SHADOW_RANGE_FIELDS = [
	{
		key: "contact_shadow_strength",
		labelKey: "profiles.editor.look_shadow_strength",
		hintKey: "profiles.hints.look.shadow_strength",
		field: "effects.avatar.contact_shadow.strength",
		rangeMin: 0,
		rangeMax: 1,
		step: 0.01,
		decimals: 2,
	},
	{
		key: "contact_shadow_radius",
		labelKey: "profiles.editor.look_shadow_radius",
		hintKey: "profiles.hints.look.shadow_radius",
		field: "effects.avatar.contact_shadow.radius",
		rangeMin: 0.05,
		rangeMax: 3,
		step: 0.05,
		decimals: 2,
	},
	{
		key: "contact_shadow_softness",
		labelKey: "profiles.editor.look_shadow_softness",
		hintKey: "profiles.hints.look.shadow_softness",
		field: "effects.avatar.contact_shadow.softness",
		rangeMin: 0.1,
		rangeMax: 8,
		step: 0.1,
		decimals: 1,
	},
	{
		key: "contact_shadow_height",
		labelKey: "profiles.editor.look_shadow_height",
		hintKey: "profiles.hints.look.shadow_height",
		field: "effects.avatar.contact_shadow.height",
		rangeMin: -1,
		rangeMax: 1,
		step: 0.01,
		decimals: 2,
	},
] as const;

export const SSAO_RANGE_FIELDS = [
	{
		key: "ssao_strength",
		labelKey: "profiles.editor.look_ssao_strength",
		hintKey: "profiles.hints.look.ssao_strength",
		field: "effects.post.ssao.strength",
		rangeMin: 0,
		rangeMax: 1,
		step: 0.01,
	},
	{
		key: "ssao_radius",
		labelKey: "profiles.editor.look_ssao_radius",
		hintKey: "profiles.hints.look.ssao_radius",
		field: "effects.post.ssao.radius",
		rangeMin: 1,
		rangeMax: 24,
		step: 1,
	},
	{
		key: "ssao_bias",
		labelKey: "profiles.editor.look_ssao_bias",
		hintKey: "profiles.hints.look.ssao_bias",
		field: "effects.post.ssao.bias",
		decimals: 4,
		rangeMin: 0,
		rangeMax: 0.02,
		step: 0.0005,
	},
	{
		key: "ssao_range",
		labelKey: "profiles.editor.look_ssao_range",
		hintKey: "profiles.hints.look.ssao_range",
		field: "effects.post.ssao.range",
		decimals: 3,
		rangeMin: 0.001,
		rangeMax: 0.2,
		step: 0.001,
	},
] as const;

export const COLOR_BASIC_RANGE_FIELDS = [
	{
		key: "color_exposure",
		labelKey: "profiles.editor.look_exposure",
		hintKey: "profiles.hints.look.exposure",
		field: "environment.color.exposure",
		rangeMin: -2,
		rangeMax: 2,
		numberMin: -4,
		numberMax: 4,
		step: 0.05,
		decimals: 2,
	},
	{
		key: "color_contrast",
		labelKey: "profiles.editor.look_contrast",
		hintKey: "profiles.hints.look.contrast",
		field: "environment.color.contrast",
		rangeMin: 0,
		rangeMax: 2,
		numberMin: 0,
		numberMax: 4,
		step: 0.05,
		decimals: 2,
	},
	{
		key: "color_saturation",
		labelKey: "profiles.editor.look_saturation",
		hintKey: "profiles.hints.look.saturation",
		field: "environment.color.saturation",
		rangeMin: 0,
		rangeMax: 2,
		numberMin: 0,
		numberMax: 4,
		step: 0.05,
		decimals: 2,
	},
] as const;

export const COLOR_LOOK_OPTIONS = [
	["neutral", "profiles.editor.color_look_neutral"],
	["warm", "profiles.editor.color_look_warm"],
	["cool", "profiles.editor.color_look_cool"],
	["film", "profiles.editor.color_look_film"],
	["soft", "profiles.editor.color_look_soft"],
	["pop", "profiles.editor.color_look_pop"],
] as const;

export const BLOOM_QUALITY_OPTIONS = [
	["compact", "Compact"],
	["high_quality", "High quality"],
] as const;
