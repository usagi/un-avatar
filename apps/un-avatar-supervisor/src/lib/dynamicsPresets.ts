export type DynamicsCategoryOverrideSetting = {
	category: string;
	name: string;
	mode: "authored" | "override_verlet" | "override_xpbd";
	dynamics_group_count: number;
	solver: string;
	damping_configured: boolean;
	damping_half_life_ms: number;
	rest_response_configured: boolean;
	rest_response: number;
	shape_preservation_configured: boolean;
	shape_preservation: number;
	bounce_configured: boolean;
	bounce_scale: number;
	stretch_range_configured: boolean;
	stretch_range_scale: number;
	stretch_motion_configured: boolean;
	stretch_motion: number;
	motion_coupling_configured: boolean;
	motion_coupling: number;
	xpbd_compliance_configured: boolean;
	xpbd_compliance: number;
	constraint_iterations_configured: boolean;
	constraint_iterations: number;
	authored_rest_response: number;
	authored_shape_preservation: number;
	authored_xpbd_compliance: number;
};

export type DynamicsGroupOverrideSetting = {
	source_id: string;
	solver?: string;
	damping_half_life_ms?: number;
	rest_response?: number;
	shape_preservation?: number;
	bounce_scale?: number;
	stretch_range_scale?: number;
	stretch_motion?: number;
	motion_coupling?: number;
	xpbd_compliance?: number;
	constraint_iterations?: number;
};

export type DynamicsMatchOverrideSetting = {
	name?: string;
	source_id?: string;
	source_id_contains?: string[];
	source_id_regex?: string[];
	solver?: string;
	damping_half_life_ms?: number;
	rest_response?: number;
	shape_preservation?: number;
	bounce_scale?: number;
	stretch_range_scale?: number;
	stretch_motion?: number;
	motion_coupling?: number;
	xpbd_compliance?: number;
	constraint_iterations?: number;
};

export type DynamicsMatchOverrideTemplate = {
	key: string;
	labelKey: string;
	override: DynamicsMatchOverrideSetting;
};

export const DYNAMICS_ENABLED_FIELD = "dynamics_enabled";
export const DYNAMICS_OVERRIDE_FIELD_PREFIX = "physics.dynamics.solver.";
export const DYNAMICS_MATCH_OVERRIDE_FIELD = "physics.dynamics.solver.match_overrides";
export const DYNAMICS_GROUP_OVERRIDE_FIELD = "physics.dynamics.solver.group_overrides";
export const DYNAMICS_BONE_COLLIDER_FIELD_PREFIX = "physics.bone_colliders.";

export function dynamicsOverrideFieldPrefix(category: string): string {
	return `${DYNAMICS_OVERRIDE_FIELD_PREFIX}overrides.${category}`;
}

export const DYNAMICS_MODE_OPTIONS = [
	["authored", "profiles.editor.dynamics_mode_model_default"],
	["override_verlet", "profiles.editor.dynamics_mode_standard"],
	["override_xpbd", "profiles.editor.dynamics_mode_extended"],
] as const;

export const DYNAMICS_DAMPING_FIELD = {
	key: "damping_half_life_ms",
	labelKey: "profiles.editor.dynamics_damping",
	hintKey: "profiles.editor.dynamics_damping_hint",
	min: 1,
	max: 10000,
	step: 1,
} as const;

export const DYNAMICS_BOUNCE_FIELD = {
	key: "bounce_scale",
	labelKey: "profiles.editor.dynamics_bounce",
	hintKey: "profiles.editor.dynamics_bounce_hint",
	min: 0,
	max: 4,
	step: 0.05,
} as const;

export const DYNAMICS_SHAPE_FIELD = {
	key: "shape_preservation",
	labelKey: "profiles.editor.dynamics_shape_preservation",
	hintKey: "profiles.editor.dynamics_shape_preservation_hint",
	min: 0,
	max: 1,
	step: 0.01,
} as const;

export const DYNAMICS_MOTION_COUPLING_FIELD = {
	key: "motion_coupling",
	labelKey: "profiles.editor.dynamics_motion_coupling",
	hintKey: "profiles.editor.dynamics_motion_coupling_hint",
	min: 0,
	max: 1,
	step: 0.01,
} as const;

export const DYNAMICS_STRETCH_RANGE_FIELD = {
	key: "stretch_range_scale",
	labelKey: "profiles.editor.dynamics_stretch_range",
	hintKey: "profiles.editor.dynamics_stretch_range_hint",
	min: 0,
	max: 4,
	step: 0.05,
} as const;

export const DYNAMICS_STRETCH_MOTION_FIELD = {
	key: "stretch_motion",
	labelKey: "profiles.editor.dynamics_stretch_motion",
	hintKey: "profiles.editor.dynamics_stretch_motion_hint",
	min: 0,
	max: 1,
	step: 0.01,
} as const;

