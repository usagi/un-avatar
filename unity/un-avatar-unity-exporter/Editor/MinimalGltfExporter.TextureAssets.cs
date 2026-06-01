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
            private UnavatarTextureAssetRecord ExportUnavatarTextureAsset(Texture texture)
            {
                if (textureAssetIndices.TryGetValue(texture, out var existing))
                {
                    return existing;
                }
                var source = GetTextureSourceInfo(texture);
                if (string.IsNullOrEmpty(source.AssetPath))
                {
                    return null;
                }
                if (!IsUnavatarExtensionOnlyTexture(source.MimeType))
                {
                    return null;
                }
                if (!source.Exists)
                {
                    return null;
                }

                try
                {
                    var bytes = File.ReadAllBytes(source.FullPath);
                    if (bytes.Length == 0)
                    {
                        return null;
                    }
                    var metadata = TextureAssetMetadata.FromTexture(texture, source.AssetPath, bytes, source.Importer);
                    var asset = new UnavatarTextureAssetRecord
                    {
                        Id = "texture-asset-" + textureAssets.Count.ToString(CultureInfo.InvariantCulture),
                        Name = texture.name,
                        AssetPath = source.AssetPath,
                        MimeType = source.MimeType,
                        SourceExtension = source.SourceExtension,
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
                        AssetPath = source.AssetPath,
                        SourceExtension = asset.SourceExtension,
                        SourceMimeType = source.MimeType,
                        SourceByteLength = bytes.Length,
                        OutputMimeType = source.MimeType,
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
        }
    }
}
