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
        private void ExportSelected()
        {
            var validation = ValidateSelection();
            if (!validation.CanExport)
            {
                lastSummary = validation.ToDisplayText();
                ShowNotification(new GUIContent("Export is not ready."));
                return;
            }

            var normalizedPath = EnsureUnavatarExtension(exportPath);
            exportPath = normalizedPath;
            forceIncludeInactiveObjects = true;
            var reportPath = normalizedPath + ".report.json";
            var tempDir = Path.Combine(Path.GetTempPath(), "un-avatar-unity-exporter-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(tempDir);

            GameObject clone = null;
            try
            {
                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Preparing clone", 0.1f);
                clone = Instantiate(avatarRoot);
                clone.name = avatarRoot.name + " (UNAvatar Export)";
                clone.hideFlags = HideFlags.HideAndDontSave;
                clone.SetActive(true);

                var sourceVariants = VariantExtractor.Extract(avatarRoot, exportMode);
                var humanoid = HumanoidExtractor.Extract(avatarRoot);
                var splitWardrobeMode = exportMode == UNAvatarExportMode.WardrobeSplit;
                var bakedWardrobeMode = exportMode == UNAvatarExportMode.WardrobeBaked;

                if (forceIncludeInactiveObjects && bakedWardrobeMode)
                {
                    SetActiveRecursive(clone.transform, true);
                }
                if (bakedWardrobeMode)
                {
                    ApplyWardrobeOperationsToRoot(clone, CurrentBaseOperations());
                }

                var bakeAttempted = ModularAvatarBridge.IsAvailable && !splitWardrobeMode;
                var bakeSucceeded = false;
                if (bakeAttempted)
                {
                    EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Baking Modular Avatar clone", 0.25f);
                    bakeSucceeded = ModularAvatarBridge.TryBake(clone, out var bakeError);
                    if (!bakeSucceeded)
                    {
                        Debug.LogWarning("[U.N. Avatar] Modular Avatar bake failed: " + bakeError);
                    }
                }
                var bakedBaseSnapshot = WardrobeSnapshotCapture.Capture(clone);
                // Per-set Modular Avatar baking is too risky for the preview exporter: some VRC avatar
                // projects can crash Unity during repeated bake/active-state mutation. Keep the exported
                // model baked, but store wardrobe sets as authored capture diffs until the bake path is hardened.
                List<WardrobeSetDraft> bakedWardrobeSets = null;

                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Exporting GLB", 0.55f);
                var glbName = SanitizeFileName(avatarRoot.name);
                var exportResult = MinimalGltfExporter.ExportGlb(clone, tempDir, glbName, ReferencedMorphTargetNamesForExport());
                var tempGlb = exportResult.Path;

                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Patching UN_avatar extension", 0.8f);
                RegenerateWardrobePreviewImagesForExport();
                // Wardrobe sets are currently stored as authored capture diffs, not per-set baked diffs.
                // Keep Base authored as well; post-bake snapshots can be altered by Modular Avatar and are
                // only safe as the wardrobe baseline once per-set baked snapshots are enabled again.
                var wardrobeBaseSnapshot = bakedWardrobeSets != null ? bakedBaseSnapshot : null;
                var exportWardrobeSets = bakedWardrobeSets ?? WardrobeSetsForExport();
                var exportPreviewImages = PreviewImagesForExport(exportWardrobeSets);
                var extension = BuildExtensionPayload(sourceVariants, humanoid, bakeAttempted, bakeSucceeded, clone, wardrobeBaseSnapshot, exportWardrobeSets, exportResult.TextureAssets);
                GlbExtensionPatcher.PatchRootExtension(tempGlb, normalizedPath, ExtensionName, extension, exportResult.TextureAssets, exportPreviewImages);

                var report = BuildReportPayload(validation, sourceVariants, humanoid, normalizedPath, bakeAttempted, bakeSucceeded, wardrobeBaseSnapshot, exportWardrobeSets, exportResult.Textures);
                File.WriteAllText(reportPath, MiniJson.Serialize(report), new UTF8Encoding(false));

                AssetDatabase.Refresh();
                lastSummary = "Exported\n" + normalizedPath + "\n\nReport\n" + reportPath;
                ShowNotification(new GUIContent("Exported .unavatar"));
            }
            catch (Exception ex)
            {
                Debug.LogException(ex);
                lastSummary = "Export failed:\n" + ex.Message;
                ShowNotification(new GUIContent("Export failed."));
            }
            finally
            {
                EditorUtility.ClearProgressBar();
                if (clone != null)
                {
                    DestroyImmediate(clone);
                }
                try
                {
                    if (Directory.Exists(tempDir))
                    {
                        Directory.Delete(tempDir, true);
                    }
                }
                catch
                {
                    // Best effort cleanup. The temp directory path is included in Unity logs if deletion fails elsewhere.
                }
            }
        }

        private Dictionary<string, object> BuildExtensionPayload(
            List<VariantRecord> variants,
            Dictionary<string, string> humanoid,
            bool bakeAttempted,
            bool bakeSucceeded,
            GameObject registryRoot,
            WardrobeSnapshotDraft exportBaseSnapshot,
            List<WardrobeSetDraft> exportWardrobeSets,
            List<UnavatarTextureAssetRecord> textureAssets)
        {
            return new Dictionary<string, object>
            {
                ["specVersion"] = SpecVersion,
                ["generator"] = "U.N. Avatar Unity Exporter 0.1.0-preview",
                ["textureCoordinateConvention"] = "gltf",
                ["manifest"] = new Dictionary<string, object>
                {
                    ["name"] = avatarRoot != null ? avatarRoot.name : "",
                    ["sourceType"] = "vrc_unity_prefab",
                    ["exportMode"] = exportMode.ToString(),
                    ["createdUtc"] = DateTime.UtcNow.ToString("O", CultureInfo.InvariantCulture)
                },
                ["humanoid"] = humanoid,
                ["nodes"] = BuildNodeRegistryPayload(registryRoot),
                ["textureAssets"] = (textureAssets ?? new List<UnavatarTextureAssetRecord>())
                    .Select(asset => asset.ToJson())
                    .Cast<object>()
                    .ToList(),
                ["variants"] = variants.Select(v => v.ToJson()).ToList<object>(),
                ["wardrobe"] = BuildWardrobePayload(variants, exportBaseSnapshot, exportWardrobeSets),
                ["provenance"] = new Dictionary<string, object>
                {
                    ["unityVersion"] = Application.unityVersion,
                    ["sourceName"] = avatarRoot != null ? avatarRoot.name : "",
                    ["redistributionAllowed"] = false
                },
                ["unityExporter"] = new Dictionary<string, object>
                {
                    ["buildMarker"] = ExporterBuildMarker,
                    ["bakeModularAvatar"] = exportMode != UNAvatarExportMode.WardrobeSplit,
                    ["modularAvatarInstalled"] = ModularAvatarBridge.IsAvailable,
                    ["modularAvatarBakeAttempted"] = bakeAttempted,
                    ["modularAvatarBakeSucceeded"] = bakeSucceeded,
                    ["forceIncludeInactiveObjects"] = forceIncludeInactiveObjects,
                    ["gltfWriter"] = "built-in"
                }
            };
        }

        private HashSet<string> ReferencedMorphTargetNamesForExport()
        {
            var names = new HashSet<string>(StringComparer.Ordinal);
            foreach (var operation in capturedWardrobeSets.SelectMany(set => set.operations))
            {
                if (operation.type == "blendShapeWeight" && !string.IsNullOrWhiteSpace(operation.name))
                {
                    names.Add(operation.name);
                }
            }
            foreach (var operation in CurrentBaseOperations())
            {
                if (operation.type == "blendShapeWeight" && !string.IsNullOrWhiteSpace(operation.name) && Math.Abs(operation.floatValue) > 0.001f)
                {
                    names.Add(operation.name);
                }
            }
            return names;
        }

        private Dictionary<string, object> BuildReportPayload(
            ExportValidation validation,
            List<VariantRecord> variants,
            Dictionary<string, string> humanoid,
            string output,
            bool bakeAttempted,
            bool bakeSucceeded,
            WardrobeSnapshotDraft exportBaseSnapshot,
            List<WardrobeSetDraft> exportWardrobeSets,
            List<ExportedTextureRecord> exportedTextures)
        {
            exportedTextures = exportedTextures ?? new List<ExportedTextureRecord>();
            var fallbackTextures = exportedTextures
                .Where(texture => texture.ExportMode == "png_fallback")
                .ToList();
            var textureSourceBytesByExtension = exportedTextures
                .Where(texture => !string.IsNullOrEmpty(texture.SourceExtension))
                .GroupBy(texture => texture.SourceExtension)
                .OrderByDescending(group => group.Sum(texture => texture.SourceByteLength))
                .Select(group => new Dictionary<string, object>
                {
                    ["extension"] = group.Key,
                    ["count"] = group.Count(),
                    ["sourceByteLength"] = group.Sum(texture => texture.SourceByteLength)
                })
                .Cast<object>()
                .ToList();

            var unsupported = BuildUnsupportedReportItems();

            return new Dictionary<string, object>
            {
                ["schema"] = "network.usagi.un-avatar.unity-exporter.report",
                ["schemaVersion"] = "0.1-preview",
                ["output"] = output,
                ["unityVersion"] = Application.unityVersion,
                ["avatarRoot"] = avatarRoot != null ? avatarRoot.name : "",
                ["exportMode"] = exportMode.ToString(),
                ["validation"] = validation.ToJson(),
                ["humanoidBoneCount"] = humanoid.Count,
                ["variantCount"] = variants.Count,
                ["variants"] = variants.Select(v => v.ToJson()).ToList<object>(),
                ["wardrobeSetCount"] = capturedWardrobeSets.Count,
                ["wardrobe"] = BuildWardrobePayload(variants, exportBaseSnapshot, exportWardrobeSets),
                ["wardrobePreviewDiagnostics"] = BuildWardrobePreviewDiagnostics(exportWardrobeSets),
                ["unityExporter"] = new Dictionary<string, object>
                {
                    ["buildMarker"] = ExporterBuildMarker,
                    ["gltfWriter"] = "built-in"
                },
                ["bake"] = new Dictionary<string, object>
                {
                    ["modularAvatarInstalled"] = ModularAvatarBridge.IsAvailable,
                    ["attempted"] = bakeAttempted,
                    ["succeeded"] = bakeSucceeded
                },
                ["textures"] = new Dictionary<string, object>
                {
                    ["count"] = exportedTextures.Count,
                    ["fallbackCount"] = fallbackTextures.Count,
                    ["sourceBytesByExtension"] = textureSourceBytesByExtension,
                    ["fallbacks"] = fallbackTextures
                        .OrderByDescending(texture => texture.SourceByteLength)
                        .Select(texture => texture.ToJson())
                        .Cast<object>()
                        .ToList(),
                    ["items"] = exportedTextures
                        .Select(texture => texture.ToJson())
                        .Cast<object>()
                        .ToList()
                },
                ["unsupported"] = unsupported
            };
        }

        private Dictionary<string, object> BuildWardrobePreviewDiagnostics(List<WardrobeSetDraft> exportWardrobeSets)
        {
            var sets = new List<WardrobeSetDraft>
            {
                new WardrobeSetDraft
                {
                    id = "base",
                    displayName = "Base",
                    previewImages = basePreviewImages ?? new List<WardrobePreviewImageDraft>()
                }
            };
            sets.AddRange(exportWardrobeSets ?? WardrobeSetsForExport());

            return new Dictionary<string, object>
            {
                ["sets"] = sets.Select(set => new Dictionary<string, object>
                {
                    ["id"] = set.id ?? "",
                    ["displayName"] = set.displayName ?? "",
                    ["previewCount"] = set.previewImages != null ? set.previewImages.Count : 0,
                    ["previews"] = (set.previewImages ?? new List<WardrobePreviewImageDraft>())
                        .Select(image => new Dictionary<string, object>
                        {
                            ["view"] = image.view ?? "",
                            ["byteLength"] = image.pngBytes != null ? image.pngBytes.Count : 0,
                            ["stateDigest"] = image.stateDigest ?? "",
                            ["stateDetails"] = (image.stateDetails ?? new List<string>()).Cast<object>().ToList(),
                            ["sha256"] = Sha256Hex(image.pngBytes)
                        })
                        .Cast<object>()
                        .ToList()
                }).Cast<object>().ToList()
            };
        }

        private static string Sha256Hex(List<byte> bytes)
        {
            if (bytes == null || bytes.Count == 0)
            {
                return "";
            }
            using (var sha = SHA256.Create())
            {
                var hash = sha.ComputeHash(bytes.ToArray());
                var sb = new StringBuilder(hash.Length * 2);
                foreach (var b in hash)
                {
                    sb.Append(b.ToString("x2", CultureInfo.InvariantCulture));
                }
                return sb.ToString();
            }
        }

        private List<object> BuildUnsupportedReportItems()
        {
            var items = new List<object>
            {
                "Full FX Animator evaluation",
                "Full Poiyomi material reproduction",
                "Full VRC contacts/interactions"
            };
            items.AddRange(BuildUnsupportedMaterialRenderStateItems());
            return items;
        }

        private List<object> BuildUnsupportedMaterialRenderStateItems()
        {
            var reports = new Dictionary<string, UnsupportedMaterialPropertyReport>();
            if (avatarRoot == null)
            {
                return new List<object>();
            }

            foreach (var renderer in avatarRoot.GetComponentsInChildren<Renderer>(true))
            {
                foreach (var material in renderer.sharedMaterials)
                {
                    if (material == null)
                    {
                        continue;
                    }
                    AddUnsupportedMaterialFloat(reports, material, "_StencilRef", 0.0f, "stencil");
                    AddUnsupportedMaterialFloat(reports, material, "_StencilReadMask", 255.0f, "stencil");
                    AddUnsupportedMaterialFloat(reports, material, "_StencilWriteMask", 255.0f, "stencil");
                    AddUnsupportedMaterialFloat(reports, material, "_StencilComp", 8.0f, "stencil");
                    AddUnsupportedMaterialFloat(reports, material, "_StencilPass", 0.0f, "stencil");
                    AddUnsupportedMaterialFloat(reports, material, "_StencilFail", 0.0f, "stencil");
                    AddUnsupportedMaterialFloat(reports, material, "_StencilZFail", 0.0f, "stencil");
                    AddUnsupportedMaterialFloat(reports, material, "_ColorMask", 15.0f, "color_mask");
                    AddUnsupportedMaterialFloat(reports, material, "_OffsetFactor", 0.0f, "depth_offset");
                    AddUnsupportedMaterialFloat(reports, material, "_OffsetUnits", 0.0f, "depth_offset");
                    AddUnsupportedMaterialFloat(reports, material, "_OutlineColorMask", 15.0f, "outline_color_mask");
                    AddUnsupportedMaterialFloat(reports, material, "_OutlineOffsetFactor", 0.0f, "outline_depth_offset");
                    AddUnsupportedMaterialFloat(reports, material, "_OutlineOffsetUnits", 0.0f, "outline_depth_offset");
                }
            }

            return reports.Values
                .OrderBy(report => report.Category)
                .ThenBy(report => report.Property)
                .ThenBy(report => report.Value)
                .Select(report => report.ToJson())
                .Cast<object>()
                .ToList();
        }

        private static void AddUnsupportedMaterialFloat(
            Dictionary<string, UnsupportedMaterialPropertyReport> reports,
            Material material,
            string property,
            float defaultValue,
            string category)
        {
            if (!material.HasProperty(property))
            {
                return;
            }
            var value = ReadMaterialFloat(material, property, defaultValue);
            if (Mathf.Approximately(value, defaultValue))
            {
                return;
            }
            var key = category + "\n" + property + "\n" + value.ToString("R", CultureInfo.InvariantCulture);
            if (!reports.TryGetValue(key, out var report))
            {
                report = new UnsupportedMaterialPropertyReport
                {
                    Category = category,
                    Property = property,
                    Value = value
                };
                reports[key] = report;
            }
            report.Count++;
            if (report.SampleMaterials.Count < 8 && !report.SampleMaterials.Contains(material.name))
            {
                report.SampleMaterials.Add(material.name);
            }
        }

        private static float ReadMaterialFloat(Material material, string property, float fallback)
        {
            return material != null && material.HasProperty(property) ? material.GetFloat(property) : fallback;
        }

        private Dictionary<string, object> BuildWardrobePayload(
            List<VariantRecord> variants,
            WardrobeSnapshotDraft exportBaseSnapshot = null,
            List<WardrobeSetDraft> exportWardrobeSets = null)
        {
            var hasExportBaseSnapshot = exportBaseSnapshot != null && exportBaseSnapshot.nodes.Count > 0;
            var baseOperations = hasExportBaseSnapshot
                ? WardrobeSnapshotCapture.BaseOperations(exportBaseSnapshot)
                : hasBaseSnapshot
                ? WardrobeSnapshotCapture.BaseOperations(baseSnapshot)
                : importedBaseOperations.Select(WardrobeSnapshotCapture.CloneOperation).ToList();
            var sets = new List<object>
            {
                new WardrobeSetDraft
                {
                    id = "base",
                    displayName = "Base",
                    source = hasExportBaseSnapshot ? "unity_baked_capture_base" : hasBaseSnapshot ? "unity_capture_base" : hasImportedBaseOperations ? "imported_unavatar_base" : "implicit_current_state",
                    operations = baseOperations,
                    previewImages = basePreviewImages ?? new List<WardrobePreviewImageDraft>()
                }.ToJson(true)
            };

            var nonBaseSets = exportWardrobeSets ?? WardrobeSetsForExport();
            foreach (var set in nonBaseSets)
            {
                sets.Add(set.ToJson(false));
            }

            if (nonBaseSets.Count == 0 && variants != null)
            {
                foreach (var variant in variants.Where(v => v.Id != "current-state"))
                {
                    sets.Add(new Dictionary<string, object>
                    {
                        ["id"] = "candidate-" + variant.Id,
                        ["displayName"] = variant.Name,
                        ["source"] = variant.Source,
                        ["default"] = false,
                        ["assetGroups"] = new List<object>(),
                        ["operations"] = variant.Operations.Cast<object>().ToList()
                    });
                }
            }

            return new Dictionary<string, object>
            {
                ["baseSet"] = "base",
                ["captureBase"] = hasExportBaseSnapshot ? SnapshotSummary(exportBaseSnapshot) : hasBaseSnapshot ? SnapshotSummary(baseSnapshot) : new Dictionary<string, object>(),
                ["sets"] = sets
            };
        }

        private List<WardrobePreviewImageDraft> PreviewImagesForExport(List<WardrobeSetDraft> exportWardrobeSets = null)
        {
            var previews = new List<WardrobePreviewImageDraft>();
            if (basePreviewImages != null)
            {
                previews.AddRange(basePreviewImages.Where(image => image != null));
            }
            var sets = exportWardrobeSets ?? WardrobeSetsForExport();
            foreach (var set in sets)
            {
                if (set.previewImages == null)
                {
                    continue;
                }
                previews.AddRange(set.previewImages.Where(image => image != null));
            }
            return previews;
        }

        private void RegenerateWardrobePreviewImagesForExport()
        {
            if (avatarRoot == null)
            {
                return;
            }

            var previewBounds = CalculateWardrobePreviewBoundsForExport();
            basePreviewImages = CapturePreviewImagesForState("base", null, previewBounds);
            for (var i = 0; i < capturedWardrobeSets.Count; i++)
            {
                capturedWardrobeSets[i].previewImages = CapturePreviewImagesForState(capturedWardrobeSets[i].id, capturedWardrobeSets[i], previewBounds);
            }
        }

        private List<WardrobePreviewImageDraft> CapturePreviewImagesForState(string label, WardrobeSetDraft set, Bounds previewBounds)
        {
            GameObject previewClone = null;
            try
            {
                previewClone = CreateWardrobePreviewClone(label);
                if (set == null)
                {
                    ApplyBaseStateToRoot(previewClone);
                }
                else
                {
                    ApplyWardrobeSetStateToRoot(previewClone, set);
                }
                PrepareWardrobePreviewRenderers(previewClone);
                var previews = WardrobePreviewCapture.Capture(previewClone, previewBounds, CurrentPreviewCaptureOptions());
                AssignPreviewStateDigest(previews, label, previewClone);
                return previews;
            }
            finally
            {
                if (previewClone != null)
                {
                    DestroyImmediate(previewClone);
                }
            }
        }

        private GameObject CreateWardrobePreviewClone(string label)
        {
            var previewClone = Instantiate(avatarRoot);
            previewClone.name = avatarRoot.name + " (UNAvatar Preview Capture " + (label ?? "state") + ")";
            previewClone.hideFlags = HideFlags.HideAndDontSave;
            previewClone.SetActive(true);
            return previewClone;
        }

        private static void PrepareWardrobePreviewRenderers(GameObject root)
        {
            foreach (var skinned in root.GetComponentsInChildren<SkinnedMeshRenderer>(true))
            {
                skinned.updateWhenOffscreen = true;
                skinned.forceMatrixRecalculationPerRender = true;
            }
        }

        private static void AssignPreviewStateDigest(List<WardrobePreviewImageDraft> previews, string label, GameObject root)
        {
            var digest = WardrobePreviewStateDigest(label, root);
            var details = WardrobePreviewStateDetails(root);
            foreach (var preview in previews ?? new List<WardrobePreviewImageDraft>())
            {
                if (preview != null)
                {
                    preview.stateDigest = digest;
                    preview.stateDetails = details;
                }
            }
        }

        private static string WardrobePreviewStateDigest(string label, GameObject root)
        {
            if (root == null)
            {
                return label + "|missing-root";
            }

            var renderers = root.GetComponentsInChildren<Renderer>(true);
            var active = renderers
                .Where(renderer => renderer != null && renderer.enabled && renderer.gameObject.activeInHierarchy)
                .Select(renderer => VariantExtractor.TransformPath(root.transform, renderer.transform))
                .OrderBy(path => path, StringComparer.Ordinal)
                .ToList();
            var probes = new[]
            {
                "Color  1",
                "Color  13",
                "add-belt",
                "Maid",
                "Outer"
            };
            var states = probes.Select(path => path + "=" + ProbePathState(root, path));
            using (var sha = SHA256.Create())
            {
                var joined = string.Join("\n", active);
                var hash = sha.ComputeHash(Encoding.UTF8.GetBytes(joined));
                var sb = new StringBuilder(hash.Length * 2);
                foreach (var b in hash)
                {
                    sb.Append(b.ToString("x2", CultureInfo.InvariantCulture));
                }
                return $"{label}|activeRenderers={active.Count}|activeHash={sb}|{string.Join(",", states)}";
            }
        }

        private static List<string> WardrobePreviewStateDetails(GameObject root)
        {
            if (root == null)
            {
                return new List<string>();
            }

            return root.GetComponentsInChildren<Renderer>(true)
                .Where(renderer => renderer != null && renderer.enabled && renderer.gameObject.activeInHierarchy)
                .OrderBy(renderer => VariantExtractor.TransformPath(root.transform, renderer.transform), StringComparer.Ordinal)
                .Select(renderer =>
                {
                    var path = VariantExtractor.TransformPath(root.transform, renderer.transform);
                    var layerName = LayerMask.LayerToName(renderer.gameObject.layer);
                    var bounds = renderer.bounds;
                    var materials = string.Join(",",
                        (renderer.sharedMaterials ?? Array.Empty<Material>())
                            .Where(material => material != null)
                            .Select(material => material.name
                                + "/" + (material.shader != null ? material.shader.name : "<no-shader>")
                                + "/rq" + material.renderQueue.ToString(CultureInfo.InvariantCulture)));
                    return path
                        + "|layer=" + renderer.gameObject.layer.ToString(CultureInfo.InvariantCulture) + ":" + layerName
                        + "|boundsCenter=" + Vec3String(bounds.center)
                        + "|boundsSize=" + Vec3String(bounds.size)
                        + "|materials=" + materials;
                })
                .ToList();
        }

        private static string Vec3String(Vector3 value)
        {
            return value.x.ToString("R", CultureInfo.InvariantCulture)
                + "," + value.y.ToString("R", CultureInfo.InvariantCulture)
                + "," + value.z.ToString("R", CultureInfo.InvariantCulture);
        }

        private Bounds CalculateWardrobePreviewBoundsForExport()
        {
            var bounds = CalculateWardrobePreviewBoundsForState(null);
            foreach (var set in capturedWardrobeSets)
            {
                var setBounds = CalculateWardrobePreviewBoundsForState(set);
                if (bounds.size == Vector3.zero)
                {
                    bounds = setBounds;
                }
                else if (setBounds.size != Vector3.zero)
                {
                    bounds.Encapsulate(setBounds);
                }
            }
            return bounds;
        }

        private Bounds CalculateWardrobePreviewBoundsForState(WardrobeSetDraft set)
        {
            GameObject previewClone = null;
            try
            {
                previewClone = CreateWardrobePreviewClone(set == null ? "base-bounds" : set.id + "-bounds");
                if (set == null)
                {
                    ApplyBaseStateToRoot(previewClone);
                }
                else
                {
                    ApplyWardrobeSetStateToRoot(previewClone, set);
                }
                PrepareWardrobePreviewRenderers(previewClone);
                return WardrobePreviewCapture.CalculateVisibleBounds(previewClone);
            }
            finally
            {
                if (previewClone != null)
                {
                    DestroyImmediate(previewClone);
                }
            }
        }

        private List<WardrobeSetDraft> WardrobeSetsForExport()
        {
            if (!hasBaseSnapshot)
            {
                return capturedWardrobeSets;
            }

            var sets = new List<WardrobeSetDraft>(capturedWardrobeSets.Count);
            foreach (var set in capturedWardrobeSets)
            {
                if (set.capturedSnapshot == null || set.capturedSnapshot.nodes.Count == 0)
                {
                    sets.Add(set);
                    continue;
                }

                var rebased = WardrobeSnapshotCapture.Diff(baseSnapshot, set.capturedSnapshot, set.displayName);
                rebased.id = set.id;
                rebased.displayName = set.displayName;
                rebased.source = set.source + "_export_rebased";
                rebased.capturedSnapshot = set.capturedSnapshot;
                rebased.previewImages = ClonePreviewImages(set.previewImages);
                sets.Add(rebased);
            }
            return sets;
        }

        private static List<WardrobePreviewImageDraft> ClonePreviewImages(List<WardrobePreviewImageDraft> previews)
        {
            return (previews ?? new List<WardrobePreviewImageDraft>())
                .Select(WardrobePreviewCapture.ClonePreview)
                .Where(image => image != null)
                .ToList();
        }

        private List<WardrobeSetDraft> BuildBakedWardrobeSets(WardrobeSnapshotDraft bakedBaseSnapshot, bool bakeWithModularAvatar)
        {
            var sets = new List<WardrobeSetDraft>();
            if (avatarRoot == null || capturedWardrobeSets.Count == 0 || bakedBaseSnapshot == null || bakedBaseSnapshot.nodes.Count == 0)
            {
                return sets;
            }

            for (var i = 0; i < capturedWardrobeSets.Count; i++)
            {
                var source = capturedWardrobeSets[i];
                GameObject setClone = null;
                try
                {
                    EditorUtility.DisplayProgressBar(
                        "U.N. Avatar Export",
                        $"Baking wardrobe set {i + 1}/{capturedWardrobeSets.Count}: {source.displayName}",
                        0.32f + 0.18f * ((float)i / Math.Max(1, capturedWardrobeSets.Count)));
                    setClone = Instantiate(avatarRoot);
                    setClone.name = avatarRoot.name + " (UNAvatar Wardrobe " + source.id + ")";
                    setClone.hideFlags = HideFlags.HideAndDontSave;
                    setClone.SetActive(true);
                    if (forceIncludeInactiveObjects && exportMode != UNAvatarExportMode.CurrentOnly)
                    {
                        SetActiveRecursive(setClone.transform, true);
                    }
                    ApplyWardrobeOperationsToRoot(setClone, CurrentBaseOperations());
                    ApplyWardrobeOperationsToRoot(setClone, source.operations);
                    if (bakeWithModularAvatar)
                    {
                        if (!ModularAvatarBridge.TryBake(setClone, out var bakeError))
                        {
                            Debug.LogWarning("[U.N. Avatar] Modular Avatar bake failed for wardrobe set " + source.displayName + ": " + bakeError);
                        }
                    }
                    var snapshot = WardrobeSnapshotCapture.Capture(setClone);
                    var baked = WardrobeSnapshotCapture.Diff(bakedBaseSnapshot, snapshot, source.displayName);
                    baked.id = source.id;
                    baked.displayName = source.displayName;
                    baked.source = source.source + "_baked";
                    baked.capturedSnapshot = snapshot;
                    baked.previewImages = ClonePreviewImages(source.previewImages);
                    sets.Add(baked);
                }
                finally
                {
                    if (setClone != null)
                    {
                        DestroyImmediate(setClone);
                    }
                }
            }
            return sets;
        }

        private List<object> BuildNodeRegistryPayload(GameObject registryRoot = null)
        {
            var nodes = new List<object>();
            var rootObject = registryRoot != null ? registryRoot : avatarRoot;
            if (rootObject == null)
            {
                return nodes;
            }
            foreach (var transform in rootObject.GetComponentsInChildren<Transform>(true))
            {
                nodes.Add(new Dictionary<string, object>
                {
                    ["nodeId"] = WardrobeSnapshotCapture.NodeIdFor(rootObject.transform, transform),
                    ["path"] = VariantExtractor.TransformPath(rootObject.transform, transform),
                    ["name"] = transform.name
                });
            }
            return nodes;
        }

        private static Dictionary<string, object> SnapshotSummary(WardrobeSnapshotDraft snapshot)
        {
            return new Dictionary<string, object>
            {
                ["rootName"] = snapshot.rootName ?? "",
                ["nodeCount"] = snapshot.nodes.Count,
                ["rendererCount"] = snapshot.renderers.Count,
                ["blendShapeCount"] = snapshot.blendShapes.Count
            };
        }
    }
}
