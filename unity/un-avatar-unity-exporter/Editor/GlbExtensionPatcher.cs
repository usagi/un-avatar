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
    internal static partial class GlbExtensionPatcher
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
            var chunks = ReadChunks(bytes);

            var jsonChunk = FindChunk(chunks, JsonChunkType);
            if (jsonChunk == null)
            {
                throw new InvalidDataException("GLB JSON chunk was not found.");
            }

            var binChunk = FindChunk(chunks, 0x004E4942);
            if (binChunk == null)
            {
                binChunk = new GlbChunk { Type = 0x004E4942, Data = Array.Empty<byte>() };
                chunks.Add(binChunk);
            }

            var json = Encoding.UTF8.GetString(jsonChunk.Data).TrimEnd('\0', ' ', '\t', '\r', '\n');
            if (textureAssets != null && textureAssets.Count > 0)
            {
                json = AppendTextureAssetBufferViews(json, binChunk, textureAssets);
                payload["textureAssets"] = TextureAssetsToJson(textureAssets);
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

        private static GlbChunk FindChunk(List<GlbChunk> chunks, uint type)
        {
            foreach (var chunk in chunks)
            {
                if (chunk.Type == type)
                {
                    return chunk;
                }
            }
            return null;
        }

        private static List<object> TextureAssetsToJson(List<UnavatarTextureAssetRecord> textureAssets)
        {
            var json = new List<object>(textureAssets.Count);
            foreach (var asset in textureAssets)
            {
                json.Add(asset.ToJson());
            }
            return json;
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
            var jsonChunk = FindChunk(chunks, JsonChunkType);
            var binChunk = FindChunk(chunks, 0x004E4942);
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
    }
}
