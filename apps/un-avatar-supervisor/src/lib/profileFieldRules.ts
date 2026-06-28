import { DYNAMICS_BONE_COLLIDER_FIELD_PREFIX, DYNAMICS_ENABLED_FIELD, DYNAMICS_OVERRIDE_FIELD_PREFIX } from "./dynamicsPresets";

export function isLaunchTimeRendererField(field: string): boolean {
	return (
		field === "avatar_path" ||
		field === "wardrobe_set" ||
		field.startsWith("wardrobe.transition.") ||
		field === "icon_path" ||
		field === "profile.display_name" ||
		field === "profile.gpu_adapter" ||
		field.startsWith("render_quality.") ||
		field.startsWith("effects.") ||
		field.startsWith("environment.") ||
		field.startsWith("expression.") ||
		field.startsWith("motion.") ||
		field.startsWith("physics.contacts.")
	);
}

export function isRuntimeWindowField(field: string): boolean {
	return field.startsWith("window.");
}

export function canApplyWithoutRestart(field: string): boolean {
	return (
		field === "icon_path" ||
		field.startsWith("wardrobe.transition.") ||
		field === "wardrobe.bindings" ||
		field.startsWith("animator.") ||
		field.startsWith("output.spout2.") ||
		field.startsWith("runtime.") ||
		field.startsWith("motion.") ||
		field === DYNAMICS_ENABLED_FIELD ||
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

export function profileFieldLabel(field: string, translate: (key: string) => string): string {
	if (field === "avatar_path") return translate("profiles.fields.avatar_file");
	if (field === "wardrobe_set") return translate("profiles.fields.wardrobe");
	if (field.startsWith("wardrobe.transition.")) return translate("profiles.fields.wardrobe");
	if (field === "icon_path") return translate("profiles.fields.icon");
	if (field === "profile.display_name") return translate("profiles.fields.name");
	if (field === "profile.group") return translate("profiles.fields.group");
	if (field === "profile.gpu_adapter") return translate("profiles.fields.gpu_adapter");
	if (field === "render_quality.render_backend") return translate("profiles.fields.render_backend");
	if (field.startsWith("render_quality.")) return translate("profiles.fields.render_quality");
	if (field.startsWith("runtime.")) return translate("profiles.fields.render_quality");
	if (field.startsWith("effects.")) return translate("profiles.fields.avatar_effects");
	if (field.startsWith("expression.")) return translate("profiles.fields.expression_settings");
	if (field.startsWith("window.")) return translate("profiles.fields.window_settings");
	if (field.startsWith("motion.")) return translate("profiles.fields.motion_settings");
	if (field.startsWith("physics.contacts.")) return translate("profiles.fields.contact_settings");
	if (field.startsWith("physics.dynamics.")) return translate("profiles.fields.dynamics_settings");
	if (field.startsWith("output.")) return translate("profiles.fields.output_settings");
	return translate("profiles.fields.this_setting");
}
