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
            private EncodedTexture EncodeTexturePng(Texture texture, string fallbackReason)
            {
                var source = GetTextureSourceInfo(texture);

                var oldActive = RenderTexture.active;
                var metadata = TextureAssetMetadata.FromTexture(texture, source.AssetPath, null, source.Importer);
                if (IsCubeTextureShape(metadata.TextureShape) && texture is Cubemap cubemap)
                {
                    return EncodeCubemapHorizontalStrip(cubemap, source, fallbackReason, metadata);
                }
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
                        AssetPath = source.AssetPath,
                        SourceExtension = source.SourceExtension,
                        SourceMimeType = source.MimeType,
                        SourceByteLength = source.SourceByteLength,
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

            private EncodedTexture EncodeCubemapHorizontalStrip(Cubemap cubemap, TextureSourceInfo source, string fallbackReason, TextureAssetMetadata metadata)
            {
                var oldActive = RenderTexture.active;
                var faceSize = cubemap.width;
                var exportExr = CubemapShouldExportAsExr(cubemap, source, metadata);
                var textureFormat = exportExr ? TextureFormat.RGBAHalf : TextureFormat.RGBA32;
                var renderTextureFormat = exportExr ? RenderTextureFormat.ARGBHalf : RenderTextureFormat.ARGB32;
                var readWrite = exportExr || metadata.SRgb != true ? RenderTextureReadWrite.Linear : RenderTextureReadWrite.sRGB;
                var linearTexture = exportExr || metadata.SRgb != true;
                var strip = new Texture2D(faceSize * 6, faceSize, textureFormat, false, linearTexture);
                var faceTexture = new Texture2D(faceSize, faceSize, textureFormat, false, linearTexture);
                var sourceFace = new Texture2D(faceSize, faceSize, cubemap.format, false, linearTexture);
                var temporary = RenderTexture.GetTemporary(faceSize, faceSize, 0, renderTextureFormat, readWrite);
                try
                {
                    for (var face = 0; face < 6; face++)
                    {
                        Graphics.CopyTexture(cubemap, face, 0, sourceFace, 0, 0);
                        Graphics.Blit(sourceFace, temporary);
                        RenderTexture.active = temporary;
                        faceTexture.ReadPixels(new Rect(0, 0, faceSize, faceSize), 0, 0);
                        faceTexture.Apply();
                        strip.SetPixels(face * faceSize, 0, faceSize, faceSize, faceTexture.GetPixels());
                    }
                    strip.Apply();
                    var bytes = exportExr
                        ? strip.EncodeToEXR(Texture2D.EXRFlags.CompressZIP)
                        : strip.EncodeToPNG();
                    return new EncodedTexture(bytes, exportExr ? "image/exr" : "image/png")
                    {
                        AssetPath = source.AssetPath,
                        SourceExtension = source.SourceExtension,
                        SourceMimeType = source.MimeType,
                        SourceByteLength = source.SourceByteLength,
                        ExportMode = "generated_cubemap_horizontal_strip",
                        FallbackReason = string.IsNullOrEmpty(fallbackReason) ? "unity_generated_cubemap" : fallbackReason,
                        SourceLayoutOverride = "horizontal_strip"
                    };
                }
                catch (Exception ex)
                {
                    Debug.LogWarning("[U.N. Avatar] Cubemap export failed for " + cubemap.name + ": " + ex.Message);
                    return null;
                }
                finally
                {
                    RenderTexture.active = oldActive;
                    RenderTexture.ReleaseTemporary(temporary);
                    UnityEngine.Object.DestroyImmediate(sourceFace);
                    UnityEngine.Object.DestroyImmediate(faceTexture);
                    UnityEngine.Object.DestroyImmediate(strip);
                }
            }

            private static bool CubemapShouldExportAsExr(Cubemap cubemap, TextureSourceInfo source, TextureAssetMetadata metadata)
            {
                var extension = source.SourceExtension ?? "";
                if (extension.Equals(".exr", StringComparison.OrdinalIgnoreCase) ||
                    extension.Equals(".hdr", StringComparison.OrdinalIgnoreCase))
                {
                    return true;
                }
                var graphicsFormat = cubemap != null ? cubemap.graphicsFormat.ToString() : "";
                return IsFloatGraphicsFormat(graphicsFormat);
            }

            private static bool IsFloatGraphicsFormat(string graphicsFormat)
            {
                return graphicsFormat.IndexOf("Float", StringComparison.OrdinalIgnoreCase) >= 0 ||
                       graphicsFormat.IndexOf("SFloat", StringComparison.OrdinalIgnoreCase) >= 0 ||
                       graphicsFormat.IndexOf("UFloat", StringComparison.OrdinalIgnoreCase) >= 0;
            }
        }
    }
}
