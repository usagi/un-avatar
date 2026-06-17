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
                    var mainTextureProperty = HasProperty(material, "_BaseMap") ? "_BaseMap" : "_MainTex";
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
                else if (HasProperty(material, "_Cutoff"))
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
        }
    }
}
