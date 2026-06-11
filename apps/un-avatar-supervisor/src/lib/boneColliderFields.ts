export const BONE_COLLIDER_PARTS = [
	{
		key: "head",
		labelKey: "profiles.editor.bone_collider_head",
		settingKey: "bone_collider_head",
	},
	{
		key: "neck_chest",
		labelKey: "profiles.editor.bone_collider_neck_chest",
		settingKey: "bone_collider_neck_chest",
	},
	{
		key: "torso",
		labelKey: "profiles.editor.bone_collider_torso",
		settingKey: "bone_collider_torso",
	},
	{
		key: "upper_arms",
		labelKey: "profiles.editor.bone_collider_upper_arms",
		settingKey: "bone_collider_upper_arms",
	},
	{
		key: "lower_arms",
		labelKey: "profiles.editor.bone_collider_lower_arms",
		settingKey: "bone_collider_lower_arms",
	},
	{
		key: "hands",
		labelKey: "profiles.editor.bone_collider_hands",
		settingKey: "bone_collider_hands",
	},
] as const;

export type BoneColliderSettingKey = (typeof BONE_COLLIDER_PARTS)[number]["settingKey"];

export type BoneColliderSetting = Record<BoneColliderSettingKey, number> & {
	bone_colliders_enabled: boolean;
};
