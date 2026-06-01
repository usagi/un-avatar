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
    internal static partial class GlbExtensionPatcher
    {
        private static string AppendTextureAssetBufferViews(string json, GlbChunk binChunk, List<UnavatarTextureAssetRecord> textureAssets)
        {
            if (textureAssets == null || textureAssets.Count == 0)
            {
                return json;
            }

            var bin = new List<byte>(binChunk.Data ?? Array.Empty<byte>());
            var viewJson = new List<string>();
            var firstBufferViewIndex = ExistingArrayLength(json, "bufferViews");
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
                asset.BufferView = firstBufferViewIndex + viewJson.Count;
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
            var firstBufferViewIndex = ExistingArrayLength(json, "bufferViews");
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
                image.bufferView = firstBufferViewIndex + viewJson.Count;
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
    }
}
