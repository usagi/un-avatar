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
    internal static partial class MinimalGltfExporter
    {
        private sealed partial class Writer
        {
            private EncodedTexture EncodeTexturePng(Texture texture, string fallbackReason)
            {
                var source = GetTextureSourceInfo(texture);

                var oldActive = RenderTexture.active;
                var metadata = TextureAssetMetadata.FromTexture(texture, source.AssetPath, null, source.Importer);
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
        }
    }
}
