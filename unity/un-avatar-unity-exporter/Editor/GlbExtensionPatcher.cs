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
    internal static class GlbExtensionPatcher
    {
        private const uint GlbMagic = 0x46546C67;
        private const uint JsonChunkType = 0x4E4F534A;

        public static string ReadRootJson(string glbPath)
        {
            var bytes = File.ReadAllBytes(glbPath);
            if (bytes.Length < 20)
            {
                throw new InvalidDataException("GLB is too small.");
            }
            var magic = ReadUInt32(bytes, 0);
            var version = ReadUInt32(bytes, 4);
            if (magic != GlbMagic || version != 2)
            {
                throw new InvalidDataException("Expected GLB version 2.");
            }

            var offset = 12;
            while (offset + 8 <= bytes.Length)
            {
                var length = checked((int)ReadUInt32(bytes, offset));
                var type = ReadUInt32(bytes, offset + 4);
                offset += 8;
                if (offset + length > bytes.Length)
                {
                    throw new InvalidDataException("GLB chunk exceeds file size.");
                }
                if (type == JsonChunkType)
                {
                    return Encoding.UTF8.GetString(bytes, offset, length).TrimEnd('\0', ' ', '\t', '\r', '\n');
                }
                offset += length;
            }

            throw new InvalidDataException("GLB JSON chunk was not found.");
        }

        public static string ExtractRootExtensionJson(string json, string extensionName)
        {
            var extensionsIndex = json.IndexOf("\"extensions\"", StringComparison.Ordinal);
            if (extensionsIndex < 0)
            {
                throw new InvalidDataException("Root extensions object was not found.");
            }
            var colon = json.IndexOf(':', extensionsIndex);
            var objectStart = json.IndexOf('{', colon);
            var objectEnd = FindMatchingBrace(json, objectStart);
            var extensionKey = "\"" + MiniJson.EscapeString(extensionName) + "\"";
            var keyIndex = json.IndexOf(extensionKey, objectStart, objectEnd - objectStart, StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                throw new InvalidDataException(extensionName + " extension was not found.");
            }
            var extensionColon = json.IndexOf(':', keyIndex);
            var extensionStart = json.IndexOf('{', extensionColon);
            var extensionEnd = FindMatchingBrace(json, extensionStart);
            return json.Substring(extensionStart, extensionEnd - extensionStart + 1);
        }

        public static void PatchRootExtension(
            string sourceGlb,
            string destinationGlb,
            string extensionName,
            Dictionary<string, object> payload,
            List<UnavatarTextureAssetRecord> textureAssets = null,
            List<WardrobePreviewImageDraft> wardrobePreviewImages = null)
        {
            var bytes = File.ReadAllBytes(sourceGlb);
            if (bytes.Length < 20)
            {
                throw new InvalidDataException("GLB is too small.");
            }
            var magic = ReadUInt32(bytes, 0);
            var version = ReadUInt32(bytes, 4);
            if (magic != GlbMagic || version != 2)
            {
                throw new InvalidDataException("Expected GLB version 2.");
            }

            var chunks = new List<GlbChunk>();
            var offset = 12;
            while (offset + 8 <= bytes.Length)
            {
                var length = checked((int)ReadUInt32(bytes, offset));
                var type = ReadUInt32(bytes, offset + 4);
                offset += 8;
                if (offset + length > bytes.Length)
                {
                    throw new InvalidDataException("GLB chunk exceeds file size.");
                }
                var data = new byte[length];
                Buffer.BlockCopy(bytes, offset, data, 0, length);
                chunks.Add(new GlbChunk { Type = type, Data = data });
                offset += length;
            }

            var jsonChunk = chunks.FirstOrDefault(c => c.Type == JsonChunkType);
            if (jsonChunk == null)
            {
                throw new InvalidDataException("GLB JSON chunk was not found.");
            }

            var binChunk = chunks.FirstOrDefault(c => c.Type == 0x004E4942);
            if (binChunk == null)
            {
                binChunk = new GlbChunk { Type = 0x004E4942, Data = Array.Empty<byte>() };
                chunks.Add(binChunk);
            }

            var json = Encoding.UTF8.GetString(jsonChunk.Data).TrimEnd('\0', ' ', '\t', '\r', '\n');
            if (textureAssets != null && textureAssets.Count > 0)
            {
                json = AppendTextureAssetBufferViews(json, binChunk, textureAssets);
                payload["textureAssets"] = textureAssets
                    .Select(asset => asset.ToJson())
                    .Cast<object>()
                    .ToList();
            }
            if (wardrobePreviewImages != null && wardrobePreviewImages.Count > 0)
            {
                json = AppendWardrobePreviewBufferViews(json, binChunk, wardrobePreviewImages);
                PatchWardrobePreviewBufferViewsInPayload(payload, wardrobePreviewImages);
            }
            json = PatchRootJson(json, extensionName, payload);
            jsonChunk.Data = Pad(Encoding.UTF8.GetBytes(json), 0x20);

            WriteGlb(destinationGlb, chunks);
        }

        public static byte[] ReadBufferViewBytes(string glbPath, int bufferViewIndex)
        {
            if (bufferViewIndex < 0)
            {
                return null;
            }
            var bytes = File.ReadAllBytes(glbPath);
            if (bytes.Length < 20)
            {
                throw new InvalidDataException("GLB is too small.");
            }

            var chunks = ReadChunks(bytes);
            var jsonChunk = chunks.FirstOrDefault(c => c.Type == JsonChunkType);
            var binChunk = chunks.FirstOrDefault(c => c.Type == 0x004E4942);
            if (jsonChunk == null || binChunk == null)
            {
                return null;
            }

            var json = Encoding.UTF8.GetString(jsonChunk.Data).TrimEnd('\0', ' ', '\t', '\r', '\n');
            var root = MiniJson.Deserialize(json) as Dictionary<string, object>;
            if (root == null || !TryGetRootList(root, "bufferViews", out var bufferViews) || bufferViewIndex >= bufferViews.Count)
            {
                return null;
            }
            var view = bufferViews[bufferViewIndex] as Dictionary<string, object>;
            if (view == null)
            {
                return null;
            }

            var byteOffset = (int)ReadRootFloat(view, "byteOffset", 0);
            var byteLength = (int)ReadRootFloat(view, "byteLength", 0);
            if (byteOffset < 0 || byteLength <= 0 || byteOffset + byteLength > binChunk.Data.Length)
            {
                return null;
            }

            var data = new byte[byteLength];
            Buffer.BlockCopy(binChunk.Data, byteOffset, data, 0, byteLength);
            return data;
        }

        private static string AppendTextureAssetBufferViews(string json, GlbChunk binChunk, List<UnavatarTextureAssetRecord> textureAssets)
        {
            if (textureAssets == null || textureAssets.Count == 0)
            {
                return json;
            }

            var bin = new List<byte>(binChunk.Data ?? Array.Empty<byte>());
            var viewJson = new List<string>();
            foreach (var asset in textureAssets)
            {
                if (asset == null || asset.Bytes == null || asset.Bytes.Length == 0)
                {
                    continue;
                }
                while ((bin.Count & 3) != 0)
                {
                    bin.Add(0);
                }
                var byteOffset = bin.Count;
                bin.AddRange(asset.Bytes);
                while ((bin.Count & 3) != 0)
                {
                    bin.Add(0);
                }
                asset.BufferView = ExistingArrayLength(json, "bufferViews") + viewJson.Count;
                viewJson.Add("{\"buffer\":0,\"byteOffset\":" + byteOffset.ToString(CultureInfo.InvariantCulture) + ",\"byteLength\":" + asset.Bytes.Length.ToString(CultureInfo.InvariantCulture) + "}");
            }
            binChunk.Data = Pad(bin.ToArray(), 0x00);
            if (viewJson.Count == 0)
            {
                return UpdatePrimaryBufferByteLength(json, binChunk.Data.Length);
            }
            json = AppendRootArrayItems(json, "bufferViews", viewJson);
            json = UpdatePrimaryBufferByteLength(json, binChunk.Data.Length);
            return json;
        }

        private static string AppendWardrobePreviewBufferViews(string json, GlbChunk binChunk, List<WardrobePreviewImageDraft> previewImages)
        {
            if (previewImages == null || previewImages.Count == 0)
            {
                return json;
            }

            var bin = new List<byte>(binChunk.Data ?? Array.Empty<byte>());
            var viewJson = new List<string>();
            foreach (var image in previewImages)
            {
                if (image == null || image.pngBytes == null || image.pngBytes.Count == 0)
                {
                    if (image != null)
                    {
                        image.bufferView = -1;
                    }
                    continue;
                }
                while ((bin.Count & 3) != 0)
                {
                    bin.Add(0);
                }
                var byteOffset = bin.Count;
                bin.AddRange(image.pngBytes);
                while ((bin.Count & 3) != 0)
                {
                    bin.Add(0);
                }
                image.bufferView = ExistingArrayLength(json, "bufferViews") + viewJson.Count;
                viewJson.Add("{\"buffer\":0,\"byteOffset\":" + byteOffset.ToString(CultureInfo.InvariantCulture) + ",\"byteLength\":" + image.pngBytes.Count.ToString(CultureInfo.InvariantCulture) + "}");
            }
            binChunk.Data = Pad(bin.ToArray(), 0x00);
            if (viewJson.Count == 0)
            {
                return UpdatePrimaryBufferByteLength(json, binChunk.Data.Length);
            }
            json = AppendRootArrayItems(json, "bufferViews", viewJson);
            json = UpdatePrimaryBufferByteLength(json, binChunk.Data.Length);
            return json;
        }

        private static void PatchWardrobePreviewBufferViewsInPayload(Dictionary<string, object> payload, List<WardrobePreviewImageDraft> previewImages)
        {
            if (payload == null || previewImages == null || previewImages.Count == 0)
            {
                return;
            }
            if (!payload.TryGetValue("wardrobe", out var wardrobeRaw))
            {
                return;
            }
            var wardrobe = wardrobeRaw as Dictionary<string, object>;
            if (wardrobe == null || !wardrobe.TryGetValue("sets", out var setsRaw))
            {
                return;
            }
            var sets = setsRaw as List<object>;
            if (sets == null)
            {
                return;
            }

            var index = 0;
            foreach (var setRaw in sets)
            {
                var set = setRaw as Dictionary<string, object>;
                if (set == null || !set.TryGetValue("previewImages", out var previewsRaw))
                {
                    continue;
                }
                var previews = previewsRaw as List<object>;
                if (previews == null)
                {
                    continue;
                }
                var retainedPreviews = new List<object>();
                foreach (var previewRaw in previews)
                {
                    if (index >= previewImages.Count)
                    {
                        return;
                    }
                    var preview = previewRaw as Dictionary<string, object>;
                    if (preview != null && previewImages[index] != null && previewImages[index].bufferView >= 0)
                    {
                        preview["bufferView"] = previewImages[index].bufferView;
                        retainedPreviews.Add(preview);
                    }
                    index++;
                }
                set["previewImages"] = retainedPreviews;
            }
        }

        private static List<GlbChunk> ReadChunks(byte[] bytes)
        {
            var magic = ReadUInt32(bytes, 0);
            var version = ReadUInt32(bytes, 4);
            if (magic != GlbMagic || version != 2)
            {
                throw new InvalidDataException("Expected GLB version 2.");
            }

            var chunks = new List<GlbChunk>();
            var offset = 12;
            while (offset + 8 <= bytes.Length)
            {
                var length = checked((int)ReadUInt32(bytes, offset));
                var type = ReadUInt32(bytes, offset + 4);
                offset += 8;
                if (offset + length > bytes.Length)
                {
                    throw new InvalidDataException("GLB chunk exceeds file size.");
                }
                var data = new byte[length];
                Buffer.BlockCopy(bytes, offset, data, 0, length);
                chunks.Add(new GlbChunk { Type = type, Data = data });
                offset += length;
            }
            return chunks;
        }

        private static bool TryGetRootList(Dictionary<string, object> map, string key, out List<object> value)
        {
            value = null;
            if (!map.TryGetValue(key, out var raw))
            {
                return false;
            }
            value = raw as List<object>;
            return value != null;
        }

        private static float ReadRootFloat(Dictionary<string, object> map, string key, float fallback)
        {
            if (!map.TryGetValue(key, out var value))
            {
                return fallback;
            }
            if (value is double d)
            {
                return (float)d;
            }
            if (value is float f)
            {
                return f;
            }
            if (value is int i)
            {
                return i;
            }
            return fallback;
        }

        private static int ExistingArrayLength(string json, string propertyName)
        {
            var keyIndex = json.IndexOf("\"" + propertyName + "\"", StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                return 0;
            }
            var colon = json.IndexOf(':', keyIndex);
            var arrayStart = json.IndexOf('[', colon);
            var arrayEnd = FindMatchingBracket(json, arrayStart);
            var inner = json.Substring(arrayStart + 1, arrayEnd - arrayStart - 1).Trim();
            if (inner.Length == 0)
            {
                return 0;
            }
            var count = 1;
            var depth = 0;
            var inString = false;
            var escaped = false;
            for (var i = 0; i < inner.Length; i++)
            {
                var c = inner[i];
                if (inString)
                {
                    if (escaped)
                    {
                        escaped = false;
                    }
                    else if (c == '\\')
                    {
                        escaped = true;
                    }
                    else if (c == '"')
                    {
                        inString = false;
                    }
                    continue;
                }
                if (c == '"')
                {
                    inString = true;
                }
                else if (c == '[' || c == '{')
                {
                    depth++;
                }
                else if (c == ']' || c == '}')
                {
                    depth--;
                }
                else if (c == ',' && depth == 0)
                {
                    count++;
                }
            }
            return count;
        }

        private static string AppendRootArrayItems(string json, string propertyName, List<string> items)
        {
            if (items == null || items.Count == 0)
            {
                return json;
            }
            var keyIndex = json.IndexOf("\"" + propertyName + "\"", StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                return InsertRootProperty(json, "\"" + propertyName + "\":[" + string.Join(",", items) + "]");
            }
            var colon = json.IndexOf(':', keyIndex);
            var arrayStart = json.IndexOf('[', colon);
            var arrayEnd = FindMatchingBracket(json, arrayStart);
            var existing = json.Substring(arrayStart + 1, arrayEnd - arrayStart - 1).Trim();
            var replacement = existing.Length == 0
                ? "[" + string.Join(",", items) + "]"
                : "[" + existing + "," + string.Join(",", items) + "]";
            return json.Substring(0, arrayStart) + replacement + json.Substring(arrayEnd + 1);
        }

        private static string UpdatePrimaryBufferByteLength(string json, int byteLength)
        {
            var buffersIndex = json.IndexOf("\"buffers\"", StringComparison.Ordinal);
            if (buffersIndex < 0)
            {
                return InsertRootProperty(json, "\"buffers\":[{\"byteLength\":" + byteLength.ToString(CultureInfo.InvariantCulture) + "}]");
            }
            var byteLengthIndex = json.IndexOf("\"byteLength\"", buffersIndex, StringComparison.Ordinal);
            if (byteLengthIndex < 0)
            {
                return json;
            }
            var colon = json.IndexOf(':', byteLengthIndex);
            var valueStart = colon + 1;
            while (valueStart < json.Length && char.IsWhiteSpace(json[valueStart]))
            {
                valueStart++;
            }
            var valueEnd = valueStart;
            while (valueEnd < json.Length && char.IsDigit(json[valueEnd]))
            {
                valueEnd++;
            }
            return json.Substring(0, valueStart) + byteLength.ToString(CultureInfo.InvariantCulture) + json.Substring(valueEnd);
        }

        private static string PatchRootJson(string json, string extensionName, Dictionary<string, object> payload)
        {
            json = AddExtensionUsed(json, extensionName);
            var extensionJson = MiniJson.Serialize(payload);
            var property = "\"" + MiniJson.EscapeString(extensionName) + "\":" + extensionJson;
            var extensionsIndex = json.IndexOf("\"extensions\"", StringComparison.Ordinal);
            if (extensionsIndex < 0)
            {
                return InsertRootProperty(json, "\"extensions\":{" + property + "}");
            }

            var colon = json.IndexOf(':', extensionsIndex);
            var objectStart = json.IndexOf('{', colon);
            var objectEnd = FindMatchingBrace(json, objectStart);
            var existing = json.Substring(objectStart + 1, objectEnd - objectStart - 1).Trim();
            var replacement = existing.Length == 0 ? "{" + property + "}" : "{" + existing + "," + property + "}";
            return json.Substring(0, objectStart) + replacement + json.Substring(objectEnd + 1);
        }

        private static string AddExtensionUsed(string json, string extensionName)
        {
            if (json.Contains("\"" + extensionName + "\""))
            {
                return json;
            }

            var keyIndex = json.IndexOf("\"extensionsUsed\"", StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                return InsertRootProperty(json, "\"extensionsUsed\":[\"" + MiniJson.EscapeString(extensionName) + "\"]");
            }

            var colon = json.IndexOf(':', keyIndex);
            var arrayStart = json.IndexOf('[', colon);
            var arrayEnd = FindMatchingBracket(json, arrayStart);
            var existing = json.Substring(arrayStart + 1, arrayEnd - arrayStart - 1).Trim();
            var replacement = existing.Length == 0
                ? "[\"" + MiniJson.EscapeString(extensionName) + "\"]"
                : "[" + existing + ",\"" + MiniJson.EscapeString(extensionName) + "\"]";
            return json.Substring(0, arrayStart) + replacement + json.Substring(arrayEnd + 1);
        }

        private static string InsertRootProperty(string json, string property)
        {
            var end = json.LastIndexOf('}');
            if (end < 0)
            {
                throw new InvalidDataException("GLB JSON root is not an object.");
            }
            var before = json.Substring(0, end).TrimEnd();
            var separator = before.EndsWith("{", StringComparison.Ordinal) ? "" : ",";
            return before + separator + property + json.Substring(end);
        }

        private static int FindMatchingBrace(string text, int openIndex)
        {
            return FindMatching(text, openIndex, '{', '}');
        }

        private static int FindMatchingBracket(string text, int openIndex)
        {
            return FindMatching(text, openIndex, '[', ']');
        }

        private static int FindMatching(string text, int openIndex, char open, char close)
        {
            if (openIndex < 0 || text[openIndex] != open)
            {
                throw new InvalidDataException("JSON delimiter was not found.");
            }
            var depth = 0;
            var inString = false;
            var escaped = false;
            for (var i = openIndex; i < text.Length; i++)
            {
                var c = text[i];
                if (inString)
                {
                    if (escaped)
                    {
                        escaped = false;
                    }
                    else if (c == '\\')
                    {
                        escaped = true;
                    }
                    else if (c == '"')
                    {
                        inString = false;
                    }
                    continue;
                }

                if (c == '"')
                {
                    inString = true;
                }
                else if (c == open)
                {
                    depth++;
                }
                else if (c == close)
                {
                    depth--;
                    if (depth == 0)
                    {
                        return i;
                    }
                }
            }
            throw new InvalidDataException("Matching JSON delimiter was not found.");
        }

        private static void WriteGlb(string path, List<GlbChunk> chunks)
        {
            var totalLength = 12 + chunks.Sum(c => 8 + c.Data.Length);
            using (var stream = File.Create(path))
            using (var writer = new BinaryWriter(stream))
            {
                writer.Write(GlbMagic);
                writer.Write((uint)2);
                writer.Write((uint)totalLength);
                foreach (var chunk in chunks)
                {
                    writer.Write((uint)chunk.Data.Length);
                    writer.Write(chunk.Type);
                    writer.Write(chunk.Data);
                }
            }
        }

        private static byte[] Pad(byte[] data, byte value)
        {
            var paddedLength = (data.Length + 3) & ~3;
            if (paddedLength == data.Length)
            {
                return data;
            }
            var padded = new byte[paddedLength];
            Buffer.BlockCopy(data, 0, padded, 0, data.Length);
            for (var i = data.Length; i < padded.Length; i++)
            {
                padded[i] = value;
            }
            return padded;
        }

        private static uint ReadUInt32(byte[] bytes, int offset)
        {
            return BitConverter.ToUInt32(bytes, offset);
        }

        private sealed class GlbChunk
        {
            public uint Type;
            public byte[] Data;
        }
    }
}

