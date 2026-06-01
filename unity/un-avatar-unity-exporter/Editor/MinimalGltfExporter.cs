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
    internal static class MinimalGltfExporter
    {
        private const uint JsonChunkType = 0x4E4F534A;
        private const uint BinChunkType = 0x004E4942;
        private const uint GlbMagic = 0x46546C67;

        public sealed class ExportResult
        {
            public string Path;
            public List<ExportedTextureRecord> Textures = new List<ExportedTextureRecord>();
            public List<UnavatarTextureAssetRecord> TextureAssets = new List<UnavatarTextureAssetRecord>();
        }

        public static ExportResult ExportGlb(GameObject root, string directory, string fileName, HashSet<string> morphTargetNames)
        {
            var writer = new Writer(root, morphTargetNames);
            var path = Path.Combine(directory, fileName + ".glb");
            writer.Export(path);
            return new ExportResult
            {
                Path = path,
                Textures = writer.ExportedTextures,
                TextureAssets = writer.TextureAssets
            };
        }

        private sealed class Writer
        {
            private readonly GameObject root;
            private readonly BinaryBuffer buffer = new BinaryBuffer();
            private readonly HashSet<string> morphTargetNames;
            private readonly Dictionary<Transform, int> nodeIndices = new Dictionary<Transform, int>();
            private readonly Dictionary<Material, int> materialIndices = new Dictionary<Material, int>();
            private readonly Dictionary<Texture, int> textureIndices = new Dictionary<Texture, int>();
            private readonly Dictionary<Texture, UnavatarTextureAssetRecord> textureAssetIndices = new Dictionary<Texture, UnavatarTextureAssetRecord>();
            private readonly Dictionary<string, int> samplerIndices = new Dictionary<string, int>(StringComparer.Ordinal);
            private int defaultMaterialIndex = -1;
            private readonly List<object> nodes = new List<object>();
            private readonly List<object> meshes = new List<object>();
            private readonly List<object> skins = new List<object>();
            private readonly List<object> accessors = new List<object>();
            private readonly List<object> bufferViews = new List<object>();
            private readonly List<object> materials = new List<object>();
            private readonly List<object> images = new List<object>();
            private readonly List<object> textures = new List<object>();
            private readonly List<object> samplers = new List<object>();
            private readonly List<ExportedTextureRecord> exportedTextures = new List<ExportedTextureRecord>();
            private readonly List<UnavatarTextureAssetRecord> textureAssets = new List<UnavatarTextureAssetRecord>();
            private bool usesTextureTransform;

            public List<ExportedTextureRecord> ExportedTextures => exportedTextures;
            public List<UnavatarTextureAssetRecord> TextureAssets => textureAssets;

            public Writer(GameObject root, HashSet<string> morphTargetNames)
            {
                this.root = root;
                this.morphTargetNames = morphTargetNames ?? new HashSet<string>(StringComparer.Ordinal);
            }

            public void Export(string path)
            {
                BuildNodeTree(root.transform);
                AttachRenderers(root.transform);

                var gltf = new Dictionary<string, object>
                {
                    ["asset"] = new Dictionary<string, object>
                    {
                        ["version"] = "2.0",
                        ["generator"] = "U.N. Avatar Unity Exporter built-in GLB writer 0.1.0-preview"
                    },
                    ["scene"] = 0,
                    ["scenes"] = new List<object>
                    {
                        new Dictionary<string, object>
                        {
                            ["name"] = root.name,
                            ["nodes"] = new List<object> { 0 }
                        }
                    },
                    ["nodes"] = nodes,
                    ["meshes"] = meshes,
                    ["accessors"] = accessors,
                    ["bufferViews"] = bufferViews,
                    ["materials"] = materials
                };

                if (skins.Count > 0)
                {
                    gltf["skins"] = skins;
                }
                if (images.Count > 0)
                {
                    gltf["images"] = images;
                    gltf["textures"] = textures;
                    gltf["samplers"] = samplers;
                }
                if (usesTextureTransform)
                {
                    gltf["extensionsUsed"] = new List<object> { "KHR_texture_transform" };
                }
                if (buffer.Length > 0)
                {
                    gltf["buffers"] = new List<object>
                    {
                        new Dictionary<string, object>
                        {
                            ["byteLength"] = buffer.Length
                        }
                    };
                }

                WriteGlb(path, MiniJson.Serialize(gltf), buffer.ToArray());
            }

            private void BuildNodeTree(Transform transform)
            {
                var index = nodes.Count;
                nodeIndices[transform] = index;
                var isExportRoot = transform == root.transform;
                var translation = isExportRoot ? Vector3.zero : UnityVectorToGltf(transform.localPosition);
                var rotation = UnityRotationToGltf(transform.localRotation);
                var node = new Dictionary<string, object>
                {
                    ["name"] = transform.name,
                    ["translation"] = FloatArray(translation.x, translation.y, translation.z),
                    ["rotation"] = FloatArray(rotation.x, rotation.y, rotation.z, rotation.w),
                    ["scale"] = FloatArray(transform.localScale.x, transform.localScale.y, transform.localScale.z),
                    ["extras"] = new Dictionary<string, object>
                    {
                        ["UN_avatar_node"] = new Dictionary<string, object>
                        {
                            ["nodeId"] = WardrobeSnapshotCapture.NodeIdFor(root.transform, transform),
                            ["path"] = VariantExtractor.TransformPath(root.transform, transform)
                        }
                    }
                };
                nodes.Add(node);

                var children = new List<object>();
                for (var i = 0; i < transform.childCount; i++)
                {
                    var child = transform.GetChild(i);
                    BuildNodeTree(child);
                    children.Add(nodeIndices[child]);
                }
                if (children.Count > 0)
                {
                    node["children"] = children;
                }
            }

            private void AttachRenderers(Transform transform)
            {
                foreach (var meshRenderer in transform.GetComponents<MeshRenderer>())
                {
                    var filter = transform.GetComponent<MeshFilter>();
                    if (filter != null && filter.sharedMesh != null)
                    {
                        var node = (Dictionary<string, object>)nodes[nodeIndices[transform]];
                        var mesh = ExportMesh(filter.sharedMesh, meshRenderer.sharedMaterials, null);
                        if (mesh >= 0) node["mesh"] = mesh;
                    }
                }

                foreach (var skinned in transform.GetComponents<SkinnedMeshRenderer>())
                {
                    if (skinned.sharedMesh == null)
                    {
                        continue;
                    }
                    var node = (Dictionary<string, object>)nodes[nodeIndices[transform]];
                    var mesh = ExportMesh(skinned.sharedMesh, skinned.sharedMaterials, skinned);
                    if (mesh >= 0) node["mesh"] = mesh;
                    var skin = ExportSkin(skinned);
                    if (skin >= 0)
                    {
                        node["skin"] = skin;
                    }
                }

                for (var i = 0; i < transform.childCount; i++)
                {
                    AttachRenderers(transform.GetChild(i));
                }
            }

            private int ExportMesh(Mesh mesh, Material[] sourceMaterials, SkinnedMeshRenderer skinned)
            {
                var vertices = mesh.vertices;
                if (vertices == null || vertices.Length == 0)
                {
                    return -1;
                }

                var normals = mesh.normals;
                var tangents = mesh.tangents;
                var uv = mesh.uv;
                var colors = mesh.colors;
                var boneWeights = skinned != null ? mesh.boneWeights : null;

                var positionAccessor = AddVec3Accessor(vertices, true, true);
                var normalAccessor = normals != null && normals.Length == vertices.Length ? AddVec3Accessor(normals, false, true) : -1;
                var tangentAccessor = tangents != null && tangents.Length == vertices.Length ? AddVec4Accessor(tangents, true) : -1;
                var uvAccessor = uv != null && uv.Length == vertices.Length ? AddVec2Accessor(uv) : -1;
                var colorAccessor = colors != null && colors.Length == vertices.Length ? AddColorAccessor(colors) : -1;
                var jointsAccessor = boneWeights != null && boneWeights.Length == vertices.Length ? AddJointsAccessor(boneWeights) : -1;
                var weightsAccessor = boneWeights != null && boneWeights.Length == vertices.Length ? AddWeightsAccessor(boneWeights) : -1;
                var morphTargets = BuildMorphTargets(mesh, vertices.Length);
                var morphWeights = skinned != null && morphTargets.Count > 0 ? BuildMorphWeights(mesh, skinned, morphTargets) : new List<object>();

                var primitives = new List<object>();
                for (var submesh = 0; submesh < mesh.subMeshCount; submesh++)
                {
                    var indices = mesh.GetIndices(submesh);
                    if (indices == null || indices.Length == 0)
                    {
                        continue;
                    }

                    var attributes = new Dictionary<string, object> { ["POSITION"] = positionAccessor };
                    if (normalAccessor >= 0) attributes["NORMAL"] = normalAccessor;
                    if (tangentAccessor >= 0) attributes["TANGENT"] = tangentAccessor;
                    if (uvAccessor >= 0) attributes["TEXCOORD_0"] = uvAccessor;
                    if (colorAccessor >= 0) attributes["COLOR_0"] = colorAccessor;
                    if (jointsAccessor >= 0 && weightsAccessor >= 0)
                    {
                        attributes["JOINTS_0"] = jointsAccessor;
                        attributes["WEIGHTS_0"] = weightsAccessor;
                    }
                    var targets = new List<object>();
                    foreach (var target in morphTargets)
                    {
                        targets.Add(target.ToJson());
                    }

                    var material = sourceMaterials != null && submesh < sourceMaterials.Length ? sourceMaterials[submesh] : null;
                    var primitive = new Dictionary<string, object>
                    {
                        ["attributes"] = attributes,
                        ["indices"] = AddIndicesAccessor(indices, true),
                        ["material"] = ExportMaterial(material),
                        ["mode"] = 4
                    };
                    if (targets.Count > 0)
                    {
                        primitive["targets"] = targets;
                    }
                    primitives.Add(primitive);
                }
                if (primitives.Count == 0)
                {
                    return -1;
                }

                var gltfMesh = new Dictionary<string, object>
                {
                    ["name"] = mesh.name,
                    ["primitives"] = primitives
                };
                if (morphWeights.Count > 0)
                {
                    gltfMesh["weights"] = morphWeights;
                }
                if (morphTargets.Count > 0)
                {
                    gltfMesh["extras"] = new Dictionary<string, object>
                    {
                        ["targetNames"] = morphTargets.Select(target => (object)target.Name).ToList()
                    };
                }
                meshes.Add(gltfMesh);
                return meshes.Count - 1;
            }

            private List<MorphTargetRecord> BuildMorphTargets(Mesh mesh, int vertexCount)
            {
                var targets = new List<MorphTargetRecord>();
                if (mesh == null || mesh.blendShapeCount <= 0 || vertexCount <= 0)
                {
                    return targets;
                }

                for (var i = 0; i < mesh.blendShapeCount; i++)
                {
                    var name = mesh.GetBlendShapeName(i);
                    if (morphTargetNames.Count > 0 && !morphTargetNames.Contains(name))
                    {
                        continue;
                    }
                    var frameCount = mesh.GetBlendShapeFrameCount(i);
                    if (frameCount <= 0)
                    {
                        continue;
                    }
                    var deltaVertices = new Vector3[vertexCount];
                    var deltaNormals = new Vector3[vertexCount];
                    var deltaTangents = new Vector3[vertexCount];
                    mesh.GetBlendShapeFrameVertices(i, frameCount - 1, deltaVertices, deltaNormals, deltaTangents);
                    var record = new MorphTargetRecord
                    {
                        Name = name,
                        PositionAccessor = AddVec3Accessor(deltaVertices, false, true),
                        NormalAccessor = HasAnyNonZero(deltaNormals) ? AddVec3Accessor(deltaNormals, false, true) : -1
                    };
                    targets.Add(record);
                }
                return targets;
            }

            private static List<object> BuildMorphWeights(Mesh mesh, SkinnedMeshRenderer skinned, List<MorphTargetRecord> morphTargets)
            {
                var weights = new List<object>();
                foreach (var target in morphTargets)
                {
                    var index = mesh.GetBlendShapeIndex(target.Name);
                    weights.Add(index >= 0 ? Mathf.Clamp01(skinned.GetBlendShapeWeight(index) / 100.0f) : 0.0f);
                }
                return weights;
            }

            private static bool HasAnyNonZero(Vector3[] values)
            {
                if (values == null)
                {
                    return false;
                }
                for (var i = 0; i < values.Length; i++)
                {
                    if (values[i].sqrMagnitude > 0.0f)
                    {
                        return true;
                    }
                }
                return false;
            }

            private int ExportSkin(SkinnedMeshRenderer renderer)
            {
                var bones = renderer.bones;
                if (bones == null || bones.Length == 0)
                {
                    return -1;
                }

                var joints = new List<object>();
                foreach (var bone in bones)
                {
                    if (bone == null || !nodeIndices.TryGetValue(bone, out var nodeIndex))
                    {
                        return -1;
                    }
                    joints.Add(nodeIndex);
                }

                var bindposes = renderer.sharedMesh != null ? renderer.sharedMesh.bindposes : null;
                var matrices = new List<Matrix4x4>();
                for (var i = 0; i < bones.Length; i++)
                {
                    matrices.Add(UnityMatrixToGltf(bindposes != null && i < bindposes.Length ? bindposes[i] : Matrix4x4.identity));
                }

                var skin = new Dictionary<string, object>
                {
                    ["joints"] = joints,
                    ["inverseBindMatrices"] = AddMat4Accessor(matrices)
                };
                if (renderer.rootBone != null && nodeIndices.TryGetValue(renderer.rootBone, out var skeleton))
                {
                    skin["skeleton"] = skeleton;
                }
                skins.Add(skin);
                return skins.Count - 1;
            }

            private int ExportMaterial(Material material)
            {
                if (material == null)
                {
                    return ExportDefaultMaterial();
                }
                if (materialIndices.TryGetValue(material, out var existing))
                {
                    return existing;
                }

                var baseColor = ReadColor(material, "_BaseColor", ReadColor(material, "_Color", Color.white));
                var pbr = new Dictionary<string, object>
                {
                    ["baseColorFactor"] = FloatArray(baseColor.r, baseColor.g, baseColor.b, baseColor.a),
                    ["metallicFactor"] = ReadFloat(material, "_Metallic", 0.0f),
                    ["roughnessFactor"] = 1.0f - ReadFloat(material, "_Glossiness", 0.5f)
                };

                var mainTex = ReadTexture(material, "_BaseMap") ?? ReadTexture(material, "_MainTex");
                if (mainTex != null)
                {
                    var mainTextureProperty = material.HasProperty("_BaseMap") ? "_BaseMap" : "_MainTex";
                    var textureIndex = ExportTexture(mainTex);
                    if (textureIndex >= 0)
                    {
                        pbr["baseColorTexture"] = TextureInfo(textureIndex, material, mainTextureProperty);
                    }
                }

                var gltfMaterial = new Dictionary<string, object>
                {
                    ["name"] = material.name,
                    ["pbrMetallicRoughness"] = pbr,
                    ["doubleSided"] = IsDoubleSidedMaterial(material)
                };
                var normalTexture = ReadTexture(material, "_BumpMap") ?? ReadTexture(material, "_NormalMap");
                if (normalTexture != null)
                {
                    var normalTextureIndex = ExportTexture(normalTexture);
                    if (normalTextureIndex >= 0)
                    {
                        gltfMaterial["normalTexture"] = new Dictionary<string, object>
                        {
                            ["index"] = normalTextureIndex,
                            ["scale"] = ReadFloat(material, "_BumpScale", 1.0f)
                        };
                    }
                }
                var sourceEmissionColor = ReadColor(material, "_EmissionColor", Color.black);
                var useEmission = IsMaterialFeatureEnabled(
                    material,
                    "_UseEmission",
                    ReadTexture(material, "_EmissionMap") != null || ReadTexture(material, "_EmissionTex") != null || sourceEmissionColor.maxColorComponent > 0.0f);
                var emissionTexture = useEmission ? ReadTexture(material, "_EmissionMap") ?? ReadTexture(material, "_EmissionTex") : null;
                var emissionMainStrength = ReadFloat(material, "_EmissionMainStrength", 1.0f);
                var emissionColor = useEmission ? sourceEmissionColor * emissionMainStrength : Color.black;
                if (emissionTexture != null)
                {
                    var emissionTextureIndex = ExportTexture(emissionTexture);
                    if (emissionTextureIndex >= 0)
                    {
                        gltfMaterial["emissiveTexture"] = new Dictionary<string, object> { ["index"] = emissionTextureIndex };
                    }
                }
                if (emissionColor.maxColorComponent > 0.0f)
                {
                    gltfMaterial["emissiveFactor"] = FloatArray(emissionColor.r, emissionColor.g, emissionColor.b);
                }
                if (IsAlphaBlendMaterial(material, baseColor))
                {
                    gltfMaterial["alphaMode"] = "BLEND";
                }
                else if (IsAlphaMaskMaterial(material))
                {
                    gltfMaterial["alphaMode"] = "MASK";
                    gltfMaterial["alphaCutoff"] = ReadFloat(material, "_Cutoff", 0.5f);
                }
                else if (material.HasProperty("_Cutoff"))
                {
                    gltfMaterial["alphaCutoff"] = ReadFloat(material, "_Cutoff", 0.5f);
                }
                var unAvatarMaterial = BuildUnAvatarMaterialExtras(material);
                if (unAvatarMaterial != null)
                {
                    gltfMaterial["extras"] = new Dictionary<string, object>
                    {
                        ["UN_avatar_material"] = unAvatarMaterial
                    };
                }

                materials.Add(gltfMaterial);
                var index = materials.Count - 1;
                materialIndices[material] = index;
                return index;
            }

            private static bool IsAlphaBlendMaterial(Material material, Color baseColor)
            {
                if (IsLilToonCutoutShader(material))
                {
                    return false;
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

            private static bool IsAlphaMaskMaterial(Material material)
            {
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
                return material.HasProperty("_Cutoff") ||
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
                    shaderName.IndexOf("Refraction", StringComparison.OrdinalIgnoreCase) >= 0 ||
                    shaderName.IndexOf("Fur", StringComparison.OrdinalIgnoreCase) >= 0);
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

            private static bool IsDoubleSidedMaterial(Material material)
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
                if (material == null || string.IsNullOrEmpty(property) || !material.HasProperty(property))
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

            private static bool IsMaterialFeatureEnabled(Material material, string property, bool fallback)
            {
                return material.HasProperty(property) ? ReadFloat(material, property, fallback ? 1.0f : 0.0f) > 0.5f : fallback;
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

            private Dictionary<string, object> BuildUnAvatarMaterialExtras(Material material)
            {
                var shaderName = material.shader != null ? material.shader.name : "";
                var lowerShader = shaderName.ToLowerInvariant();
                var looksToon = lowerShader.Contains("liltoon") || lowerShader.Contains("mtoon") || material.HasProperty("_ShadeColor") || material.HasProperty("_ShadeTex");
                if (!looksToon)
                {
                    return null;
                }

                var mtoon = new Dictionary<string, object>();
                var baseColor = ReadColor(material, "_BaseColor", ReadColor(material, "_Color", Color.white));
                var useShadow = IsMaterialFeatureEnabled(material, "_UseShadow", material.HasProperty("_ShadeColor") || material.HasProperty("_ShadowColor"));
                var shadeColor = useShadow
                    ? ReadColor(material, "_ShadeColor", ReadColor(material, "_ShadowColor", new Color(0.97f, 0.97f, 0.97f, 1.0f)))
                    : baseColor;
                mtoon["shadeColorFactor"] = FloatArray(shadeColor.r, shadeColor.g, shadeColor.b);
                AddTextureIndex(mtoon, "shadowColorTextureIndex", useShadow ? ReadTexture(material, "_ShadowColorTex") : null);
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

                var useMatCap = IsMaterialFeatureEnabled(material, "_UseMatCap", ReadTexture(material, "_MatCapTex") != null || ReadTexture(material, "_MatcapTex") != null);
                var matcapMainStrength = ReadFloat(material, "_MatCapMainStrength", ReadFloat(material, "_MatCapBlend", 1.0f));
                var matcapColor = useMatCap ? ReadColor(material, "_MatCapColor", Color.white) * matcapMainStrength : Color.black;
                mtoon["matcapFactor"] = FloatArray(matcapColor.r, matcapColor.g, matcapColor.b);
                AddTextureIndex(mtoon, "matcapTextureIndex", useMatCap ? ReadTexture(material, "_MatCapTex") ?? ReadTexture(material, "_MatcapTex") : null);
                AddTextureIndex(mtoon, "matcapBlendMaskTextureIndex", useMatCap ? ReadTexture(material, "_MatCapBlendMask") : null);
                var useMatCap2nd = IsMaterialFeatureEnabled(material, "_UseMatCap2nd", ReadTexture(material, "_MatCap2ndTex") != null);
                AddTextureIndex(mtoon, "matcap2ndTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndTex") : null);
                AddTextureIndex(mtoon, "matcap2ndBlendMaskTextureIndex", useMatCap2nd ? ReadTexture(material, "_MatCap2ndBlendMask") : null);

                var useRim = IsMaterialFeatureEnabled(material, "_UseRim", material.HasProperty("_RimColor") || ReadTexture(material, "_RimColorTex") != null);
                var rimMainStrength = ReadFloat(material, "_RimMainStrength", 1.0f);
                var rimColor = useRim ? ReadColor(material, "_RimColor", Color.black) * rimMainStrength : Color.black;
                mtoon["parametricRimColorFactor"] = FloatArray(rimColor.r, rimColor.g, rimColor.b);
                mtoon["parametricRimFresnelPowerFactor"] = ReadFloat(material, "_RimFresnelPower", 5.0f);
                mtoon["rimLightingMixFactor"] = useRim ? ReadFloat(material, "_RimEnableLighting", 1.0f) : 0.0f;
                mtoon["rimBlendMode"] = ReadFloat(material, "_RimBlendMode", 1.0f);
                AddTextureIndex(mtoon, "rimMultiplyTextureIndex", useRim ? ReadTexture(material, "_RimColorTex") : null);

                var useEmission = IsMaterialFeatureEnabled(
                    material,
                    "_UseEmission",
                    ReadTexture(material, "_EmissionMap") != null || ReadTexture(material, "_EmissionTex") != null || ReadColor(material, "_EmissionColor", Color.black).maxColorComponent > 0.0f);
                AddTextureIndex(mtoon, "emissionTextureIndex", useEmission ? ReadTexture(material, "_EmissionMap") ?? ReadTexture(material, "_EmissionTex") : null);

                AddTextureIndex(mtoon, "reflectionColorTextureIndex", ReadTexture(material, "_ReflectionColorTex"));
                AddTextureIndex(mtoon, "smoothnessTextureIndex", ReadTexture(material, "_SmoothnessTex"));
                AddTextureIndex(mtoon, "metallicGlossTextureIndex", ReadTexture(material, "_MetallicGlossMap"));
                AddTextureIndex(mtoon, "reflectionCubeTextureIndex", ReadTexture(material, "_ReflectionCubeTex"));

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

                var mainTextureProperty = material.HasProperty("_BaseMap") ? "_BaseMap" : "_MainTex";
                var mainTextureScale = Vector2.one;
                var mainTextureOffset = Vector2.zero;
                if (material.HasProperty(mainTextureProperty))
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

                mtoon["transparentWithZWrite"] = ReadFloat(material, "_ZWrite", 0.0f) > 0.5f || ReadFloat(material, "_ZWriteMode", 0.0f) > 0.5f;

                return new Dictionary<string, object>
                {
                    ["sourceShader"] = shaderName,
                    ["family"] = lowerShader.Contains("liltoon") ? "liltoon" : lowerShader.Contains("mtoon") ? "mtoon" : "toon",
                    ["unMaterialModel"] = "UNToon",
                    ["renderQueue"] = material.renderQueue,
                    ["floatParams"] = BuildMaterialFloatParams(material),
                    ["colorParams"] = BuildMaterialColorParams(material),
                    ["mtoon"] = mtoon
                };
            }

            private static Dictionary<string, object> BuildMaterialFloatParams(Material material)
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
                        type != UnityEngine.Rendering.ShaderPropertyType.Range)
                    {
                        continue;
                    }
                    var name = shader.GetPropertyName(i);
                    if (!string.IsNullOrEmpty(name) && material.HasProperty(name))
                    {
                        values[name] = material.GetFloat(name);
                    }
                }
                return values;
            }

            private static Dictionary<string, object> BuildMaterialColorParams(Material material)
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
                    if (string.IsNullOrEmpty(name) || !material.HasProperty(name))
                    {
                        continue;
                    }
                    var color = material.GetColor(name);
                    values[name] = FloatArray(color.r, color.g, color.b, color.a);
                }
                return values;
            }

            private void AddTextureIndex(Dictionary<string, object> dst, string key, Texture texture)
            {
                if (texture == null)
                {
                    return;
                }
                var textureIndex = ExportTexture(texture);
                if (textureIndex >= 0)
                {
                    dst[key] = textureIndex;
                    return;
                }
                var asset = ExportUnavatarTextureAsset(texture);
                if (asset != null)
                {
                    dst[key + "Asset"] = asset.Id;
                }
            }

            private int ExportTexture(Texture texture)
            {
                if (texture == null)
                {
                    return -1;
                }
                if (textureIndices.TryGetValue(texture, out var existing))
                {
                    return existing;
                }

                string fallbackReason;
                var encoded = TryReadSourceTextureBytes(texture, out fallbackReason);
                if (encoded == null && IsUnavatarExtensionOnlyTexture(MimeTypeFromPath(AssetDatabase.GetAssetPath(texture))))
                {
                    return -1;
                }
                if (encoded == null)
                {
                    encoded = EncodeTexturePng(texture, fallbackReason);
                }
                if (encoded == null || encoded.Bytes == null || encoded.Bytes.Length == 0)
                {
                    return -1;
                }

                var view = AddBufferView(encoded.Bytes);
                images.Add(new Dictionary<string, object>
                {
                    ["name"] = texture.name,
                    ["bufferView"] = view,
                    ["mimeType"] = encoded.MimeType,
                    ["extras"] = new Dictionary<string, object>
                    {
                        ["UN_avatar_image"] = BuildImageMetadataJson(texture)
                    }
                });
                exportedTextures.Add(new ExportedTextureRecord
                {
                    Name = texture.name,
                    AssetPath = encoded.AssetPath,
                    SourceExtension = encoded.SourceExtension,
                    SourceMimeType = encoded.SourceMimeType,
                    SourceByteLength = encoded.SourceByteLength,
                    OutputMimeType = encoded.MimeType,
                    OutputByteLength = encoded.Bytes.Length,
                    ExportMode = encoded.ExportMode,
                    FallbackReason = encoded.FallbackReason
                });
                textures.Add(new Dictionary<string, object>
                {
                    ["sampler"] = ExportSampler(texture),
                    ["source"] = images.Count - 1
                });
                var index = textures.Count - 1;
                textureIndices[texture] = index;
                return index;
            }

            private int ExportSampler(Texture texture)
            {
                var magFilter = texture.filterMode == FilterMode.Point ? 9728 : 9729;
                var minFilter = magFilter;
                var wrapS = GltfWrapMode(texture.wrapModeU);
                var wrapT = GltfWrapMode(texture.wrapModeV);
                var key = magFilter.ToString(CultureInfo.InvariantCulture) + "/" +
                    minFilter.ToString(CultureInfo.InvariantCulture) + "/" +
                    wrapS.ToString(CultureInfo.InvariantCulture) + "/" +
                    wrapT.ToString(CultureInfo.InvariantCulture);
                if (samplerIndices.TryGetValue(key, out var existing))
                {
                    return existing;
                }
                samplers.Add(BuildSamplerJson(magFilter, minFilter, wrapS, wrapT));
                var index = samplers.Count - 1;
                samplerIndices[key] = index;
                return index;
            }

            private static Dictionary<string, object> BuildSamplerJson(Texture texture)
            {
                var magFilter = texture.filterMode == FilterMode.Point ? 9728 : 9729;
                var minFilter = magFilter;
                return BuildSamplerJson(
                    magFilter,
                    minFilter,
                    GltfWrapMode(texture.wrapModeU),
                    GltfWrapMode(texture.wrapModeV));
            }

            private static Dictionary<string, object> BuildSamplerJson(int magFilter, int minFilter, int wrapS, int wrapT)
            {
                return new Dictionary<string, object>
                {
                    ["magFilter"] = magFilter,
                    ["minFilter"] = minFilter,
                    ["wrapS"] = wrapS,
                    ["wrapT"] = wrapT
                };
            }

            private static Dictionary<string, object> BuildImageMetadataJson(Texture texture)
            {
                var metadata = TextureAssetMetadata.FromTexture(texture, AssetDatabase.GetAssetPath(texture), null);
                var json = new Dictionary<string, object>
                {
                    ["colorSpace"] = metadata.ColorSpace,
                    ["textureType"] = metadata.TextureType ?? "",
                    ["textureShape"] = metadata.TextureShape ?? ""
                };
                if (!string.IsNullOrEmpty(metadata.SourcePixelFormat))
                {
                    json["sourcePixelFormat"] = metadata.SourcePixelFormat;
                }
                if (!string.IsNullOrEmpty(metadata.Channels))
                {
                    json["channels"] = metadata.Channels;
                }
                if (metadata.SRgb.HasValue)
                {
                    json["sRGB"] = metadata.SRgb.Value;
                }
                return json;
            }

            private static int GltfWrapMode(TextureWrapMode mode)
            {
                switch (mode)
                {
                    case TextureWrapMode.Clamp:
                        return 33071;
                    case TextureWrapMode.Mirror:
                    case TextureWrapMode.MirrorOnce:
                        return 33648;
                    default:
                        return 10497;
                }
            }

            private sealed class EncodedTexture
            {
                public byte[] Bytes;
                public string MimeType;
                public string AssetPath;
                public string SourceExtension;
                public string SourceMimeType;
                public long SourceByteLength;
                public string ExportMode;
                public string FallbackReason;

                public EncodedTexture(byte[] bytes, string mimeType)
                {
                    Bytes = bytes;
                    MimeType = mimeType;
                }
            }

            private static EncodedTexture TryReadSourceTextureBytes(Texture texture, out string fallbackReason)
            {
                fallbackReason = "";
                var assetPath = AssetDatabase.GetAssetPath(texture);
                if (string.IsNullOrEmpty(assetPath))
                {
                    fallbackReason = "generated_or_runtime_texture";
                    return null;
                }

                var mimeType = GltfImageMimeTypeFromPath(assetPath);
                if (string.IsNullOrEmpty(mimeType))
                {
                    fallbackReason = "unsupported_source_mime";
                    return null;
                }

                var fullPath = Path.IsPathRooted(assetPath)
                    ? assetPath
                    : Path.Combine(Directory.GetCurrentDirectory(), assetPath);
                if (!File.Exists(fullPath))
                {
                    fallbackReason = "source_file_not_found";
                    return null;
                }

                try
                {
                    var bytes = File.ReadAllBytes(fullPath);
                    if (bytes.Length <= 0)
                    {
                        fallbackReason = "empty_source_file";
                        return null;
                    }
                    return new EncodedTexture(bytes, mimeType)
                    {
                        AssetPath = assetPath,
                        SourceExtension = Path.GetExtension(assetPath).ToLowerInvariant(),
                        SourceMimeType = mimeType,
                        SourceByteLength = bytes.Length,
                        ExportMode = "source",
                        FallbackReason = ""
                    };
                }
                catch (Exception ex)
                {
                    Debug.LogWarning("[U.N. Avatar] Source texture read failed for " + texture.name + ": " + ex.Message);
                    fallbackReason = "source_read_failed";
                    return null;
                }
            }

            private static string MimeTypeFromPath(string path)
            {
                var extension = Path.GetExtension(path).ToLowerInvariant();
                switch (extension)
                {
                    case ".png":
                        return "image/png";
                    case ".jpg":
                    case ".jpeg":
                        return "image/jpeg";
                    case ".exr":
                        return "image/exr";
                    default:
                        return null;
                }
            }

            private static string GltfImageMimeTypeFromPath(string path)
            {
                var extension = Path.GetExtension(path).ToLowerInvariant();
                switch (extension)
                {
                    case ".png":
                        return "image/png";
                    case ".jpg":
                    case ".jpeg":
                        return "image/jpeg";
                    default:
                        return null;
                }
            }

            private UnavatarTextureAssetRecord ExportUnavatarTextureAsset(Texture texture)
            {
                if (textureAssetIndices.TryGetValue(texture, out var existing))
                {
                    return existing;
                }
                var assetPath = AssetDatabase.GetAssetPath(texture);
                if (string.IsNullOrEmpty(assetPath))
                {
                    return null;
                }
                var mimeType = MimeTypeFromPath(assetPath);
                if (!IsUnavatarExtensionOnlyTexture(mimeType))
                {
                    return null;
                }
                var fullPath = Path.IsPathRooted(assetPath)
                    ? assetPath
                    : Path.Combine(Directory.GetCurrentDirectory(), assetPath);
                if (!File.Exists(fullPath))
                {
                    return null;
                }

                try
                {
                    var bytes = File.ReadAllBytes(fullPath);
                    if (bytes.Length == 0)
                    {
                        return null;
                    }
                    var metadata = TextureAssetMetadata.FromTexture(texture, assetPath, bytes);
                    var asset = new UnavatarTextureAssetRecord
                    {
                        Id = "texture-asset-" + textureAssets.Count.ToString(CultureInfo.InvariantCulture),
                        Name = texture.name,
                        AssetPath = assetPath,
                        MimeType = mimeType,
                        SourceExtension = Path.GetExtension(assetPath).ToLowerInvariant(),
                        SourcePixelFormat = metadata.SourcePixelFormat,
                        ColorSpace = metadata.ColorSpace,
                        Channels = metadata.Channels,
                        TextureType = metadata.TextureType,
                        TextureShape = metadata.TextureShape,
                        SRgb = metadata.SRgb,
                        Sampler = BuildSamplerJson(texture),
                        Width = metadata.Width,
                        Height = metadata.Height,
                        Bytes = bytes
                    };
                    textureAssets.Add(asset);
                    textureAssetIndices[texture] = asset;
                    exportedTextures.Add(new ExportedTextureRecord
                    {
                        Name = texture.name,
                        AssetPath = assetPath,
                        SourceExtension = asset.SourceExtension,
                        SourceMimeType = mimeType,
                        SourceByteLength = bytes.Length,
                        OutputMimeType = mimeType,
                        OutputByteLength = bytes.Length,
                        ExportMode = "unavatar_source_asset",
                        FallbackReason = ""
                    });
                    return asset;
                }
                catch (Exception ex)
                {
                    Debug.LogWarning("[U.N. Avatar] Source texture asset read failed for " + texture.name + ": " + ex.Message);
                    return null;
                }
            }

            private static bool IsUnavatarExtensionOnlyTexture(string mimeType)
            {
                return mimeType == "image/exr";
            }

            private sealed class TextureAssetMetadata
            {
                public string SourcePixelFormat = "";
                public string ColorSpace = "linear";
                public string Channels = "";
                public string TextureType = "";
                public string TextureShape = "";
                public bool? SRgb;
                public int Width;
                public int Height;

                public static TextureAssetMetadata FromTexture(Texture texture, string assetPath, byte[] bytes)
                {
                    var extension = Path.GetExtension(assetPath ?? "").ToLowerInvariant();
                    var importer = !string.IsNullOrEmpty(assetPath) ? AssetImporter.GetAtPath(assetPath) as TextureImporter : null;
                    var colorSpace = TextureColorSpace(texture, importer);
                    var textureType = importer != null ? importer.textureType.ToString() : "";
                    var textureShape = importer != null ? importer.textureShape.ToString() : TextureShapeFromTexture(texture);
                    var srgb = TextureSrgb(texture, importer);
                    if (extension == ".exr")
                    {
                        var exr = TryReadExrMetadata(bytes);
                        if (exr != null)
                        {
                            exr.TextureType = textureType;
                            exr.TextureShape = textureShape;
                            exr.SRgb = srgb;
                            return exr;
                        }
                        return new TextureAssetMetadata
                        {
                            SourcePixelFormat = "unknown_float",
                            ColorSpace = "linear",
                            Channels = "",
                            TextureType = textureType,
                            TextureShape = textureShape,
                            SRgb = srgb
                        };
                    }

                    var pixelFormat = SourcePixelFormatHintFromTexture(texture, assetPath);
                    return new TextureAssetMetadata
                    {
                        SourcePixelFormat = pixelFormat,
                        ColorSpace = colorSpace,
                        Channels = ChannelsHintFromPixelFormat(pixelFormat),
                        TextureType = textureType,
                        TextureShape = textureShape,
                        SRgb = srgb,
                        Width = texture != null ? texture.width : 0,
                        Height = texture != null ? texture.height : 0
                    };
                }

                private static TextureAssetMetadata TryReadExrMetadata(byte[] bytes)
                {
                    try
                    {
                        if (bytes == null || bytes.Length < 12 || BitConverter.ToUInt32(bytes, 0) != 20000630u)
                        {
                            return null;
                        }

                        var offset = 8;
                        var width = 0;
                        var height = 0;
                        var channelNames = new List<string>();
                        var pixelTypes = new List<int>();

                        while (offset < bytes.Length)
                        {
                            var name = ReadNullTerminatedAscii(bytes, ref offset);
                            if (name == null)
                            {
                                return null;
                            }
                            if (name.Length == 0)
                            {
                                break;
                            }
                            var type = ReadNullTerminatedAscii(bytes, ref offset);
                            if (type == null || offset + 4 > bytes.Length)
                            {
                                return null;
                            }
                            var size = BitConverter.ToInt32(bytes, offset);
                            offset += 4;
                            if (size < 0 || offset + size > bytes.Length)
                            {
                                return null;
                            }

                            if (name == "channels" && type == "chlist")
                            {
                                ReadExrChannels(bytes, offset, size, channelNames, pixelTypes);
                            }
                            else if (name == "dataWindow" && type == "box2i" && size >= 16)
                            {
                                var minX = BitConverter.ToInt32(bytes, offset);
                                var minY = BitConverter.ToInt32(bytes, offset + 4);
                                var maxX = BitConverter.ToInt32(bytes, offset + 8);
                                var maxY = BitConverter.ToInt32(bytes, offset + 12);
                                width = Math.Max(0, maxX - minX + 1);
                                height = Math.Max(0, maxY - minY + 1);
                            }

                            offset += size;
                        }

                        var channels = CanonicalChannels(channelNames);
                        var pixelFormat = PixelFormatFromExrChannels(channels, pixelTypes);
                        return new TextureAssetMetadata
                        {
                            SourcePixelFormat = pixelFormat,
                            ColorSpace = "linear",
                            Channels = channels,
                            Width = width,
                            Height = height
                        };
                    }
                    catch
                    {
                        return null;
                    }
                }

                private static void ReadExrChannels(byte[] bytes, int start, int size, List<string> channelNames, List<int> pixelTypes)
                {
                    var offset = start;
                    var end = start + size;
                    while (offset < end)
                    {
                        var channelName = ReadNullTerminatedAscii(bytes, ref offset);
                        if (channelName == null || channelName.Length == 0)
                        {
                            break;
                        }
                        if (offset + 16 > end)
                        {
                            break;
                        }
                        var pixelType = BitConverter.ToInt32(bytes, offset);
                        offset += 16;
                        channelNames.Add(channelName);
                        pixelTypes.Add(pixelType);
                    }
                }

                private static string ReadNullTerminatedAscii(byte[] bytes, ref int offset)
                {
                    if (offset >= bytes.Length)
                    {
                        return null;
                    }
                    var start = offset;
                    while (offset < bytes.Length && bytes[offset] != 0)
                    {
                        offset++;
                    }
                    if (offset >= bytes.Length)
                    {
                        return null;
                    }
                    var value = Encoding.ASCII.GetString(bytes, start, offset - start);
                    offset++;
                    return value;
                }

                private static string CanonicalChannels(List<string> channelNames)
                {
                    if (channelNames == null || channelNames.Count == 0)
                    {
                        return "";
                    }
                    var names = new HashSet<string>(channelNames.Select(c => c.ToUpperInvariant()));
                    if (names.SetEquals(new[] { "R", "G", "B", "A" }))
                    {
                        return "rgba";
                    }
                    if (names.SetEquals(new[] { "R", "G", "B" }))
                    {
                        return "rgb";
                    }
                    if (names.SetEquals(new[] { "R", "G" }))
                    {
                        return "rg";
                    }
                    if (names.SetEquals(new[] { "R" }) || names.SetEquals(new[] { "Y" }))
                    {
                        return "r";
                    }
                    return "";
                }

                private static string PixelFormatFromExrChannels(string channels, List<int> pixelTypes)
                {
                    if (string.IsNullOrEmpty(channels) || pixelTypes == null || pixelTypes.Count == 0)
                    {
                        return "unknown_float";
                    }
                    var distinctTypes = new HashSet<int>(pixelTypes);
                    if (distinctTypes.Count != 1)
                    {
                        return "unknown_float";
                    }

                    string suffix;
                    switch (pixelTypes[0])
                    {
                        case 0:
                            suffix = "32U";
                            break;
                        case 1:
                            suffix = "16F";
                            break;
                        case 2:
                            suffix = "32F";
                            break;
                        default:
                            return "unknown_float";
                    }
                    return channels.ToUpperInvariant() + suffix;
                }
            }

            private static string SourcePixelFormatHintFromTexture(Texture texture, string assetPath)
            {
                var extension = Path.GetExtension(assetPath ?? "").ToLowerInvariant();
                if (extension == ".exr")
                {
                    return "unknown_float";
                }
                if (texture != null && texture.graphicsFormat.ToString().IndexOf("16", StringComparison.Ordinal) >= 0)
                {
                    return texture.graphicsFormat.ToString();
                }
                return "";
            }

            private static string TextureColorSpace(Texture texture, TextureImporter importer)
            {
                if (importer != null)
                {
                    return importer.sRGBTexture ? "srgb" : "linear";
                }
                var graphicsFormat = texture != null ? texture.graphicsFormat.ToString() : "";
                return graphicsFormat.IndexOf("SRGB", StringComparison.OrdinalIgnoreCase) >= 0 ? "srgb" : "linear";
            }

            private static bool? TextureSrgb(Texture texture, TextureImporter importer)
            {
                if (importer != null)
                {
                    return importer.sRGBTexture;
                }
                if (texture == null)
                {
                    return null;
                }
                return texture.graphicsFormat.ToString().IndexOf("SRGB", StringComparison.OrdinalIgnoreCase) >= 0;
            }

            private static string TextureShapeFromTexture(Texture texture)
            {
                if (texture is Cubemap)
                {
                    return "Cube";
                }
                if (texture is Texture3D)
                {
                    return "3D";
                }
                if (texture is Texture2DArray || texture is CubemapArray)
                {
                    return "Array";
                }
                return texture != null ? "2D" : "";
            }

            private static string ChannelsHintFromPixelFormat(string pixelFormat)
            {
                if (string.IsNullOrEmpty(pixelFormat))
                {
                    return "";
                }
                var upper = pixelFormat.ToUpperInvariant();
                if (upper.StartsWith("RGBA", StringComparison.Ordinal))
                {
                    return "rgba";
                }
                if (upper.StartsWith("RGB", StringComparison.Ordinal))
                {
                    return "rgb";
                }
                if (upper.StartsWith("RG", StringComparison.Ordinal))
                {
                    return "rg";
                }
                if (upper.StartsWith("R", StringComparison.Ordinal))
                {
                    return "r";
                }
                return "";
            }

            private static EncodedTexture EncodeTexturePng(Texture texture, string fallbackReason)
            {
                var assetPath = AssetDatabase.GetAssetPath(texture);
                var sourceExtension = string.IsNullOrEmpty(assetPath) ? "" : Path.GetExtension(assetPath).ToLowerInvariant();
                var sourceMimeType = string.IsNullOrEmpty(assetPath) ? "" : MimeTypeFromPath(assetPath) ?? "";
                var sourceByteLength = 0L;
                if (!string.IsNullOrEmpty(assetPath))
                {
                    var fullPath = Path.IsPathRooted(assetPath)
                        ? assetPath
                        : Path.Combine(Directory.GetCurrentDirectory(), assetPath);
                    if (File.Exists(fullPath))
                    {
                        sourceByteLength = new FileInfo(fullPath).Length;
                    }
                }

                var oldActive = RenderTexture.active;
                var metadata = TextureAssetMetadata.FromTexture(texture, assetPath, null);
                var readWrite = metadata.SRgb == true ? RenderTextureReadWrite.sRGB : RenderTextureReadWrite.Linear;
                var temporary = RenderTexture.GetTemporary(texture.width, texture.height, 0, RenderTextureFormat.ARGB32, readWrite);
                try
                {
                    Graphics.Blit(texture, temporary);
                    RenderTexture.active = temporary;
                    var readable = new Texture2D(texture.width, texture.height, TextureFormat.RGBA32, false);
                    readable.ReadPixels(new Rect(0, 0, texture.width, texture.height), 0, 0);
                    readable.Apply();
                    var png = readable.EncodeToPNG();
                    UnityEngine.Object.DestroyImmediate(readable);
                    return new EncodedTexture(png, "image/png")
                    {
                        AssetPath = assetPath,
                        SourceExtension = sourceExtension,
                        SourceMimeType = sourceMimeType,
                        SourceByteLength = sourceByteLength,
                        ExportMode = "png_fallback",
                        FallbackReason = string.IsNullOrEmpty(fallbackReason) ? "source_bytes_unavailable" : fallbackReason
                    };
                }
                catch (Exception ex)
                {
                    Debug.LogWarning("[U.N. Avatar] Texture export failed for " + texture.name + ": " + ex.Message);
                    return null;
                }
                finally
                {
                    RenderTexture.active = oldActive;
                    RenderTexture.ReleaseTemporary(temporary);
                }
            }

            private static Vector3 UnityVectorToGltf(Vector3 value)
            {
                return new Vector3(-value.x, value.y, value.z);
            }

            private static Quaternion UnityRotationToGltf(Quaternion value)
            {
                return new Quaternion(value.x, -value.y, -value.z, value.w);
            }

            private static Vector4 UnityTangentToGltf(Vector4 value)
            {
                return new Vector4(-value.x, value.y, value.z, -value.w);
            }

            private static Matrix4x4 UnityMatrixToGltf(Matrix4x4 value)
            {
                for (var row = 0; row < 4; row++)
                {
                    value[row, 0] = -value[row, 0];
                }
                for (var col = 0; col < 4; col++)
                {
                    value[0, col] = -value[0, col];
                }
                return value;
            }

            private int AddVec3Accessor(Vector3[] values, bool minMax, bool convertUnityToGltf)
            {
                var bytes = new byte[values.Length * 12];
                var min = new Vector3(float.PositiveInfinity, float.PositiveInfinity, float.PositiveInfinity);
                var max = new Vector3(float.NegativeInfinity, float.NegativeInfinity, float.NegativeInfinity);
                for (var i = 0; i < values.Length; i++)
                {
                    var value = convertUnityToGltf ? UnityVectorToGltf(values[i]) : values[i];
                    WriteFloat(bytes, i * 12, value.x);
                    WriteFloat(bytes, i * 12 + 4, value.y);
                    WriteFloat(bytes, i * 12 + 8, value.z);
                    min = Vector3.Min(min, value);
                    max = Vector3.Max(max, value);
                }
                var view = AddBufferView(bytes);
                var accessor = Accessor(view, values.Length, 5126, "VEC3");
                if (minMax)
                {
                    accessor["min"] = FloatArray(min.x, min.y, min.z);
                    accessor["max"] = FloatArray(max.x, max.y, max.z);
                }
                accessors.Add(accessor);
                return accessors.Count - 1;
            }

            private int AddVec4Accessor(Vector4[] values, bool convertUnityTangentToGltf)
            {
                var bytes = new byte[values.Length * 16];
                for (var i = 0; i < values.Length; i++)
                {
                    var value = convertUnityTangentToGltf ? UnityTangentToGltf(values[i]) : values[i];
                    WriteFloat(bytes, i * 16, value.x);
                    WriteFloat(bytes, i * 16 + 4, value.y);
                    WriteFloat(bytes, i * 16 + 8, value.z);
                    WriteFloat(bytes, i * 16 + 12, value.w);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5126, "VEC4"));
                return accessors.Count - 1;
            }

            private int AddVec2Accessor(Vector2[] values)
            {
                var bytes = new byte[values.Length * 8];
                for (var i = 0; i < values.Length; i++)
                {
                    WriteFloat(bytes, i * 8, values[i].x);
                    WriteFloat(bytes, i * 8 + 4, 1.0f - values[i].y);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5126, "VEC2"));
                return accessors.Count - 1;
            }

            private static float GltfTextureOffsetY(float unityOffsetY, float unityScaleY)
            {
                return 1.0f - unityScaleY - unityOffsetY;
            }

            private int AddColorAccessor(Color[] values)
            {
                var bytes = new byte[values.Length * 16];
                for (var i = 0; i < values.Length; i++)
                {
                    WriteFloat(bytes, i * 16, values[i].r);
                    WriteFloat(bytes, i * 16 + 4, values[i].g);
                    WriteFloat(bytes, i * 16 + 8, values[i].b);
                    WriteFloat(bytes, i * 16 + 12, values[i].a);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5126, "VEC4"));
                return accessors.Count - 1;
            }

            private int AddJointsAccessor(BoneWeight[] values)
            {
                var bytes = new byte[values.Length * 8];
                for (var i = 0; i < values.Length; i++)
                {
                    WriteUShort(bytes, i * 8, values[i].boneIndex0);
                    WriteUShort(bytes, i * 8 + 2, values[i].boneIndex1);
                    WriteUShort(bytes, i * 8 + 4, values[i].boneIndex2);
                    WriteUShort(bytes, i * 8 + 6, values[i].boneIndex3);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5123, "VEC4"));
                return accessors.Count - 1;
            }

            private int AddWeightsAccessor(BoneWeight[] values)
            {
                var bytes = new byte[values.Length * 16];
                for (var i = 0; i < values.Length; i++)
                {
                    WriteFloat(bytes, i * 16, values[i].weight0);
                    WriteFloat(bytes, i * 16 + 4, values[i].weight1);
                    WriteFloat(bytes, i * 16 + 8, values[i].weight2);
                    WriteFloat(bytes, i * 16 + 12, values[i].weight3);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5126, "VEC4"));
                return accessors.Count - 1;
            }

            private int AddMat4Accessor(List<Matrix4x4> values)
            {
                var bytes = new byte[values.Count * 64];
                for (var i = 0; i < values.Count; i++)
                {
                    var offset = i * 64;
                    var m = values[i];
                    WriteFloat(bytes, offset, m.m00);
                    WriteFloat(bytes, offset + 4, m.m10);
                    WriteFloat(bytes, offset + 8, m.m20);
                    WriteFloat(bytes, offset + 12, m.m30);
                    WriteFloat(bytes, offset + 16, m.m01);
                    WriteFloat(bytes, offset + 20, m.m11);
                    WriteFloat(bytes, offset + 24, m.m21);
                    WriteFloat(bytes, offset + 28, m.m31);
                    WriteFloat(bytes, offset + 32, m.m02);
                    WriteFloat(bytes, offset + 36, m.m12);
                    WriteFloat(bytes, offset + 40, m.m22);
                    WriteFloat(bytes, offset + 44, m.m32);
                    WriteFloat(bytes, offset + 48, m.m03);
                    WriteFloat(bytes, offset + 52, m.m13);
                    WriteFloat(bytes, offset + 56, m.m23);
                    WriteFloat(bytes, offset + 60, m.m33);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Count, 5126, "MAT4"));
                return accessors.Count - 1;
            }

            private int AddIndicesAccessor(int[] indices, bool reverseWinding)
            {
                var useUshort = indices.All(i => i >= 0 && i <= ushort.MaxValue);
                var bytes = new byte[indices.Length * (useUshort ? 2 : 4)];
                for (var i = 0; i < indices.Length; i++)
                {
                    var value = indices[i];
                    if (reverseWinding)
                    {
                        var triangleOffset = i % 3;
                        var triangleStart = i - triangleOffset;
                        if (triangleStart + 2 < indices.Length)
                        {
                            if (triangleOffset == 1)
                            {
                                value = indices[i + 1];
                            }
                            else if (triangleOffset == 2)
                            {
                                value = indices[i - 1];
                            }
                        }
                    }
                    if (useUshort)
                    {
                        WriteUShort(bytes, i * 2, value);
                    }
                    else
                    {
                        WriteUInt(bytes, i * 4, (uint)value);
                    }
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, indices.Length, useUshort ? 5123 : 5125, "SCALAR"));
                return accessors.Count - 1;
            }

            private int AddBufferView(byte[] bytes)
            {
                var offset = buffer.Append(bytes);
                bufferViews.Add(new Dictionary<string, object>
                {
                    ["buffer"] = 0,
                    ["byteOffset"] = offset,
                    ["byteLength"] = bytes.Length
                });
                return bufferViews.Count - 1;
            }

            private static Dictionary<string, object> Accessor(int bufferView, int count, int componentType, string type)
            {
                return new Dictionary<string, object>
                {
                    ["bufferView"] = bufferView,
                    ["byteOffset"] = 0,
                    ["componentType"] = componentType,
                    ["count"] = count,
                    ["type"] = type
                };
            }

            private static Color ReadColor(Material material, string property, Color fallback)
            {
                return material.HasProperty(property) ? material.GetColor(property) : fallback;
            }

            private static float ReadFloat(Material material, string property, float fallback)
            {
                return material.HasProperty(property) ? material.GetFloat(property) : fallback;
            }

            private static Vector4 ReadVector(Material material, string property, Vector4 fallback)
            {
                return material.HasProperty(property) ? material.GetVector(property) : fallback;
            }

            private static Texture ReadTexture(Material material, string property)
            {
                return material.HasProperty(property) ? material.GetTexture(property) : null;
            }

            private static void WriteGlb(string path, string json, byte[] bin)
            {
                var jsonBytes = Pad(Encoding.UTF8.GetBytes(json), 0x20);
                var binBytes = Pad(bin ?? Array.Empty<byte>(), 0x00);
                var total = 12 + 8 + jsonBytes.Length + (binBytes.Length > 0 ? 8 + binBytes.Length : 0);
                using (var stream = File.Create(path))
                using (var writer = new BinaryWriter(stream))
                {
                    writer.Write(GlbMagic);
                    writer.Write((uint)2);
                    writer.Write((uint)total);
                    writer.Write((uint)jsonBytes.Length);
                    writer.Write(JsonChunkType);
                    writer.Write(jsonBytes);
                    if (binBytes.Length > 0)
                    {
                        writer.Write((uint)binBytes.Length);
                        writer.Write(BinChunkType);
                        writer.Write(binBytes);
                    }
                }
            }

            private static List<object> FloatArray(params float[] values)
            {
                return values.Select(v => (object)v).ToList();
            }

            private static byte[] Pad(byte[] data, byte value)
            {
                var length = (data.Length + 3) & ~3;
                if (length == data.Length)
                {
                    return data;
                }
                var padded = new byte[length];
                Buffer.BlockCopy(data, 0, padded, 0, data.Length);
                for (var i = data.Length; i < padded.Length; i++)
                {
                    padded[i] = value;
                }
                return padded;
            }

            private static void WriteFloat(byte[] bytes, int offset, float value)
            {
                Buffer.BlockCopy(BitConverter.GetBytes(value), 0, bytes, offset, 4);
            }

            private static void WriteUShort(byte[] bytes, int offset, int value)
            {
                Buffer.BlockCopy(BitConverter.GetBytes((ushort)value), 0, bytes, offset, 2);
            }

            private static void WriteUInt(byte[] bytes, int offset, uint value)
            {
                Buffer.BlockCopy(BitConverter.GetBytes(value), 0, bytes, offset, 4);
            }
        }

        private sealed class MorphTargetRecord
        {
            public string Name;
            public int PositionAccessor;
            public int NormalAccessor;

            public Dictionary<string, object> ToJson()
            {
                var json = new Dictionary<string, object>();
                if (PositionAccessor >= 0)
                {
                    json["POSITION"] = PositionAccessor;
                }
                if (NormalAccessor >= 0)
                {
                    json["NORMAL"] = NormalAccessor;
                }
                return json;
            }
        }

        private sealed class BinaryBuffer
        {
            private readonly List<byte> bytes = new List<byte>();

            public int Length => bytes.Count;

            public int Append(byte[] data)
            {
                while ((bytes.Count & 3) != 0)
                {
                    bytes.Add(0);
                }
                var offset = bytes.Count;
                bytes.AddRange(data);
                while ((bytes.Count & 3) != 0)
                {
                    bytes.Add(0);
                }
                return offset;
            }

            public byte[] ToArray()
            {
                return bytes.ToArray();
            }
        }
    }
}

