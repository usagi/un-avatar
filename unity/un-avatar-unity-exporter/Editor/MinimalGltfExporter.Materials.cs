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
            private bool IsAlphaBlendMaterial(Material material, Color baseColor)
            {
                if (IsLilToonCutoutShader(material))
                {
                    return false;
                }
                if (IsLilToonRefractionShader(material))
                {
                    return false;
                }
                if (IsLilToonGemShader(material))
                {
                    return true;
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

            private bool IsAlphaMaskMaterial(Material material)
            {
                if (IsLilToonRefractionShader(material))
                {
                    return false;
                }
                if (IsLilToonGemShader(material))
                {
                    return false;
                }
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
                return HasProperty(material, "_Cutoff") ||
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
                    shaderName.IndexOf("Fur", StringComparison.OrdinalIgnoreCase) >= 0);
            }

            private static bool IsLilToonRefractionShader(Material material)
            {
                var shaderName = material.shader != null ? material.shader.name : "";
                return shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0 &&
                    (shaderName.IndexOf("Refraction", StringComparison.OrdinalIgnoreCase) >= 0 ||
                    shaderName.IndexOf("lilToonRef", StringComparison.OrdinalIgnoreCase) >= 0);
            }

            private static bool IsLilToonGemShader(Material material)
            {
                var shaderName = material.shader != null ? material.shader.name : "";
                return shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0 &&
                    shaderName.IndexOf("Gem", StringComparison.OrdinalIgnoreCase) >= 0;
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

            private bool IsDoubleSidedMaterial(Material material)
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
                if (material == null || string.IsNullOrEmpty(property) || !HasProperty(material, property))
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

        }
    }
}
