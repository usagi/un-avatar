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
    }
}
