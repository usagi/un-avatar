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
                public string SourceLayoutOverride;

                public EncodedTexture(byte[] bytes, string mimeType)
                {
                    Bytes = bytes;
                    MimeType = mimeType;
                }
            }

            private TextureSourceInfo GetTextureSourceInfo(Texture texture)
            {
                if (texture == null)
                {
                    return TextureSourceInfo.Empty;
                }
                if (textureSourceInfos.TryGetValue(texture, out var existing))
                {
                    return existing;
                }

                var assetPath = AssetDatabase.GetAssetPath(texture);
                var info = new TextureSourceInfo
                {
                    AssetPath = assetPath ?? "",
                    SourceExtension = string.IsNullOrEmpty(assetPath) ? "" : Path.GetExtension(assetPath).ToLowerInvariant(),
                    MimeType = string.IsNullOrEmpty(assetPath) ? "" : MimeTypeFromPath(assetPath) ?? "",
                    GltfMimeType = string.IsNullOrEmpty(assetPath) ? "" : GltfImageMimeTypeFromPath(assetPath) ?? "",
                    Importer = !string.IsNullOrEmpty(assetPath) ? AssetImporter.GetAtPath(assetPath) as TextureImporter : null
                };
                if (!string.IsNullOrEmpty(assetPath))
                {
                    info.FullPath = Path.IsPathRooted(assetPath)
                        ? assetPath
                        : Path.Combine(Directory.GetCurrentDirectory(), assetPath);
                    info.Exists = File.Exists(info.FullPath);
                    if (info.Exists)
                    {
                        info.SourceByteLength = new FileInfo(info.FullPath).Length;
                    }
                }

                textureSourceInfos[texture] = info;
                return info;
            }

            private sealed class TextureSourceInfo
            {
                public static readonly TextureSourceInfo Empty = new TextureSourceInfo();
                public string AssetPath = "";
                public string FullPath = "";
                public string SourceExtension = "";
                public string MimeType = "";
                public string GltfMimeType = "";
                public long SourceByteLength;
                public bool Exists;
                public TextureImporter Importer;
            }

            private EncodedTexture TryReadSourceTextureBytes(Texture texture, out string fallbackReason)
            {
                fallbackReason = "";
                var source = GetTextureSourceInfo(texture);
                if (TextureNeedsGeneratedCubemapBake(texture))
                {
                    fallbackReason = "unity_generated_cubemap_requires_baked_faces";
                    return null;
                }
                if (string.IsNullOrEmpty(source.AssetPath))
                {
                    fallbackReason = "generated_or_runtime_texture";
                    return null;
                }

                if (string.IsNullOrEmpty(source.GltfMimeType))
                {
                    fallbackReason = "unsupported_source_mime";
                    return null;
                }

                if (!source.Exists)
                {
                    fallbackReason = "source_file_not_found";
                    return null;
                }

                var bytes = ReadSourceTextureBytes(texture, source, "Source texture read failed for", out fallbackReason);
                if (bytes == null)
                {
                    return null;
                }
                return new EncodedTexture(bytes, source.GltfMimeType)
                {
                    AssetPath = source.AssetPath,
                    SourceExtension = source.SourceExtension,
                    SourceMimeType = source.GltfMimeType,
                    SourceByteLength = bytes.Length,
                    ExportMode = "source",
                    FallbackReason = ""
                };
            }

            private byte[] ReadSourceTextureBytes(Texture texture, TextureSourceInfo source, string warningPrefix, out string failureReason)
            {
                failureReason = "";
                if (source == null || string.IsNullOrEmpty(source.FullPath))
                {
                    failureReason = "source_file_not_found";
                    return null;
                }
                if (textureSourceBytes.TryGetValue(source.FullPath, out var cached))
                {
                    return cached;
                }

                try
                {
                    var bytes = File.ReadAllBytes(source.FullPath);
                    if (bytes.Length <= 0)
                    {
                        failureReason = "empty_source_file";
                        return null;
                    }
                    textureSourceBytes[source.FullPath] = bytes;
                    return bytes;
                }
                catch (Exception ex)
                {
                    Debug.LogWarning("[U.N. Avatar] " + warningPrefix + " " + texture.name + ": " + ex.Message);
                    failureReason = "source_read_failed";
                    return null;
                }
            }

            private bool TextureNeedsGeneratedCubemapBake(Texture texture)
            {
                var source = GetTextureSourceInfo(texture);
                var shape = source.Importer != null ? source.Importer.textureShape.ToString() : TextureShapeFromTexture(texture);
                return IsCubeTextureShape(shape) && !string.IsNullOrEmpty(UnityGenerateCubemap(source.Importer));
            }

            private bool TextureNeedsGeneratedCubemapAsset(Texture texture)
            {
                if (!TextureNeedsGeneratedCubemapBake(texture) || !(texture is Cubemap cubemap))
                {
                    return false;
                }
                var source = GetTextureSourceInfo(texture);
                var metadata = TextureAssetMetadata.FromTexture(texture, source.AssetPath, null, source.Importer);
                return CubemapShouldExportAsExr(cubemap, source, metadata);
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
                    case ".hdr":
                        return "image/vnd.radiance";
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
        }
    }
}
