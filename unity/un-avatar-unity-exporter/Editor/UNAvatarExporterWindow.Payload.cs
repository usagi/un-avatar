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
                ["textureAssets"] = TextureAssetsToJson(textureAssets),
                ["variants"] = VariantsToJson(variants),
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
            foreach (var set in capturedWardrobeSets)
            {
                foreach (var operation in set.operations)
                {
                    if (operation.type == "blendShapeWeight" && !string.IsNullOrWhiteSpace(operation.name))
                    {
                        names.Add(operation.name);
                    }
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
            var fallbackTextures = new List<ExportedTextureRecord>();
            var textureSourceBytes = new Dictionary<string, TextureSourceByteSummary>(StringComparer.Ordinal);
            foreach (var texture in exportedTextures)
            {
                if (texture.ExportMode == "png_fallback")
                {
                    fallbackTextures.Add(texture);
                }
                if (string.IsNullOrEmpty(texture.SourceExtension))
                {
                    continue;
                }
                if (!textureSourceBytes.TryGetValue(texture.SourceExtension, out var summary))
                {
                    summary = new TextureSourceByteSummary { Extension = texture.SourceExtension };
                    textureSourceBytes[texture.SourceExtension] = summary;
                }
                summary.Count++;
                summary.SourceByteLength += texture.SourceByteLength;
            }
            var textureSourceBytesByExtension = TextureSourceByteSummariesToJson(textureSourceBytes.Values);
            fallbackTextures.Sort((a, b) => b.SourceByteLength.CompareTo(a.SourceByteLength));

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
                ["variants"] = VariantsToJson(variants),
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
                    ["fallbacks"] = TextureRecordsToJson(fallbackTextures),
                    ["items"] = TextureRecordsToJson(exportedTextures)
                },
                ["unsupported"] = unsupported
            };
        }

        private sealed class TextureSourceByteSummary
        {
            public string Extension;
            public int Count;
            public long SourceByteLength;
        }

        private static List<object> TextureSourceByteSummariesToJson(IEnumerable<TextureSourceByteSummary> summaries)
        {
            var list = new List<TextureSourceByteSummary>(summaries);
            list.Sort((a, b) => b.SourceByteLength.CompareTo(a.SourceByteLength));
            var json = new List<object>(list.Count);
            foreach (var summary in list)
            {
                json.Add(new Dictionary<string, object>
                {
                    ["extension"] = summary.Extension,
                    ["count"] = summary.Count,
                    ["sourceByteLength"] = summary.SourceByteLength
                });
            }
            return json;
        }

        private static List<object> TextureRecordsToJson(List<ExportedTextureRecord> textures)
        {
            var json = new List<object>(textures != null ? textures.Count : 0);
            if (textures == null)
            {
                return json;
            }
            foreach (var texture in textures)
            {
                json.Add(texture.ToJson());
            }
            return json;
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
                ["sets"] = WardrobePreviewDiagnosticSetsToJson(sets)
            };
        }

        private static List<object> TextureAssetsToJson(List<UnavatarTextureAssetRecord> textureAssets)
        {
            var json = new List<object>(textureAssets != null ? textureAssets.Count : 0);
            if (textureAssets == null)
            {
                return json;
            }
            foreach (var asset in textureAssets)
            {
                json.Add(asset.ToJson());
            }
            return json;
        }

        private static List<object> VariantsToJson(List<VariantRecord> variants)
        {
            var json = new List<object>(variants != null ? variants.Count : 0);
            if (variants == null)
            {
                return json;
            }
            foreach (var variant in variants)
            {
                json.Add(variant.ToJson());
            }
            return json;
        }

        private static List<object> WardrobePreviewDiagnosticSetsToJson(List<WardrobeSetDraft> sets)
        {
            var json = new List<object>(sets.Count);
            foreach (var set in sets)
            {
                json.Add(new Dictionary<string, object>
                {
                    ["id"] = set.id ?? "",
                    ["displayName"] = set.displayName ?? "",
                    ["previewCount"] = set.previewImages != null ? set.previewImages.Count : 0,
                    ["previews"] = WardrobePreviewDiagnosticsToJson(set.previewImages)
                });
            }
            return json;
        }

        private static List<object> WardrobePreviewDiagnosticsToJson(List<WardrobePreviewImageDraft> previews)
        {
            var json = new List<object>(previews != null ? previews.Count : 0);
            if (previews == null)
            {
                return json;
            }
            foreach (var image in previews)
            {
                if (image == null)
                {
                    continue;
                }
                json.Add(new Dictionary<string, object>
                {
                    ["view"] = image.view ?? "",
                    ["byteLength"] = image.pngBytes != null ? image.pngBytes.Count : 0,
                    ["stateDigest"] = image.stateDigest ?? "",
                    ["stateDetails"] = StringListToObjectList(image.stateDetails),
                    ["sha256"] = Sha256Hex(image.pngBytes)
                });
            }
            return json;
        }

        private static List<object> StringListToObjectList(List<string> values)
        {
            var json = new List<object>(values != null ? values.Count : 0);
            if (values == null)
            {
                return json;
            }
            foreach (var value in values)
            {
                json.Add(value);
            }
            return json;
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

            var sorted = new List<UnsupportedMaterialPropertyReport>(reports.Values);
            sorted.Sort(CompareUnsupportedMaterialPropertyReport);
            var json = new List<object>(sorted.Count);
            foreach (var report in sorted)
            {
                json.Add(report.ToJson());
            }
            return json;
        }

        private static int CompareUnsupportedMaterialPropertyReport(
            UnsupportedMaterialPropertyReport left,
            UnsupportedMaterialPropertyReport right)
        {
            var category = string.Compare(left.Category, right.Category, StringComparison.Ordinal);
            if (category != 0)
            {
                return category;
            }
            var property = string.Compare(left.Property, right.Property, StringComparison.Ordinal);
            if (property != 0)
            {
                return property;
            }
            return left.Value.CompareTo(right.Value);
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
                : CloneWardrobeOperations(importedBaseOperations);
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
                foreach (var variant in variants)
                {
                    if (variant.Id == "current-state")
                    {
                        continue;
                    }
                    sets.Add(new Dictionary<string, object>
                    {
                        ["id"] = "candidate-" + variant.Id,
                        ["displayName"] = variant.Name,
                        ["source"] = variant.Source,
                        ["default"] = false,
                        ["assetGroups"] = new List<object>(),
                        ["operations"] = VariantOperationsAsObjects(variant.Operations)
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

        private static List<WardrobeOperationDraft> CloneWardrobeOperations(List<WardrobeOperationDraft> operations)
        {
            var cloned = new List<WardrobeOperationDraft>(operations != null ? operations.Count : 0);
            if (operations == null)
            {
                return cloned;
            }
            foreach (var operation in operations)
            {
                if (operation != null)
                {
                    cloned.Add(WardrobeSnapshotCapture.CloneOperation(operation));
                }
            }
            return cloned;
        }

        private static List<object> WardrobeOperationsAsObjects(List<WardrobeOperationDraft> operations)
        {
            var json = new List<object>(operations != null ? operations.Count : 0);
            if (operations == null)
            {
                return json;
            }
            foreach (var operation in operations)
            {
                json.Add(operation);
            }
            return json;
        }

        private static List<object> VariantOperationsAsObjects(List<Dictionary<string, object>> operations)
        {
            var json = new List<object>(operations != null ? operations.Count : 0);
            if (operations == null)
            {
                return json;
            }
            foreach (var operation in operations)
            {
                json.Add(operation);
            }
            return json;
        }
    }
}
