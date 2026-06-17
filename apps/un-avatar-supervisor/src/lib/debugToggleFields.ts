export const debugToggleFields = [
	{
		key: "debug_disable_rim_lighting",
		field: "debug.disable_rim_lighting",
		label: "Disable rim lighting",
		hint: "UNToon rim lighting (rim_color × fresnel) を 0 に固定。目周辺のリングが rim 由来か切り分ける診断用。",
	},
	{
		key: "debug_force_shading_shift_zero",
		field: "debug.force_shading_shift_zero",
		label: "Force shading shift = 0",
		hint: "shadingShiftFactor / shadingShiftTexture を 0 に固定し、shade 色への falloff を素の dot(n, l) だけにする診断 toggle。",
	},
	{
		key: "debug_disable_matcap",
		field: "debug.disable_matcap",
		label: "Disable matcap",
		hint: "UNToon matcap / sphere add 寄与を 0 に固定。matcap テクスチャによる目周辺の擬似ハイライト/シャドウを切り分ける診断用。",
	},
	{
		key: "debug_disable_emissive",
		field: "debug.disable_emissive",
		label: "Disable emissive",
		hint: "emissive (emissive_factor × emissive_tex) 寄与を 0 に固定。眉間/目周辺に肌色寄りの emissive が焼き込まれているケースを切り分ける診断用。",
	},
	{
		key: "debug_disable_shade_color",
		field: "debug.disable_shade_color",
		label: "Disable shade color",
		hint: "UNToon shade term (shade_color × shade_tex) を base 色で置換。shade_tex そのものが肌色リングの原因か（=shade_tex の特定領域が肌色寄り）切り分ける診断用。",
	},
	{
		key: "debug_disable_normal_map",
		field: "debug.disable_normal_map",
		label: "Disable normal map",
		hint: "normalTexture を使わず頂点法線のみで shading / rim を計算。rim が normal map / TBN 由来で強く出ているか切り分ける診断用。",
	},
	{
		key: "debug_base_texture_only",
		field: "debug.base_texture_only",
		label: "Base texture only",
		hint: "UNToon fragment output を base (alb × base_color) のみに固定。shading / GI / rim / matcap / emissive / shade term を全部スキップ。",
	},
] as const;

export type DebugToggleFieldKey = (typeof debugToggleFields)[number]["key"];

export type DebugToggleSetting = Record<DebugToggleFieldKey, boolean>;
