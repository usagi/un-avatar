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
    internal static class WardrobeSnapshotCapture
    {
        private const float BlendShapeEpsilon = 0.001f;

        public static WardrobeSnapshotDraft Capture(GameObject root)
        {
            var snapshot = new WardrobeSnapshotDraft { rootName = root.name };
            foreach (var transform in root.GetComponentsInChildren<Transform>(true))
            {
                var path = VariantExtractor.TransformPath(root.transform, transform);
                snapshot.nodes.Add(new NodeStateDraft
                {
                    nodeId = NodeIdFor(root.transform, transform),
                    path = path,
                    activeSelf = transform.gameObject.activeSelf,
                    visible = transform.gameObject.activeSelf
                });

                foreach (var renderer in transform.GetComponents<Renderer>())
                {
                    snapshot.renderers.Add(new RendererStateDraft
                    {
                        nodeId = NodeIdFor(root.transform, transform),
                        path = path,
                        enabled = renderer.enabled
                    });
                }

                foreach (var skinned in transform.GetComponents<SkinnedMeshRenderer>())
                {
                    var mesh = skinned.sharedMesh;
                    if (mesh == null)
                    {
                        continue;
                    }
                    for (var i = 0; i < mesh.blendShapeCount; i++)
                    {
                        snapshot.blendShapes.Add(new BlendShapeStateDraft
                        {
                            nodeId = NodeIdFor(root.transform, transform),
                            path = path,
                            name = mesh.GetBlendShapeName(i),
                            weight = skinned.GetBlendShapeWeight(i)
                        });
                    }
                }
            }
            return snapshot;
        }

        public static void ApplyToRoot(GameObject root, WardrobeSnapshotDraft snapshot)
        {
            if (root == null || snapshot == null)
            {
                return;
            }

            var nodes = BuildNodeLookup(root);

            foreach (var node in snapshot.nodes)
            {
                var target = ResolveNode(nodes, node.nodeId, node.path);
                if (target != null)
                {
                    target.Transform.gameObject.SetActive(node.activeSelf);
                }
            }

            foreach (var rendererState in snapshot.renderers)
            {
                var target = ResolveNode(nodes, rendererState.nodeId, rendererState.path);
                if (target == null)
                {
                    continue;
                }
                foreach (var renderer in target.Renderers)
                {
                    renderer.enabled = rendererState.enabled;
                }
            }

            foreach (var shape in snapshot.blendShapes)
            {
                var target = ResolveNode(nodes, shape.nodeId, shape.path);
                if (target == null)
                {
                    continue;
                }
                foreach (var skinned in target.SkinnedRenderers)
                {
                    var mesh = skinned.sharedMesh;
                    var index = mesh != null ? mesh.GetBlendShapeIndex(shape.name) : -1;
                    if (index >= 0)
                    {
                        skinned.SetBlendShapeWeight(index, shape.weight);
                    }
                }
            }
        }

        private static SnapshotNodeLookup BuildNodeLookup(GameObject root)
        {
            var lookup = new SnapshotNodeLookup();
            foreach (var transform in root.GetComponentsInChildren<Transform>(true))
            {
                var node = new SnapshotNode
                {
                    Transform = transform,
                    Renderers = transform.GetComponents<Renderer>(),
                    SkinnedRenderers = transform.GetComponents<SkinnedMeshRenderer>()
                };
                var id = NodeIdFor(root.transform, transform);
                if (!lookup.ById.ContainsKey(id))
                {
                    lookup.ById[id] = node;
                }
                var path = VariantExtractor.TransformPath(root.transform, transform);
                if (!lookup.ByPath.ContainsKey(path))
                {
                    lookup.ByPath[path] = node;
                }
            }
            return lookup;
        }

        private static SnapshotNode ResolveNode(
            SnapshotNodeLookup nodes,
            string nodeId,
            string path)
        {
            var node = default(SnapshotNode);
            if (!string.IsNullOrEmpty(nodeId))
            {
                nodes.ById.TryGetValue(nodeId, out node);
            }
            if (node == null && !string.IsNullOrEmpty(path))
            {
                nodes.ByPath.TryGetValue(path, out node);
            }
            return node;
        }

        private sealed class SnapshotNodeLookup
        {
            public readonly Dictionary<string, SnapshotNode> ById = new Dictionary<string, SnapshotNode>(StringComparer.Ordinal);
            public readonly Dictionary<string, SnapshotNode> ByPath = new Dictionary<string, SnapshotNode>(StringComparer.Ordinal);
        }

        private sealed class SnapshotNode
        {
            public Transform Transform;
            public Renderer[] Renderers;
            public SkinnedMeshRenderer[] SkinnedRenderers;
        }

        public static List<WardrobeOperationDraft> FilterInheritedHiddenOperations(IEnumerable<WardrobeOperationDraft> operations)
        {
            var list = operations?
                .Where(operation => operation != null)
                .Select(CloneOperation)
                .ToList() ?? new List<WardrobeOperationDraft>();
            var hiddenPaths = list
                .Where(IsVisibilityFalseOperation)
                .Select(operation => operation.target != null ? operation.target.path : null)
                .Where(path => !string.IsNullOrEmpty(path))
                .Distinct()
                .ToList();
            if (hiddenPaths.Count == 0)
            {
                return list;
            }

            return list
                .Where(operation => !IsInheritedHiddenOperation(operation, hiddenPaths))
                .ToList();
        }

        private static bool IsInheritedHiddenOperation(WardrobeOperationDraft operation, IReadOnlyList<string> hiddenPaths)
        {
            if (!IsVisibilityFalseOperation(operation) || operation.target == null || string.IsNullOrEmpty(operation.target.path))
            {
                return false;
            }

            var path = NormalizePath(operation.target.path);
            return hiddenPaths.Any(hidden =>
            {
                var hiddenPath = NormalizePath(hidden);
                return !string.IsNullOrEmpty(hiddenPath)
                    && hiddenPath != path
                    && path.StartsWith(hiddenPath + "/", StringComparison.Ordinal);
            });
        }

        private static bool IsVisibilityFalseOperation(WardrobeOperationDraft operation)
        {
            return operation != null
                && (operation.type == "subtreeEnabled"
                    || operation.type == "subtreeVisibility"
                    || operation.type == "nodeEnabled"
                    || operation.type == "nodeVisibility"
                    || operation.type == "rendererEnabled"
                    || operation.type == "rendererVisibility")
                && !operation.boolValue;
        }

        public static string NormalizePath(string path)
        {
            return string.IsNullOrEmpty(path)
                ? string.Empty
                : path.Replace('\\', '/').Trim('/');
        }

        public static WardrobeSetDraft Diff(WardrobeSnapshotDraft baseline, WardrobeSnapshotDraft current, string displayName)
        {
            var setName = string.IsNullOrWhiteSpace(displayName) ? "Outfit" : displayName.Trim();
            var set = new WardrobeSetDraft
            {
                id = MakeId(setName),
                displayName = setName,
                source = "unity_capture_diff"
            };

            var baseNodes = ToFirstDictionary(baseline.nodes, n => n.nodeId);
            foreach (var node in current.nodes)
            {
                if (baseNodes.TryGetValue(node.nodeId, out var baseNode) && baseNode.visible != node.visible)
                {
                    set.operations.Add(new WardrobeOperationDraft
                    {
                        type = "subtreeEnabled",
                        target = Target(node.nodeId, node.path),
                        boolValue = node.visible
                    });
                    AddAssetGroupIfVisible(set, node.path, node.visible);
                }
            }
            AddDisabledDescendantsUnderEnabledSubtrees(set, current.nodes);

            var baseRenderers = ToFirstDictionary(baseline.renderers, RendererKey);
            foreach (var renderer in current.renderers)
            {
                if (baseRenderers.TryGetValue(RendererKey(renderer), out var baseRenderer) && baseRenderer.enabled != renderer.enabled)
                {
                    set.operations.Add(new WardrobeOperationDraft
                    {
                        type = "rendererEnabled",
                        target = Target(renderer.nodeId, renderer.path),
                        boolValue = renderer.enabled
                    });
                    AddAssetGroupIfVisible(set, renderer.path, renderer.enabled);
                }
            }

            var baseShapes = ToFirstDictionary(baseline.blendShapes, BlendShapeKey);
            foreach (var shape in current.blendShapes)
            {
                if (baseShapes.TryGetValue(BlendShapeKey(shape), out var baseShape) && Math.Abs(baseShape.weight - shape.weight) > BlendShapeEpsilon)
                {
                    set.operations.Add(new WardrobeOperationDraft
                    {
                        type = "blendShapeWeight",
                        target = Target(shape.nodeId, shape.path),
                        name = shape.name,
                        floatValue = shape.weight
                    });
                }
            }

            return set;
        }

        private static void AddDisabledDescendantsUnderEnabledSubtrees(WardrobeSetDraft set, IEnumerable<NodeStateDraft> currentNodes)
        {
            var enabledSubtreePaths = set.operations
                .Where(operation => operation.type == "subtreeEnabled" && operation.boolValue && operation.target != null)
                .Select(operation => NormalizePath(operation.target.path ?? ""))
                .Where(path => !string.IsNullOrWhiteSpace(path))
                .ToList();
            if (enabledSubtreePaths.Count == 0)
            {
                return;
            }

            var existingVisibilityTargets = new HashSet<string>(set.operations
                .Where(operation => (operation.type == "subtreeEnabled" || operation.type == "nodeEnabled") && operation.target != null)
                .Select(operation => operation.target.nodeId ?? ""));
            foreach (var node in currentNodes)
            {
                if (node.visible || existingVisibilityTargets.Contains(node.nodeId))
                {
                    continue;
                }
                var nodePath = NormalizePath(node.path);
                var isDisabledDescendant = false;
                foreach (var path in enabledSubtreePaths)
                {
                    if (IsAncestorOrSelf(path, nodePath) && !string.Equals(path, nodePath, StringComparison.Ordinal))
                    {
                        isDisabledDescendant = true;
                        break;
                    }
                }
                if (!isDisabledDescendant)
                {
                    continue;
                }
                set.operations.Add(new WardrobeOperationDraft
                {
                    type = "nodeEnabled",
                    target = Target(node.nodeId, node.path),
                    boolValue = false
                });
                existingVisibilityTargets.Add(node.nodeId);
            }
        }

        public static List<WardrobeOperationDraft> BaseOperations(WardrobeSnapshotDraft snapshot)
        {
            var operations = new List<WardrobeOperationDraft>();
            foreach (var node in snapshot.nodes)
            {
                if (!node.visible)
                {
                    operations.Add(new WardrobeOperationDraft
                    {
                        type = "subtreeEnabled",
                        target = Target(node.nodeId, node.path),
                        boolValue = false
                    });
                }
            }
            foreach (var shape in snapshot.blendShapes)
            {
                operations.Add(new WardrobeOperationDraft
                {
                    type = "blendShapeWeight",
                    target = Target(shape.nodeId, shape.path),
                    name = shape.name,
                    floatValue = shape.weight
                });
            }
            return operations;
        }

        public static WardrobeOperationDraft CloneOperation(WardrobeOperationDraft source)
        {
            return new WardrobeOperationDraft
            {
                type = source.type,
                target = Target(source.target != null ? source.target.nodeId : "", source.target != null ? source.target.path : ""),
                name = source.name,
                boolValue = source.boolValue,
                floatValue = source.floatValue
            };
        }

        public static string MakeId(string value)
        {
            var source = (value ?? "outfit").Trim().ToLowerInvariant();
            var sb = new StringBuilder(source.Length);
            var lastWasDash = false;
            foreach (var c in source)
            {
                if (char.IsLetterOrDigit(c))
                {
                    sb.Append(c);
                    lastWasDash = false;
                    continue;
                }
                if (!lastWasDash && sb.Length > 0)
                {
                    sb.Append('-');
                    lastWasDash = true;
                }
            }
            var normalized = sb.ToString().Trim('-');
            return string.IsNullOrEmpty(normalized) ? "outfit" : normalized;
        }

        private static WardrobeTargetDraft Target(string nodeId, string path)
        {
            return new WardrobeTargetDraft { nodeId = nodeId ?? "", path = path ?? "" };
        }

        private static string RendererKey(RendererStateDraft state)
        {
            return state.nodeId;
        }

        private static string BlendShapeKey(BlendShapeStateDraft state)
        {
            return state.nodeId + "\n" + state.name;
        }

        private static void AddAssetGroupIfVisible(WardrobeSetDraft set, string path, bool visible)
        {
            if (!visible || string.IsNullOrWhiteSpace(path))
            {
                return;
            }
            var top = path.Split('/')[0].Trim();
            if (string.IsNullOrWhiteSpace(top))
            {
                return;
            }
            var group = "outfit:" + MakeId(top);
            if (!set.assetGroups.Contains(group))
            {
                set.assetGroups.Add(group);
            }
        }

        private static void CompressVisibilityOperations(WardrobeSetDraft set)
        {
            CompressVisibilityOperations(set.operations);
        }

        private static void CompressVisibilityOperations(List<WardrobeOperationDraft> operations)
        {
            var compressed = new List<WardrobeOperationDraft>();
            foreach (var operation in operations.OrderBy(OperationPathDepth))
            {
                if (operation.type != "subtreeEnabled" && operation.type != "subtreeVisibility")
                {
                    compressed.Add(operation);
                    continue;
                }

                var path = operation.target != null ? operation.target.path ?? "" : "";
                var isRedundant = compressed.Any(existing =>
                    (existing.type == "subtreeEnabled" || existing.type == "subtreeVisibility") &&
                    existing.boolValue == operation.boolValue &&
                    IsAncestorOrSelf(existing.target != null ? existing.target.path ?? "" : "", path));
                if (!isRedundant)
                {
                    compressed.Add(operation);
                }
            }

            operations.Clear();
            operations.AddRange(compressed);
        }

        private static int OperationPathDepth(WardrobeOperationDraft operation)
        {
            if ((operation.type != "subtreeEnabled" && operation.type != "subtreeVisibility") ||
                operation.target == null ||
                string.IsNullOrWhiteSpace(operation.target.path))
            {
                return int.MaxValue;
            }
            return operation.target.path.Count(c => c == '/');
        }

        private static bool IsAncestorOrSelf(string ancestorPath, string path)
        {
            if (string.IsNullOrEmpty(ancestorPath))
            {
                return true;
            }
            return string.Equals(ancestorPath, path, StringComparison.Ordinal) ||
                path.StartsWith(ancestorPath + "/", StringComparison.Ordinal);
        }

        private static Dictionary<string, T> ToFirstDictionary<T>(IEnumerable<T> values, Func<T, string> keySelector)
        {
            var result = new Dictionary<string, T>();
            foreach (var value in values)
            {
                var key = keySelector(value) ?? "";
                if (!result.ContainsKey(key))
                {
                    result[key] = value;
                }
            }
            return result;
        }

        public static string NodeIdFor(Transform root, Transform target)
        {
            return "node_" + HashStablePath(StableTransformPath(root, target)).ToString("x16", CultureInfo.InvariantCulture);
        }

        private static string StableTransformPath(Transform root, Transform target)
        {
            if (root == target)
            {
                return "$root[0]";
            }
            var parts = new Stack<string>();
            var current = target;
            while (current != null)
            {
                if (current == root)
                {
                    parts.Push("$root[0]");
                    break;
                }
                parts.Push(current.name + "[" + SiblingIndex(current).ToString(CultureInfo.InvariantCulture) + "]");
                current = current.parent;
            }
            return string.Join("/", parts.ToArray());
        }

        private static int SiblingIndex(Transform transform)
        {
            if (transform.parent == null)
            {
                return 0;
            }
            var index = 0;
            for (var i = 0; i < transform.parent.childCount; i++)
            {
                var sibling = transform.parent.GetChild(i);
                if (sibling == transform)
                {
                    return index;
                }
                if (sibling.name == transform.name)
                {
                    index++;
                }
            }
            return index;
        }

        private static ulong HashStablePath(string value)
        {
            const ulong offset = 14695981039346656037UL;
            const ulong prime = 1099511628211UL;
            var hash = offset;
            foreach (var b in Encoding.UTF8.GetBytes(value ?? ""))
            {
                hash ^= b;
                hash *= prime;
            }
            return hash;
        }
    }
}

