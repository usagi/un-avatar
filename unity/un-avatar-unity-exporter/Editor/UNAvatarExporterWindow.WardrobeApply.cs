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
        private void RebaseWardrobeSetsFromSnapshots()
        {
            if (!hasBaseSnapshot)
            {
                lastSummary = "Capture Base is missing.";
                return;
            }

            var rebased = 0;
            var skipped = 0;
            for (var i = 0; i < capturedWardrobeSets.Count; i++)
            {
                var set = capturedWardrobeSets[i];
                if (set.capturedSnapshot == null || set.capturedSnapshot.nodes.Count == 0)
                {
                    skipped++;
                    continue;
                }
                var next = WardrobeSnapshotCapture.Diff(baseSnapshot, set.capturedSnapshot, set.displayName);
                next.id = set.id;
                next.displayName = set.displayName;
                next.source = set.source + "_rebased";
                next.capturedSnapshot = set.capturedSnapshot;
                next.previewImages = ClonePreviewImages(set.previewImages);
                capturedWardrobeSets[i] = next;
                rebased++;
            }

            lastSummary = $"Rebased wardrobe sets: {rebased}. Skipped sets without snapshots: {skipped}.";
        }

        private void ApplyBaseToScene()
        {
            if (!EnsureCanApplyWardrobe())
            {
                return;
            }

            lastSummary = ApplyBaseStateToRoot(avatarRoot) + " to scene.";
            selectedWardrobeSetIndex = BaseSelectionIndex;
            SceneView.RepaintAll();
        }

        private void ApplySelectedWardrobeSetToScene()
        {
            if (!EnsureCanApplyWardrobe())
            {
                return;
            }
            if (selectedWardrobeSetIndex < 0 || selectedWardrobeSetIndex >= capturedWardrobeSets.Count)
            {
                lastSummary = "No wardrobe set is selected.";
                return;
            }

            var set = capturedWardrobeSets[selectedWardrobeSetIndex];
            lastSummary = ApplyWardrobeSetStateToRoot(avatarRoot, set) + " to scene.";
            SceneView.RepaintAll();
        }

        private string ApplyBaseStateToRoot(GameObject root)
        {
            if (root == null)
            {
                return "Avatar root is missing.";
            }

            if (hasBaseSnapshot && baseSnapshot != null && baseSnapshot.nodes.Count > 0)
            {
                WardrobeSnapshotCapture.ApplyToRoot(root, baseSnapshot);
                return "Applied base wardrobe snapshot";
            }

            var report = ApplyWardrobeOperationsToRoot(root, CurrentBaseOperationsForSceneApply());
            return "Applied base wardrobe state. " + report.ToSummary();
        }

        private string ApplyWardrobeSetStateToRoot(GameObject root, WardrobeSetDraft set)
        {
            if (root == null)
            {
                return "Avatar root is missing.";
            }
            if (set == null)
            {
                return "No wardrobe set is selected.";
            }

            if (set.capturedSnapshot != null && set.capturedSnapshot.nodes.Count > 0)
            {
                WardrobeSnapshotCapture.ApplyToRoot(root, set.capturedSnapshot);
                return "Applied wardrobe set snapshot `" + set.displayName + "`";
            }

            var baseSummary = ApplyBaseStateToRoot(root);
            var setReport = ApplyWardrobeOperationsToRoot(root, set.operations);
            return "Applied wardrobe set `" + set.displayName + "`. Base: " + baseSummary + " Set: " + setReport.ToSummary();
        }

        private List<WardrobeOperationDraft> CurrentBaseOperations()
        {
            if (hasBaseSnapshot)
            {
                return WardrobeSnapshotCapture.BaseOperations(baseSnapshot);
            }
            if (!hasImportedBaseOperations)
            {
                return new List<WardrobeOperationDraft>();
            }

            var operations = new List<WardrobeOperationDraft>(importedBaseOperations.Count);
            foreach (var operation in importedBaseOperations)
            {
                if (operation != null)
                {
                    operations.Add(WardrobeSnapshotCapture.CloneOperation(operation));
                }
            }
            return operations;
        }

        private List<WardrobeOperationDraft> CurrentBaseOperationsForSceneApply()
        {
            return WardrobeSnapshotCapture.FilterInheritedHiddenOperations(CurrentBaseOperations());
        }

        private string BaseStatusText()
        {
            if (hasBaseSnapshot)
            {
                return $"{baseSnapshot.nodes.Count} nodes, {baseSnapshot.blendShapes.Count} blendshapes";
            }
            if (hasImportedBaseOperations)
            {
                return $"imported: {importedBaseOperations.Count} ops";
            }
            return "not captured";
        }

        private bool EnsureCanApplyWardrobe()
        {
            if (avatarRoot == null)
            {
                lastSummary = "Avatar Root is missing.";
                return false;
            }
            if (!hasBaseSnapshot && !hasImportedBaseOperations)
            {
                lastSummary = "Capture Base or imported Base operations are missing. Re-import the .unavatar; if this persists, the importer did not recognize the base set.";
                return false;
            }
            return true;
        }

        private bool EnsureBaseCanBeApplied(bool updateSummary)
        {
            if (avatarRoot == null)
            {
                if (updateSummary)
                {
                    lastSummary = "Avatar Root is missing.";
                }
                return false;
            }
            if (!hasBaseSnapshot && !hasImportedBaseOperations)
            {
                if (updateSummary)
                {
                    lastSummary = "Capture Base or imported Base operations are missing. Re-import the .unavatar; if this persists, the importer did not recognize the base set.";
                }
                return false;
            }
            return true;
        }

        private WardrobeApplyReport ApplyWardrobeOperations(IEnumerable<WardrobeOperationDraft> operations)
        {
            return ApplyWardrobeOperationsToRoot(avatarRoot, operations);
        }

        private static WardrobeApplyReport ApplyWardrobeOperationsToRoot(GameObject root, IEnumerable<WardrobeOperationDraft> operations)
        {
            var report = new WardrobeApplyReport();
            if (root == null || operations == null)
            {
                return report;
            }

            var nodes = BuildWardrobeApplyLookup(root);

            foreach (var operation in operations)
            {
                if (operation == null || operation.target == null)
                {
                    continue;
                }
                report.Total++;
                var transform = default(Transform);
                var node = default(WardrobeApplyNode);
                if (!string.IsNullOrEmpty(operation.target.nodeId))
                {
                    nodes.ById.TryGetValue(operation.target.nodeId, out node);
                }
                if (node == null && !string.IsNullOrEmpty(operation.target.path))
                {
                    nodes.ByPath.TryGetValue(operation.target.path, out node);
                }
                if (node == null && !string.IsNullOrEmpty(operation.target.path))
                {
                    node = ResolveNodeByPathSuffix(nodes.ByNormalizedPath, operation.target.path);
                }
                transform = node != null ? node.Transform : null;
                if (transform == null)
                {
                    report.Missing++;
                    if (report.MissingTargets.Count < 16)
                    {
                        report.MissingTargets.Add(TargetDebugName(operation));
                    }
                    continue;
                }
                report.Matched++;

                if (operation.type == "subtreeEnabled" || operation.type == "subtreeVisibility" || operation.type == "nodeEnabled" || operation.type == "nodeVisibility")
                {
                    if (transform.gameObject.activeSelf != operation.boolValue)
                    {
                        report.VisibilityChanged++;
                    }
                    if (operation.boolValue)
                    {
                        transform.gameObject.SetActive(true);
                    }
                    else
                    {
                        transform.gameObject.SetActive(false);
                    }
                }
                else if (operation.type == "rendererEnabled" || operation.type == "rendererVisibility")
                {
                    foreach (var renderer in node.Renderers)
                    {
                        if (renderer.enabled != operation.boolValue)
                        {
                            report.RendererChanged++;
                        }
                        renderer.enabled = operation.boolValue;
                    }
                }
                else if (operation.type == "blendShapeWeight" && !string.IsNullOrEmpty(operation.name))
                {
                    foreach (var skinned in node.SkinnedRenderers)
                    {
                        var mesh = skinned.sharedMesh;
                        var index = mesh != null ? mesh.GetBlendShapeIndex(operation.name) : -1;
                        if (index >= 0)
                        {
                            if (Math.Abs(skinned.GetBlendShapeWeight(index) - operation.floatValue) > 0.001f)
                            {
                                report.BlendShapeChanged++;
                            }
                            skinned.SetBlendShapeWeight(index, operation.floatValue);
                        }
                    }
                }
            }
            return report;
        }

        private static WardrobeApplyLookup BuildWardrobeApplyLookup(GameObject root)
        {
            var lookup = new WardrobeApplyLookup();
            foreach (var transform in root.GetComponentsInChildren<Transform>(true))
            {
                var node = new WardrobeApplyNode
                {
                    Transform = transform,
                    Renderers = transform.GetComponents<Renderer>(),
                    SkinnedRenderers = transform.GetComponents<SkinnedMeshRenderer>()
                };
                var nodeId = WardrobeSnapshotCapture.NodeIdFor(root.transform, transform);
                if (!lookup.ById.ContainsKey(nodeId))
                {
                    lookup.ById[nodeId] = node;
                }
                var path = VariantExtractor.TransformPath(root.transform, transform);
                if (!lookup.ByPath.ContainsKey(path))
                {
                    lookup.ByPath[path] = node;
                }
                var normalizedPath = WardrobeSnapshotCapture.NormalizePath(path);
                if (!lookup.ByNormalizedPath.ContainsKey(normalizedPath))
                {
                    lookup.ByNormalizedPath[normalizedPath] = node;
                }
            }
            return lookup;
        }

        private sealed class WardrobeApplyLookup
        {
            public readonly Dictionary<string, WardrobeApplyNode> ById = new Dictionary<string, WardrobeApplyNode>(StringComparer.Ordinal);
            public readonly Dictionary<string, WardrobeApplyNode> ByPath = new Dictionary<string, WardrobeApplyNode>(StringComparer.Ordinal);
            public readonly Dictionary<string, WardrobeApplyNode> ByNormalizedPath = new Dictionary<string, WardrobeApplyNode>(StringComparer.Ordinal);
        }

        private sealed class WardrobeApplyNode
        {
            public Transform Transform;
            public Renderer[] Renderers;
            public SkinnedMeshRenderer[] SkinnedRenderers;
        }

        private static WardrobeApplyNode ResolveNodeByPathSuffix(Dictionary<string, WardrobeApplyNode> nodesByNormalizedPath, string importedPath)
        {
            var path = WardrobeSnapshotCapture.NormalizePath(importedPath);
            if (string.IsNullOrEmpty(path))
            {
                return null;
            }
            if (nodesByNormalizedPath.TryGetValue(path, out var exact))
            {
                return exact;
            }

            var suffix = "/" + path;
            var match = default(WardrobeApplyNode);
            foreach (var entry in nodesByNormalizedPath)
            {
                if (!entry.Key.EndsWith(suffix, StringComparison.Ordinal))
                {
                    continue;
                }
                if (match != null)
                {
                    return null;
                }
                match = entry.Value;
            }
            return match;
        }

        private static string TargetDebugName(WardrobeOperationDraft operation)
        {
            var path = operation.target != null ? operation.target.path : "";
            var nodeId = operation.target != null ? operation.target.nodeId : "";
            if (!string.IsNullOrEmpty(path))
            {
                return operation.type + ":" + path;
            }
            if (!string.IsNullOrEmpty(nodeId))
            {
                return operation.type + ":" + nodeId;
            }
            return operation.type ?? "<unknown>";
        }
    }
}
