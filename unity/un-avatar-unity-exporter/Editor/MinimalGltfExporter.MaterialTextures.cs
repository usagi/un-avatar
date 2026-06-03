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
                if (encoded == null && IsUnavatarTextureAssetMime(GetTextureSourceInfo(texture).MimeType))
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

            private Dictionary<string, object> BuildImageMetadataJson(Texture texture)
            {
                var source = GetTextureSourceInfo(texture);
                var metadata = TextureAssetMetadata.FromTexture(texture, source.AssetPath, null, source.Importer);
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
                if (!string.IsNullOrEmpty(metadata.SourceLayout))
                {
                    json["sourceLayout"] = metadata.SourceLayout;
                }
                if (!string.IsNullOrEmpty(metadata.UnityGenerateCubemap))
                {
                    json["unityGenerateCubemap"] = metadata.UnityGenerateCubemap;
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
        }
    }
}
