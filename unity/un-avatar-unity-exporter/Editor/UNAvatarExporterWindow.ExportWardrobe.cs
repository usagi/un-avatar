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
    public sealed partial class UNAvatarExporterWindow
    {
        private List<WardrobeSetDraft> BuildBakedWardrobeSets(WardrobeSnapshotDraft bakedBaseSnapshot, bool bakeWithModularAvatar)
        {
            var sets = new List<WardrobeSetDraft>();
            if (avatarRoot == null || capturedWardrobeSets.Count == 0 || bakedBaseSnapshot == null || bakedBaseSnapshot.nodes.Count == 0)
            {
                return sets;
            }

            for (var i = 0; i < capturedWardrobeSets.Count; i++)
            {
                var source = capturedWardrobeSets[i];
                GameObject setClone = null;
                try
                {
                    EditorUtility.DisplayProgressBar(
                        "U.N. Avatar Export",
                        $"Baking wardrobe set {i + 1}/{capturedWardrobeSets.Count}: {source.displayName}",
                        0.32f + 0.18f * ((float)i / Math.Max(1, capturedWardrobeSets.Count)));
                    setClone = Instantiate(avatarRoot);
                    setClone.name = avatarRoot.name + " (UNAvatar Wardrobe " + source.id + ")";
                    setClone.hideFlags = HideFlags.HideAndDontSave;
                    setClone.SetActive(true);
                    if (forceIncludeInactiveObjects && exportMode != UNAvatarExportMode.CurrentOnly)
                    {
                        SetActiveRecursive(setClone.transform, true);
                    }
                    ApplyWardrobeOperationsToRoot(setClone, CurrentBaseOperations());
                    ApplyWardrobeOperationsToRoot(setClone, source.operations);
                    if (bakeWithModularAvatar)
                    {
                        if (!ModularAvatarBridge.TryBake(setClone, out var bakeError))
                        {
                            Debug.LogWarning("[U.N. Avatar] Modular Avatar bake failed for wardrobe set " + source.displayName + ": " + bakeError);
                        }
                    }
                    var snapshot = WardrobeSnapshotCapture.Capture(setClone);
                    var baked = WardrobeSnapshotCapture.Diff(bakedBaseSnapshot, snapshot, source.displayName);
                    baked.id = source.id;
                    baked.displayName = source.displayName;
                    baked.source = source.source + "_baked";
                    baked.capturedSnapshot = snapshot;
                    baked.previewImages = ClonePreviewImages(source.previewImages);
                    sets.Add(baked);
                }
                finally
                {
                    if (setClone != null)
                    {
                        DestroyImmediate(setClone);
                    }
                }
            }
            return sets;
        }

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
