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
    internal enum UNAvatarExportMode
    {
        [InspectorName("Current Only")]
        CurrentOnly = 1,
        [InspectorName("Wardrobe (Baked)")]
        WardrobeBaked = 0,
        [InspectorName("Wardrobe (Split)")]
        WardrobeSplit = 2
    }

    [Serializable]
    internal sealed class WardrobeTargetDraft
    {
        public string nodeId;
        public string path;

        public Dictionary<string, object> ToJson()
        {
            return new Dictionary<string, object>
            {
                ["nodeId"] = nodeId ?? "",
                ["path"] = path ?? ""
            };
        }
    }

    [Serializable]
    internal sealed class WardrobeOperationDraft
    {
        public string type;
        public WardrobeTargetDraft target = new WardrobeTargetDraft();
        public string name;
        public bool boolValue;
        public float floatValue;

        public Dictionary<string, object> ToJson()
        {
            var json = new Dictionary<string, object>
            {
                ["type"] = NormalizeOperationType(type),
                ["target"] = target != null ? target.ToJson() : new Dictionary<string, object>()
            };
            if (!string.IsNullOrEmpty(name))
            {
                json["name"] = name;
            }
            if (type == "blendShapeWeight")
            {
                json["value"] = floatValue;
            }
            else
            {
                json["visible"] = boolValue;
            }
            return json;
        }

        private static string NormalizeOperationType(string value)
        {
            switch (value)
            {
                case "subtreeVisibility":
                    return "subtreeEnabled";
                case "nodeVisibility":
                    return "nodeEnabled";
                case "rendererVisibility":
                    return "rendererEnabled";
                default:
                    return value ?? "";
            }
        }
    }

    [Serializable]
    internal sealed class WardrobeSetDraft
    {
        public string id;
        public string displayName;
        public string source = "unity_capture_diff";
        public List<string> assetGroups = new List<string>();
        public List<WardrobeOperationDraft> operations = new List<WardrobeOperationDraft>();
        public List<WardrobePreviewImageDraft> previewImages = new List<WardrobePreviewImageDraft>();
        public WardrobeSnapshotDraft capturedSnapshot;

        public Dictionary<string, object> ToJson(bool isDefault)
        {
            var json = new Dictionary<string, object>
            {
                ["id"] = id ?? "",
                ["displayName"] = displayName ?? "",
                ["source"] = source ?? "",
                ["default"] = isDefault,
                ["assetGroups"] = StringsToObjectList(assetGroups),
                ["operations"] = OperationsToJson(operations),
                ["previewImages"] = PreviewImagesToJson(previewImages)
            };
            return json;
        }

        private static List<object> StringsToObjectList(List<string> values)
        {
            var json = new List<object>(values != null ? values.Count : 0);
            if (values == null)
            {
                return json;
            }
            foreach (var value in values)
            {
                json.Add(value);
            }
            return json;
        }

        private static List<object> OperationsToJson(List<WardrobeOperationDraft> values)
        {
            var json = new List<object>(values != null ? values.Count : 0);
            if (values == null)
            {
                return json;
            }
            foreach (var value in values)
            {
                if (value != null)
                {
                    json.Add(value.ToJson());
                }
            }
            return json;
        }

        private static List<object> PreviewImagesToJson(List<WardrobePreviewImageDraft> values)
        {
            var json = new List<object>(values != null ? values.Count : 0);
            if (values == null)
            {
                return json;
            }
            foreach (var value in values)
            {
                if (value != null)
                {
                    json.Add(value.ToJson());
                }
            }
            return json;
        }
    }

    [Serializable]
    internal sealed class WardrobePreviewImageDraft
    {
        public string id;
        public string view;
        public int width;
        public int height;
        public string mimeType = "image/png";
        public string pixelFormat = "RGBA8";
        public string colorSpace = "sRGB";
        public string renderMode = "standard";
        public int antiAliasingSamples = 1;
        public string stateDigest = "";
        public List<string> stateDetails = new List<string>();
        public float fovYDegrees;
        public float nearClip;
        public float farClip;
        public Vector3 cameraPosition;
        public Vector3 cameraRotationEuler;
        public Vector3 target;
        public int bufferView = -1;
        public byte[] pngBytes = Array.Empty<byte>();

        public Dictionary<string, object> ToJson()
        {
            var json = new Dictionary<string, object>
            {
                ["id"] = id ?? "",
                ["view"] = view ?? "",
                ["width"] = width,
                ["height"] = height,
                ["mimeType"] = mimeType ?? "image/png",
                ["pixelFormat"] = pixelFormat ?? "RGBA8",
                ["colorSpace"] = colorSpace ?? "sRGB",
                ["render"] = new Dictionary<string, object>
                {
                    ["mode"] = renderMode ?? "standard",
                    ["antiAliasingSamples"] = antiAliasingSamples,
                    ["stateDigest"] = stateDigest ?? "",
                    ["stateDetails"] = StringsToObjectList(stateDetails)
                },
                ["camera"] = new Dictionary<string, object>
                {
                    ["projection"] = "perspective",
                    ["fovYDegrees"] = fovYDegrees,
                    ["nearClip"] = nearClip,
                    ["farClip"] = farClip,
                    ["position"] = FloatArray(cameraPosition),
                    ["rotationEulerDegrees"] = FloatArray(cameraRotationEuler),
                    ["target"] = FloatArray(target)
                }
            };
            if (bufferView >= 0)
            {
                json["bufferView"] = bufferView;
            }
            return json;
        }

        private static List<object> FloatArray(Vector3 value)
        {
            return new List<object> { value.x, value.y, value.z };
        }

        private static List<object> StringsToObjectList(List<string> values)
        {
            var json = new List<object>(values != null ? values.Count : 0);
            if (values == null)
            {
                return json;
            }
            foreach (var value in values)
            {
                json.Add(value);
            }
            return json;
        }
    }

    [Serializable]
    internal sealed class WardrobeSnapshotDraft
    {
        public string rootName;
        public List<NodeStateDraft> nodes = new List<NodeStateDraft>();
        public List<RendererStateDraft> renderers = new List<RendererStateDraft>();
        public List<BlendShapeStateDraft> blendShapes = new List<BlendShapeStateDraft>();
    }

    internal sealed class UnavatarRendererAssetRecord
    {
        public string nodeId = "";
        public string path = "";
        public int mesh = -1;
        public List<int> materials = new List<int>();
        public List<int> images = new List<int>();

        public Dictionary<string, object> ToJson()
        {
            return new Dictionary<string, object>
            {
                ["nodeId"] = nodeId ?? "",
                ["path"] = path ?? "",
                ["mesh"] = mesh,
                ["materials"] = IntsToObjectList(materials),
                ["images"] = IntsToObjectList(images)
            };
        }

        private static List<object> IntsToObjectList(List<int> values)
        {
            var json = new List<object>(values != null ? values.Count : 0);
            if (values == null)
            {
                return json;
            }
            foreach (var value in values)
            {
                json.Add(value);
            }
            return json;
        }
    }

    internal sealed class WardrobeApplyReport
    {
        public int Total;
        public int Matched;
        public int Missing;
        public int VisibilityChanged;
        public int RendererChanged;
        public int BlendShapeChanged;
        public List<string> MissingTargets = new List<string>();

        public string ToSummary()
        {
            var summary = $"ops={Total}, matched={Matched}, missing={Missing}, active={VisibilityChanged}, renderer={RendererChanged}, blendshape={BlendShapeChanged}.";
            if (MissingTargets.Count > 0)
            {
                var count = Math.Min(8, MissingTargets.Count);
                var targets = new string[count];
                for (var i = 0; i < count; i++)
                {
                    targets[i] = MissingTargets[i];
                }
                summary += " Missing: " + string.Join(", ", targets);
            }
            return summary;
        }
    }

    [Serializable]
    internal sealed class NodeStateDraft
    {
        public string nodeId;
        public string path;
        public bool activeSelf;
        public bool visible;
    }

    [Serializable]
    internal sealed class RendererStateDraft
    {
        public string nodeId;
        public string path;
        public bool enabled;
    }

    [Serializable]
    internal sealed class BlendShapeStateDraft
    {
        public string nodeId;
        public string path;
        public string name;
        public float weight;
    }

    [Serializable]
    internal sealed class WardrobeCaptureSessionDraft
    {
        public string schema = "network.usagi.un-avatar.unity-exporter.wardrobe-capture";
        public string schemaVersion = "0.1-preview";
        public string avatarRootName;
        public string setName;
        public bool hasBaseSnapshot;
        public WardrobeSnapshotDraft baseSnapshot = new WardrobeSnapshotDraft();
        public List<WardrobePreviewImageDraft> basePreviewImages = new List<WardrobePreviewImageDraft>();
        public List<WardrobeSetDraft> sets = new List<WardrobeSetDraft>();
    }

    internal sealed class TextureDiagnostic
    {
        public string Name;
        public string AssetPath;
        public string Extension;
        public long ByteLength;
    }

    internal sealed class ExportedTextureRecord
    {
        public string Name;
        public string AssetPath;
        public string SourceExtension;
        public string SourceMimeType;
        public long SourceByteLength;
        public string OutputMimeType;
        public int OutputByteLength;
        public string ExportMode;
        public string FallbackReason;

        public Dictionary<string, object> ToJson()
        {
            return new Dictionary<string, object>
            {
                ["name"] = Name ?? "",
                ["assetPath"] = AssetPath ?? "",
                ["sourceExtension"] = SourceExtension ?? "",
                ["sourceMimeType"] = SourceMimeType ?? "",
                ["sourceByteLength"] = SourceByteLength,
                ["outputMimeType"] = OutputMimeType ?? "",
                ["outputByteLength"] = OutputByteLength,
                ["exportMode"] = ExportMode ?? "",
                ["fallbackReason"] = FallbackReason ?? ""
            };
        }
    }

    internal sealed class UnavatarTextureAssetRecord
    {
        public string Id;
        public string Name;
        public string AssetPath;
        public string MimeType;
        public string SourceExtension;
        public string SourcePixelFormat;
        public string ColorSpace;
        public string Channels;
        public string TextureType;
        public string TextureShape;
        public string SourceLayout;
        public string UnityGenerateCubemap;
        public bool? SRgb;
        public Dictionary<string, object> Sampler;
        public int Width;
        public int Height;
        public byte[] Bytes;
        public int BufferView = -1;

        public Dictionary<string, object> ToJson()
        {
            var json = new Dictionary<string, object>
            {
                ["id"] = Id ?? "",
                ["name"] = Name ?? "",
                ["assetPath"] = AssetPath ?? "",
                ["mimeType"] = MimeType ?? "",
                ["sourceExtension"] = SourceExtension ?? "",
                ["sourcePixelFormat"] = SourcePixelFormat ?? "",
                ["colorSpace"] = ColorSpace ?? "linear",
                ["channels"] = Channels ?? "",
                ["byteLength"] = Bytes != null ? Bytes.Length : 0
            };
            if (!string.IsNullOrEmpty(TextureType))
            {
                json["textureType"] = TextureType;
            }
            if (!string.IsNullOrEmpty(TextureShape))
            {
                json["textureShape"] = TextureShape;
            }
            if (!string.IsNullOrEmpty(SourceLayout))
            {
                json["sourceLayout"] = SourceLayout;
            }
            if (!string.IsNullOrEmpty(UnityGenerateCubemap))
            {
                json["unityGenerateCubemap"] = UnityGenerateCubemap;
            }
            if (SRgb.HasValue)
            {
                json["sRGB"] = SRgb.Value;
            }
            if (Sampler != null)
            {
                json["sampler"] = Sampler;
            }
            if (Width > 0)
            {
                json["width"] = Width;
            }
            if (Height > 0)
            {
                json["height"] = Height;
            }
            if (BufferView >= 0)
            {
                json["bufferView"] = BufferView;
            }
            return json;
        }
    }
}