export const DYNAMICS_XPBD_FIELDS = [
	{
		key: "xpbd_compliance",
		labelKey: "profiles.editor.dynamics_xpbd_compliance",
		hintKey: "profiles.editor.dynamics_xpbd_compliance_hint",
		min: 0,
		max: 10,
		step: 0.001,
	},
	{
		key: "constraint_iterations",
		labelKey: "profiles.editor.dynamics_iterations",
		hintKey: "profiles.editor.dynamics_iterations_hint",
		min: 1,
		max: 32,
		step: 1,
	},
] as const;

export const DYNAMICS_VERLET_FIELDS = [
	{
		key: "rest_response",
		labelKey: "profiles.editor.dynamics_rest_response",
		hintKey: "profiles.editor.dynamics_rest_response_hint",
		min: 0,
		max: 1,
		step: 0.01,
	},
] as const;

export function defaultDynamicsCategoryOverrides(): DynamicsCategoryOverrideSetting[] {
	return ["hair", "ears", "tail", "cloth", "accessory", "soft_body", "other"].map((category) => ({
		category,
		name: category
			.split("_")
			.map((part) => part.charAt(0).toUpperCase() + part.slice(1))
			.join(" "),
		mode: "authored" as const,
		dynamics_group_count: 0,
		solver: "verlet",
		damping_configured: false,
		damping_half_life_ms: 120,
		rest_response_configured: false,
		rest_response: 0.1,
		shape_preservation_configured: false,
		shape_preservation: 0.1,
		bounce_configured: false,
		bounce_scale: 1,
		stretch_range_configured: false,
		stretch_range_scale: 1,
		stretch_motion_configured: false,
		stretch_motion: 1,
		motion_coupling_configured: false,
		motion_coupling: 0.5,
		xpbd_compliance_configured: false,
		xpbd_compliance: 0.025,
		constraint_iterations_configured: false,
		constraint_iterations: 4,
		authored_rest_response: 0.1,
		authored_shape_preservation: 0.1,
		authored_xpbd_compliance: 0.025,
	}));
}

export const DYNAMICS_MATCH_OVERRIDE_TEMPLATES: DynamicsMatchOverrideTemplate[] = [
	{
		key: "animal_ears",
		labelKey: "profiles.editor.dynamics_template_animal_ears",
		override: {
			name: "Animal ears",
			source_id_contains: ["ear", "耳", "kemomimi"],
			solver: "verlet",
			damping_half_life_ms: 90,
			rest_response: 0.18,
			shape_preservation: 0.15,
			bounce_scale: 0.7,
			motion_coupling: 0.55,
		},
	},
	{
		key: "tail",
		labelKey: "profiles.editor.dynamics_template_tail",
		override: {
			name: "Tail",
			source_id_contains: ["tail", "尻尾", "しっぽ"],
			solver: "verlet",
			damping_half_life_ms: 180,
			rest_response: 0.08,
			shape_preservation: 0.06,
			bounce_scale: 0.95,
			motion_coupling: 0.4,
		},
	},
	{
		key: "cloth",
		labelKey: "profiles.editor.dynamics_template_cloth",
		override: {
			name: "Cloth",
			source_id_contains: ["cloth", "skirt", "cape", "sleeve", "dress", "coat", "布", "スカート", "ケープ", "袖"],
			solver: "verlet",
			damping_half_life_ms: 180,
			rest_response: 0.05,
			shape_preservation: 0.025,
			bounce_scale: 0.65,
			motion_coupling: 0.3,
		},
	},
	{
		key: "accessory",
		labelKey: "profiles.editor.dynamics_template_accessory",
		override: {
			name: "Accessory",
			source_id_contains: ["hat", "watch", "bag", "tie", "necklace", "cable", "帽子", "時計", "鞄", "ネクタイ"],
			solver: "verlet",
			damping_half_life_ms: 110,
			rest_response: 0.16,
			shape_preservation: 0.18,
			bounce_scale: 0.45,
			motion_coupling: 0.65,
		},
	},
	{
		key: "soft_body",
		labelKey: "profiles.editor.dynamics_template_soft_body",
		override: {
			name: "Soft body",
			source_id_contains: ["breast", "bust", "butt", "cheek", "胸", "尻", "頬"],
			solver: "verlet",
			damping_half_life_ms: 140,
			rest_response: 0.07,
			shape_preservation: 0.02,
			bounce_scale: 0.8,
			motion_coupling: 0.45,
		},
	},
];

export function dynamicsRecommendedPresets(category: string): string[] {
	switch (category) {
		case "hair":
		case "ears":
			return ["soft", "natural", "snappy"];
		case "tail":
			return ["soft", "natural", "heavy"];
		case "cloth":
			return ["light", "natural", "firm"];
		case "soft_body":
			return ["subtle", "natural", "lively"];
		default:
			return [];
	}
}

export function dynamicsPresetLabel(category: string, preset: string, translate: (key: string) => string): string {
	const key = `profiles.editor.dynamics_preset_${category}_${preset}`;
	const translated = translate(key);
	return translated === key ? preset : translated;
}
