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
    public sealed partial class UNAvatarExporterWindow
    {
        private List<object> BuildNodeRegistryPayload(GameObject registryRoot = null)
        {
            var nodes = new List<object>();
            var rootObject = registryRoot != null ? registryRoot : avatarRoot;
            if (rootObject == null)
            {
                return nodes;
            }
            foreach (var transform in rootObject.GetComponentsInChildren<Transform>(true))
            {
                nodes.Add(new Dictionary<string, object>
                {
                    ["nodeId"] = WardrobeSnapshotCapture.NodeIdFor(rootObject.transform, transform),
                    ["path"] = VariantExtractor.TransformPath(rootObject.transform, transform),
                    ["name"] = transform.name
                });
            }
            return nodes;
        }

        private static Dictionary<string, object> SnapshotSummary(WardrobeSnapshotDraft snapshot)
        {
            return new Dictionary<string, object>
            {
                ["rootName"] = snapshot.rootName ?? "",
                ["nodeCount"] = snapshot.nodes.Count,
                ["rendererCount"] = snapshot.renderers.Count,
                ["blendShapeCount"] = snapshot.blendShapes.Count
            };
        }
    }
}
