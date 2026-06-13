import {
	DYNAMICS_BONE_COLLIDER_FIELD_PREFIX,
	DYNAMICS_ENABLE_ALL_ON_LAUNCH_FIELD,
	DYNAMICS_ENABLED_FIELD,
	DYNAMICS_OVERRIDE_FIELD_PREFIX,
} from "./dynamicsPresets";

export function isLaunchTimeRendererField(field: string): boolean {
	return (
		field === "avatar_path" ||
		field === "wardrobe_set" ||
		field === "icon_path" ||
		field === "profile.display_name" ||
		field.startsWith("render_quality.") ||
		field.startsWith("effects.") ||
		field.startsWith("environment.") ||
		field.startsWith("expression.") ||
		field.startsWith("motion.") ||
		field.startsWith("physics.contacts.") ||
		field.startsWith("output.")
	);
}

export function isRuntimeWindowField(field: string): boolean {
	return field.startsWith("window.");
}

export function canApplyWithoutRestart(field: string): boolean {
	return (
		field === "icon_path" ||
		field.startsWith("motion.") ||
		field === DYNAMICS_ENABLED_FIELD ||
		field === DYNAMICS_ENABLE_ALL_ON_LAUNCH_FIELD ||
		field.startsWith(DYNAMICS_OVERRIDE_FIELD_PREFIX) ||
		field.startsWith(DYNAMICS_BONE_COLLIDER_FIELD_PREFIX) ||
		field.startsWith("effects.avatar.outline.") ||
		field.startsWith("effects.avatar.contact_shadow.") ||
		field.startsWith("effects.post.ssao.") ||
		field.startsWith("effects.post.bloom.") ||
		field.startsWith("environment.color.") ||
		field.startsWith("environment.lighting.")
	);
}

export function profileFieldLabel(field: string): string {
	if (field === "avatar_path") return "Avatar File";
	if (field === "wardrobe_set") return "Wardrobe";
	if (field === "icon_path") return "Icon";
	if (field === "profile.display_name") return "Name";
	if (field === "profile.group") return "Group";
	if (field.startsWith("render_quality.")) return "Render quality";
	if (field.startsWith("effects.")) return "Avatar effects";
	if (field.startsWith("expression.")) return "Expression settings";
	if (field.startsWith("window.")) return "Window settings";
	if (field.startsWith("motion.")) return "Motion settings";
	if (field.startsWith("physics.contacts.")) return "Contact settings";
	if (field.startsWith("physics.dynamics.")) return "Dynamics settings";
	if (field.startsWith("output.")) return "Output settings";
	return "This setting";
}
