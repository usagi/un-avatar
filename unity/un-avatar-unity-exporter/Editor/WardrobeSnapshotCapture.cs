using System;
using System.Collections.Generic;
using System.Globalization;
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
                var nodeId = NodeIdFor(root.transform, transform);
                snapshot.nodes.Add(new NodeStateDraft
                {
                    nodeId = nodeId,
                    path = path,
                    activeSelf = transform.gameObject.activeSelf,
                    visible = transform.gameObject.activeSelf
                });

                var renderers = transform.GetComponents<Renderer>();
                foreach (var renderer in renderers)
                {
                    snapshot.renderers.Add(new RendererStateDraft
                    {
                        nodeId = nodeId,
                        path = path,
                        enabled = renderer.enabled
                    });
                }

                var skinnedRenderers = transform.GetComponents<SkinnedMeshRenderer>();
                foreach (var skinned in skinnedRenderers)
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
                            nodeId = nodeId,
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
            DisableNodesMissingFromSnapshot(root, snapshot);

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

        private static void DisableNodesMissingFromSnapshot(GameObject root, WardrobeSnapshotDraft snapshot)
        {
            var known = SnapshotNodeKeys(snapshot);
            foreach (var transform in root.GetComponentsInChildren<Transform>(true))
            {
                if (transform == root.transform)
                {
                    continue;
                }
                var nodeId = NodeIdFor(root.transform, transform);
                var path = NormalizePath(VariantExtractor.TransformPath(root.transform, transform));
                if (!known.Contains(nodeId) && !known.Contains(path))
                {
                    transform.gameObject.SetActive(false);
                }
            }
        }

        private static SnapshotNodeLookup BuildNodeLookup(GameObject root)
        {
            var transforms = root.GetComponentsInChildren<Transform>(true);
            var lookup = new SnapshotNodeLookup(transforms.Length);
            foreach (var transform in transforms)
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
            public readonly Dictionary<string, SnapshotNode> ById;
            public readonly Dictionary<string, SnapshotNode> ByPath;

            public SnapshotNodeLookup(int capacity)
            {
                ById = new Dictionary<string, SnapshotNode>(capacity, StringComparer.Ordinal);
                ByPath = new Dictionary<string, SnapshotNode>(capacity, StringComparer.Ordinal);
            }
        }

        private sealed class SnapshotNode
        {
            public Transform Transform;
            public Renderer[] Renderers;
            public SkinnedMeshRenderer[] SkinnedRenderers;
        }

        public static List<WardrobeOperationDraft> FilterInheritedHiddenOperations(IEnumerable<WardrobeOperationDraft> operations)
        {
            var list = new List<WardrobeOperationDraft>();
            if (operations != null)
            {
                foreach (var operation in operations)
                {
                    if (operation != null)
                    {
                        list.Add(CloneOperation(operation));
                    }
                }
            }

            var hiddenPaths = new HashSet<string>(StringComparer.Ordinal);
            foreach (var operation in list)
            {
                if (!IsVisibilityFalseOperation(operation) || operation.target == null)
                {
                    continue;
                }
                var path = NormalizePath(operation.target.path);
                if (!string.IsNullOrEmpty(path))
                {
                    hiddenPaths.Add(path);
                }
            }
            if (hiddenPaths.Count == 0)
            {
                return list;
            }

            var filtered = new List<WardrobeOperationDraft>(list.Count);
            foreach (var operation in list)
            {
                if (!IsInheritedHiddenOperation(operation, hiddenPaths))
                {
                    filtered.Add(operation);
                }
            }
            return filtered;
        }

        private static bool IsInheritedHiddenOperation(WardrobeOperationDraft operation, IEnumerable<string> hiddenPaths)
        {
            if (!IsVisibilityFalseOperation(operation) || operation.target == null || string.IsNullOrEmpty(operation.target.path))
            {
                return false;
            }

            var path = NormalizePath(operation.target.path);
            foreach (var hiddenPath in hiddenPaths)
            {
                if (!string.IsNullOrEmpty(hiddenPath)
                    && hiddenPath != path
                    && IsStrictDescendantPath(hiddenPath, path))
                {
                    return true;
                }
            }
            return false;
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
                id = MakeWardrobeSetId(setName),
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
            var enabledSubtreePaths = new List<string>();
            foreach (var operation in set.operations)
            {
                if (operation.type != "subtreeEnabled" || !operation.boolValue || operation.target == null)
                {
                    continue;
                }
                var path = NormalizePath(operation.target.path ?? "");
                if (!string.IsNullOrWhiteSpace(path))
                {
                    enabledSubtreePaths.Add(path);
                }
            }
            if (enabledSubtreePaths.Count == 0)
            {
                return;
            }

            var existingVisibilityTargets = new HashSet<string>(StringComparer.Ordinal);
            foreach (var operation in set.operations)
            {
                if ((operation.type == "subtreeEnabled" || operation.type == "nodeEnabled") && operation.target != null)
                {
                    existingVisibilityTargets.Add(operation.target.nodeId ?? "");
                }
            }
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
            return BaseOperations(snapshot, null);
        }

        public static List<WardrobeOperationDraft> BaseOperations(WardrobeSnapshotDraft snapshot, GameObject referenceRoot)
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
            AddMissingReferenceNodesAsDisabled(operations, snapshot, referenceRoot);
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
            CompressVisibilityOperations(operations);
            return operations;
        }

        public static WardrobeSetDraft Diff(
            WardrobeSnapshotDraft baseline,
            WardrobeSnapshotDraft current,
            string displayName,
            GameObject referenceRoot)
        {
            var set = Diff(baseline, current, displayName);
            AddMissingReferenceNodesAsDisabled(set.operations, current, referenceRoot);
            CompressVisibilityOperations(set.operations);
            return set;
        }

        private static void AddMissingReferenceNodesAsDisabled(
            List<WardrobeOperationDraft> operations,
            WardrobeSnapshotDraft snapshot,
            GameObject referenceRoot)
        {
            if (snapshot == null || referenceRoot == null)
            {
                return;
            }

            var known = SnapshotNodeKeys(snapshot);
            var existingTargets = new HashSet<string>(StringComparer.Ordinal);
            foreach (var operation in operations)
            {
                if (operation == null || operation.target == null)
                {
                    continue;
                }
                if (!string.IsNullOrEmpty(operation.target.nodeId))
                {
                    existingTargets.Add(operation.target.nodeId);
                }
                var path = NormalizePath(operation.target.path);
                if (!string.IsNullOrEmpty(path))
                {
                    existingTargets.Add(path);
                }
            }

            foreach (var transform in referenceRoot.GetComponentsInChildren<Transform>(true))
            {
                if (transform == referenceRoot.transform)
                {
                    continue;
                }
                var nodeId = NodeIdFor(referenceRoot.transform, transform);
                var path = NormalizePath(VariantExtractor.TransformPath(referenceRoot.transform, transform));
                if (known.Contains(nodeId) || known.Contains(path) || existingTargets.Contains(nodeId) || existingTargets.Contains(path))
                {
                    continue;
                }
                operations.Add(new WardrobeOperationDraft
                {
                    type = "subtreeEnabled",
                    target = Target(nodeId, path),
                    boolValue = false
                });
                existingTargets.Add(nodeId);
                existingTargets.Add(path);
            }
        }

        private static HashSet<string> SnapshotNodeKeys(WardrobeSnapshotDraft snapshot)
        {
            var keys = new HashSet<string>(StringComparer.Ordinal);
            if (snapshot == null)
            {
                return keys;
            }
            foreach (var node in snapshot.nodes)
            {
                if (!string.IsNullOrEmpty(node.nodeId))
                {
                    keys.Add(node.nodeId);
                }
                var path = NormalizePath(node.path);
                if (!string.IsNullOrEmpty(path))
                {
                    keys.Add(path);
                }
            }
            return keys;
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

        public static string MakeWardrobeSetId(string value)
        {
            var source = (value ?? "").Trim();
            return string.IsNullOrEmpty(source) ? "Outfit" : source;
        }

        public static string NormalizeWardrobeSetId(string existingId, string displayName)
        {
            var displayId = MakeWardrobeSetId(displayName);
            if (string.IsNullOrWhiteSpace(existingId) || string.Equals(existingId, MakeId(displayName), StringComparison.Ordinal))
            {
                return displayId;
            }
            return existingId.Trim();
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
            var separator = path.IndexOf('/');
            var top = (separator >= 0 ? path.Substring(0, separator) : path).Trim();
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
            var sorted = new List<WardrobeOperationDraft>(operations);
            sorted.Sort((left, right) => OperationPathDepth(left).CompareTo(OperationPathDepth(right)));
            var compressed = new List<WardrobeOperationDraft>();
            foreach (var operation in sorted)
            {
                if (operation.type != "subtreeEnabled" && operation.type != "subtreeVisibility")
                {
                    compressed.Add(operation);
                    continue;
                }

                var path = operation.target != null ? operation.target.path ?? "" : "";
                var isRedundant = false;
                foreach (var existing in compressed)
                {
                    if ((existing.type == "subtreeEnabled" || existing.type == "subtreeVisibility") &&
                        existing.boolValue == operation.boolValue &&
                        IsAncestorOrSelf(existing.target != null ? existing.target.path ?? "" : "", path))
                    {
                        isRedundant = true;
                        break;
                    }
                }
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
            var depth = 0;
            foreach (var c in operation.target.path)
            {
                if (c == '/')
                {
                    depth++;
                }
            }
            return depth;
        }

        private static bool IsAncestorOrSelf(string ancestorPath, string path)
        {
            if (string.IsNullOrEmpty(ancestorPath))
            {
                return true;
            }
            return string.Equals(ancestorPath, path, StringComparison.Ordinal) ||
                IsStrictDescendantPath(ancestorPath, path);
        }

        private static bool IsStrictDescendantPath(string ancestorPath, string path)
        {
            return !string.IsNullOrEmpty(path) &&
                path.Length > ancestorPath.Length &&
                path[ancestorPath.Length] == '/' &&
                path.StartsWith(ancestorPath, StringComparison.Ordinal);
        }

        private static Dictionary<string, T> ToFirstDictionary<T>(IEnumerable<T> values, Func<T, string> keySelector)
        {
            var result = new Dictionary<string, T>(StringComparer.Ordinal);
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
            var parts = new List<string>();
            var current = target;
            var length = 0;
            while (current != null)
            {
                if (current == root)
                {
                    const string rootPart = "$root[0]";
                    parts.Add(rootPart);
                    length += rootPart.Length + 1;
                    break;
                }
                var part = current.name + "[" + SiblingIndex(current).ToString(CultureInfo.InvariantCulture) + "]";
                parts.Add(part);
                length += part.Length + 1;
                current = current.parent;
            }
            if (parts.Count == 0)
            {
                return "";
            }

            var builder = new StringBuilder(Math.Max(0, length - 1));
            for (var i = parts.Count - 1; i >= 0; i--)
            {
                if (builder.Length > 0)
                {
                    builder.Append('/');
                }
                builder.Append(parts[i]);
            }
            return builder.ToString();
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

