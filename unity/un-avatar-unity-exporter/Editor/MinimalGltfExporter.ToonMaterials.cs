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
            private static readonly string[] ToonTexturePropertyNames =
            {
                "_MainTex", "_BaseMap", "_Main2ndTex", "_Main2ndBlendMask", "_Main2ndDissolveMask", "_Main2ndDissolveNoiseMask", "_Main3rdTex", "_Main3rdBlendMask", "_Main3rdDissolveMask", "_Main3rdDissolveNoiseMask",
                "_BumpMap", "_BumpMap2nd", "_NormalMap2nd", "_Bump2ndMap", "_Bump2ndScaleMask",
                "_ShadowColorTex", "_Shadow2ndColorTex", "_Shadow3rdColorTex", "_ShadowStrengthMask", "_ShadowBorderMask", "_ShadowBlurMask", "_ShadeTex", "_1st_ShadeMap",
                "_MatCapTex", "_MatcapTex", "_MatCapBlendMask", "_MatCapBumpMap", "_MatCap2ndTex", "_MatCap2ndBlendMask", "_MatCap2ndBumpMap",
                "_RimColorTex", "_RimShadeMask", "_BacklightColorTex", "_EmissionMap", "_EmissionTex", "_EmissionBlendMask", "_EmissionGradTex", "_Emission2ndMap", "_Emission2ndBlendMask", "_Emission2ndGradTex",
                "_GlitterColorTex", "_GlitterShapeTex", "_DissolveMask", "_DissolveNoiseMask", "_ParallaxMap",
                "_ReflectionColorTex", "_SmoothnessTex", "_MetallicGlossMap", "_ReflectionCubeTex",
                "_OutlineTex", "_OutlineWidthMask", "_AlphaMask", "_MainGradationTex", "_GradationMap",
                "_UvAnimMaskTexture",
                "_AnisotropyTangentMap", "_AnisotropyScaleMask", "_AnisotropyShiftNoiseMask",
                "_FurVectorTex", "_FurLengthMask", "_FurNoiseMask", "_FurMask"
            };

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

                var untoon = new Dictionary<string, object>();
                untoon["runtimeVariant"] = lowerShader.Contains("liltoongem")
                    ? "gem"
                    : (lowerShader.Contains("liltoonref") || lowerShader.Contains("liltoonmultirefraction") ? "refraction" : "untoon");
                var baseColor = ReadColor(material, "_BaseColor", ReadColor(material, "_Color", Color.white));
                var useShadow = IsMaterialFeatureEnabled(material, "_UseShadow", HasProperty(material, "_ShadeColor") || HasProperty(material, "_ShadowColor"));
                var shadeColor = useShadow
                    ? ReadColor(material, "_ShadeColor", ReadColor(material, "_ShadowColor", new Color(0.97f, 0.97f, 0.97f, 1.0f)))
                    : baseColor;
                untoon["shadeColorFactor"] = FloatArray(shadeColor.r, shadeColor.g, shadeColor.b);
                AddTextureIndex(untoon, "shadowColorTextureIndex", useShadow ? ReadTexture(material, "_ShadowColorTex") : null);
                AddTextureIndex(untoon, "shadow2ndColorTextureIndex", useShadow ? ReadTexture(material, "_Shadow2ndColorTex") : null);
                AddTextureIndex(untoon, "shadow3rdColorTextureIndex", useShadow ? ReadTexture(material, "_Shadow3rdColorTex") : null);
                AddTextureIndex(untoon, "shadowStrengthMaskTextureIndex", useShadow ? ReadTexture(material, "_ShadowStrengthMask") : null);
                AddTextureIndex(untoon, "shadowBorderMaskTextureIndex", useShadow ? ReadTexture(material, "_ShadowBorderMask") : null);
                AddTextureIndex(untoon, "shadowBlurMaskTextureIndex", useShadow ? ReadTexture(material, "_ShadowBlurMask") : null);
                AddTextureIndex(
                    untoon,
                    "shadeMultiplyTextureIndex",
                    useShadow
                        ? ReadTexture(material, "_ShadeTex") ?? ReadTexture(material, "_1st_ShadeMap") ?? ReadTexture(material, "_ShadowColorTex")
                        : null);
                untoon["shadingShiftFactor"] = useShadow ? ReadFloat(material, "_ShadeShift", ReadFloat(material, "_ShadowBorder", 0.0f)) : 1.0f;
                untoon["shadingToonyFactor"] = useShadow ? 1.0f - Mathf.Clamp01(ReadFloat(material, "_ShadowBlur", 0.0f)) : 1.0f;

                var useMain2nd = IsMaterialFeatureEnabled(material, "_UseMain2ndTex", ReadTexture(material, "_Main2ndTex") != null);
                var useMain3rd = IsMaterialFeatureEnabled(material, "_UseMain3rdTex", ReadTexture(material, "_Main3rdTex") != null);
                AddTextureIndex(untoon, "main2ndTextureIndex", useMain2nd ? ReadTexture(material, "_Main2ndTex") : null);
                AddTextureIndex(untoon, "main2ndBlendMaskTextureIndex", useMain2nd ? ReadTexture(material, "_Main2ndBlendMask") : null);
                AddTextureIndex(untoon, "main2ndDissolveMaskTextureIndex", useMain2nd ? ReadTexture(material, "_Main2ndDissolveMask") : null);
                AddTextureIndex(untoon, "main2ndDissolveNoiseMaskTextureIndex", useMain2nd ? ReadTexture(material, "_Main2ndDissolveNoiseMask") : null);
                AddTextureIndex(untoon, "main3rdTextureIndex", useMain3rd ? ReadTexture(material, "_Main3rdTex") : null);
                AddTextureIndex(untoon, "main3rdBlendMaskTextureIndex", useMain3rd ? ReadTexture(material, "_Main3rdBlendMask") : null);
                AddTextureIndex(untoon, "main3rdDissolveMaskTextureIndex", useMain3rd ? ReadTexture(material, "_Main3rdDissolveMask") : null);
                AddTextureIndex(untoon, "main3rdDissolveNoiseMaskTextureIndex", useMain3rd ? ReadTexture(material, "_Main3rdDissolveNoiseMask") : null);

                var useMatCap = IsMaterialFeatureEnabled(material, "_UseMatCap", ReadTexture(material, "_MatCapTex") != null || ReadTexture(material, "_MatcapTex") != null);
                var matcapMainStrength = ReadFloat(material, "_MatCapMainStrength", ReadFloat(material, "_MatCapBlend", 1.0f));
                var matcapColor = useMatCap ? ReadColor(material, "_MatCapColor", Color.white) : Color.white;
                untoon["matcapEnabledFactor"] = useMatCap ? 1.0f : 0.0f;
                untoon["matcapFactor"] = FloatArray(matcapColor.r, matcapColor.g, matcapColor.b);
                untoon["matcapColorAlphaFactor"] = matcapColor.a;
                untoon["matcapMainStrengthFactor"] = useMatCap ? matcapMainStrength : 0.0f;
                AddTextureIndex(untoon, "matcapTextureIndex", useMatCap ? ReadTexture(material, "_MatCapTex") ?? ReadTexture(material, "_MatcapTex") : null);
                AddTextureIndex(untoon, "matcapBlendMaskTextureIndex", useMatCap ? ReadTexture(material, "_MatCapBlendMask") : null);
                AddTextureIndex(untoon, "matcapBumpTextureIndex", useMatCap ? ReadTexture(material, "_MatCapBumpMap") : null);
                var useMatCap2nd = IsMaterialFeatureEnabled(material, "_UseMatCap2nd", ReadTexture(material, "_MatCap2ndTex") != null);
                AddTextureIndex(untoon, "matcap2ndTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndTex") : null);
                AddTextureIndex(untoon, "matcap2ndBlendMaskTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndBlendMask") : null);
                AddTextureIndex(untoon, "matcap2ndBumpTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndBumpMap") : null);

                var useRim = IsMaterialFeatureEnabled(material, "_UseRim", HasProperty(material, "_RimColor") || ReadTexture(material, "_RimColorTex") != null);
                var rimMainStrength = ReadFloat(material, "_RimMainStrength", 1.0f);
                var rimColor = useRim ? ReadColor(material, "_RimColor", Color.white) : Color.white;
                untoon["rimEnabledFactor"] = useRim ? 1.0f : 0.0f;
                untoon["parametricRimColorFactor"] = FloatArray(rimColor.r, rimColor.g, rimColor.b);
                untoon["rimMainStrengthFactor"] = useRim ? rimMainStrength : 0.0f;
                untoon["parametricRimFresnelPowerFactor"] = ReadFloat(material, "_RimFresnelPower", 5.0f);
                untoon["rimLightingMixFactor"] = useRim ? ReadFloat(material, "_RimEnableLighting", 1.0f) : 0.0f;
                untoon["rimBlendMode"] = ReadFloat(material, "_RimBlendMode", 1.0f);
                AddTextureIndex(untoon, "rimMultiplyTextureIndex", useRim ? ReadTexture(material, "_RimColorTex") : null);
                var useRimShade = IsMaterialFeatureEnabled(material, "_UseRimShade", ReadTexture(material, "_RimShadeMask") != null);
                AddTextureIndex(untoon, "rimShadeMaskTextureIndex", useRimShade ? ReadTexture(material, "_RimShadeMask") : null);
                var useBacklight = IsMaterialFeatureEnabled(material, "_UseBacklight", ReadTexture(material, "_BacklightColorTex") != null);
                AddTextureIndex(untoon, "backlightColorTextureIndex", useBacklight ? ReadTexture(material, "_BacklightColorTex") : null);

                var useGlitter = IsMaterialFeatureEnabled(material, "_UseGlitter", ReadTexture(material, "_GlitterColorTex") != null);
                AddTextureIndex(untoon, "glitterColorTextureIndex", useGlitter ? ReadTexture(material, "_GlitterColorTex") : null);
                AddTextureIndex(untoon, "glitterShapeTextureIndex", useGlitter ? ReadTexture(material, "_GlitterShapeTex") : null);
                var useDissolve = ReadVector(material, "_DissolveParams", new Vector4(0.0f, 0.0f, 0.5f, 0.1f)).x != 0.0f;
                AddTextureIndex(untoon, "dissolveMaskTextureIndex", useDissolve ? ReadTexture(material, "_DissolveMask") : null);
                AddTextureIndex(untoon, "dissolveNoiseMaskTextureIndex", useDissolve ? ReadTexture(material, "_DissolveNoiseMask") : null);
                var useParallax = ReadFloat(material, "_UseParallax", ReadTexture(material, "_ParallaxMap") != null ? 1.0f : 0.0f) > 0.5f;
                AddTextureIndex(untoon, "parallaxTextureIndex", useParallax ? ReadTexture(material, "_ParallaxMap") : null);

                var useEmission = IsMaterialFeatureEnabled(
                    material,
                    "_UseEmission",
                    ReadTexture(material, "_EmissionMap") != null || ReadTexture(material, "_EmissionTex") != null || ReadColor(material, "_EmissionColor", Color.black).maxColorComponent > 0.0f);
                AddTextureIndex(untoon, "emissionTextureIndex", useEmission ? ReadTexture(material, "_EmissionMap") ?? ReadTexture(material, "_EmissionTex") : null);
                AddTextureIndex(untoon, "emissionBlendMaskTextureIndex", useEmission ? ReadTexture(material, "_EmissionBlendMask") : null);
                var useEmissionGradation = useEmission && IsMaterialFeatureEnabled(material, "_EmissionUseGrad", ReadTexture(material, "_EmissionGradTex") != null);
                AddTextureIndex(untoon, "emissionGradationTextureIndex", useEmissionGradation ? ReadTexture(material, "_EmissionGradTex") : null);
                var useEmission2nd = IsMaterialFeatureEnabled(material, "_UseEmission2nd", ReadTexture(material, "_Emission2ndMap") != null);
                var useEmission2ndGradation = useEmission2nd && IsMaterialFeatureEnabled(material, "_Emission2ndUseGrad", ReadTexture(material, "_Emission2ndGradTex") != null);
                AddTextureIndex(untoon, "emission2ndTextureIndex", useEmission2nd ? ReadTexture(material, "_Emission2ndMap") : null);
                AddTextureIndex(untoon, "emission2ndBlendMaskTextureIndex", useEmission2nd ? ReadTexture(material, "_Emission2ndBlendMask") : null);
                AddTextureIndex(untoon, "emission2ndGradationTextureIndex", useEmission2ndGradation ? ReadTexture(material, "_Emission2ndGradTex") : null);

                AddTextureIndex(untoon, "reflectionColorTextureIndex", ReadTexture(material, "_ReflectionColorTex"));
                AddTextureIndex(untoon, "smoothnessTextureIndex", ReadTexture(material, "_SmoothnessTex"));
                AddTextureIndex(untoon, "metallicGlossTextureIndex", ReadTexture(material, "_MetallicGlossMap"));
                AddTextureIndex(untoon, "reflectionCubeTextureIndex", ReadTexture(material, "_ReflectionCubeTex"));
                var useAnisotropy = IsMaterialFeatureEnabled(material, "_UseAnisotropy", ReadTexture(material, "_AnisotropyTangentMap") != null);
                AddTextureIndex(untoon, "anisotropyTangentTextureIndex", useAnisotropy ? ReadTexture(material, "_AnisotropyTangentMap") : null);
                AddTextureIndex(untoon, "anisotropyScaleMaskTextureIndex", useAnisotropy ? ReadTexture(material, "_AnisotropyScaleMask") : null);
                AddTextureIndex(untoon, "anisotropyShiftNoiseMaskTextureIndex", useAnisotropy ? ReadTexture(material, "_AnisotropyShiftNoiseMask") : null);

                var useOutline = lowerShader.Contains("outline") || IsMaterialFeatureEnabled(material, "_UseOutline", false);
                var outlineWidth = useOutline ? ReadFloat(material, "_OutlineWidth", 0.0f) : 0.0f;
                var outlineWidthFactor = lowerShader.Contains("liltoon") ? outlineWidth * 0.01f : outlineWidth;
                untoon["outlineWidthMode"] = outlineWidthFactor > 0.0f ? "world_coordinates" : "none";
                untoon["outlineWidthFactor"] = outlineWidthFactor;
                untoon["outlineWidthFactorUnit"] = "meters";
                var outlineColor = ReadColor(material, "_OutlineColor", Color.black);
                untoon["outlineColorFactor"] = FloatArray(outlineColor.r, outlineColor.g, outlineColor.b);
                untoon["outlineLightingMixFactor"] = ReadFloat(material, "_OutlineEnableLighting", 1.0f);
                AddTextureIndex(untoon, "outlineTextureIndex", useOutline ? ReadTexture(material, "_OutlineTex") : null);
                AddTextureIndex(untoon, "outlineWidthMultiplyTextureIndex", useOutline ? ReadTexture(material, "_OutlineWidthMask") : null);
                AddTextureIndex(untoon, "alphaMaskTextureIndex", ReadTexture(material, "_AlphaMask"));
                AddTextureIndex(untoon, "furVectorTextureIndex", ReadTexture(material, "_FurVectorTex"));
                AddTextureIndex(untoon, "furLengthMaskTextureIndex", ReadTexture(material, "_FurLengthMask"));
                AddTextureIndex(untoon, "furNoiseMaskTextureIndex", ReadTexture(material, "_FurNoiseMask"));
                AddTextureIndex(untoon, "furMaskTextureIndex", ReadTexture(material, "_FurMask"));
                untoon["lightMinLimitFactor"] = ReadFloat(material, "_LightMinLimit", 0.05f);
                untoon["lightMaxLimitFactor"] = ReadFloat(material, "_LightMaxLimit", 1.0f);
                untoon["monochromeLightingFactor"] = ReadFloat(material, "_MonochromeLighting", 0.0f);
                untoon["asUnlitFactor"] = ReadFloat(material, "_AsUnlit", 0.0f);
                untoon["vertexLightStrengthFactor"] = ReadFloat(material, "_VertexLightStrength", 0.0f);
                untoon["aaStrengthFactor"] = ReadFloat(material, "_AAStrength", 1.0f);
                untoon["gsaaStrengthFactor"] = ReadFloat(material, "_GSAAStrength", 0.0f);
                var mainGradationStrength = ReadFloat(material, "_MainGradationStrength", 0.0f);
                var useGradationMap = mainGradationStrength > 0.0f || IsMaterialFeatureEnabled(material, "_UseGradationMap", ReadTexture(material, "_GradationMap") != null);
                AddTextureIndex(untoon, "gradationMapTextureIndex", useGradationMap ? ReadTexture(material, "_MainGradationTex") ?? ReadTexture(material, "_GradationMap") : null);
                var mainTextureHsvg = ReadVector(material, "_MainTexHSVG", new Vector4(0.0f, 1.0f, 1.0f, 1.0f));
                untoon["mainTexHsvgFactor"] = FloatArray(mainTextureHsvg.x, mainTextureHsvg.y, mainTextureHsvg.z, mainTextureHsvg.w);

                var mainTextureProperty = HasProperty(material, "_BaseMap") ? "_BaseMap" : "_MainTex";
                var mainTextureScale = Vector2.one;
                var mainTextureOffset = Vector2.zero;
                if (HasProperty(material, mainTextureProperty))
                {
                    mainTextureScale = material.GetTextureScale(mainTextureProperty);
                    mainTextureOffset = material.GetTextureOffset(mainTextureProperty);
                }
                untoon["uvOffsetScale"] = FloatArray(
                    mainTextureOffset.x,
                    GltfTextureOffsetY(mainTextureOffset.y, mainTextureScale.y),
                    mainTextureScale.x,
                    mainTextureScale.y);

                var lilMainScrollRotate = ReadVector(material, "_MainTex_ScrollRotate", Vector4.zero);
                untoon["uvAnimationScrollXSpeedFactor"] = ReadFloat(material, "_UvAnimScrollX", lilMainScrollRotate.x);
                untoon["uvAnimationScrollYSpeedFactor"] = ReadFloat(material, "_UvAnimScrollY", lilMainScrollRotate.y);
                untoon["uvAnimationRotationSpeedFactor"] = ReadFloat(material, "_UvAnimRotation", lilMainScrollRotate.z);
                AddTextureIndex(untoon, "uvAnimationMaskTextureIndex", ReadTexture(material, "_UvAnimMaskTexture"));

                var normal2ndTexture = ReadTexture(material, "_BumpMap2nd") ?? ReadTexture(material, "_NormalMap2nd") ?? ReadTexture(material, "_Bump2ndMap");
                var useNormal2nd = HasProperty(material, "_UseBumpMap2nd")
                    ? ReadFloat(material, "_UseBumpMap2nd", normal2ndTexture != null ? 1.0f : 0.0f) > 0.5f
                    : IsMaterialFeatureEnabled(material, "_UseNormalMap2nd", normal2ndTexture != null);
                AddTextureIndex(untoon, "normal2ndTextureIndex", useNormal2nd ? normal2ndTexture : null);
                AddTextureIndex(untoon, "normal2ndScaleMaskTextureIndex", useNormal2nd ? ReadTexture(material, "_Bump2ndScaleMask") : null);
                untoon["normal2ndScaleFactor"] = useNormal2nd ? ReadFloat(material, "_BumpScale2nd", ReadFloat(material, "_NormalScale2nd", 1.0f)) : 1.0f;

                untoon["transparentWithZWrite"] = ReadFloat(material, "_ZWrite", 0.0f) > 0.5f || ReadFloat(material, "_ZWriteMode", 0.0f) > 0.5f;

                return new Dictionary<string, object>
                {
                    ["sourceShader"] = shaderName,
                    ["family"] = lowerShader.Contains("liltoon") ? "liltoon" : lowerShader.Contains("mtoon") ? "mtoon" : "toon",
                    ["unMaterialModel"] = "UNToon",
                    ["colorFactorColorSpace"] = "srgb",
                    ["renderQueue"] = material.renderQueue,
                    ["enabledKeywords"] = MaterialEnabledKeywordNames(material),
                    ["floatParams"] = BuildMaterialFloatParams(material),
                    ["colorParams"] = BuildMaterialColorParams(material),
                    ["vectorParams"] = BuildMaterialVectorParams(material),
                    ["textureParams"] = BuildMaterialTextureParams(material),
                    ["textureUvOffsetScales"] = BuildTextureUvOffsetScales(material),
                    ["textureUvModeFactors"] = BuildTextureUvModeFactors(material),
                    ["untoon"] = untoon
                };
            }

            private Dictionary<string, object> BuildMaterialTextureParams(Material material)
            {
                var values = new Dictionary<string, object>();
                foreach (var property in ToonTexturePropertyNames)
                {
                    if (!HasProperty(material, property))
                    {
                        continue;
                    }
                    AddTextureIndex(values, property, ReadTexture(material, property));
                }
                return values;
            }

            private Dictionary<string, object> BuildTextureUvOffsetScales(Material material)
            {
                var values = new Dictionary<string, object>();
                foreach (var property in ToonTexturePropertyNames)
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
                foreach (var property in ToonTexturePropertyNames)
                {
                    var uvModeProperty = property + "_UVMode";
                    if (HasProperty(material, uvModeProperty))
                    {
                        values[property] = ReadFloat(material, uvModeProperty, 0.0f);
                    }
                }
                return values;
            }

            private Dictionary<string, object> BuildMaterialFloatParams(Material material)
            {
                var shader = material.shader;
                if (shader == null)
                {
                    return new Dictionary<string, object>();
                }
                var count = shader.GetPropertyCount();
                var values = new Dictionary<string, object>(count);
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
                var shader = material.shader;
                if (shader == null)
                {
                    return new Dictionary<string, object>();
                }
                var count = shader.GetPropertyCount();
                var values = new Dictionary<string, object>(count);
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
                var shader = material.shader;
                if (shader == null)
                {
                    return new Dictionary<string, object>();
                }
                var count = shader.GetPropertyCount();
                var values = new Dictionary<string, object>(count);
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
