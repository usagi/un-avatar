export type DynamicsCategoryOverrideSetting = {
	category: string;
	name: string;
	mode: "authored" | "override_verlet" | "override_xpbd";
	spring_bone_count: number;
	solver: string;
	damping_configured: boolean;
	damping_half_life_ms: number;
	stiffness_configured: boolean;
	stiffness_hz: number;
	xpbd_compliance_configured: boolean;
	xpbd_compliance: number;
	constraint_iterations_configured: boolean;
	constraint_iterations: number;
	authored_stiffness_hz: number;
	authored_xpbd_compliance: number;
};

export const DYNAMICS_ENABLED_FIELD = "dynamics_enabled";
export const DYNAMICS_ENABLE_ALL_ON_LAUNCH_FIELD = "physics.dynamics.enable_all_on_launch";
export const DYNAMICS_OVERRIDE_FIELD_PREFIX = "physics.spring_bone.";
export const DYNAMICS_BONE_COLLIDER_FIELD_PREFIX = "physics.bone_colliders.";

export function dynamicsOverrideFieldPrefix(category: string): string {
	return `${DYNAMICS_OVERRIDE_FIELD_PREFIX}overrides.${category}`;
}

export const DYNAMICS_MODE_OPTIONS = [
	["authored", "profiles.editor.spring_bone_mode_authored_verlet"],
	["override_verlet", "profiles.editor.spring_bone_mode_override_verlet"],
	["override_xpbd", "profiles.editor.spring_bone_mode_override_xpbd"],
] as const;

export const DYNAMICS_DAMPING_FIELD = {
	key: "damping_half_life_ms",
	labelKey: "profiles.editor.spring_bone_damping",
	hintKey: "profiles.editor.spring_bone_damping_hint",
	min: 1,
	max: 10000,
	step: 1,
} as const;

export const DYNAMICS_XPBD_FIELDS = [
	{
		key: "xpbd_compliance",
		labelKey: "profiles.editor.spring_bone_xpbd_compliance",
		hintKey: "profiles.editor.spring_bone_xpbd_compliance_hint",
		min: 0,
		max: 10,
		step: 0.001,
	},
	{
		key: "constraint_iterations",
		labelKey: "profiles.editor.spring_bone_iterations",
		hintKey: "profiles.editor.spring_bone_iterations_hint",
		min: 1,
		max: 32,
		step: 1,
	},
] as const;

export const DYNAMICS_VERLET_FIELDS = [
	{
		key: "stiffness_hz",
		labelKey: "profiles.editor.spring_bone_verlet_stiffness",
		hintKey: "profiles.editor.spring_bone_verlet_stiffness_hint",
		min: 0,
		max: 60,
		step: 0.1,
	},
] as const;

export function defaultDynamicsCategoryOverrides(): DynamicsCategoryOverrideSetting[] {
	return ["hair", "ears", "tail", "cloth", "accessory", "other"].map((category) => ({
		category,
		name: category
			.split("_")
			.map((part) => part.charAt(0).toUpperCase() + part.slice(1))
			.join(" "),
		mode: "authored" as const,
		spring_bone_count: 0,
		solver: "verlet",
		damping_configured: false,
		damping_half_life_ms: 120,
		stiffness_configured: false,
		stiffness_hz: 3.5,
		xpbd_compliance_configured: false,
		xpbd_compliance: 0.025,
		constraint_iterations_configured: false,
		constraint_iterations: 4,
		authored_stiffness_hz: 3.5,
		authored_xpbd_compliance: 0.025,
	}));
}

export function dynamicsRecommendedPresets(category: string): string[] {
	switch (category) {
		case "hair":
		case "ears":
			return ["soft", "natural", "snappy"];
		case "tail":
			return ["soft", "natural", "heavy"];
		case "cloth":
			return ["light", "natural", "firm"];
		default:
			return [];
	}
}

export function dynamicsPresetLabel(category: string, preset: string, translate: (key: string) => string): string {
	const key = `profiles.editor.spring_bone_preset_${category}_${preset}`;
	const translated = translate(key);
	return translated === key ? preset : translated;
}
