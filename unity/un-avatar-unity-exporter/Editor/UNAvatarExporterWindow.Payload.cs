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
            List<object> dynamicsPayload,
            List<object> contactsPayload,
            WardrobeSnapshotDraft exportBaseSnapshot,
            List<WardrobeSetDraft> exportWardrobeSets,
            List<UnavatarTextureAssetRecord> textureAssets,
            List<UnavatarRendererAssetRecord> rendererAssets)
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
                ["dynamics"] = dynamicsPayload ?? new List<object>(),
                ["contacts"] = contactsPayload ?? new List<object>(),
                ["textureAssets"] = TextureAssetsToJson(textureAssets),
                ["variants"] = VariantsToJson(variants),
                ["wardrobe"] = BuildWardrobePayload(variants, exportBaseSnapshot, exportWardrobeSets, registryRoot, rendererAssets, dynamicsPayload),
                ["modularAvatar"] = BuildModularAvatarPayload(registryRoot, textureAssets),
                ["provenance"] = new Dictionary<string, object>
                {
                    ["unityVersion"] = Application.unityVersion,
                    ["sourceName"] = avatarRoot != null ? avatarRoot.name : "",
                    ["redistributionAllowed"] = false
                },
                ["unityExporter"] = new Dictionary<string, object>
                {
                    ["buildMarker"] = ExporterBuildMarker,
                    ["bakeModularAvatar"] = false,
                    ["modularAvatarInstalled"] = ModularAvatarBridge.IsAvailable,
                    ["modularAvatarBakeAttempted"] = bakeAttempted,
                    ["modularAvatarBakeSucceeded"] = bakeSucceeded,
                    ["forceIncludeInactiveObjects"] = forceIncludeInactiveObjects,
                    ["gltfWriter"] = "built-in"
                }
            };
        }

        private Dictionary<string, object> BuildReportPayload(
            ExportValidation validation,
            List<VariantRecord> variants,
            Dictionary<string, string> humanoid,
            string output,
            bool bakeAttempted,
            bool bakeSucceeded,
            List<object> dynamicsPayload,
            List<object> contactsPayload,
            WardrobeSnapshotDraft exportBaseSnapshot,
            List<WardrobeSetDraft> exportWardrobeSets,
            List<ExportedTextureRecord> exportedTextures,
            List<UnavatarRendererAssetRecord> rendererAssets)
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
                ["variants"] = VariantSummariesToJson(variants),
                ["wardrobeSetCount"] = exportWardrobeSets != null ? exportWardrobeSets.Count : capturedWardrobeSets.Count,
                ["wardrobe"] = BuildWardrobeReportSummary(variants, exportBaseSnapshot, exportWardrobeSets, avatarRoot),
                ["wardrobeAssetOwnershipDiagnostics"] = BuildWardrobeAssetOwnershipDiagnostics(rendererAssets),
                ["wardrobePreviewDiagnostics"] = BuildWardrobePreviewDiagnostics(exportWardrobeSets),
                ["modularAvatar"] = BuildModularAvatarReportSummary(avatarRoot),
                ["dynamics"] = BuildDynamicsReportSummary(dynamicsPayload),
                ["contacts"] = BuildContactsReportSummary(contactsPayload),
                ["materialAlphaDiagnostics"] = BuildMaterialAlphaDiagnosticsReport(),
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
                    ["fallbacks"] = TextureRecordsToJson(fallbackTextures, 16)
                },
                ["unsupported"] = unsupported
            };
        }

        private static Dictionary<string, object> BuildWardrobeAssetOwnershipDiagnostics(List<UnavatarRendererAssetRecord> rendererAssets)
        {
            var items = new List<object>();
            var rendererCount = rendererAssets != null ? rendererAssets.Count : 0;
            if (rendererAssets != null)
            {
                for (var i = 0; i < rendererAssets.Count && i < 96; i++)
                {
                    items.Add(rendererAssets[i].ToJson());
                }
            }
            return new Dictionary<string, object>
            {
                ["rendererAssetCount"] = rendererCount,
                ["itemLimit"] = 96,
                ["items"] = items
            };
        }

        private sealed class TextureSourceByteSummary
        {
            public string Extension;
            public int Count;
            public long SourceByteLength;
        }

        private Dictionary<string, object> BuildMaterialAlphaDiagnosticsReport()
        {
            var items = new List<object>();
            var totalCandidateSlots = 0;
            if (avatarRoot == null)
            {
                return new Dictionary<string, object>
                {
                    ["candidateSlotCount"] = 0,
                    ["items"] = items
                };
            }

            foreach (var renderer in avatarRoot.GetComponentsInChildren<Renderer>(true))
            {
                if (renderer == null)
                {
                    continue;
                }
                var materials = renderer.sharedMaterials;
                for (var slot = 0; slot < materials.Length; slot++)
                {
                    var material = materials[slot];
                    if (!ShouldReportMaterialAlphaDiagnostics(renderer, material))
                    {
                        continue;
                    }
                    totalCandidateSlots++;
                    if (items.Count < 96)
                    {
                        items.Add(BuildMaterialAlphaDiagnosticItem(renderer, slot, material));
                    }
                }
            }

            return new Dictionary<string, object>
            {
                ["candidateSlotCount"] = totalCandidateSlots,
                ["itemLimit"] = 96,
                ["items"] = items
            };
        }

        private bool ShouldReportMaterialAlphaDiagnostics(Renderer renderer, Material material)
        {
            if (material == null)
            {
                return false;
            }
            var shaderName = material.shader != null ? material.shader.name : "";
            var lilToon = shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0;
            if (!lilToon)
            {
                return false;
            }
            if (shaderName.IndexOf("Transparent", StringComparison.OrdinalIgnoreCase) >= 0 ||
                shaderName.IndexOf("Cutout", StringComparison.OrdinalIgnoreCase) >= 0 ||
                shaderName.IndexOf("Refraction", StringComparison.OrdinalIgnoreCase) >= 0 ||
                shaderName.IndexOf("lilToonRef", StringComparison.OrdinalIgnoreCase) >= 0 ||
                shaderName.IndexOf("Gem", StringComparison.OrdinalIgnoreCase) >= 0 ||
                shaderName.IndexOf("Fur", StringComparison.OrdinalIgnoreCase) >= 0)
            {
                return true;
            }
            if (material.renderQueue >= 2450)
            {
                return true;
            }
            return ReadMaterialFloat(material, "_TransparentMode", 0.0f) >= 0.5f ||
                ReadMaterialFloat(material, "_AlphaMode", 0.0f) >= 0.5f ||
                ReadMaterialFloat(material, "_BlendMode", 0.0f) >= 0.5f ||
                ReadMaterialFloat(material, "_Mode", 0.0f) >= 0.5f;
        }

        private Dictionary<string, object> BuildMaterialAlphaDiagnosticItem(Renderer renderer, int slot, Material material)
        {
            var shaderName = material != null && material.shader != null ? material.shader.name : "";
            var baseColor = ReadMaterialColor(material, "_BaseColor", ReadMaterialColor(material, "_Color", Color.white));
            var mainTextureProperty = material != null && material.HasProperty("_BaseMap") ? "_BaseMap" : "_MainTex";
            var mainTexture = material != null && material.HasProperty(mainTextureProperty) ? material.GetTexture(mainTextureProperty) : null;
            return new Dictionary<string, object>
            {
                ["rendererPath"] = avatarRoot != null && renderer != null ? VariantExtractor.TransformPath(avatarRoot.transform, renderer.transform) : "",
                ["rendererEnabled"] = renderer != null && renderer.enabled,
                ["rendererActiveInHierarchy"] = renderer != null && renderer.gameObject.activeInHierarchy,
                ["slot"] = slot,
                ["material"] = material != null ? material.name : "",
                ["shader"] = shaderName,
                ["renderQueue"] = material != null ? material.renderQueue : -1,
                ["exporterAlphaMode"] = ExporterAlphaModeDiagnostic(material, baseColor),
                ["baseColor"] = FloatArray(baseColor.r, baseColor.g, baseColor.b, baseColor.a),
                ["floats"] = MaterialAlphaDiagnosticFloats(material),
                ["mainTextureProperty"] = mainTexture != null ? mainTextureProperty : "",
                ["mainTexture"] = TextureAlphaDiagnostic(mainTexture),
                ["textureScaleOffset"] = TextureScaleOffsetDiagnostic(material, mainTextureProperty)
            };
        }

        private static string ExporterAlphaModeDiagnostic(Material material, Color baseColor)
        {
            if (material == null)
            {
                return "NONE";
            }
            var shaderName = material.shader != null ? material.shader.name : "";
            var lilRefraction = shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0 &&
                (shaderName.IndexOf("Refraction", StringComparison.OrdinalIgnoreCase) >= 0 ||
                shaderName.IndexOf("lilToonRef", StringComparison.OrdinalIgnoreCase) >= 0);
            var lilBlend = shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0 &&
                (shaderName.IndexOf("Transparent", StringComparison.OrdinalIgnoreCase) >= 0 ||
                shaderName.IndexOf("Fur", StringComparison.OrdinalIgnoreCase) >= 0);
            var lilGem = shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0 &&
                shaderName.IndexOf("Gem", StringComparison.OrdinalIgnoreCase) >= 0;
            var lilCutout = shaderName.IndexOf("lilToon", StringComparison.OrdinalIgnoreCase) >= 0 &&
                shaderName.IndexOf("Cutout", StringComparison.OrdinalIgnoreCase) >= 0;
            if (lilRefraction)
            {
                return "OPAQUE";
            }
            if (!lilCutout && (lilBlend || lilGem || baseColor.a < 0.999f || material.renderQueue >= 3000 ||
                ReadMaterialFloat(material, "_TransparentMode", 0.0f) >= 1.5f ||
                ReadMaterialFloat(material, "_AlphaMode", 0.0f) >= 1.5f ||
                ReadMaterialFloat(material, "_BlendMode", 0.0f) >= 1.5f ||
                ReadMaterialFloat(material, "_Mode", 0.0f) >= 1.5f))
            {
                return "BLEND";
            }
            if (!lilGem && (lilCutout || (material.renderQueue >= 2450 && material.renderQueue < 3000) ||
                ReadMaterialFloat(material, "_TransparentMode", 0.0f) >= 0.5f ||
                ReadMaterialFloat(material, "_AlphaMode", 0.0f) >= 0.5f ||
                ReadMaterialFloat(material, "_BlendMode", 0.0f) >= 0.5f ||
                ReadMaterialFloat(material, "_Mode", 0.0f) >= 0.5f))
            {
                return "MASK";
            }
            return "OPAQUE";
        }

        private static Dictionary<string, object> MaterialAlphaDiagnosticFloats(Material material)
        {
            var properties = new[]
            {
                "_TransparentMode", "_AlphaMode", "_BlendMode", "_Mode",
                "_Cutoff", "_PreCutoff", "_SubpassCutoff",
                "_SrcBlend", "_DstBlend", "_SrcBlendAlpha", "_DstBlendAlpha",
                "_ZWrite", "_PreZWrite", "_Cull", "_PreCull",
                "_AlphaMaskMode", "_AlphaMaskScale", "_AlphaMaskValue",
                "_Main2ndTexAlphaMode", "_Main3rdTexAlphaMode",
                "_RefractionStrength", "_RefractionFresnelPower", "_RefractionColorFromMain"
            };
            var json = new Dictionary<string, object>();
            if (material == null)
            {
                return json;
            }
            foreach (var property in properties)
            {
                if (material.HasProperty(property))
                {
                    json[property] = material.GetFloat(property);
                }
            }
            return json;
        }

        private static Dictionary<string, object> TextureAlphaDiagnostic(Texture texture)
        {
            var json = new Dictionary<string, object>();
            if (texture == null)
            {
                return json;
            }
            var path = AssetDatabase.GetAssetPath(texture);
            var importer = !string.IsNullOrEmpty(path) ? AssetImporter.GetAtPath(path) as TextureImporter : null;
            json["name"] = texture.name;
            json["type"] = texture.GetType().Name;
            json["width"] = texture.width;
            json["height"] = texture.height;
            json["wrapMode"] = texture.wrapMode.ToString();
            json["filterMode"] = texture.filterMode.ToString();
            json["assetPath"] = path ?? "";
            if (importer != null)
            {
                json["textureType"] = importer.textureType.ToString();
                json["textureShape"] = importer.textureShape.ToString();
                json["sRGBTexture"] = importer.sRGBTexture;
                json["alphaSource"] = importer.alphaSource.ToString();
                json["alphaIsTransparency"] = importer.alphaIsTransparency;
                json["doesSourceTextureHaveAlpha"] = importer.DoesSourceTextureHaveAlpha();
            }
            return json;
        }

        private static Dictionary<string, object> TextureScaleOffsetDiagnostic(Material material, string property)
        {
            if (material == null || string.IsNullOrEmpty(property) || !material.HasProperty(property))
            {
                return new Dictionary<string, object>();
            }
            var scale = material.GetTextureScale(property);
            var offset = material.GetTextureOffset(property);
            return new Dictionary<string, object>
            {
                ["scale"] = FloatArray(scale.x, scale.y),
                ["offsetUnity"] = FloatArray(offset.x, offset.y),
                ["offsetGltf"] = FloatArray(offset.x, 1.0f - scale.y - offset.y)
            };
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

        private static List<object> TextureRecordsToJson(List<ExportedTextureRecord> textures, int limit = int.MaxValue)
        {
            var json = new List<object>(textures != null ? textures.Count : 0);
            if (textures == null)
            {
                return json;
            }
            var count = 0;
            foreach (var texture in textures)
            {
                if (count >= limit)
                {
                    break;
                }
                json.Add(texture.ToJson());
                count++;
            }
            return json;
        }

        private static List<object> VariantSummariesToJson(List<VariantRecord> variants)
        {
            var json = new List<object>(variants != null ? variants.Count : 0);
            if (variants == null)
            {
                return json;
            }
            foreach (var variant in variants)
            {
                json.Add(new Dictionary<string, object>
                {
                    ["id"] = variant.Id ?? "",
                    ["name"] = variant.Name ?? "",
                    ["source"] = variant.Source ?? "",
                    ["operationCount"] = variant.Operations != null ? variant.Operations.Count : 0
                });
            }
            return json;
        }

        private Dictionary<string, object> BuildWardrobeReportSummary(
            List<VariantRecord> variants,
            WardrobeSnapshotDraft exportBaseSnapshot,
            List<WardrobeSetDraft> exportWardrobeSets,
            GameObject referenceRoot)
        {
            var full = BuildWardrobePayload(variants, exportBaseSnapshot, exportWardrobeSets, referenceRoot);
            var sets = new List<object>();
            if (full.TryGetValue("sets", out var rawSets) && rawSets is List<object> fullSets)
            {
                foreach (var item in fullSets)
                {
                    if (!(item is Dictionary<string, object> set))
                    {
                        continue;
                    }
                    var operations = set.TryGetValue("operations", out var rawOps) && rawOps is List<object> ops ? ops : new List<object>();
                    sets.Add(new Dictionary<string, object>
                    {
                        ["id"] = set.TryGetValue("id", out var id) ? id : "",
                        ["displayName"] = set.TryGetValue("displayName", out var displayName) ? displayName : "",
                        ["source"] = set.TryGetValue("source", out var source) ? source : "",
                        ["default"] = set.TryGetValue("default", out var isDefault) ? isDefault : false,
                        ["assetGroups"] = set.TryGetValue("assetGroups", out var assetGroups) ? assetGroups : new List<object>(),
                        ["operationCount"] = operations.Count,
                        ["operationCounts"] = CountOperationTypes(operations)
                    });
                }
            }
            return new Dictionary<string, object>
            {
                ["baseSet"] = full.TryGetValue("baseSet", out var baseSet) ? baseSet : "base",
                ["setCount"] = sets.Count,
                ["sets"] = sets
            };
        }

        private static Dictionary<string, object> CountOperationTypes(List<object> operations)
        {
            var counts = new Dictionary<string, int>(StringComparer.Ordinal);
            foreach (var item in operations)
            {
                if (!(item is Dictionary<string, object> operation))
                {
                    continue;
                }
                var type = operation.TryGetValue("type", out var rawType) ? rawType as string : null;
                if (string.IsNullOrEmpty(type))
                {
                    type = "unknown";
                }
                counts[type] = counts.TryGetValue(type, out var count) ? count + 1 : 1;
            }
            var json = new Dictionary<string, object>(StringComparer.Ordinal);
            foreach (var pair in counts)
            {
                json[pair.Key] = pair.Value;
            }
            return json;
        }

        private Dictionary<string, object> BuildModularAvatarReportSummary(GameObject root)
        {
            var typeCounts = new Dictionary<string, int>(StringComparer.Ordinal);
            var supportCounts = new Dictionary<string, int>(StringComparer.Ordinal);
            var disabledTypeCounts = new Dictionary<string, int>(StringComparer.Ordinal);
            var samples = new List<object>(32);
            var componentCount = 0;
            var disabledCount = 0;
            if (root != null)
            {
                var components = root.GetComponentsInChildren<Component>(true);
                foreach (var component in components)
                {
                    if (!IsModularAvatarComponent(component))
                    {
                        continue;
                    }
                    componentCount++;
                    var typeName = component.GetType().Name;
                    var enabled = !(component is Behaviour behaviour) || behaviour.enabled;
                    typeCounts[typeName] = typeCounts.TryGetValue(typeName, out var count) ? count + 1 : 1;
                    var supportKind = ModularAvatarComponentSupportKind(typeName);
                    supportCounts[supportKind] = supportCounts.TryGetValue(supportKind, out var supportCount) ? supportCount + 1 : 1;
                    if (!enabled)
                    {
                        disabledCount += 1;
                        supportCounts["disabled"] = supportCounts.TryGetValue("disabled", out var disabledSupport) ? disabledSupport + 1 : 1;
                        disabledTypeCounts[typeName] = disabledTypeCounts.TryGetValue(typeName, out var disabledTypeCount) ? disabledTypeCount + 1 : 1;
                    }
                    if (samples.Count < 32 && (typeName == "ModularAvatarBoneProxy" || typeName == "ModularAvatarMergeArmature" || typeName == "ModularAvatarMeshSettings"))
                    {
                        var sample = new Dictionary<string, object>
                        {
                            ["shortType"] = typeName,
                            ["target"] = TransformTargetJson(root.transform, component.transform),
                            ["enabled"] = enabled
                        };
                        if (typeName == "ModularAvatarBoneProxy")
                        {
                            sample["resolvedTarget"] = BuildBoneProxyResolvedTarget(root.transform, component);
                        }
                        samples.Add(sample);
                    }
                }
            }
            var countsJson = new Dictionary<string, object>(StringComparer.Ordinal);
            foreach (var pair in typeCounts)
            {
                countsJson[pair.Key] = pair.Value;
            }
            var supportCountsJson = new Dictionary<string, object>(StringComparer.Ordinal);
            foreach (var pair in supportCounts)
            {
                supportCountsJson[pair.Key] = pair.Value;
            }
            var disabledTypeCountsJson = new Dictionary<string, object>(StringComparer.Ordinal);
            foreach (var pair in disabledTypeCounts)
            {
                disabledTypeCountsJson[pair.Key] = pair.Value;
            }
            return new Dictionary<string, object>
            {
                ["schemaVersion"] = "0.1-preview",
                ["available"] = ModularAvatarBridge.IsAvailable,
                ["componentCount"] = componentCount,
                ["componentCounts"] = countsJson,
                ["supportCounts"] = supportCountsJson,
                ["disabledTypeCounts"] = disabledTypeCountsJson,
                ["disabledComponentCount"] = disabledCount,
                ["samples"] = samples
            };
        }

        private static string ModularAvatarComponentSupportKind(string shortType)
        {
            return shortType switch
            {
                "ModularAvatarBlendshapeSync" or
                "ModularAvatarMergeArmature" or
                "ModularAvatarMeshCutter" or
                "ModularAvatarMeshSettings" or
                "ModularAvatarScaleAdjuster" or
                "ModularAvatarShapeChanger" => "approximate",
                "ModularAvatarBoneProxy" or
                "ModularAvatarRemoveVertexColor" or
                "ModularAvatarReplaceObject" => "resolver",
                "ModularAvatarMaterialSetter" or
                "ModularAvatarMaterialSwap" or
                "ModularAvatarObjectToggle" => "runtime_action",
                "ModularAvatarConvertConstraints" or
                "ModularAvatarFloorAdjuster" or
                "ModularAvatarMMDLayerControl" or
                "ModularAvatarMergeAnimator" or
                "ModularAvatarMergeBlendTree" or
                "ModularAvatarPlatformFilter" or
                "ModularAvatarRenameVRChatCollisionTags" or
                "ModularAvatarVRChatSettings" or
                "ModularAvatarWorldFixedObject" or
                "ModularAvatarWorldScaleObject" or
                "MAMoveIndependently" => "unsupported",
                "ModularAvatarMenuItem" or
                "ModularAvatarMenuGroup" or
                "ModularAvatarMenuInstaller" or
                "ModularAvatarMenuInstallTarget" or
                "VRCExpressionsMenuControl" or
                "ModularAvatarParameters" or
                "ModularAvatarGlobalCollider" or
                "ModularAvatarPBBlocker" or
                "ModularAvatarSyncParameterSequence" or
                "ModularAvatarVisibleHeadAccessory" or
                "VertexFilterByAxisComponent" or
                "VertexFilterByBoneComponent" or
                "VertexFilterByMaskComponent" or
                "VertexFilterByShapeComponent" => "metadata",
                _ => "unsupported",
            };
        }

        private Dictionary<string, object> BuildWardrobePreviewDiagnostics(List<WardrobeSetDraft> exportWardrobeSets)
        {
            var nonBaseSets = exportWardrobeSets ?? WardrobeSetsForExport();
            var sets = new List<WardrobeSetDraft>(1 + (nonBaseSets != null ? nonBaseSets.Count : 0));
            sets.Add(new WardrobeSetDraft
            {
                id = "base",
                displayName = "Base",
                previewImages = basePreviewImages ?? new List<WardrobePreviewImageDraft>()
            });
            if (nonBaseSets != null)
            {
                sets.AddRange(nonBaseSets);
            }

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
                    ["previewCount"] = set.previewImages != null ? set.previewImages.Count : 0
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
                    ["byteLength"] = image.pngBytes != null ? image.pngBytes.Length : 0,
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

        private static string Sha256Hex(byte[] bytes)
        {
            if (bytes == null || bytes.Length == 0)
            {
                return "";
            }
            using (var sha = SHA256.Create())
            {
                var hash = sha.ComputeHash(bytes);
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

        private static Color ReadMaterialColor(Material material, string property, Color fallback)
        {
            return material != null && material.HasProperty(property) ? material.GetColor(property) : fallback;
        }

        private static List<object> FloatArray(params float[] values)
        {
            var json = new List<object>(values != null ? values.Length : 0);
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

        private Dictionary<string, object> BuildWardrobePayload(
            List<VariantRecord> variants,
            WardrobeSnapshotDraft exportBaseSnapshot = null,
            List<WardrobeSetDraft> exportWardrobeSets = null,
            GameObject referenceRoot = null,
            List<UnavatarRendererAssetRecord> rendererAssets = null,
            List<object> dynamicsPayload = null)
        {
            var rootForReference = referenceRoot != null ? referenceRoot : avatarRoot;
            var hasExportBaseSnapshot = exportBaseSnapshot != null && exportBaseSnapshot.nodes.Count > 0;
            var baseOperations = hasExportBaseSnapshot
                ? WardrobeSnapshotCapture.BaseOperations(exportBaseSnapshot, rootForReference)
                : hasBaseSnapshot
                ? WardrobeSnapshotCapture.BaseOperations(baseSnapshot, rootForReference)
                : CloneWardrobeOperations(importedBaseOperations);
            var sets = new List<object>
            {
                new WardrobeSetDraft
                {
                    id = "base",
                    displayName = "Base",
                    source = hasExportBaseSnapshot ? "unity_current_capture_base" : hasBaseSnapshot ? "unity_capture_base" : hasImportedBaseOperations ? "imported_unavatar_base" : "implicit_current_state",
                    assetGroups = new List<string> { "" },
                    operations = baseOperations,
                    previewImages = basePreviewImages ?? new List<WardrobePreviewImageDraft>()
                }.ToJson(true)
            };

            var nonBaseSets = exportWardrobeSets ?? WardrobeSetsForExport();
            var declaredAssetGroups = new HashSet<string>(StringComparer.Ordinal);
            var ownershipAmbiguities = new List<object>();
            foreach (var set in nonBaseSets)
            {
                AddDeclaredAssetGroups(declaredAssetGroups, set);
                sets.Add(set.ToJson(false));
            }
            var ownershipPathHints = BuildWardrobeAssetGroupOwnershipPathHints(nonBaseSets, declaredAssetGroups);

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

            var wardrobe = new Dictionary<string, object>
            {
                ["baseSet"] = "base",
                ["captureBase"] = hasExportBaseSnapshot ? SnapshotSummary(exportBaseSnapshot) : hasBaseSnapshot ? SnapshotSummary(baseSnapshot) : new Dictionary<string, object>(),
                ["sets"] = sets
            };
            var ambiguitySignatures = new HashSet<string>(StringComparer.Ordinal);
            var assetGroupOwnership = BuildWardrobeAssetGroupOwnership(
                rendererAssets,
                dynamicsPayload,
                declaredAssetGroups,
                ownershipPathHints,
                ownershipAmbiguities,
                ambiguitySignatures);
            if (assetGroupOwnership.Count > 0)
            {
                wardrobe["assetGroupOwnership"] = assetGroupOwnership;
            }
            if (ownershipAmbiguities.Count > 0)
            {
                wardrobe["assetGroupOwnershipAmbiguities"] = new Dictionary<string, object>
                {
                    ["itemLimit"] = 64,
                    ["itemCount"] = ownershipAmbiguities.Count,
                    ["items"] = ownershipAmbiguities
                };
            }
            return wardrobe;
        }

        private static void AddDeclaredAssetGroups(HashSet<string> groups, WardrobeSetDraft set)
        {
            if (groups == null || set == null || set.assetGroups == null)
            {
                return;
            }
            foreach (var group in set.assetGroups)
            {
                if (!string.IsNullOrWhiteSpace(group))
                {
                    groups.Add(group.Trim());
                }
            }
        }

        private sealed class WardrobeAssetGroupOwnershipBuilder
        {
            public readonly List<object> MeshPrimitives = new List<object>();
            public readonly List<int> Materials = new List<int>();
            public readonly List<int> Images = new List<int>();
            public readonly List<string> DynamicsSourceIds = new List<string>();
            private readonly HashSet<string> meshPrimitiveKeys = new HashSet<string>(StringComparer.Ordinal);

            public void Add(UnavatarRendererAssetRecord record)
            {
                if (record == null)
                {
                    return;
                }
                if (record.mesh >= 0 && record.primitives != null)
                {
                    foreach (var primitiveIndex in record.primitives)
                    {
                        if (primitiveIndex < 0)
                        {
                            continue;
                        }
                        var key = record.mesh.ToString(CultureInfo.InvariantCulture) + "/" + primitiveIndex.ToString(CultureInfo.InvariantCulture);
                        if (meshPrimitiveKeys.Add(key))
                        {
                            MeshPrimitives.Add(new Dictionary<string, object>
                            {
                                ["meshIndex"] = record.mesh,
                                ["primitiveIndex"] = primitiveIndex
                            });
                        }
                    }
                }
                AddUniqueSorted(Materials, record.materials);
                AddUniqueSorted(Images, record.images);
            }

            public void AddDynamicSourceId(string sourceId)
            {
                if (!string.IsNullOrWhiteSpace(sourceId) && !DynamicsSourceIds.Contains(sourceId))
                {
                    DynamicsSourceIds.Add(sourceId);
                    DynamicsSourceIds.Sort(StringComparer.Ordinal);
                }
            }
        }

        private static List<object> BuildWardrobeAssetGroupOwnership(
            List<UnavatarRendererAssetRecord> rendererAssets,
            List<object> dynamicsPayload,
            HashSet<string> declaredAssetGroups,
            Dictionary<string, string> explicitPathHints,
            List<object> ambiguousMatches,
            HashSet<string> ambiguousMatchSignatures)
        {
            var result = new List<object>();
            if ((rendererAssets == null || rendererAssets.Count == 0) && (dynamicsPayload == null || dynamicsPayload.Count == 0))
            {
                return result;
            }
            if (declaredAssetGroups == null || declaredAssetGroups.Count == 0)
            {
                return result;
            }
            var byGroup = new Dictionary<string, WardrobeAssetGroupOwnershipBuilder>(StringComparer.Ordinal);
            if (rendererAssets != null)
            {
                foreach (var record in rendererAssets)
                {
                    var path = NormalizeWardrobeAssetPath(record != null ? record.path : "");
                    var group = WardrobeAssetGroupForPath(path, declaredAssetGroups, explicitPathHints, out var ambiguous);
                    if (ambiguous)
                    {
                        RecordWardrobeAssetGroupAmbiguity(ambiguousMatches, ambiguousMatchSignatures, path, declaredAssetGroups, path);
                        continue;
                    }
                    if (string.IsNullOrWhiteSpace(group) || !declaredAssetGroups.Contains(group))
                    {
                        continue;
                    }
                    GetWardrobeAssetGroupOwnershipBuilder(byGroup, group).Add(record);
                }
            }
            if (dynamicsPayload != null)
            {
                foreach (var dynamicGroup in dynamicsPayload)
                {
                    if (!(dynamicGroup is Dictionary<string, object> groupPayload))
                    {
                        continue;
                    }
                    var sourceId = DynamicSourceId(groupPayload);
                    if (string.IsNullOrWhiteSpace(sourceId))
                    {
                        continue;
                    }
                    var dynamicPath = DynamicSourcePath(groupPayload, sourceId);
                    var path = NormalizeWardrobeAssetPath(dynamicPath);
                    var group = WardrobeAssetGroupForPath(path, declaredAssetGroups, explicitPathHints, out var ambiguous);
                    if (ambiguous)
                    {
                        RecordWardrobeAssetGroupAmbiguity(ambiguousMatches, ambiguousMatchSignatures, path, declaredAssetGroups, dynamicPath);
                        continue;
                    }
                    if (string.IsNullOrWhiteSpace(group) || !declaredAssetGroups.Contains(group))
                    {
                        continue;
                    }
                    GetWardrobeAssetGroupOwnershipBuilder(byGroup, group).AddDynamicSourceId(sourceId);
                }
            }
            var groups = new List<string>(byGroup.Keys);
            groups.Sort(StringComparer.Ordinal);
            foreach (var group in groups)
            {
                var builder = byGroup[group];
                result.Add(new Dictionary<string, object>
                {
                    ["groupId"] = group,
                    ["meshPrimitives"] = builder.MeshPrimitives,
                    ["materials"] = IntsToObjectList(builder.Materials),
                    ["images"] = IntsToObjectList(builder.Images),
                    ["dynamicsSourceIds"] = StringsToObjectList(builder.DynamicsSourceIds)
                });
            }
            return result;
        }

        private static Dictionary<string, string> BuildWardrobeAssetGroupOwnershipPathHints(
            IEnumerable<WardrobeSetDraft> sets,
            HashSet<string> declaredAssetGroups)
        {
            var result = new Dictionary<string, string>(StringComparer.Ordinal);
            if (declaredAssetGroups == null || declaredAssetGroups.Count == 0 || sets == null)
            {
                return result;
            }
            foreach (var set in sets)
            {
                if (set == null || set.assetGroupOwnershipHints == null)
                {
                    continue;
                }
                foreach (var hint in set.assetGroupOwnershipHints)
                {
                    if (hint == null || string.IsNullOrWhiteSpace(hint.path) || string.IsNullOrWhiteSpace(hint.groupId))
                    {
                        continue;
                    }
                    var path = NormalizeWardrobeAssetPath(hint.path);
                    if (string.IsNullOrWhiteSpace(path) || !declaredAssetGroups.Contains(hint.groupId.Trim()))
                    {
                        continue;
                    }
                    result[path] = hint.groupId.Trim();
                }
            }
            return result;
        }

        private static string NormalizeWardrobeAssetPath(string path)
        {
            if (string.IsNullOrWhiteSpace(path))
            {
                return "";
            }
            return path.Replace('\\', '/').Trim('/');
        }

        private static void RecordWardrobeAssetGroupAmbiguity(
            List<object> diagnostics,
            HashSet<string> seenSignatures,
            string path,
            HashSet<string> declaredAssetGroups,
            string sourcePath)
        {
            if (diagnostics == null || declaredAssetGroups == null || declaredAssetGroups.Count == 0 || string.IsNullOrWhiteSpace(path))
            {
                return;
            }
            var candidates = new List<string>();
            var topSlug = WardrobeAssetGroupTopSlug(path);
            foreach (var group in declaredAssetGroups)
            {
                if (string.IsNullOrWhiteSpace(group))
                {
                    continue;
                }
                if (string.Equals(WardrobeAssetGroupSuffixSlug(group), topSlug, StringComparison.Ordinal))
                {
                    candidates.Add(group);
                }
            }
            if (candidates.Count <= 1)
            {
                return;
            }
            candidates.Sort(StringComparer.Ordinal);
            var signature = path + "||" + string.Join("|", candidates);
            if (seenSignatures != null && !seenSignatures.Add(signature))
            {
                return;
            }
            diagnostics.Add(new Dictionary<string, object>
            {
                ["sourcePath"] = sourcePath ?? "",
                ["normalizedPath"] = path,
                ["topSlug"] = topSlug,
                ["candidateGroups"] = candidates
            });
        }

        private static string WardrobeAssetGroupForPath(string path, HashSet<string> declaredAssetGroups)
        {
            return WardrobeAssetGroupForPath(path, declaredAssetGroups, null, out _);
        }

        private static string WardrobeAssetGroupForPath(
            string path,
            HashSet<string> declaredAssetGroups,
            Dictionary<string, string> explicitPathHints,
            out bool isAmbiguous)
        {
            isAmbiguous = false;
            if (declaredAssetGroups == null || declaredAssetGroups.Count == 0)
            {
                return "";
            }
            if (!string.IsNullOrWhiteSpace(path) &&
                explicitPathHints != null &&
                explicitPathHints.TryGetValue(path, out var explicitGroup))
            {
                if (declaredAssetGroups.Contains(explicitGroup))
                {
                    return explicitGroup;
                }
                return "";
            }
            var outfitGroup = WardrobeSnapshotCapture.AssetGroupForPath(path);
            if (!string.IsNullOrWhiteSpace(outfitGroup) && declaredAssetGroups.Contains(outfitGroup))
            {
                return outfitGroup;
            }
            var topSlug = WardrobeAssetGroupTopSlug(path);
            if (string.IsNullOrWhiteSpace(topSlug))
            {
                return "";
            }
            string match = null;
            foreach (var group in declaredAssetGroups)
            {
                if (string.IsNullOrWhiteSpace(group))
                {
                    continue;
                }
                var suffix = WardrobeAssetGroupSuffixSlug(group);
                if (!string.Equals(suffix, topSlug, StringComparison.Ordinal))
                {
                    continue;
                }
                if (match != null)
                {
                    isAmbiguous = true;
                    return "";
                }
                match = group;
            }
            return match ?? "";
        }

        private static string WardrobeAssetGroupTopSlug(string path)
        {
            if (string.IsNullOrWhiteSpace(path))
            {
                return "";
            }
            var separator = path.IndexOf('/');
            var top = (separator >= 0 ? path.Substring(0, separator) : path).Trim();
            return string.IsNullOrWhiteSpace(top) ? "" : WardrobeSnapshotCapture.MakeId(top);
        }

        private static string WardrobeAssetGroupSuffixSlug(string group)
        {
            if (string.IsNullOrWhiteSpace(group))
            {
                return "";
            }
            var separator = group.IndexOf(':');
            var suffix = (separator >= 0 ? group.Substring(separator + 1) : group).Trim();
            return string.IsNullOrWhiteSpace(suffix) ? "" : WardrobeSnapshotCapture.MakeId(suffix);
        }

        private static WardrobeAssetGroupOwnershipBuilder GetWardrobeAssetGroupOwnershipBuilder(
            Dictionary<string, WardrobeAssetGroupOwnershipBuilder> byGroup,
            string group)
        {
            if (!byGroup.TryGetValue(group, out var builder))
            {
                builder = new WardrobeAssetGroupOwnershipBuilder();
                byGroup[group] = builder;
            }
            return builder;
        }

        private static string DynamicSourceId(Dictionary<string, object> groupPayload)
        {
            return groupPayload.TryGetValue("id", out var rawId) && rawId is string id ? id : "";
        }

        private static string DynamicSourcePath(Dictionary<string, object> groupPayload, string sourceId)
        {
            if (groupPayload.TryGetValue("component", out var rawComponent) &&
                rawComponent is Dictionary<string, object> component &&
                component.TryGetValue("path", out var rawPath) &&
                rawPath is string path)
            {
                return path;
            }
            return !string.IsNullOrWhiteSpace(sourceId) && sourceId.StartsWith("physbone:", StringComparison.Ordinal)
                ? sourceId.Substring("physbone:".Length)
                : "";
        }

        private static void AddUniqueSorted(List<int> target, List<int> values)
        {
            if (target == null || values == null)
            {
                return;
            }
            foreach (var value in values)
            {
                if (!target.Contains(value))
                {
                    target.Add(value);
                }
            }
            target.Sort();
        }

        private static List<object> IntsToObjectList(List<int> values)
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

        private static List<object> StringsToObjectList(List<string> values)
        {
            var json = new List<object>(values != null ? values.Count : 0);
            if (values == null)
            {
                return json;
            }
            foreach (var value in values)
            {
                json.Add(value ?? "");
            }
            return json;
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
