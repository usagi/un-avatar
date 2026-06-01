using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace UNAvatar.UnityExporter
{
    internal static partial class MinimalGltfExporter
    {
        private sealed partial class Writer
        {
            private static bool IsAlphaBlendMaterial(Material material, Color baseColor)
            {
                if (IsLilToonCutoutShader(material))
                {
                    return false;
                }
                if (IsLilToonBlendShader(material))
                {
                    return true;
                }
                if (baseColor.a < 0.999f || material.renderQueue >= 3000)
                {
                    return true;
                }
                return ReadFloat(material, "_TransparentMode", 0.0f) >= 1.5f ||
                    ReadFloat(material, "_AlphaMode", 0.0f) >= 1.5f ||
                    ReadFloat(material, "_BlendMode", 0.0f) >= 1.5f ||
                    ReadFloat(material, "_Mode", 0.0f) >= 1.5f;
            }

            private static bool IsAlphaMaskMaterial(Material material)
            {
                if (IsLilToonCutoutShader(material))
                {
                    return true;
                }
                if (material.renderQueue >= 2450 && material.renderQueue < 3000)
                {
                    return true;
                }
                if (IsLilToonMaterial(material))
                {
                    return ReadFloat(material, "_TransparentMode", 0.0f) >= 0.5f ||
                        ReadFloat(material, "_AlphaMode", 0.0f) >= 0.5f ||
                        ReadFloat(material, "_BlendMode", 0.0f) >= 0.5f ||
                        ReadFloat(material, "_Mode", 0.0f) >= 0.5f;
                }
                return material.HasProperty("_Cutoff") ||
                    ReadFloat(material, "_TransparentMode", 0.0f) >= 0.5f ||
                    ReadFloat(material, "_AlphaMode", 0.0f) >= 0.5f ||
                    ReadFloat(material, "_BlendMode", 0.0f) >= 0.5f ||
                    ReadFloat(material, "_Mode", 0.0f) >= 0.5f;
            }

            private static bool IsLilToonBlendShader(Material material)
            {
                var shaderName = material.shader != null ? material.shader.name : "";
                return shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0 &&
                    (shaderName.IndexOf("Transparent", StringComparison.OrdinalIgnoreCase) >= 0 ||
                    shaderName.IndexOf("Refraction", StringComparison.OrdinalIgnoreCase) >= 0 ||
                    shaderName.IndexOf("Fur", StringComparison.OrdinalIgnoreCase) >= 0);
            }

            private static bool IsLilToonCutoutShader(Material material)
            {
                var shaderName = material.shader != null ? material.shader.name : "";
                return shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0 &&
                    shaderName.IndexOf("Cutout", StringComparison.OrdinalIgnoreCase) >= 0;
            }

            private static bool IsLilToonMaterial(Material material)
            {
                var shaderName = material.shader != null ? material.shader.name : "";
                return shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0;
            }

            private static bool IsDoubleSidedMaterial(Material material)
            {
                var cull = ReadFloat(material, "_Cull", ReadFloat(material, "_CullMode", -1.0f));
                if (cull >= 1.5f)
                {
                    return false;
                }
                if (cull >= 0.0f && cull < 0.5f)
                {
                    return true;
                }
                return true;
            }

            private Dictionary<string, object> TextureInfo(int textureIndex, Material material, string property)
            {
                var info = new Dictionary<string, object> { ["index"] = textureIndex };
                if (material == null || string.IsNullOrEmpty(property) || !material.HasProperty(property))
                {
                    return info;
                }
                var scale = material.GetTextureScale(property);
                var offset = material.GetTextureOffset(property);
                if (Mathf.Approximately(scale.x, 1.0f) &&
                    Mathf.Approximately(scale.y, 1.0f) &&
                    Mathf.Approximately(offset.x, 0.0f) &&
                    Mathf.Approximately(offset.y, 0.0f))
                {
                    return info;
                }
                info["extensions"] = new Dictionary<string, object>
                {
                    ["KHR_texture_transform"] = new Dictionary<string, object>
                    {
                        ["offset"] = FloatArray(offset.x, GltfTextureOffsetY(offset.y, scale.y)),
                        ["scale"] = FloatArray(scale.x, scale.y)
                    }
                };
                usesTextureTransform = true;
                return info;
            }

            private static bool IsMaterialFeatureEnabled(Material material, string property, bool fallback)
            {
                return material.HasProperty(property) ? ReadFloat(material, property, fallback ? 1.0f : 0.0f) > 0.5f : fallback;
            }

            private int ExportDefaultMaterial()
            {
                if (defaultMaterialIndex >= 0)
                {
                    return defaultMaterialIndex;
                }
                materials.Add(new Dictionary<string, object>
                {
                    ["name"] = "Default",
                    ["pbrMetallicRoughness"] = new Dictionary<string, object>
                    {
                        ["baseColorFactor"] = FloatArray(1, 1, 1, 1),
                        ["metallicFactor"] = 0,
                        ["roughnessFactor"] = 0.5
                    },
                    ["doubleSided"] = true
                });
                defaultMaterialIndex = materials.Count - 1;
                return defaultMaterialIndex;
            }

            private Dictionary<string, object> BuildUnAvatarMaterialExtras(Material material)
            {
                var shaderName = material.shader != null ? material.shader.name : "";
                var lowerShader = shaderName.ToLowerInvariant();
                var looksToon = lowerShader.Contains("liltoon") || lowerShader.Contains("mtoon") || material.HasProperty("_ShadeColor") || material.HasProperty("_ShadeTex");
                if (!looksToon)
                {
                    return null;
                }

                var mtoon = new Dictionary<string, object>();
                var baseColor = ReadColor(material, "_BaseColor", ReadColor(material, "_Color", Color.white));
                var useShadow = IsMaterialFeatureEnabled(material, "_UseShadow", material.HasProperty("_ShadeColor") || material.HasProperty("_ShadowColor"));
                var shadeColor = useShadow
                    ? ReadColor(material, "_ShadeColor", ReadColor(material, "_ShadowColor", new Color(0.97f, 0.97f, 0.97f, 1.0f)))
                    : baseColor;
                mtoon["shadeColorFactor"] = FloatArray(shadeColor.r, shadeColor.g, shadeColor.b);
                AddTextureIndex(mtoon, "shadowColorTextureIndex", useShadow ? ReadTexture(material, "_ShadowColorTex") : null);
                AddTextureIndex(mtoon, "shadowStrengthMaskTextureIndex", useShadow ? ReadTexture(material, "_ShadowStrengthMask") : null);
                AddTextureIndex(mtoon, "shadowBorderMaskTextureIndex", useShadow ? ReadTexture(material, "_ShadowBorderMask") : null);
                AddTextureIndex(mtoon, "shadowBlurMaskTextureIndex", useShadow ? ReadTexture(material, "_ShadowBlurMask") : null);
                AddTextureIndex(
                    mtoon,
                    "shadeMultiplyTextureIndex",
                    useShadow
                        ? ReadTexture(material, "_ShadeTex") ?? ReadTexture(material, "_1st_ShadeMap") ?? ReadTexture(material, "_ShadowColorTex")
                        : null);
                mtoon["shadingShiftFactor"] = useShadow ? ReadFloat(material, "_ShadeShift", ReadFloat(material, "_ShadowBorder", 0.0f)) : 1.0f;
                mtoon["shadingToonyFactor"] = useShadow ? 1.0f - Mathf.Clamp01(ReadFloat(material, "_ShadowBlur", 0.0f)) : 1.0f;

                var useMatCap = IsMaterialFeatureEnabled(material, "_UseMatCap", ReadTexture(material, "_MatCapTex") != null || ReadTexture(material, "_MatcapTex") != null);
                var matcapMainStrength = ReadFloat(material, "_MatCapMainStrength", ReadFloat(material, "_MatCapBlend", 1.0f));
                var matcapColor = useMatCap ? ReadColor(material, "_MatCapColor", Color.white) * matcapMainStrength : Color.black;
                mtoon["matcapFactor"] = FloatArray(matcapColor.r, matcapColor.g, matcapColor.b);
                AddTextureIndex(mtoon, "matcapTextureIndex", useMatCap ? ReadTexture(material, "_MatCapTex") ?? ReadTexture(material, "_MatcapTex") : null);
                AddTextureIndex(mtoon, "matcapBlendMaskTextureIndex", useMatCap ? ReadTexture(material, "_MatCapBlendMask") : null);
                var useMatCap2nd = IsMaterialFeatureEnabled(material, "_UseMatCap2nd", ReadTexture(material, "_MatCap2ndTex") != null);
                AddTextureIndex(mtoon, "matcap2ndTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndTex") : null);
                AddTextureIndex(mtoon, "matcap2ndBlendMaskTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndBlendMask") : null);

                var useRim = IsMaterialFeatureEnabled(material, "_UseRim", material.HasProperty("_RimColor") || ReadTexture(material, "_RimColorTex") != null);
                var rimMainStrength = ReadFloat(material, "_RimMainStrength", 1.0f);
                var rimColor = useRim ? ReadColor(material, "_RimColor", Color.black) * rimMainStrength : Color.black;
                mtoon["parametricRimColorFactor"] = FloatArray(rimColor.r, rimColor.g, rimColor.b);
                mtoon["parametricRimFresnelPowerFactor"] = ReadFloat(material, "_RimFresnelPower", 5.0f);
                mtoon["rimLightingMixFactor"] = useRim ? ReadFloat(material, "_RimEnableLighting", 1.0f) : 0.0f;
                mtoon["rimBlendMode"] = ReadFloat(material, "_RimBlendMode", 1.0f);
                AddTextureIndex(mtoon, "rimMultiplyTextureIndex", useRim ? ReadTexture(material, "_RimColorTex") : null);

                var useEmission = IsMaterialFeatureEnabled(
                    material,
                    "_UseEmission",
                    ReadTexture(material, "_EmissionMap") != null || ReadTexture(material, "_EmissionTex") != null || ReadColor(material, "_EmissionColor", Color.black).maxColorComponent > 0.0f);
                AddTextureIndex(mtoon, "emissionTextureIndex", useEmission ? ReadTexture(material, "_EmissionMap") ?? ReadTexture(material, "_EmissionTex") : null);

                AddTextureIndex(mtoon, "reflectionColorTextureIndex", ReadTexture(material, "_ReflectionColorTex"));
                AddTextureIndex(mtoon, "smoothnessTextureIndex", ReadTexture(material, "_SmoothnessTex"));
                AddTextureIndex(mtoon, "metallicGlossTextureIndex", ReadTexture(material, "_MetallicGlossMap"));
                AddTextureIndex(mtoon, "reflectionCubeTextureIndex", ReadTexture(material, "_ReflectionCubeTex"));

                var useOutline = IsMaterialFeatureEnabled(material, "_UseOutline", lowerShader.Contains("outline"));
                var outlineWidth = useOutline ? ReadFloat(material, "_OutlineWidth", 0.0f) : 0.0f;
                var outlineWidthFactor = lowerShader.Contains("liltoon") ? outlineWidth * 0.01f : outlineWidth;
                mtoon["outlineWidthMode"] = outlineWidthFactor > 0.0f ? "world_coordinates" : "none";
                mtoon["outlineWidthFactor"] = outlineWidthFactor;
                mtoon["outlineWidthFactorUnit"] = "meters";
                var outlineColor = ReadColor(material, "_OutlineColor", Color.black);
                mtoon["outlineColorFactor"] = FloatArray(outlineColor.r, outlineColor.g, outlineColor.b);
                mtoon["outlineLightingMixFactor"] = ReadFloat(material, "_OutlineEnableLighting", 1.0f);
                AddTextureIndex(mtoon, "outlineTextureIndex", useOutline ? ReadTexture(material, "_OutlineTex") : null);
                AddTextureIndex(mtoon, "outlineWidthMultiplyTextureIndex", useOutline ? ReadTexture(material, "_OutlineWidthMask") : null);
                AddTextureIndex(mtoon, "alphaMaskTextureIndex", ReadTexture(material, "_AlphaMask"));

                var mainTextureProperty = material.HasProperty("_BaseMap") ? "_BaseMap" : "_MainTex";
                var mainTextureScale = Vector2.one;
                var mainTextureOffset = Vector2.zero;
                if (material.HasProperty(mainTextureProperty))
                {
                    mainTextureScale = material.GetTextureScale(mainTextureProperty);
                    mainTextureOffset = material.GetTextureOffset(mainTextureProperty);
                }
                mtoon["uvOffsetScale"] = FloatArray(
                    mainTextureOffset.x,
                    GltfTextureOffsetY(mainTextureOffset.y, mainTextureScale.y),
                    mainTextureScale.x,
                    mainTextureScale.y);

                var lilMainScrollRotate = ReadVector(material, "_MainTex_ScrollRotate", Vector4.zero);
                mtoon["uvAnimationScrollXSpeedFactor"] = ReadFloat(material, "_UvAnimScrollX", lilMainScrollRotate.x);
                mtoon["uvAnimationScrollYSpeedFactor"] = ReadFloat(material, "_UvAnimScrollY", lilMainScrollRotate.y);
                mtoon["uvAnimationRotationSpeedFactor"] = ReadFloat(material, "_UvAnimRotation", lilMainScrollRotate.z);
                AddTextureIndex(mtoon, "uvAnimationMaskTextureIndex", ReadTexture(material, "_UvAnimMaskTexture"));

                mtoon["transparentWithZWrite"] = ReadFloat(material, "_ZWrite", 0.0f) > 0.5f || ReadFloat(material, "_ZWriteMode", 0.0f) > 0.5f;

                return new Dictionary<string, object>
                {
                    ["sourceShader"] = shaderName,
                    ["family"] = lowerShader.Contains("liltoon") ? "liltoon" : lowerShader.Contains("mtoon") ? "mtoon" : "toon",
                    ["unMaterialModel"] = "UNToon",
                    ["renderQueue"] = material.renderQueue,
                    ["floatParams"] = BuildMaterialFloatParams(material),
                    ["colorParams"] = BuildMaterialColorParams(material),
                    ["mtoon"] = mtoon
                };
            }

            private static Dictionary<string, object> BuildMaterialFloatParams(Material material)
            {
                var values = new Dictionary<string, object>();
                var shader = material.shader;
                if (shader == null)
                {
                    return values;
                }
                var count = shader.GetPropertyCount();
                for (var i = 0; i < count; i++)
                {
                    var type = shader.GetPropertyType(i);
                    if (type != UnityEngine.Rendering.ShaderPropertyType.Float &&
                        type != UnityEngine.Rendering.ShaderPropertyType.Range)
                    {
                        continue;
                    }
                    var name = shader.GetPropertyName(i);
                    if (!string.IsNullOrEmpty(name) && material.HasProperty(name))
                    {
                        values[name] = material.GetFloat(name);
                    }
                }
                return values;
            }

            private static Dictionary<string, object> BuildMaterialColorParams(Material material)
            {
                var values = new Dictionary<string, object>();
                var shader = material.shader;
                if (shader == null)
                {
                    return values;
                }
                var count = shader.GetPropertyCount();
                for (var i = 0; i < count; i++)
                {
                    if (shader.GetPropertyType(i) != UnityEngine.Rendering.ShaderPropertyType.Color)
                    {
                        continue;
                    }
                    var name = shader.GetPropertyName(i);
                    if (string.IsNullOrEmpty(name) || !material.HasProperty(name))
                    {
                        continue;
                    }
                    var color = material.GetColor(name);
                    values[name] = FloatArray(color.r, color.g, color.b, color.a);
                }
                return values;
            }

            private void AddTextureIndex(Dictionary<string, object> dst, string key, Texture texture)
            {
                if (texture == null)
                {
                    return;
                }
                var textureIndex = ExportTexture(texture);
                if (textureIndex >= 0)
                {
                    dst[key] = textureIndex;
                    return;
                }
                var asset = ExportUnavatarTextureAsset(texture);
                if (asset != null)
                {
                    dst[key + "Asset"] = asset.Id;
                }
            }

            private int ExportTexture(Texture texture)
            {
                if (texture == null)
                {
                    return -1;
                }
                if (textureIndices.TryGetValue(texture, out var existing))
                {
                    return existing;
                }

                string fallbackReason;
                var encoded = TryReadSourceTextureBytes(texture, out fallbackReason);
                if (encoded == null && IsUnavatarExtensionOnlyTexture(GetTextureSourceInfo(texture).MimeType))
                {
                    return -1;
                }
                if (encoded == null)
                {
                    encoded = EncodeTexturePng(texture, fallbackReason);
                }
                if (encoded == null || encoded.Bytes == null || encoded.Bytes.Length == 0)
                {
                    return -1;
                }

                var view = AddBufferView(encoded.Bytes);
                images.Add(new Dictionary<string, object>
                {
                    ["name"] = texture.name,
                    ["bufferView"] = view,
                    ["mimeType"] = encoded.MimeType,
                    ["extras"] = new Dictionary<string, object>
                    {
                        ["UN_avatar_image"] = BuildImageMetadataJson(texture)
                    }
                });
                exportedTextures.Add(new ExportedTextureRecord
                {
                    Name = texture.name,
                    AssetPath = encoded.AssetPath,
                    SourceExtension = encoded.SourceExtension,
                    SourceMimeType = encoded.SourceMimeType,
                    SourceByteLength = encoded.SourceByteLength,
                    OutputMimeType = encoded.MimeType,
                    OutputByteLength = encoded.Bytes.Length,
                    ExportMode = encoded.ExportMode,
                    FallbackReason = encoded.FallbackReason
                });
                textures.Add(new Dictionary<string, object>
                {
                    ["sampler"] = ExportSampler(texture),
                    ["source"] = images.Count - 1
                });
                var index = textures.Count - 1;
                textureIndices[texture] = index;
                return index;
            }

            private int ExportSampler(Texture texture)
            {
                var magFilter = texture.filterMode == FilterMode.Point ? 9728 : 9729;
                var minFilter = magFilter;
                var wrapS = GltfWrapMode(texture.wrapModeU);
                var wrapT = GltfWrapMode(texture.wrapModeV);
                var key = magFilter.ToString(CultureInfo.InvariantCulture) + "/" +
                    minFilter.ToString(CultureInfo.InvariantCulture) + "/" +
                    wrapS.ToString(CultureInfo.InvariantCulture) + "/" +
                    wrapT.ToString(CultureInfo.InvariantCulture);
                if (samplerIndices.TryGetValue(key, out var existing))
                {
                    return existing;
                }
                samplers.Add(BuildSamplerJson(magFilter, minFilter, wrapS, wrapT));
                var index = samplers.Count - 1;
                samplerIndices[key] = index;
                return index;
            }

            private static Dictionary<string, object> BuildSamplerJson(Texture texture)
            {
                var magFilter = texture.filterMode == FilterMode.Point ? 9728 : 9729;
                var minFilter = magFilter;
                return BuildSamplerJson(
                    magFilter,
                    minFilter,
                    GltfWrapMode(texture.wrapModeU),
                    GltfWrapMode(texture.wrapModeV));
            }

            private static Dictionary<string, object> BuildSamplerJson(int magFilter, int minFilter, int wrapS, int wrapT)
            {
                return new Dictionary<string, object>
                {
                    ["magFilter"] = magFilter,
                    ["minFilter"] = minFilter,
                    ["wrapS"] = wrapS,
                    ["wrapT"] = wrapT
                };
            }

            private Dictionary<string, object> BuildImageMetadataJson(Texture texture)
            {
                var source = GetTextureSourceInfo(texture);
                var metadata = TextureAssetMetadata.FromTexture(texture, source.AssetPath, null, source.Importer);
                var json = new Dictionary<string, object>
                {
                    ["colorSpace"] = metadata.ColorSpace,
                    ["textureType"] = metadata.TextureType ?? "",
                    ["textureShape"] = metadata.TextureShape ?? ""
                };
                if (!string.IsNullOrEmpty(metadata.SourcePixelFormat))
                {
                    json["sourcePixelFormat"] = metadata.SourcePixelFormat;
                }
                if (!string.IsNullOrEmpty(metadata.Channels))
                {
                    json["channels"] = metadata.Channels;
                }
                if (metadata.SRgb.HasValue)
                {
                    json["sRGB"] = metadata.SRgb.Value;
                }
                return json;
            }

            private static int GltfWrapMode(TextureWrapMode mode)
            {
                switch (mode)
                {
                    case TextureWrapMode.Clamp:
                        return 33071;
                    case TextureWrapMode.Mirror:
                    case TextureWrapMode.MirrorOnce:
                        return 33648;
                    default:
                        return 10497;
                }
            }
        }
    }
}
