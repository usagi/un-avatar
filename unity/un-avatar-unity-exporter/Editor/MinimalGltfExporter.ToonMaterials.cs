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

            private static List<string> MaterialEnabledKeywordNames(Material material)
            {
                var names = new SortedSet<string>(StringComparer.Ordinal);
                if (material == null)
                {
                    return new List<string>();
                }

                foreach (var keyword in material.enabledKeywords)
                {
                    if (!string.IsNullOrEmpty(keyword.name))
                    {
                        names.Add(keyword.name);
                    }
                }

                foreach (var keyword in material.shaderKeywords)
                {
                    if (!string.IsNullOrEmpty(keyword))
                    {
                        names.Add(keyword);
                    }
                }

                return new List<string>(names);
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
                AddTextureIndex(mtoon, "shadow2ndColorTextureIndex", useShadow ? ReadTexture(material, "_Shadow2ndColorTex") : null);
                AddTextureIndex(mtoon, "shadow3rdColorTextureIndex", useShadow ? ReadTexture(material, "_Shadow3rdColorTex") : null);
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

                var useMain2nd = IsMaterialFeatureEnabled(material, "_UseMain2ndTex", ReadTexture(material, "_Main2ndTex") != null);
                var useMain3rd = IsMaterialFeatureEnabled(material, "_UseMain3rdTex", ReadTexture(material, "_Main3rdTex") != null);
                AddTextureIndex(mtoon, "main2ndTextureIndex", useMain2nd ? ReadTexture(material, "_Main2ndTex") : null);
                AddTextureIndex(mtoon, "main2ndBlendMaskTextureIndex", useMain2nd ? ReadTexture(material, "_Main2ndBlendMask") : null);
                AddTextureIndex(mtoon, "main2ndDissolveMaskTextureIndex", useMain2nd ? ReadTexture(material, "_Main2ndDissolveMask") : null);
                AddTextureIndex(mtoon, "main2ndDissolveNoiseMaskTextureIndex", useMain2nd ? ReadTexture(material, "_Main2ndDissolveNoiseMask") : null);
                AddTextureIndex(mtoon, "main3rdTextureIndex", useMain3rd ? ReadTexture(material, "_Main3rdTex") : null);
                AddTextureIndex(mtoon, "main3rdBlendMaskTextureIndex", useMain3rd ? ReadTexture(material, "_Main3rdBlendMask") : null);
                AddTextureIndex(mtoon, "main3rdDissolveMaskTextureIndex", useMain3rd ? ReadTexture(material, "_Main3rdDissolveMask") : null);
                AddTextureIndex(mtoon, "main3rdDissolveNoiseMaskTextureIndex", useMain3rd ? ReadTexture(material, "_Main3rdDissolveNoiseMask") : null);

                var useMatCap = IsMaterialFeatureEnabled(material, "_UseMatCap", ReadTexture(material, "_MatCapTex") != null || ReadTexture(material, "_MatcapTex") != null);
                var matcapMainStrength = ReadFloat(material, "_MatCapMainStrength", ReadFloat(material, "_MatCapBlend", 1.0f));
                var matcapColor = useMatCap ? ReadColor(material, "_MatCapColor", Color.white) * matcapMainStrength : Color.black;
                mtoon["matcapFactor"] = FloatArray(matcapColor.r, matcapColor.g, matcapColor.b);
                AddTextureIndex(mtoon, "matcapTextureIndex", useMatCap ? ReadTexture(material, "_MatCapTex") ?? ReadTexture(material, "_MatcapTex") : null);
                AddTextureIndex(mtoon, "matcapBlendMaskTextureIndex", useMatCap ? ReadTexture(material, "_MatCapBlendMask") : null);
                AddTextureIndex(mtoon, "matcapBumpTextureIndex", useMatCap ? ReadTexture(material, "_MatCapBumpMap") : null);
                var useMatCap2nd = IsMaterialFeatureEnabled(material, "_UseMatCap2nd", ReadTexture(material, "_MatCap2ndTex") != null);
                AddTextureIndex(mtoon, "matcap2ndTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndTex") : null);
                AddTextureIndex(mtoon, "matcap2ndBlendMaskTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndBlendMask") : null);
                AddTextureIndex(mtoon, "matcap2ndBumpTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndBumpMap") : null);

                var useRim = IsMaterialFeatureEnabled(material, "_UseRim", HasProperty(material, "_RimColor") || ReadTexture(material, "_RimColorTex") != null);
                var rimMainStrength = ReadFloat(material, "_RimMainStrength", 1.0f);
                var rimColor = useRim ? ReadColor(material, "_RimColor", Color.black) * rimMainStrength : Color.black;
                mtoon["parametricRimColorFactor"] = FloatArray(rimColor.r, rimColor.g, rimColor.b);
                mtoon["parametricRimFresnelPowerFactor"] = ReadFloat(material, "_RimFresnelPower", 5.0f);
                mtoon["rimLightingMixFactor"] = useRim ? ReadFloat(material, "_RimEnableLighting", 1.0f) : 0.0f;
                mtoon["rimBlendMode"] = ReadFloat(material, "_RimBlendMode", 1.0f);
                AddTextureIndex(mtoon, "rimMultiplyTextureIndex", useRim ? ReadTexture(material, "_RimColorTex") : null);
                var useRimShade = IsMaterialFeatureEnabled(material, "_UseRimShade", ReadTexture(material, "_RimShadeMask") != null);
                AddTextureIndex(mtoon, "rimShadeMaskTextureIndex", useRimShade ? ReadTexture(material, "_RimShadeMask") : null);
                var useBacklight = IsMaterialFeatureEnabled(material, "_UseBacklight", ReadTexture(material, "_BacklightColorTex") != null);
                AddTextureIndex(mtoon, "backlightColorTextureIndex", useBacklight ? ReadTexture(material, "_BacklightColorTex") : null);

                var useGlitter = IsMaterialFeatureEnabled(material, "_UseGlitter", ReadTexture(material, "_GlitterColorTex") != null);
                AddTextureIndex(mtoon, "glitterColorTextureIndex", useGlitter ? ReadTexture(material, "_GlitterColorTex") : null);
                AddTextureIndex(mtoon, "glitterShapeTextureIndex", useGlitter ? ReadTexture(material, "_GlitterShapeTex") : null);
                var useDissolve = ReadVector(material, "_DissolveParams", new Vector4(0.0f, 0.0f, 0.5f, 0.1f)).x != 0.0f;
                AddTextureIndex(mtoon, "dissolveMaskTextureIndex", useDissolve ? ReadTexture(material, "_DissolveMask") : null);
                AddTextureIndex(mtoon, "dissolveNoiseMaskTextureIndex", useDissolve ? ReadTexture(material, "_DissolveNoiseMask") : null);
                var useParallax = ReadFloat(material, "_UseParallax", ReadTexture(material, "_ParallaxMap") != null ? 1.0f : 0.0f) > 0.5f;
                AddTextureIndex(mtoon, "parallaxTextureIndex", useParallax ? ReadTexture(material, "_ParallaxMap") : null);

                var useEmission = IsMaterialFeatureEnabled(
                    material,
                    "_UseEmission",
                    ReadTexture(material, "_EmissionMap") != null || ReadTexture(material, "_EmissionTex") != null || ReadColor(material, "_EmissionColor", Color.black).maxColorComponent > 0.0f);
                AddTextureIndex(mtoon, "emissionTextureIndex", useEmission ? ReadTexture(material, "_EmissionMap") ?? ReadTexture(material, "_EmissionTex") : null);
                AddTextureIndex(mtoon, "emissionBlendMaskTextureIndex", useEmission ? ReadTexture(material, "_EmissionBlendMask") : null);
                var useEmissionGradation = useEmission && IsMaterialFeatureEnabled(material, "_EmissionUseGrad", ReadTexture(material, "_EmissionGradTex") != null);
                AddTextureIndex(mtoon, "emissionGradationTextureIndex", useEmissionGradation ? ReadTexture(material, "_EmissionGradTex") : null);
                var useEmission2nd = IsMaterialFeatureEnabled(material, "_UseEmission2nd", ReadTexture(material, "_Emission2ndMap") != null);
                var useEmission2ndGradation = useEmission2nd && IsMaterialFeatureEnabled(material, "_Emission2ndUseGrad", ReadTexture(material, "_Emission2ndGradTex") != null);
                AddTextureIndex(mtoon, "emission2ndTextureIndex", useEmission2nd ? ReadTexture(material, "_Emission2ndMap") : null);
                AddTextureIndex(mtoon, "emission2ndBlendMaskTextureIndex", useEmission2nd ? ReadTexture(material, "_Emission2ndBlendMask") : null);
                AddTextureIndex(mtoon, "emission2ndGradationTextureIndex", useEmission2ndGradation ? ReadTexture(material, "_Emission2ndGradTex") : null);

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
                AddTextureIndex(mtoon, "furVectorTextureIndex", ReadTexture(material, "_FurVectorTex"));
                AddTextureIndex(mtoon, "furLengthMaskTextureIndex", ReadTexture(material, "_FurLengthMask"));
                AddTextureIndex(mtoon, "furNoiseMaskTextureIndex", ReadTexture(material, "_FurNoiseMask"));
                AddTextureIndex(mtoon, "furMaskTextureIndex", ReadTexture(material, "_FurMask"));
                var mainGradationStrength = ReadFloat(material, "_MainGradationStrength", 0.0f);
                var useGradationMap = mainGradationStrength > 0.0f || IsMaterialFeatureEnabled(material, "_UseGradationMap", ReadTexture(material, "_GradationMap") != null);
                AddTextureIndex(mtoon, "gradationMapTextureIndex", useGradationMap ? ReadTexture(material, "_MainGradationTex") ?? ReadTexture(material, "_GradationMap") : null);
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
                AddTextureIndex(mtoon, "normal2ndScaleMaskTextureIndex", useNormal2nd ? ReadTexture(material, "_Bump2ndScaleMask") : null);
                mtoon["normal2ndScaleFactor"] = useNormal2nd ? ReadFloat(material, "_BumpScale2nd", ReadFloat(material, "_NormalScale2nd", 1.0f)) : 1.0f;

                mtoon["transparentWithZWrite"] = ReadFloat(material, "_ZWrite", 0.0f) > 0.5f || ReadFloat(material, "_ZWriteMode", 0.0f) > 0.5f;

                return new Dictionary<string, object>
                {
                    ["sourceShader"] = shaderName,
                    ["family"] = lowerShader.Contains("liltoon") ? "liltoon" : lowerShader.Contains("mtoon") ? "mtoon" : "toon",
                    ["unMaterialModel"] = "UNToon",
                    ["renderQueue"] = material.renderQueue,
                    ["enabledKeywords"] = MaterialEnabledKeywordNames(material),
                    ["floatParams"] = BuildMaterialFloatParams(material),
                    ["colorParams"] = BuildMaterialColorParams(material),
                    ["vectorParams"] = BuildMaterialVectorParams(material),
                    ["textureUvOffsetScales"] = BuildTextureUvOffsetScales(material),
                    ["textureUvModeFactors"] = BuildTextureUvModeFactors(material),
                    ["mtoon"] = mtoon
                };
            }

            private Dictionary<string, object> BuildTextureUvOffsetScales(Material material)
            {
                var values = new Dictionary<string, object>();
                foreach (var property in ToonTextureProperties())
                {
                    if (!HasProperty(material, property) || ReadTexture(material, property) == null)
                    {
                        continue;
                    }
                    var scale = material.GetTextureScale(property);
                    var offset = material.GetTextureOffset(property);
                    if (Mathf.Approximately(offset.x, 0.0f) &&
                        Mathf.Approximately(offset.y, 0.0f) &&
                        Mathf.Approximately(scale.x, 1.0f) &&
                        Mathf.Approximately(scale.y, 1.0f))
                    {
                        continue;
                    }
                    values[property] = FloatArray(offset.x, GltfTextureOffsetY(offset.y, scale.y), scale.x, scale.y);
                }
                return values;
            }

            private Dictionary<string, object> BuildTextureUvModeFactors(Material material)
            {
                var values = new Dictionary<string, object>();
                foreach (var property in ToonTextureProperties())
                {
                    var uvModeProperty = property + "_UVMode";
                    if (HasProperty(material, uvModeProperty))
                    {
                        values[property] = ReadFloat(material, uvModeProperty, 0.0f);
                    }
                }
                return values;
            }

            private static string[] ToonTextureProperties()
            {
                return new[]
                {
                    "_MainTex", "_BaseMap", "_Main2ndTex", "_Main2ndBlendMask", "_Main2ndDissolveMask", "_Main2ndDissolveNoiseMask", "_Main3rdTex", "_Main3rdBlendMask", "_Main3rdDissolveMask", "_Main3rdDissolveNoiseMask",
                    "_BumpMap", "_BumpMap2nd", "_NormalMap2nd", "_Bump2ndMap", "_Bump2ndScaleMask",
                    "_ShadowColorTex", "_Shadow2ndColorTex", "_Shadow3rdColorTex", "_ShadowStrengthMask", "_ShadowBorderMask", "_ShadowBlurMask", "_ShadeTex", "_1st_ShadeMap",
                    "_MatCapTex", "_MatcapTex", "_MatCapBlendMask", "_MatCapBumpMap", "_MatCap2ndTex", "_MatCap2ndBlendMask", "_MatCap2ndBumpMap",
                    "_RimColorTex", "_RimShadeMask", "_BacklightColorTex", "_EmissionMap", "_EmissionTex", "_EmissionBlendMask", "_EmissionGradTex", "_Emission2ndMap", "_Emission2ndBlendMask", "_Emission2ndGradTex",
                    "_GlitterColorTex", "_GlitterShapeTex", "_DissolveMask", "_DissolveNoiseMask", "_ParallaxMap",
                    "_ReflectionColorTex", "_SmoothnessTex", "_MetallicGlossMap", "_ReflectionCubeTex",
                    "_OutlineTex", "_OutlineWidthMask", "_AlphaMask", "_MainGradationTex", "_GradationMap",
                    "_AnisotropyTangentMap", "_AnisotropyScaleMask", "_AnisotropyShiftNoiseMask",
                    "_FurVectorTex", "_FurLengthMask", "_FurNoiseMask", "_FurMask"
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
                        type != UnityEngine.Rendering.ShaderPropertyType.Range &&
                        type != UnityEngine.Rendering.ShaderPropertyType.Int)
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

            private Dictionary<string, object> BuildMaterialVectorParams(Material material)
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
                    if (shader.GetPropertyType(i) != UnityEngine.Rendering.ShaderPropertyType.Vector)
                    {
                        continue;
                    }
                    var name = shader.GetPropertyName(i);
                    if (string.IsNullOrEmpty(name) || !HasProperty(material, name))
                    {
                        continue;
                    }
                    var value = material.GetVector(name);
                    values[name] = FloatArray(value.x, value.y, value.z, value.w);
                }
                return values;
            }
        }
    }
}
