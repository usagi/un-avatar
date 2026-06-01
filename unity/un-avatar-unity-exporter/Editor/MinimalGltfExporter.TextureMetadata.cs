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

                public static TextureAssetMetadata FromTexture(Texture texture, string assetPath, byte[] bytes, TextureImporter importer = null)
                {
                    var extension = Path.GetExtension(assetPath ?? "").ToLowerInvariant();
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
                    var names = new HashSet<string>(StringComparer.Ordinal);
                    foreach (var channelName in channelNames)
                    {
                        names.Add(channelName.ToUpperInvariant());
                    }
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
        }
    }
}
