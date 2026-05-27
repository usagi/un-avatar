import type { TextureCompressionAdvanced } from "./profileTypes";

export const BCN_CPU_THREAD_OPTIONS = [1, 2, 4, 8, 16, 32, 64] as const;

export const BCN_CPU_THREAD_SELECT_OPTIONS = BCN_CPU_THREAD_OPTIONS.map(
  (count) => [String(count), String(count)] as const,
);

export const AA_MODE_OPTIONS = [
  ["off", "Off"],
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
  ["auto", "profiles.editor.options.compression_auto"],
  ["advanced", "profiles.editor.options.compression_advanced"],
] as const;

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
  ["source", "Source (非圧縮)"],
  ["auto", "Auto (役割の既定)"],
  ["high_quality", "High Quality (BC7 等)"],
  ["small", "Small (BasisLZ 等)"],
  ["gpu_native", "GPU Native (BC5 等)"],
] as const;

export const TEXTURE_COMPRESSION_ROLES: Array<{
  key: keyof TextureCompressionAdvanced;
  label: string;
  hint: string;
}> = [
  { key: "face", label: "Face", hint: "顔系のメッシュ。既定 source（最高品質）" },
  { key: "eyes", label: "Eyes", hint: "瞳・白目など。既定 source（最高品質）" },
  { key: "clothing", label: "Clothing", hint: "服や髪の base color。既定 auto。" },
  { key: "normal", label: "Normal", hint: "法線マップ。既定 gpu_native (Windows なら BC5)" },
  { key: "occlusion", label: "Occlusion", hint: "AO マップ。既定 gpu_native (BC4)" },
  { key: "emissive", label: "Emissive", hint: "発光マップ。既定 high_quality (BC7)" },
  { key: "generic_color", label: "Generic Color", hint: "上記以外の色テクスチャ。既定 auto" },
  { key: "data", label: "Data", hint: "metallic-roughness など、品質劣化が表情に効くもの。既定 source" },
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
  ["vulkan", "Vulkan (default)"],
  ["dx12", "DX12"],
  ["auto", "Auto"],
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
