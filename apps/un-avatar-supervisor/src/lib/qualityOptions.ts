import type { TextureCompressionAdvanced, TextureCompressionMode } from "./profileTypes";

export const BCN_CPU_THREAD_OPTIONS = [1, 2, 4, 8, 16, 32, 64] as const;

export const BCN_CPU_THREAD_SELECT_OPTIONS = BCN_CPU_THREAD_OPTIONS.map((count) => [String(count), String(count)] as const);

export const AA_MODE_OPTIONS = [
	["off", "profiles.editor.options.aa_off"],
	["fxaa", "profiles.editor.options.aa_fxaa"],
	["smaa", "profiles.editor.options.aa_smaa"],
	["msaa", "profiles.editor.options.aa_msaa"],
] as const;

export const TEXTURE_LIMIT_OPTIONS = [
	["off", "profiles.editor.options.texture_limit_off"],
	["auto", "profiles.editor.options.texture_limit_auto"],
	["8k", "8K"],
	["4k", "4K"],
	["2k", "2K"],
	["1k", "1K"],
] as const;

export const TEXTURE_COMPRESSION_OPTIONS = [
	["source", "profiles.editor.options.compression_source"],
	["balanced", "profiles.editor.options.compression_balanced"],
	["memory", "profiles.editor.options.compression_memory"],
	["compat", "profiles.editor.options.compression_compat"],
] as const satisfies ReadonlyArray<readonly [TextureCompressionMode, string]>;

/// Keep this in sync with renderer `TextureCompressionAdvancedOptions::default()`.
const DEFAULT_TEXTURE_COMPRESSION_ADVANCED: TextureCompressionAdvanced = {
	face: "source",
	eyes: "source",
	clothing: "auto",
	normal: "gpu_native",
	occlusion: "gpu_native",
	emissive: "high_quality",
	generic_color: "auto",
	data: "source",
};

export function defaultTextureCompressionAdvanced(): TextureCompressionAdvanced {
	return { ...DEFAULT_TEXTURE_COMPRESSION_ADVANCED };
}

export const TEXTURE_COMPRESSION_PREF_OPTIONS = [
	["source", "profiles.editor.options.texture_pref_source"],
	["auto", "profiles.editor.options.texture_pref_auto"],
	["high_quality", "profiles.editor.options.texture_pref_high_quality"],
	["small", "profiles.editor.options.texture_pref_small"],
	["gpu_native", "profiles.editor.options.texture_pref_gpu_native"],
] as const;

export const TEXTURE_COMPRESSION_ROLES: Array<{
	key: keyof TextureCompressionAdvanced;
	labelKey: string;
	hintKey: string;
}> = [
	{ key: "face", labelKey: "profiles.editor.options.texture_role_face", hintKey: "profiles.hints.quality.texture_role_face" },
	{ key: "eyes", labelKey: "profiles.editor.options.texture_role_eyes", hintKey: "profiles.hints.quality.texture_role_eyes" },
	{ key: "clothing", labelKey: "profiles.editor.options.texture_role_clothing", hintKey: "profiles.hints.quality.texture_role_clothing" },
	{ key: "normal", labelKey: "profiles.editor.options.texture_role_normal", hintKey: "profiles.hints.quality.texture_role_normal" },
	{
		key: "occlusion",
		labelKey: "profiles.editor.options.texture_role_occlusion",
		hintKey: "profiles.hints.quality.texture_role_occlusion",
	},
	{ key: "emissive", labelKey: "profiles.editor.options.texture_role_emissive", hintKey: "profiles.hints.quality.texture_role_emissive" },
	{
		key: "generic_color",
		labelKey: "profiles.editor.options.texture_role_generic_color",
		hintKey: "profiles.hints.quality.texture_role_generic_color",
	},
	{ key: "data", labelKey: "profiles.editor.options.texture_role_data", hintKey: "profiles.hints.quality.texture_role_data" },
];

export const MIPMAP_FILTER_OPTIONS = [
	["mitchell", "Mitchell-Netravali (default)"],
	["lanczos3", "Lanczos3"],
	["bicubic", "Bicubic"],
	["catmull_rom", "Catmull-Rom"],
	["bilinear", "Bilinear"],
	["box2x2", "Box 2x2 (legacy)"],
] as const;

export const RENDER_BACKEND_OPTIONS = [
	["auto", "Auto"],
	["vulkan", "Vulkan"],
	["dx12", "DX12"],
] as const;

export const BCN_ENCODER_OPTIONS = [
	["gpu", "GPU (default)"],
	["cpu", "CPU"],
] as const;

export const TEXTURE_SELECT_FIELDS = [
	{
		key: "texture_resolution_limit",
		labelKey: "profiles.editor.texture_limit",
		hintKey: "profiles.hints.quality.texture_limit",
		field: "render_quality.texture_resolution_limit",
		options: TEXTURE_LIMIT_OPTIONS,
	},
	{
		key: "texture_compression",
		labelKey: "profiles.editor.compression",
		hintKey: "profiles.editor.compression_hint",
		field: "render_quality.texture_compression",
		options: TEXTURE_COMPRESSION_OPTIONS,
	},
	{
		key: "mipmap_filter",
		labelKey: "profiles.editor.mipmap_filter",
		hintKey: "profiles.hints.quality.mipmap_filter",
		field: "render_quality.mipmap_filter",
		options: MIPMAP_FILTER_OPTIONS,
	},
	{
		key: "block_compression_encoder",
		labelKey: "profiles.editor.bcn_encoder",
		hintKey: "profiles.hints.quality.bcn_encoder",
		field: "render_quality.block_compression_encoder",
		options: BCN_ENCODER_OPTIONS,
	},
] as const;
