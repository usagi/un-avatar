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
            var baseOperations = CurrentBaseOperations();

            var built = 0;
            for (var i = 0; i < capturedWardrobeSets.Count; i++)
            {
                ApplyWardrobeOperations(baseOperations);
                ApplyWardrobeOperations(capturedWardrobeSets[i].operations);
                capturedWardrobeSets[i].capturedSnapshot = WardrobeSnapshotCapture.Capture(avatarRoot);
                built++;
            }

            ApplyWardrobeOperations(baseOperations);
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
            var set = WardrobeSnapshotCapture.Diff(baseSnapshot, current, wardrobeSetName, avatarRoot);
            set.capturedSnapshot = current;
            set.previewImages = WardrobePreviewCapture.Capture(avatarRoot, CurrentPreviewCaptureOptions());
            capturedWardrobeSets.Add(set);
            selectedWardrobeSetIndex = capturedWardrobeSets.Count - 1;
            lastSummary = $"Captured wardrobe set `{set.displayName}`: {set.operations.Count} operations, {set.previewImages.Count} previews." + WardrobeAssetGroupSummarySuffix(set);
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
            var updated = WardrobeSnapshotCapture.Diff(baseSnapshot, current, nextName, avatarRoot);
            updated.id = string.Equals(nextName, existing.displayName, StringComparison.Ordinal)
                ? WardrobeSnapshotCapture.NormalizeWardrobeSetId(existing.id, nextName)
                : WardrobeSnapshotCapture.MakeWardrobeSetId(nextName);
            updated.displayName = nextName;
            updated.source = "unity_capture_diff_update";
            updated.capturedSnapshot = current;
            updated.previewImages = WardrobePreviewCapture.Capture(avatarRoot, CurrentPreviewCaptureOptions());
            capturedWardrobeSets[selectedWardrobeSetIndex] = updated;
            lastSummary = $"Updated wardrobe set `{updated.displayName}`: {updated.operations.Count} operations, {updated.previewImages.Count} previews." + WardrobeAssetGroupSummarySuffix(updated);
        }

        private void RebuildSelectedWardrobeSetSnapshotFromOperations()
        {
            if (avatarRoot == null)
            {
                lastSummary = "Avatar Root is missing.";
                return;
            }
            if (!EnsureBaseCanBeApplied(true))
            {
                return;
            }
            if (selectedWardrobeSetIndex < 0 || selectedWardrobeSetIndex >= capturedWardrobeSets.Count)
            {
                lastSummary = "No wardrobe set is selected.";
                return;
            }

            var set = capturedWardrobeSets[selectedWardrobeSetIndex];
            var baseSummary = ApplyBaseStateToRoot(avatarRoot);
            var setReport = ApplyWardrobeOperationsToRoot(avatarRoot, set.operations);
            var rebuiltSnapshot = WardrobeSnapshotCapture.Capture(avatarRoot);
            var rebased = WardrobeSnapshotCapture.Diff(baseSnapshot, rebuiltSnapshot, set.displayName, avatarRoot);

            rebased.id = WardrobeSnapshotCapture.NormalizeWardrobeSetId(set.id, set.displayName);
            rebased.displayName = set.displayName;
            rebased.source = set.source + "_snapshot_rebuilt";
            rebased.capturedSnapshot = rebuiltSnapshot;
            rebased.previewImages = WardrobePreviewCapture.Capture(avatarRoot, CurrentPreviewCaptureOptions());
            capturedWardrobeSets[selectedWardrobeSetIndex] = rebased;
            lastSummary = $"Rebuilt wardrobe set `{rebased.displayName}` from stored operations: {rebased.operations.Count} operations, {rebased.previewImages.Count} previews. Base: {baseSummary} Set: {setReport.ToSummary()}" + WardrobeAssetGroupSummarySuffix(rebased);
            SceneView.RepaintAll();
        }

        private static string WardrobeAssetGroupSummarySuffix(WardrobeSetDraft set)
        {
            var groups = NormalizedNonBaseAssetGroups(set != null ? set.assetGroups : null);
            return groups.Count > 0
                ? "\nAsset groups: " + string.Join(", ", groups) + "."
                : "";
        }

        private static List<string> NormalizedNonBaseAssetGroups(IEnumerable<string> groups)
        {
            var result = new List<string>();
            if (groups == null)
            {
                return result;
            }
            foreach (var group in groups)
            {
                var normalized = group != null ? group.Trim() : "";
                if (string.IsNullOrWhiteSpace(normalized) || result.Contains(normalized))
                {
                    continue;
                }
                result.Add(normalized);
            }
            result.Sort(StringComparer.Ordinal);
            return result;
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
                id = WardrobeSnapshotCapture.MakeWardrobeSetId(source.displayName + "-copy-" + capturedWardrobeSets.Count.ToString(CultureInfo.InvariantCulture)),
                displayName = source.displayName + " Copy",
                source = "unity_capture_diff_duplicate",
                assetGroups = new List<string>(source.assetGroups),
                assetGroupOwnershipHints = WardrobeSetDraft.CloneHints(source.assetGroupOwnershipHints),
                operations = CloneWardrobeSetOperations(source.operations),
                previewImages = ClonePreviewImages(source.previewImages),
                capturedSnapshot = source.capturedSnapshot
            };
            capturedWardrobeSets.Insert(index + 1, copy);
            selectedWardrobeSetIndex = index + 1;
        }

        private static List<WardrobeOperationDraft> CloneWardrobeSetOperations(List<WardrobeOperationDraft> operations)
        {
            var cloned = new List<WardrobeOperationDraft>(operations != null ? operations.Count : 0);
            if (operations == null)
            {
                return cloned;
            }
            foreach (var operation in operations)
            {
                if (operation != null)
                {
                    cloned.Add(WardrobeSnapshotCapture.CloneOperation(operation));
                }
            }
            return cloned;
        }

        private void ImportCapturedSetsFromUnavatar()
        {
            var path = EditorUtility.OpenFilePanel("Restore wardrobe settings from .unavatar", ResolveInitialExportDirectory(exportPath), "unavatar");
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
                lastSummary = $"Imported wardrobe sets from .unavatar: {capturedWardrobeSets.Count} sets. Base operations: {importedBaseOperations.Count}. Imported ids: {string.Join(", ", imported.importedSetIds)}.";
                if (TryAutoAssignAvatarRoot(false))
                {
                    BuildSnapshotsFromCurrentSets();
                    lastSummary = $"Imported wardrobe sets from .unavatar and rebuilt snapshots for the current scene: {capturedWardrobeSets.Count} sets. Base operations: {importedBaseOperations.Count}. Imported ids: {string.Join(", ", imported.importedSetIds)}.";
                }
                else
                {
                    lastSummary += "\nAvatar Root is not selected. Select the avatar root or press Auto, then restore again to rebuild editable snapshots.";
                }
            }
            catch (Exception ex)
            {
                Debug.LogException(ex);
                lastSummary = "Failed to import wardrobe sets:\n" + ex.Message;
            }
        }
    }
}
