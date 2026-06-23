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
    public sealed partial class UNAvatarExporterWindow
    {
        private string BuildDeveloperDiagnostics()
        {
            if (avatarRoot == null)
            {
                return "Avatar Root is missing.";
            }

            var renderers = avatarRoot.GetComponentsInChildren<Renderer>(true);
            var materials = DistinctRendererMaterials(renderers);
            var textures = CollectMaterialTextures(materials);
            var textureDiagnostics = BuildTextureDiagnostics(textures);
            var sourceTextureCount = textureDiagnostics.SourceCount;
            var generatedTextures = textures.Count - sourceTextureCount;

            var lines = new List<string>
            {
                $"Exporter build marker: {ExporterBuildMarker}",
                $"Renderers: {renderers.Length}",
                $"Materials: {materials.Count}",
                $"Distinct material textures: {textures.Count}",
                $"Source-backed textures: {sourceTextureCount}",
                $"Generated/fallback textures: {generatedTextures}",
                "",
                "Source texture bytes by extension:",
                textureDiagnostics.ByExtension,
                "",
                "Largest source textures:",
                textureDiagnostics.Largest,
                "",
                "Source extensions that will use PNG fallback in v0.1:",
                textureDiagnostics.FallbackExtensions,
                "",
                "Wardrobe preview state probe:",
                BuildWardrobePreviewStateDiagnostics(),
                "",
                "Selected GameObject probe:",
                BuildSelectedGameObjectProbe(),
                "",
                "Hints:",
                "JPG/JPEG source bytes are usually worth preserving.",
                "Large PNG normal/mask textures may be smaller after exporter PNG fallback or later optimizer transcode.",
                "If generated/fallback textures are high, export time will include GPU readback and PNG encode."
            };
            return string.Join("\n", lines);
        }

        private string BuildSelectedGameObjectProbe()
        {
            if (Selection.activeGameObject == null)
            {
                return "No selected GameObject.";
            }

            var selected = Selection.activeGameObject;
            if (avatarRoot != null)
            {
                var transform = selected.transform;
                if (transform != avatarRoot.transform && !transform.IsChildOf(avatarRoot.transform))
                {
                    return $"Selected object is outside avatarRoot: {selected.name}";
                }
            }

            var lines = new List<string>
            {
                "name: " + selected.name,
                "path: " + BuildTransformPathForProbe(selected.transform),
                "instanceId: " + selected.GetInstanceID().ToString(CultureInfo.InvariantCulture),
                "activeSelf: " + selected.activeSelf,
                "activeInHierarchy: " + selected.activeInHierarchy,
                "tag: " + selected.tag,
                "layer: " + selected.layer.ToString(CultureInfo.InvariantCulture) + "/" + LayerMask.LayerToName(selected.layer),
                "componentCount: " + selected.GetComponents<Component>().Length
            };

            lines.Add("renderer summary:");
            var renderer = selected.GetComponent<Renderer>();
            if (renderer == null)
            {
                lines.Add("  [none]");
            }
            else
            {
                lines.Add("  type: " + renderer.GetType().Name);
                lines.Add("  enabled: " + renderer.enabled);
                lines.Add("  sharedMaterials: " + FormatRendererMaterialSlots(renderer.sharedMaterials));
                if (renderer is SkinnedMeshRenderer skinnedMeshRenderer)
                {
                    lines.Add("  skinnedMeshRenderer rootBone: " + FormatTransformPath(skinnedMeshRenderer.rootBone));
                    lines.Add("  skinnedMeshRenderer bones: " + FormatBonePaths(skinnedMeshRenderer.bones));
                    lines.Add("  skinnedMeshRenderer sharedMesh bindposes: " + FormatBindPoseSummary(skinnedMeshRenderer.sharedMesh));
                }
            }

            lines.Add("components:");
            foreach (var component in selected.GetComponents<Component>())
            {
                if (component == null)
                {
                    lines.Add("  <missing component>");
                    continue;
                }

                lines.Add("  " + component.GetType().FullName);
                lines.Add("  payload:");
                lines.AddRange(IndentLines(TruncateDiagnosticText(DescribeComponentPayload(component), 3000), "    "));
            }

            return string.Join("\n", lines);
        }

        private static string FormatTransformPathForProbe(Transform transform)
        {
            if (transform == null)
            {
                return "<missing transform>";
            }

            var parts = new List<string>();
            for (var current = transform; current != null; current = current.parent)
            {
                parts.Add(current.name);
            }
            parts.Reverse();
            return string.Join("/", parts);
        }

        private static string BuildTransformPathForProbe(Transform transform)
        {
            return FormatTransformPathForProbe(transform);
        }

        private static string FormatTransformPath(Transform transform)
        {
            if (transform == null)
            {
                return "<null>";
            }
            return BuildTransformPathForProbe(transform);
        }

        private static string FormatRendererMaterialSlots(Material[] materials)
        {
            if (materials == null || materials.Length == 0)
            {
                return "<none>";
            }

            var slots = new List<string>(materials.Length);
            for (var i = 0; i < materials.Length; i++)
            {
                var material = materials[i];
                slots.Add(i.ToString(CultureInfo.InvariantCulture) + ":" + (material != null ? material.name : "<null>"));
            }
            return string.Join(", ", slots);
        }

        private static string FormatBonePaths(Transform[] bones)
        {
            if (bones == null || bones.Length == 0)
            {
                return "<none>";
            }

            var parts = new List<string>(bones.Length);
            for (var i = 0; i < bones.Length; i++)
            {
                var bone = bones[i];
                if (bone == null)
                {
                    parts.Add(i.ToString(CultureInfo.InvariantCulture) + ":<null>");
                }
                else
                {
                    parts.Add(i.ToString(CultureInfo.InvariantCulture) + ":" + bone.name);
                }
            }
            return string.Join(", ", parts);
        }

        private static string FormatBindPoseSummary(Mesh mesh)
        {
            if (mesh == null || mesh.bindposes == null)
            {
                return "<none>";
            }

            return mesh.bindposes.Length.ToString(CultureInfo.InvariantCulture) + " entries in " + mesh.name;
        }

        private static string DescribeComponentPayload(Component component)
        {
            try
            {
                var payload = EditorJsonUtility.ToJson(component, true);
                if (string.IsNullOrEmpty(payload))
                {
                    return "<empty>";
                }
                return payload;
            }
            catch
            {
                return "<unserializable>";
            }
        }

        private static string TruncateDiagnosticText(string value, int maxCharacters)
        {
            if (string.IsNullOrEmpty(value))
            {
                return "<empty>";
            }
            return value.Length <= maxCharacters ? value : value.Substring(0, maxCharacters) + "...(truncated)";
        }

        private static IEnumerable<string> IndentLines(string value, string indent)
        {
            if (string.IsNullOrEmpty(value))
            {
                yield return indent + "<empty>";
                yield break;
            }
            using (var reader = new StringReader(value))
            {
                while (true)
                {
                    var line = reader.ReadLine();
                    if (line == null)
                    {
                        break;
                    }
                    yield return indent + line;
                }
            }
        }

        private static List<Material> DistinctRendererMaterials(Renderer[] renderers)
        {
            var materials = new List<Material>(renderers != null ? renderers.Length : 0);
            var seen = new HashSet<Material>();
            if (renderers == null)
            {
                return materials;
            }
            foreach (var renderer in renderers)
            {
                if (renderer == null)
                {
                    continue;
                }
                foreach (var material in renderer.sharedMaterials)
                {
                    if (material != null && seen.Add(material))
                    {
                        materials.Add(material);
                    }
                }
            }
            return materials;
        }

        private sealed class TextureDiagnosticsSummary
        {
            public int SourceCount;
            public string ByExtension;
            public string Largest;
            public string FallbackExtensions;
        }

        private static TextureDiagnosticsSummary BuildTextureDiagnostics(List<TextureDiagnostic> textures)
        {
            var sourceTextures = new List<TextureDiagnostic>();
            var byExtension = new Dictionary<string, TextureExtensionSummary>(StringComparer.Ordinal);
            var fallbackByExtension = new Dictionary<string, TextureExtensionSummary>(StringComparer.Ordinal);
            foreach (var texture in textures)
            {
                if (string.IsNullOrEmpty(texture.AssetPath))
                {
                    continue;
                }
                sourceTextures.Add(texture);
                AddTextureExtensionSummary(byExtension, texture);
                if (!IsV01DirectTextureSource(texture.AssetPath))
                {
                    AddTextureExtensionSummary(fallbackByExtension, texture);
                }
            }
            sourceTextures.Sort((a, b) => b.ByteLength.CompareTo(a.ByteLength));
            return new TextureDiagnosticsSummary
            {
                SourceCount = sourceTextures.Count,
                ByExtension = FormatTextureExtensionSummaries(byExtension.Values),
                Largest = FormatLargestTextures(sourceTextures),
                FallbackExtensions = FormatTextureExtensionSummaries(fallbackByExtension.Values)
            };
        }

        private sealed class TextureExtensionSummary
        {
            public string Extension;
            public int Count;
            public long ByteLength;
        }

        private static void AddTextureExtensionSummary(Dictionary<string, TextureExtensionSummary> summaries, TextureDiagnostic texture)
        {
            if (!summaries.TryGetValue(texture.Extension, out var summary))
            {
                summary = new TextureExtensionSummary { Extension = texture.Extension };
                summaries[texture.Extension] = summary;
            }
            summary.Count++;
            summary.ByteLength += texture.ByteLength;
        }

        private static string FormatTextureExtensionSummaries(IEnumerable<TextureExtensionSummary> summaries)
        {
            var list = new List<TextureExtensionSummary>(summaries);
            if (list.Count == 0)
            {
                return "(none)";
            }
            list.Sort((a, b) => b.ByteLength.CompareTo(a.ByteLength));
            var lines = new List<string>(list.Count);
            foreach (var summary in list)
            {
                lines.Add($"{summary.Extension}: {summary.Count} files, {FormatBytes(summary.ByteLength)}");
            }
            return string.Join("\n", lines);
        }

        private static string FormatLargestTextures(List<TextureDiagnostic> sourceTextures)
        {
            if (sourceTextures.Count == 0)
            {
                return "(none)";
            }
            var count = Math.Min(8, sourceTextures.Count);
            var lines = new List<string>(count);
            for (var i = 0; i < count; i++)
            {
                var texture = sourceTextures[i];
                lines.Add($"{FormatBytes(texture.ByteLength)}  {texture.Name}  ({texture.Extension})");
            }
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

                var lines = new List<string>(3 + capturedWardrobeSets.Count)
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

            var activeRenderers = CountActiveRenderers(probeRoot);
            var probes = WardrobePreviewProbePaths;
            var pathLookup = BuildProbePathLookup(probeRoot);
            var states = new List<string>(probes.Length);
            foreach (var path in probes)
            {
                states.Add(path + "=" + ProbePathState(pathLookup, path));
            }
            return $"{label}: snapshot={(set == null ? hasBaseSnapshot : set.capturedSnapshot != null && set.capturedSnapshot.nodes.Count > 0)}, ops={(set != null ? set.operations.Count : CurrentBaseOperations().Count)}, activeRenderers={activeRenderers}; {string.Join(", ", states)}";
        }

        private static readonly string[] WardrobePreviewProbePaths =
        {
            "Color  1",
            "Color  13",
            "add-belt",
            "Maid",
            "Outer"
        };

        private static int CountActiveRenderers(GameObject root)
        {
            var count = 0;
            foreach (var renderer in root.GetComponentsInChildren<Renderer>(true))
            {
                if (renderer != null && renderer.enabled && renderer.gameObject.activeInHierarchy)
                {
                    count++;
                }
            }
            return count;
        }

        private static Dictionary<string, Transform> BuildProbePathLookup(GameObject root)
        {
            var transforms = root.GetComponentsInChildren<Transform>(true);
            var lookup = new Dictionary<string, Transform>(transforms.Length, StringComparer.Ordinal);
            foreach (var candidate in transforms)
            {
                var path = VariantExtractor.TransformPath(root.transform, candidate);
                if (!lookup.ContainsKey(path))
                {
                    lookup[path] = candidate;
                }
            }
            return lookup;
        }

        private static string ProbePathState(Dictionary<string, Transform> pathLookup, string path)
        {
            if (!pathLookup.TryGetValue(path, out var transform) || transform == null)
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

            var sceneObjects = SceneObjects();
            var descriptorCandidates = new List<GameObject>();
            foreach (var go in sceneObjects)
            {
                if (HasAvatarDescriptorComponent(go))
                {
                    descriptorCandidates.Add(go);
                }
            }
            var preferredDescriptor = PreferNamedAvatarRootCandidate(descriptorCandidates);
            if (preferredDescriptor != null)
            {
                return preferredDescriptor;
            }
            if (descriptorCandidates.Count == 1)
            {
                return descriptorCandidates[0];
            }

            var humanoidCandidates = new List<GameObject>();
            var seenHumanoid = new HashSet<GameObject>();
            foreach (var go in sceneObjects)
            {
                var animator = go.GetComponent<Animator>();
                if (animator != null && animator.avatar != null && animator.avatar.isHuman && seenHumanoid.Add(animator.gameObject))
                {
                    humanoidCandidates.Add(animator.gameObject);
                }
            }
            var preferredHumanoid = PreferNamedAvatarRootCandidate(humanoidCandidates);
            if (preferredHumanoid != null)
            {
                return preferredHumanoid;
            }
            if (humanoidCandidates.Count == 1)
            {
                return humanoidCandidates[0];
            }

            return null;
        }

        private static GameObject PreferNamedAvatarRootCandidate(List<GameObject> candidates)
        {
            if (candidates == null || candidates.Count == 0)
            {
                return null;
            }

            var vrc = new List<GameObject>();
            foreach (var candidate in candidates)
            {
                var name = candidate != null ? candidate.name ?? "" : "";
                if (name.IndexOf("vrc", StringComparison.OrdinalIgnoreCase) >= 0)
                {
                    vrc.Add(candidate);
                }
            }
            if (vrc.Count == 1)
            {
                return vrc[0];
            }

            var filtered = new List<GameObject>();
            foreach (var candidate in candidates)
            {
                var name = candidate != null ? candidate.name ?? "" : "";
                if (name.IndexOf("vrm", StringComparison.OrdinalIgnoreCase) >= 0 ||
                    name.IndexOf("merge", StringComparison.OrdinalIgnoreCase) >= 0 ||
                    name.IndexOf("merged", StringComparison.OrdinalIgnoreCase) >= 0)
                {
                    continue;
                }
                filtered.Add(candidate);
            }
            return filtered.Count == 1 ? filtered[0] : null;
        }

        private static List<GameObject> SceneObjects()
        {
            var objects = new List<GameObject>();
            foreach (var go in Resources.FindObjectsOfTypeAll<GameObject>())
            {
                if (IsSceneObject(go))
                {
                    objects.Add(go);
                }
            }
            return objects;
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

            return new List<TextureDiagnostic>(byKey.Values);
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
                var renderers = avatarRoot.GetComponentsInChildren<Renderer>(true);
                validation.RendererCount = renderers.Length;
                validation.SkinnedMeshRendererCount = CountSkinnedMeshRenderers(renderers);
                validation.MaterialCount = CountDistinctMaterials(renderers);
                validation.VariantCount = VariantExtractor.Extract(avatarRoot, exportMode).Count;
                validation.WardrobeSetCount = IsCurrentToBaseOnlyExportMode() ? 0 : capturedWardrobeSets.Count;
                validation.HumanoidBoneCount = HumanoidExtractor.Extract(avatarRoot).Count;
            }

            return validation;
        }

        private static int CountSkinnedMeshRenderers(Renderer[] renderers)
        {
            var count = 0;
            foreach (var renderer in renderers)
            {
                if (renderer is SkinnedMeshRenderer)
                {
                    count++;
                }
            }
            return count;
        }

        private static int CountDistinctMaterials(Renderer[] renderers)
        {
            var materials = new HashSet<Material>();
            foreach (var renderer in renderers)
            {
                foreach (var material in renderer.sharedMaterials)
                {
                    if (material != null)
                    {
                        materials.Add(material);
                    }
                }
            }
            return materials.Count;
        }
    }
}
