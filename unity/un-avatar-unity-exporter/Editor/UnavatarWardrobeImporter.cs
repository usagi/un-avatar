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
    internal sealed class ImportedWardrobeDraft
    {
        public bool hasBaseOperations;
        public List<WardrobeOperationDraft> baseOperations = new List<WardrobeOperationDraft>();
        public List<WardrobeSetDraft> sets = new List<WardrobeSetDraft>();
        public List<WardrobePreviewImageDraft> basePreviewImages = new List<WardrobePreviewImageDraft>();
        public List<string> importedSetIds = new List<string>();
    }

    internal static class UnavatarWardrobeImporter
    {
        public static ImportedWardrobeDraft Import(string path)
        {
            var json = GlbExtensionPatcher.ReadRootJson(path);
            var extensionJson = GlbExtensionPatcher.ExtractRootExtensionJson(json, "UN_avatar");
            var extension = MiniJson.Deserialize(extensionJson) as Dictionary<string, object>;
            if (extension == null || !TryGetMap(extension, "wardrobe", out var wardrobe))
            {
                throw new InvalidDataException("UN_avatar.wardrobe was not found.");
            }

            var result = new ImportedWardrobeDraft();
            if (!TryGetList(wardrobe, "sets", out var sets))
            {
                return result;
            }
            var baseSetId = ReadString(wardrobe, "baseSet", "base");

            foreach (var item in sets)
            {
                var map = item as Dictionary<string, object>;
                if (map == null)
                {
                    continue;
                }

                var set = ReadSet(map, path);
                result.importedSetIds.Add(string.IsNullOrEmpty(set.id) ? "<empty>" : set.id);
                if (string.Equals(set.id, baseSetId, StringComparison.Ordinal) ||
                    string.Equals(set.id, "base", StringComparison.OrdinalIgnoreCase) ||
                    string.Equals(set.displayName, "base", StringComparison.OrdinalIgnoreCase) ||
                    ReadBool(map, "default", false))
                {
                    result.hasBaseOperations = true;
                    result.baseOperations = set.operations;
                    result.basePreviewImages = set.previewImages;
                    continue;
                }
                result.sets.Add(set);
            }

            return result;
        }

        private static WardrobeSetDraft ReadSet(Dictionary<string, object> map, string glbPath)
        {
            var set = new WardrobeSetDraft
            {
                id = ReadString(map, "id", ""),
                displayName = ReadString(map, "displayName", ReadString(map, "name", "Imported Set")),
                source = "imported_unavatar"
            };

            if (TryGetList(map, "assetGroups", out var assetGroups))
            {
                foreach (var group in assetGroups)
                {
                    var text = group as string;
                    if (!string.IsNullOrWhiteSpace(text))
                    {
                        set.assetGroups.Add(text);
                    }
                }
            }

            if (TryGetList(map, "operations", out var operations))
            {
                foreach (var item in operations)
                {
                    var opMap = item as Dictionary<string, object>;
                    if (opMap == null)
                    {
                        continue;
                    }
                    set.operations.Add(ReadOperation(opMap));
                }
            }

            if (TryGetList(map, "previewImages", out var previewImages))
            {
                foreach (var item in previewImages)
                {
                    var imageMap = item as Dictionary<string, object>;
                    if (imageMap == null)
                    {
                        continue;
                    }
                    set.previewImages.Add(ReadPreviewImage(imageMap, glbPath));
                }
            }

            return set;
        }

        private static WardrobePreviewImageDraft ReadPreviewImage(Dictionary<string, object> map, string glbPath)
        {
            var image = new WardrobePreviewImageDraft
            {
                id = ReadString(map, "id", ""),
                view = ReadString(map, "view", ""),
                width = (int)ReadFloat(map, "width", 0),
                height = (int)ReadFloat(map, "height", 0),
                mimeType = ReadString(map, "mimeType", "image/png"),
                pixelFormat = ReadString(map, "pixelFormat", "RGBA8"),
                colorSpace = ReadString(map, "colorSpace", "sRGB"),
                bufferView = (int)ReadFloat(map, "bufferView", -1)
            };
            if (TryGetMap(map, "camera", out var camera))
            {
                image.fovYDegrees = ReadFloat(camera, "fovYDegrees", 0.0f);
                image.nearClip = ReadFloat(camera, "nearClip", 0.0f);
                image.farClip = ReadFloat(camera, "farClip", 0.0f);
                image.cameraPosition = ReadVector3(camera, "position");
                image.cameraRotationEuler = ReadVector3(camera, "rotationEulerDegrees");
                image.target = ReadVector3(camera, "target");
            }
            if (TryGetMap(map, "render", out var render))
            {
                image.renderMode = ReadString(render, "mode", image.renderMode);
                image.antiAliasingSamples = (int)ReadFloat(render, "antiAliasingSamples", image.antiAliasingSamples);
            }
            if (image.bufferView >= 0)
            {
                var bytes = GlbExtensionPatcher.ReadBufferViewBytes(glbPath, image.bufferView);
                image.pngBytes = bytes != null ? bytes.ToList() : new List<byte>();
            }
            return image;
        }

        private static WardrobeOperationDraft ReadOperation(Dictionary<string, object> map)
        {
            var operation = new WardrobeOperationDraft
            {
                type = ReadString(map, "type", ReadString(map, "op", "")),
                name = ReadString(map, "name", "")
            };
            if (TryGetMap(map, "target", out var target))
            {
                operation.target = new WardrobeTargetDraft
                {
                    nodeId = ReadString(target, "nodeId", ""),
                    path = ReadString(target, "path", "")
                };
            }
            else
            {
                operation.target = new WardrobeTargetDraft
                {
                    nodeId = ReadString(map, "nodeId", ""),
                    path = ReadString(map, "path", "")
                };
            }
            operation.boolValue = ReadBool(map, "visible", ReadBool(map, "enabled", false));
            operation.floatValue = ReadFloat(map, "value", 0.0f);
            return operation;
        }

        private static bool TryGetMap(Dictionary<string, object> map, string key, out Dictionary<string, object> value)
        {
            value = null;
            if (!map.TryGetValue(key, out var raw))
            {
                return false;
            }
            value = raw as Dictionary<string, object>;
            return value != null;
        }

        private static bool TryGetList(Dictionary<string, object> map, string key, out List<object> value)
        {
            value = null;
            if (!map.TryGetValue(key, out var raw))
            {
                return false;
            }
            value = raw as List<object>;
            return value != null;
        }

        private static string ReadString(Dictionary<string, object> map, string key, string fallback)
        {
            return map.TryGetValue(key, out var value) && value is string text ? text : fallback;
        }

        private static bool ReadBool(Dictionary<string, object> map, string key, bool fallback)
        {
            return map.TryGetValue(key, out var value) && value is bool b ? b : fallback;
        }

        private static float ReadFloat(Dictionary<string, object> map, string key, float fallback)
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

        private static Vector3 ReadVector3(Dictionary<string, object> map, string key)
        {
            if (!TryGetList(map, key, out var list) || list.Count < 3)
            {
                return Vector3.zero;
            }
            return new Vector3(ReadListFloat(list, 0), ReadListFloat(list, 1), ReadListFloat(list, 2));
        }

        private static float ReadListFloat(List<object> list, int index)
        {
            var value = list[index];
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
            return 0.0f;
        }
    }
}

