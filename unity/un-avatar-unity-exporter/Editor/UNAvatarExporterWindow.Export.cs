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
        private void ExportSelected()
        {
            var validation = ValidateSelection();
            if (!validation.CanExport)
            {
                lastSummary = validation.ToDisplayText();
                ShowNotification(new GUIContent("Export is not ready."));
                return;
            }

            var normalizedPath = EnsureUnavatarExtension(exportPath);
            exportPath = normalizedPath;
            forceIncludeInactiveObjects = true;
            var reportPath = normalizedPath + ".report.json";
            var tempDir = Path.Combine(Path.GetTempPath(), "un-avatar-unity-exporter-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(tempDir);

            GameObject clone = null;
            try
            {
                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Preparing clone", 0.1f);
                clone = Instantiate(avatarRoot);
                clone.name = avatarRoot.name + " (UNAvatar Export)";
                clone.hideFlags = HideFlags.HideAndDontSave;
                clone.SetActive(true);

                var sourceVariants = VariantExtractor.Extract(avatarRoot, exportMode);
                var humanoid = HumanoidExtractor.Extract(avatarRoot);
                var splitWardrobeMode = exportMode == UNAvatarExportMode.WardrobeSplit;
                var bakedWardrobeMode = exportMode == UNAvatarExportMode.WardrobeBaked;

                if (forceIncludeInactiveObjects && bakedWardrobeMode)
                {
                    SetActiveRecursive(clone.transform, true);
                }
                if (bakedWardrobeMode)
                {
                    ApplyWardrobeOperationsToRoot(clone, CurrentBaseOperations());
                }

                var bakeAttempted = ModularAvatarBridge.IsAvailable && !splitWardrobeMode;
                var bakeSucceeded = false;
                if (bakeAttempted)
                {
                    EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Baking Modular Avatar clone", 0.25f);
                    bakeSucceeded = ModularAvatarBridge.TryBake(clone, out var bakeError);
                    if (!bakeSucceeded)
                    {
                        Debug.LogWarning("[U.N. Avatar] Modular Avatar bake failed: " + bakeError);
                    }
                }
                // Per-set Modular Avatar baking is too risky for the preview exporter: some VRC avatar
                // projects can crash Unity during repeated bake/active-state mutation. Keep the exported
                // model baked, but store wardrobe sets as authored capture diffs until the bake path is hardened.
                List<WardrobeSetDraft> bakedWardrobeSets = null;
                var bakedBaseSnapshot = bakedWardrobeSets != null ? WardrobeSnapshotCapture.Capture(clone) : null;

                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Exporting GLB", 0.55f);
                var glbName = SanitizeFileName(avatarRoot.name);
                var exportResult = MinimalGltfExporter.ExportGlb(clone, tempDir, glbName, null);
                var tempGlb = exportResult.Path;

                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Patching UN_avatar extension", 0.8f);
                RegenerateWardrobePreviewImagesForExport();
                // Wardrobe sets are currently stored as authored capture diffs, not per-set baked diffs.
                // Keep Base authored as well; post-bake snapshots can be altered by Modular Avatar and are
                // only safe as the wardrobe baseline once per-set baked snapshots are enabled again.
                var wardrobeBaseSnapshot = bakedWardrobeSets != null ? bakedBaseSnapshot : null;
                var exportWardrobeSets = bakedWardrobeSets ?? WardrobeSetsForExport();
                var exportPreviewImages = PreviewImagesForExport(exportWardrobeSets);
                var dynamicsPayload = BuildDynamicsPayload(clone);
                var extension = BuildExtensionPayload(sourceVariants, humanoid, bakeAttempted, bakeSucceeded, clone, dynamicsPayload, wardrobeBaseSnapshot, exportWardrobeSets, exportResult.TextureAssets, exportResult.RendererAssets);
                GlbExtensionPatcher.PatchRootExtension(tempGlb, normalizedPath, ExtensionName, extension, exportResult.TextureAssets, exportPreviewImages);

                var report = BuildReportPayload(validation, sourceVariants, humanoid, normalizedPath, bakeAttempted, bakeSucceeded, dynamicsPayload, wardrobeBaseSnapshot, exportWardrobeSets, exportResult.Textures, exportResult.RendererAssets);
                File.WriteAllText(reportPath, MiniJson.Serialize(report), new UTF8Encoding(false));

                AssetDatabase.Refresh();
                lastSummary = "Exported\n" + normalizedPath + "\n\nReport\n" + reportPath;
                ShowNotification(new GUIContent("Exported .unavatar"));
            }
            catch (Exception ex)
            {
                Debug.LogException(ex);
                lastSummary = "Export failed:\n" + ex.Message;
                ShowNotification(new GUIContent("Export failed."));
            }
            finally
            {
                EditorUtility.ClearProgressBar();
                if (clone != null)
                {
                    DestroyImmediate(clone);
                }
                try
                {
                    if (Directory.Exists(tempDir))
                    {
                        Directory.Delete(tempDir, true);
                    }
                }
                catch
                {
                    // Best effort cleanup. The temp directory path is included in Unity logs if deletion fails elsewhere.
                }
            }
        }
    }
}
