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
    internal sealed class UnsupportedMaterialPropertyReport
    {
        public string Category;
        public string Property;
        public float Value;
        public int Count;
        public readonly List<string> SampleMaterials = new List<string>();

        public Dictionary<string, object> ToJson()
        {
            return new Dictionary<string, object>
            {
                ["type"] = "unsupported_material_render_state",
                ["category"] = Category ?? "",
                ["property"] = Property ?? "",
                ["value"] = Value,
                ["count"] = Count,
                ["sampleMaterials"] = SampleMaterials.Cast<object>().ToList()
            };
        }
    }

    internal sealed class ExportValidation
    {
        public bool ModularAvatarInstalled;
        public bool AvatarRootSet;
        public bool OutputPathSet;
        public int RendererCount;
        public int SkinnedMeshRendererCount;
        public int MaterialCount;
        public int VariantCount;
        public int WardrobeSetCount;
        public int HumanoidBoneCount;

        public bool CanExport => AvatarRootSet && OutputPathSet;

        public string ToDisplayText()
        {
            var lines = new List<string>
            {
                "Built-in GLB writer: available",
                "Modular Avatar: " + (ModularAvatarInstalled ? "installed" : "not detected"),
                "Avatar root: " + (AvatarRootSet ? "set" : "missing"),
                "Output path: " + (OutputPathSet ? "set" : "missing"),
                "Renderers: " + RendererCount,
                "Skinned meshes: " + SkinnedMeshRendererCount,
                "Materials: " + MaterialCount,
                "Variants: " + VariantCount,
                "Wardrobe sets: " + WardrobeSetCount,
                "Humanoid bones: " + HumanoidBoneCount,
                "Can export: " + CanExport
            };
            return string.Join("\n", lines);
        }

        public Dictionary<string, object> ToJson()
        {
            return new Dictionary<string, object>
            {
                ["gltfWriter"] = "built-in",
                ["modularAvatarInstalled"] = ModularAvatarInstalled,
                ["avatarRootSet"] = AvatarRootSet,
                ["outputPathSet"] = OutputPathSet,
                ["rendererCount"] = RendererCount,
                ["skinnedMeshRendererCount"] = SkinnedMeshRendererCount,
                ["materialCount"] = MaterialCount,
                ["variantCount"] = VariantCount,
                ["wardrobeSetCount"] = WardrobeSetCount,
                ["humanoidBoneCount"] = HumanoidBoneCount,
                ["canExport"] = CanExport
            };
        }
    }

    internal static class ModularAvatarBridge
    {
        private const string ProcessorTypeName = "nadena.dev.modular_avatar.core.editor.AvatarProcessor";

        public static bool IsAvailable => FindType(ProcessorTypeName) != null;

        public static bool TryBake(GameObject root, out string error)
        {
            error = "";
            var type = FindType(ProcessorTypeName);
            if (type == null)
            {
                error = "Modular Avatar AvatarProcessor was not found.";
                return false;
            }
            var method = type.GetMethod("ProcessAvatar", BindingFlags.Public | BindingFlags.Static, null, new[] { typeof(GameObject) }, null);
            if (method == null)
            {
                error = "Modular Avatar ProcessAvatar(GameObject) was not found.";
                return false;
            }
            try
            {
                method.Invoke(null, new object[] { root });
                return true;
            }
            catch (TargetInvocationException ex)
            {
                error = ex.InnerException != null ? ex.InnerException.Message : ex.Message;
                return false;
            }
            catch (Exception ex)
            {
                error = ex.Message;
                return false;
            }
        }

        private static Type FindType(string fullName)
        {
            foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies())
            {
                var type = assembly.GetType(fullName, false);
                if (type != null)
                {
                    return type;
                }
            }
            return null;
        }
    }
}

