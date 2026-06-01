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
    public sealed partial class UNAvatarExporterWindow
    {
        private string BuildDeveloperDiagnostics()
        {
            if (avatarRoot == null)
            {
                return "Avatar Root is missing.";
            }

            var renderers = avatarRoot.GetComponentsInChildren<Renderer>(true);
            var materials = renderers
                .SelectMany(renderer => renderer.sharedMaterials)
                .Where(material => material != null)
                .Distinct()
                .ToList();
            var textures = CollectMaterialTextures(materials);
            var sourceTextures = textures
                .Where(texture => !string.IsNullOrEmpty(texture.AssetPath))
                .ToList();
            var generatedTextures = textures.Count - sourceTextures.Count;
            var byExtension = sourceTextures
                .GroupBy(texture => texture.Extension)
                .OrderByDescending(group => group.Sum(texture => texture.ByteLength))
                .Select(group => $"{group.Key}: {group.Count()} files, {FormatBytes(group.Sum(texture => texture.ByteLength))}");
            var largest = sourceTextures
                .OrderByDescending(texture => texture.ByteLength)
                .Take(8)
                .Select(texture => $"{FormatBytes(texture.ByteLength)}  {texture.Name}  ({texture.Extension})");
            var fallbackExtensions = sourceTextures
                .Where(texture => !IsV01DirectTextureSource(texture.AssetPath))
                .GroupBy(texture => texture.Extension)
                .OrderByDescending(group => group.Sum(texture => texture.ByteLength))
                .Select(group => $"{group.Key}: {group.Count()} files, {FormatBytes(group.Sum(texture => texture.ByteLength))}");

            var lines = new List<string>
            {
                $"Exporter build marker: {ExporterBuildMarker}",
                $"Renderers: {renderers.Length}",
                $"Materials: {materials.Count}",
                $"Distinct material textures: {textures.Count}",
                $"Source-backed textures: {sourceTextures.Count}",
                $"Generated/fallback textures: {generatedTextures}",
                "",
                "Source texture bytes by extension:",
                byExtension.Any() ? string.Join("\n", byExtension) : "(none)",
                "",
                "Largest source textures:",
                largest.Any() ? string.Join("\n", largest) : "(none)",
                "",
                "Source extensions that will use PNG fallback in v0.1:",
                fallbackExtensions.Any() ? string.Join("\n", fallbackExtensions) : "(none)",
                "",
                "Wardrobe preview state probe:",
                BuildWardrobePreviewStateDiagnostics(),
                "",
                "Hints:",
                "JPG/JPEG source bytes are usually worth preserving.",
                "Large PNG normal/mask textures may be smaller after exporter PNG fallback or later optimizer transcode.",
                "If generated/fallback textures are high, export time will include GPU readback and PNG encode."
            };
            return string.Join("\n", lines);
        }

        private string BuildWardrobePreviewStateDiagnostics()
        {
            if (avatarRoot == null)
            {
                return "Avatar Root is missing.";
            }

            GameObject probeClone = null;
            try
            {
                probeClone = Instantiate(avatarRoot);
                probeClone.name = avatarRoot.name + " (UNAvatar Preview State Probe)";
                probeClone.hideFlags = HideFlags.HideAndDontSave;
                probeClone.SetActive(true);

                var lines = new List<string>
                {
                    $"hasBaseSnapshot: {hasBaseSnapshot}, base nodes: {(baseSnapshot != null ? baseSnapshot.nodes.Count : 0)}, base blendshapes: {(baseSnapshot != null ? baseSnapshot.blendShapes.Count : 0)}",
                    $"importedBaseOperations: {importedBaseOperations.Count}, captured sets: {capturedWardrobeSets.Count}",
                    ProbeWardrobeStateLine(probeClone, "base", null)
                };
                foreach (var set in capturedWardrobeSets)
                {
                    lines.Add(ProbeWardrobeStateLine(probeClone, set.displayName, set));
                }
                return string.Join("\n", lines);
            }
            catch (Exception ex)
            {
                return "probe failed: " + ex.Message;
            }
            finally
            {
                if (probeClone != null)
                {
                    DestroyImmediate(probeClone);
                }
            }
        }

        private string ProbeWardrobeStateLine(GameObject probeRoot, string label, WardrobeSetDraft set)
        {
            if (set == null)
            {
                ApplyBaseStateToRoot(probeRoot);
            }
            else
            {
                ApplyWardrobeSetStateToRoot(probeRoot, set);
            }

            var renderers = probeRoot.GetComponentsInChildren<Renderer>(true);
            var activeRenderers = renderers.Count(renderer => renderer != null && renderer.enabled && renderer.gameObject.activeInHierarchy);
            var probes = new[]
            {
                "Color  1",
                "Color  13",
                "add-belt",
                "Maid",
                "Outer"
            };
            var states = probes.Select(path => path + "=" + ProbePathState(probeRoot, path));
            return $"{label}: snapshot={(set == null ? hasBaseSnapshot : set.capturedSnapshot != null && set.capturedSnapshot.nodes.Count > 0)}, ops={(set != null ? set.operations.Count : CurrentBaseOperations().Count)}, activeRenderers={activeRenderers}; {string.Join(", ", states)}";
        }

        private static string ProbePathState(GameObject root, string path)
        {
            var transform = root.GetComponentsInChildren<Transform>(true)
                .FirstOrDefault(candidate => VariantExtractor.TransformPath(root.transform, candidate) == path);
            if (transform == null)
            {
                return "missing";
            }
            return (transform.gameObject.activeSelf ? "self:on" : "self:off") + "/" + (transform.gameObject.activeInHierarchy ? "hier:on" : "hier:off");
        }

        private bool TryAutoAssignAvatarRoot(bool updateSummary)
        {
            if (avatarRoot != null)
            {
                return true;
            }

            var candidate = ResolveAvatarRootCandidate();
            if (candidate == null)
            {
                if (updateSummary)
                {
                    lastSummary = "Avatar Root auto-detect found no unique scene avatar. Select the avatar root once.";
                }
                return false;
            }

            avatarRoot = candidate;
            if (updateSummary)
            {
                lastSummary = "Avatar Root auto-detected: " + avatarRoot.name;
            }
            return true;
        }

        private static GameObject ResolveAvatarRootCandidate()
        {
            var selected = Selection.activeGameObject;
            var fromSelection = ResolveAvatarRootFromSelection(selected);
            if (fromSelection != null)
            {
                return fromSelection;
            }

            var sceneObjects = Resources.FindObjectsOfTypeAll<GameObject>()
                .Where(IsSceneObject)
                .ToList();

            var descriptorCandidates = sceneObjects
                .Where(HasAvatarDescriptorComponent)
                .ToList();
            if (descriptorCandidates.Count == 1)
            {
                return descriptorCandidates[0];
            }

            var humanoidCandidates = sceneObjects
                .Select(go => go.GetComponent<Animator>())
                .Where(animator => animator != null && animator.avatar != null && animator.avatar.isHuman)
                .Select(animator => animator.gameObject)
                .Distinct()
                .ToList();
            if (humanoidCandidates.Count == 1)
            {
                return humanoidCandidates[0];
            }

            return null;
        }

        private static GameObject ResolveAvatarRootFromSelection(GameObject selected)
        {
            if (selected == null || !IsSceneObject(selected))
            {
                return null;
            }

            for (var transform = selected.transform; transform != null; transform = transform.parent)
            {
                if (HasAvatarDescriptorComponent(transform.gameObject))
                {
                    return transform.gameObject;
                }
            }

            for (var transform = selected.transform; transform != null; transform = transform.parent)
            {
                var animator = transform.GetComponent<Animator>();
                if (animator != null && animator.avatar != null && animator.avatar.isHuman)
                {
                    return transform.gameObject;
                }
            }

            return selected.transform.parent == null ? selected : null;
        }

        private static bool IsSceneObject(GameObject go)
        {
            return go != null && go.scene.IsValid() && !EditorUtility.IsPersistent(go);
        }

        private static bool HasAvatarDescriptorComponent(GameObject go)
        {
            if (go == null)
            {
                return false;
            }
            foreach (var component in go.GetComponents<Component>())
            {
                if (component == null)
                {
                    continue;
                }
                var type = component.GetType();
                if (type.Name == "VRCAvatarDescriptor" || type.FullName == "VRC.SDK3.Avatars.Components.VRCAvatarDescriptor")
                {
                    return true;
                }
            }
            return false;
        }

        private static bool IsV01DirectTextureSource(string path)
        {
            var extension = Path.GetExtension(path).ToLowerInvariant();
            return extension == ".png" || extension == ".jpg" || extension == ".jpeg";
        }

        private static List<TextureDiagnostic> CollectMaterialTextures(List<Material> materials)
        {
            var byKey = new Dictionary<string, TextureDiagnostic>(StringComparer.Ordinal);
            foreach (var material in materials)
            {
                string[] texturePropertyNames;
                try
                {
                    texturePropertyNames = material.GetTexturePropertyNames();
                }
                catch
                {
                    texturePropertyNames = Array.Empty<string>();
                }

                foreach (var propertyName in texturePropertyNames)
                {
                    var texture = material.GetTexture(propertyName);
                    if (texture == null)
                    {
                        continue;
                    }

                    var assetPath = AssetDatabase.GetAssetPath(texture);
                    var key = string.IsNullOrEmpty(assetPath) ? "texture:" + texture.GetInstanceID().ToString(CultureInfo.InvariantCulture) : "asset:" + assetPath;
                    if (byKey.ContainsKey(key))
                    {
                        continue;
                    }

                    var byteLength = 0L;
                    var extension = "(generated)";
                    if (!string.IsNullOrEmpty(assetPath))
                    {
                        extension = Path.GetExtension(assetPath).ToLowerInvariant();
                        var fullPath = Path.IsPathRooted(assetPath)
                            ? assetPath
                            : Path.Combine(Directory.GetCurrentDirectory(), assetPath);
                        if (File.Exists(fullPath))
                        {
                            byteLength = new FileInfo(fullPath).Length;
                        }
                    }

                    byKey[key] = new TextureDiagnostic
                    {
                        Name = texture.name,
                        AssetPath = assetPath,
                        Extension = string.IsNullOrEmpty(extension) ? "(none)" : extension,
                        ByteLength = byteLength
                    };
                }
            }

            return byKey.Values.ToList();
        }

        private static string FormatBytes(long bytes)
        {
            if (bytes >= 1024L * 1024L)
            {
                return (bytes / 1024.0 / 1024.0).ToString("0.0", CultureInfo.InvariantCulture) + " MB";
            }
            if (bytes >= 1024L)
            {
                return (bytes / 1024.0).ToString("0.0", CultureInfo.InvariantCulture) + " KB";
            }
            return bytes.ToString(CultureInfo.InvariantCulture) + " B";
        }

        private ExportValidation ValidateSelection()
        {
            var validation = new ExportValidation();
            validation.ModularAvatarInstalled = ModularAvatarBridge.IsAvailable;
            validation.AvatarRootSet = avatarRoot != null;
            validation.OutputPathSet = !string.IsNullOrWhiteSpace(exportPath);

            if (avatarRoot != null)
            {
                validation.RendererCount = avatarRoot.GetComponentsInChildren<Renderer>(true).Length;
                validation.SkinnedMeshRendererCount = avatarRoot.GetComponentsInChildren<SkinnedMeshRenderer>(true).Length;
                validation.MaterialCount = avatarRoot.GetComponentsInChildren<Renderer>(true)
                    .SelectMany(r => r.sharedMaterials)
                    .Where(m => m != null)
                    .Distinct()
                    .Count();
                validation.VariantCount = VariantExtractor.Extract(avatarRoot, exportMode).Count;
                validation.WardrobeSetCount = capturedWardrobeSets.Count;
                validation.HumanoidBoneCount = HumanoidExtractor.Extract(avatarRoot).Count;
            }

            return validation;
        }
    }
}
