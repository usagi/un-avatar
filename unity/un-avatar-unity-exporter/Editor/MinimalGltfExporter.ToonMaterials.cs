using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
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
            private bool IsMaterialFeatureEnabled(Material material, string property, bool fallback)
            {
                return HasProperty(material, property) ? ReadFloat(material, property, fallback ? 1.0f : 0.0f) > 0.5f : fallback;
            }

            private Dictionary<string, object> BuildUnAvatarMaterialExtras(Material material)
            {
                var shaderName = material.shader != null ? material.shader.name : "";
                var lowerShader = shaderName.ToLowerInvariant();
                var looksToon = lowerShader.Contains("liltoon") || lowerShader.Contains("mtoon") || HasProperty(material, "_ShadeColor") || HasProperty(material, "_ShadeTex");
                if (!looksToon)
                {
                    return null;
                }

                var mtoon = new Dictionary<string, object>();
                var baseColor = ReadColor(material, "_BaseColor", ReadColor(material, "_Color", Color.white));
                var useShadow = IsMaterialFeatureEnabled(material, "_UseShadow", HasProperty(material, "_ShadeColor") || HasProperty(material, "_ShadowColor"));
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

                var useRim = IsMaterialFeatureEnabled(material, "_UseRim", HasProperty(material, "_RimColor") || ReadTexture(material, "_RimColorTex") != null);
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
                var useEmissionGradation = useEmission && IsMaterialFeatureEnabled(material, "_EmissionUseGrad", ReadTexture(material, "_EmissionGradTex") != null);
                AddTextureIndex(mtoon, "emissionGradationTextureIndex", useEmissionGradation ? ReadTexture(material, "_EmissionGradTex") : null);

                AddTextureIndex(mtoon, "reflectionColorTextureIndex", ReadTexture(material, "_ReflectionColorTex"));
                AddTextureIndex(mtoon, "smoothnessTextureIndex", ReadTexture(material, "_SmoothnessTex"));
                AddTextureIndex(mtoon, "metallicGlossTextureIndex", ReadTexture(material, "_MetallicGlossMap"));
                AddTextureIndex(mtoon, "reflectionCubeTextureIndex", ReadTexture(material, "_ReflectionCubeTex"));
                var useAnisotropy = IsMaterialFeatureEnabled(material, "_UseAnisotropy", ReadTexture(material, "_AnisotropyTangentMap") != null);
                AddTextureIndex(mtoon, "anisotropyTangentTextureIndex", useAnisotropy ? ReadTexture(material, "_AnisotropyTangentMap") : null);
                AddTextureIndex(mtoon, "anisotropyScaleMaskTextureIndex", useAnisotropy ? ReadTexture(material, "_AnisotropyScaleMask") : null);
                AddTextureIndex(mtoon, "anisotropyShiftNoiseMaskTextureIndex", useAnisotropy ? ReadTexture(material, "_AnisotropyShiftNoiseMask") : null);

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
                var useGradationMap = IsMaterialFeatureEnabled(material, "_UseGradationMap", ReadTexture(material, "_GradationMap") != null);
                AddTextureIndex(mtoon, "gradationMapTextureIndex", useGradationMap ? ReadTexture(material, "_GradationMap") : null);
                var mainTextureHsvg = ReadVector(material, "_MainTexHSVG", new Vector4(0.0f, 1.0f, 1.0f, 1.0f));
                mtoon["mainTexHsvgFactor"] = FloatArray(mainTextureHsvg.x, mainTextureHsvg.y, mainTextureHsvg.z, mainTextureHsvg.w);

                var mainTextureProperty = HasProperty(material, "_BaseMap") ? "_BaseMap" : "_MainTex";
                var mainTextureScale = Vector2.one;
                var mainTextureOffset = Vector2.zero;
                if (HasProperty(material, mainTextureProperty))
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

                var normal2ndTexture = ReadTexture(material, "_BumpMap2nd") ?? ReadTexture(material, "_NormalMap2nd") ?? ReadTexture(material, "_Bump2ndMap");
                var useNormal2nd = HasProperty(material, "_UseBumpMap2nd")
                    ? ReadFloat(material, "_UseBumpMap2nd", normal2ndTexture != null ? 1.0f : 0.0f) > 0.5f
                    : IsMaterialFeatureEnabled(material, "_UseNormalMap2nd", normal2ndTexture != null);
                AddTextureIndex(mtoon, "normal2ndTextureIndex", useNormal2nd ? normal2ndTexture : null);
                mtoon["normal2ndScaleFactor"] = useNormal2nd ? ReadFloat(material, "_BumpScale2nd", ReadFloat(material, "_NormalScale2nd", 1.0f)) : 1.0f;

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

            private Dictionary<string, object> BuildMaterialFloatParams(Material material)
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
                    if (!string.IsNullOrEmpty(name) && HasProperty(material, name))
                    {
                        values[name] = material.GetFloat(name);
                    }
                }
                return values;
            }

            private Dictionary<string, object> BuildMaterialColorParams(Material material)
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
                    if (string.IsNullOrEmpty(name) || !HasProperty(material, name))
                    {
                        continue;
                    }
                    var color = material.GetColor(name);
                    values[name] = FloatArray(color.r, color.g, color.b, color.a);
                }
                return values;
            }
        }
    }
}
