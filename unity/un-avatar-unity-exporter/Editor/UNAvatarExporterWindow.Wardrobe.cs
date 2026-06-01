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
        private void DrawWardrobeCaptureGui()
        {
            EditorGUILayout.Space(8);
            useHighQualitySampleRender = EditorGUILayout.ToggleLeft("Use high quality render for sample image", useHighQualitySampleRender);
            useAntiAliasingForSampleImage = EditorGUILayout.ToggleLeft("Use Anti-aliasing for sample image", useAntiAliasingForSampleImage);

            EditorGUILayout.Space(4);
            EditorGUILayout.LabelField("1. Base", EditorStyles.boldLabel);
            if (GUILayout.Button("Capture Current As Base", GUILayout.Height(24)))
            {
                CaptureBase();
            }
            using (new EditorGUILayout.HorizontalScope())
            {
                using (new EditorGUI.DisabledScope(!EnsureBaseCanBeApplied(false)))
                {
                    var baseSelected = selectedWardrobeSetIndex == BaseSelectionIndex;
                    if (GUILayout.Button(baseSelected ? "Base ✓" : "Base", GUILayout.Height(22)))
                    {
                        selectedWardrobeSetIndex = BaseSelectionIndex;
                        ApplyBaseToScene();
                    }
                }
                GUILayout.Label(BaseStatusText(), GUILayout.Width(64 + 88 * 3 + 12));
            }

            EditorGUILayout.Space(8);
            EditorGUILayout.LabelField("2. Wardrobe Sets", EditorStyles.boldLabel);
            wardrobeSetName = EditorGUILayout.TextField("Set Name", wardrobeSetName);
            using (new EditorGUI.DisabledScope(!hasBaseSnapshot))
            {
                if (GUILayout.Button("Capture Current As New Set", GUILayout.Height(24)))
                {
                    CaptureWardrobeSet();
                }
            }
            EditorGUILayout.LabelField("Captured Sets", capturedWardrobeSets.Count.ToString(CultureInfo.InvariantCulture));

            for (var i = 0; i < capturedWardrobeSets.Count; i++)
            {
                var set = capturedWardrobeSets[i];
                using (new EditorGUILayout.HorizontalScope())
                {
                    var selected = selectedWardrobeSetIndex == i;
                    if (GUILayout.Button(selected ? set.displayName + " ✓" : set.displayName))
                    {
                        selectedWardrobeSetIndex = i;
                        wardrobeSetName = set.displayName;
                        ApplySelectedWardrobeSetToScene();
                    }
                    GUILayout.Label(set.operations.Count + " ops", GUILayout.Width(64));
                    using (new EditorGUI.DisabledScope(!hasBaseSnapshot))
                    {
                        if (GUILayout.Button("Update", GUILayout.Width(88)))
                        {
                            if (selectedWardrobeSetIndex != i)
                            {
                                wardrobeSetName = set.displayName;
                            }
                            selectedWardrobeSetIndex = i;
                            wardrobeSetName = string.IsNullOrWhiteSpace(wardrobeSetName) ? set.displayName : wardrobeSetName;
                            UpdateSelectedWardrobeSetFromScene();
                        }
                    }
                    if (GUILayout.Button("Duplicate", GUILayout.Width(88)))
                    {
                        DuplicateWardrobeSet(i);
                    }
                    if (GUILayout.Button("Remove", GUILayout.Width(88)))
                    {
                        capturedWardrobeSets.RemoveAt(i);
                        selectedWardrobeSetIndex = Mathf.Clamp(selectedWardrobeSetIndex, -1, capturedWardrobeSets.Count - 1);
                        GUIUtility.ExitGUI();
                    }
                }
            }

            EditorGUILayout.Space(8);
            EditorGUILayout.LabelField("3. WIP Operations", EditorStyles.boldLabel);
            EditorGUILayout.LabelField("Save / load draft state. Useful, not required.");
            using (new EditorGUILayout.HorizontalScope())
            {
                if (GUILayout.Button("Save Draft", GUILayout.Height(22)))
                {
                    SaveCaptureDraft();
                }
                if (GUILayout.Button("Load Draft", GUILayout.Height(22)))
                {
                    LoadCaptureDraft();
                }
                if (GUILayout.Button("Import From .unavatar", GUILayout.Height(22)))
                {
                    ImportCapturedSetsFromUnavatar();
                }
            }
        }

        private void BuildSnapshotsFromCurrentSets()
        {
            if (avatarRoot == null)
            {
                lastSummary = "Avatar Root is missing.";
                return;
            }
            if (!hasBaseSnapshot && !hasImportedBaseOperations)
            {
                lastSummary = "Capture Base or imported Base operations are missing.";
                return;
            }

            ApplyWardrobeOperations(CurrentBaseOperations());
            baseSnapshot = WardrobeSnapshotCapture.Capture(avatarRoot);
            hasBaseSnapshot = true;

            var built = 0;
            for (var i = 0; i < capturedWardrobeSets.Count; i++)
            {
                ApplyWardrobeOperations(CurrentBaseOperations());
                ApplyWardrobeOperations(capturedWardrobeSets[i].operations);
                capturedWardrobeSets[i].capturedSnapshot = WardrobeSnapshotCapture.Capture(avatarRoot);
                built++;
            }

            ApplyWardrobeOperations(CurrentBaseOperations());
            lastSummary = $"Built wardrobe snapshots from current sets: {built}.";
            SceneView.RepaintAll();
        }

        private void CaptureBase()
        {
            if (avatarRoot == null)
            {
                lastSummary = "Avatar Root is missing.";
                return;
            }
            baseSnapshot = WardrobeSnapshotCapture.Capture(avatarRoot);
            basePreviewImages = WardrobePreviewCapture.Capture(avatarRoot, CurrentPreviewCaptureOptions());
            hasBaseSnapshot = true;
            hasImportedBaseOperations = false;
            importedBaseOperations.Clear();
            selectedWardrobeSetIndex = BaseSelectionIndex;
            lastSummary = $"Captured base: {baseSnapshot.nodes.Count} nodes, {baseSnapshot.renderers.Count} renderers, {baseSnapshot.blendShapes.Count} blendshapes, {basePreviewImages.Count} previews.";
        }

        private void CaptureWardrobeSet()
        {
            if (avatarRoot == null)
            {
                lastSummary = "Avatar Root is missing.";
                return;
            }
            if (!hasBaseSnapshot)
            {
                CaptureBase();
                lastSummary += "\nBase was missing, so current state was captured as base. Change the scene to an outfit state and capture again.";
                return;
            }
            var current = WardrobeSnapshotCapture.Capture(avatarRoot);
            var set = WardrobeSnapshotCapture.Diff(baseSnapshot, current, wardrobeSetName);
            set.capturedSnapshot = current;
            set.previewImages = WardrobePreviewCapture.Capture(avatarRoot, CurrentPreviewCaptureOptions());
            capturedWardrobeSets.Add(set);
            selectedWardrobeSetIndex = capturedWardrobeSets.Count - 1;
            lastSummary = $"Captured wardrobe set `{set.displayName}`: {set.operations.Count} operations, {set.previewImages.Count} previews.";
        }

        private void UpdateSelectedWardrobeSetFromScene()
        {
            if (avatarRoot == null)
            {
                lastSummary = "Avatar Root is missing.";
                return;
            }
            if (!hasBaseSnapshot)
            {
                lastSummary = "Capture Base is missing. Imported Base operations can be applied, but updating a set needs a Base snapshot.";
                return;
            }
            if (selectedWardrobeSetIndex < 0 || selectedWardrobeSetIndex >= capturedWardrobeSets.Count)
            {
                lastSummary = "No wardrobe set is selected.";
                return;
            }

            var current = WardrobeSnapshotCapture.Capture(avatarRoot);
            var existing = capturedWardrobeSets[selectedWardrobeSetIndex];
            var nextName = string.IsNullOrWhiteSpace(wardrobeSetName) ? existing.displayName : wardrobeSetName.Trim();
            var updated = WardrobeSnapshotCapture.Diff(baseSnapshot, current, nextName);
            updated.id = string.Equals(nextName, existing.displayName, StringComparison.Ordinal) ? existing.id : WardrobeSnapshotCapture.MakeId(nextName);
            updated.displayName = nextName;
            updated.source = "unity_capture_diff_update";
            updated.capturedSnapshot = current;
            updated.previewImages = WardrobePreviewCapture.Capture(avatarRoot, CurrentPreviewCaptureOptions());
            capturedWardrobeSets[selectedWardrobeSetIndex] = updated;
            lastSummary = $"Updated wardrobe set `{updated.displayName}`: {updated.operations.Count} operations, {updated.previewImages.Count} previews.";
        }

        private WardrobePreviewCaptureOptions CurrentPreviewCaptureOptions()
        {
            return new WardrobePreviewCaptureOptions
            {
                HighQualityRender = useHighQualitySampleRender,
                AntiAliasing = useAntiAliasingForSampleImage
            };
        }

        private void DuplicateWardrobeSet(int index)
        {
            if (index < 0 || index >= capturedWardrobeSets.Count)
            {
                return;
            }
            var source = capturedWardrobeSets[index];
            var copy = new WardrobeSetDraft
            {
                id = WardrobeSnapshotCapture.MakeId(source.displayName + "-copy-" + capturedWardrobeSets.Count.ToString(CultureInfo.InvariantCulture)),
                displayName = source.displayName + " Copy",
                source = "unity_capture_diff_duplicate",
                assetGroups = new List<string>(source.assetGroups),
                operations = source.operations.Select(WardrobeSnapshotCapture.CloneOperation).ToList(),
                previewImages = (source.previewImages ?? new List<WardrobePreviewImageDraft>()).Select(WardrobePreviewCapture.ClonePreview).Where(image => image != null).ToList(),
                capturedSnapshot = source.capturedSnapshot
            };
            capturedWardrobeSets.Insert(index + 1, copy);
            selectedWardrobeSetIndex = index + 1;
        }

        private void SaveCaptureDraft()
        {
            var path = EditorUtility.SaveFilePanel("Save wardrobe capture draft", ResolveInitialExportDirectory(exportPath), ResolveDraftFileName(), "json");
            if (string.IsNullOrEmpty(path))
            {
                return;
            }

            var draft = new WardrobeCaptureSessionDraft
            {
                avatarRootName = avatarRoot != null ? avatarRoot.name : "",
                setName = wardrobeSetName,
                hasBaseSnapshot = hasBaseSnapshot,
                baseSnapshot = baseSnapshot,
                basePreviewImages = basePreviewImages,
                sets = capturedWardrobeSets
            };
            File.WriteAllText(path, JsonUtility.ToJson(draft, true), new UTF8Encoding(false));
            lastSummary = "Saved wardrobe capture draft\n" + path;
            AssetDatabase.Refresh();
        }

        private void LoadCaptureDraft()
        {
            var path = EditorUtility.OpenFilePanel("Load wardrobe capture draft", ResolveInitialExportDirectory(exportPath), "json");
            if (string.IsNullOrEmpty(path))
            {
                return;
            }

            var draft = JsonUtility.FromJson<WardrobeCaptureSessionDraft>(File.ReadAllText(path, Encoding.UTF8));
            if (draft == null)
            {
                lastSummary = "Failed to load wardrobe capture draft.";
                return;
            }

            wardrobeSetName = string.IsNullOrWhiteSpace(draft.setName) ? wardrobeSetName : draft.setName;
            hasBaseSnapshot = draft.hasBaseSnapshot;
            baseSnapshot = draft.baseSnapshot ?? new WardrobeSnapshotDraft();
            basePreviewImages = draft.basePreviewImages ?? new List<WardrobePreviewImageDraft>();
            hasImportedBaseOperations = false;
            importedBaseOperations.Clear();
            capturedWardrobeSets = draft.sets ?? new List<WardrobeSetDraft>();
            selectedWardrobeSetIndex = hasBaseSnapshot ? BaseSelectionIndex : capturedWardrobeSets.Count > 0 ? 0 : -1;
            lastSummary = $"Loaded wardrobe capture draft: {capturedWardrobeSets.Count} sets.";
        }

        private void ImportCapturedSetsFromUnavatar()
        {
            var path = EditorUtility.OpenFilePanel("Import wardrobe sets from .unavatar", ResolveInitialExportDirectory(exportPath), "unavatar");
            if (string.IsNullOrEmpty(path))
            {
                return;
            }

            try
            {
                var imported = UnavatarWardrobeImporter.Import(path);
                importedBaseOperations = imported.baseOperations;
                basePreviewImages = imported.basePreviewImages;
                hasImportedBaseOperations = imported.hasBaseOperations || importedBaseOperations.Count > 0;
                hasBaseSnapshot = false;
                baseSnapshot = new WardrobeSnapshotDraft();
                capturedWardrobeSets = imported.sets;
                selectedWardrobeSetIndex = hasImportedBaseOperations ? BaseSelectionIndex : capturedWardrobeSets.Count > 0 ? 0 : -1;
                wardrobeSetName = capturedWardrobeSets.Count > 0 ? capturedWardrobeSets[capturedWardrobeSets.Count - 1].displayName : wardrobeSetName;
                lastSummary = $"Imported wardrobe sets from .unavatar: {capturedWardrobeSets.Count} sets. Base operations: {importedBaseOperations.Count}. Imported ids: {string.Join(", ", imported.importedSetIds.ToArray())}.";
            }
            catch (Exception ex)
            {
                Debug.LogException(ex);
                lastSummary = "Failed to import wardrobe sets:\n" + ex.Message;
            }
        }

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
            return hasBaseSnapshot
                ? WardrobeSnapshotCapture.BaseOperations(baseSnapshot)
                : hasImportedBaseOperations
                ? importedBaseOperations.Select(WardrobeSnapshotCapture.CloneOperation).ToList()
                : new List<WardrobeOperationDraft>();
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

            var nodes = root.GetComponentsInChildren<Transform>(true)
                .ToDictionary(transform => WardrobeSnapshotCapture.NodeIdFor(root.transform, transform), transform => transform);
            var nodesByPath = root.GetComponentsInChildren<Transform>(true)
                .GroupBy(transform => VariantExtractor.TransformPath(root.transform, transform))
                .ToDictionary(group => group.Key, group => group.First());
            var nodesByNormalizedPath = root.GetComponentsInChildren<Transform>(true)
                .GroupBy(transform => WardrobeSnapshotCapture.NormalizePath(VariantExtractor.TransformPath(root.transform, transform)))
                .ToDictionary(group => group.Key, group => group.First());

            foreach (var operation in operations)
            {
                if (operation == null || operation.target == null)
                {
                    continue;
                }
                report.Total++;
                var transform = default(Transform);
                if (!string.IsNullOrEmpty(operation.target.nodeId))
                {
                    nodes.TryGetValue(operation.target.nodeId, out transform);
                }
                if (transform == null && !string.IsNullOrEmpty(operation.target.path))
                {
                    nodesByPath.TryGetValue(operation.target.path, out transform);
                }
                if (transform == null && !string.IsNullOrEmpty(operation.target.path))
                {
                    transform = ResolveTransformByPathSuffix(nodesByNormalizedPath, operation.target.path);
                }
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
                    foreach (var renderer in transform.GetComponents<Renderer>())
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
                    foreach (var skinned in transform.GetComponents<SkinnedMeshRenderer>())
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

        private static Transform ResolveTransformByPathSuffix(Dictionary<string, Transform> nodesByNormalizedPath, string importedPath)
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
            var match = default(Transform);
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
