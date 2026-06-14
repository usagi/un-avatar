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
                var currentToBaseOnlyMode = IsCurrentToBaseOnlyExportMode();
                var bakeAttempted = false;
                var bakeSucceeded = false;

                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Exporting GLB", 0.55f);
                var glbName = SanitizeFileName(avatarRoot.name);
                var modularAvatarExtraTextures = CollectModularAvatarMaskTextures(clone);
                var exportResult = MinimalGltfExporter.ExportGlb(clone, tempDir, glbName, null, modularAvatarExtraTextures);
                var tempGlb = exportResult.Path;

                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Patching UN_avatar extension", 0.8f);
                RegenerateWardrobePreviewImagesForExport();
                var wardrobeBaseSnapshot = currentToBaseOnlyMode
                    ? WardrobeSnapshotCapture.Capture(clone)
                    : null;
                var exportWardrobeSets = currentToBaseOnlyMode
                    ? new List<WardrobeSetDraft>()
                    : WardrobeSetsForExport();
                var exportPreviewImages = PreviewImagesForExport(exportWardrobeSets);
                var dynamicsPayload = BuildDynamicsPayload(clone);
                var contactsPayload = BuildContactsPayload(clone);
                var extension = BuildExtensionPayload(sourceVariants, humanoid, bakeAttempted, bakeSucceeded, clone, dynamicsPayload, contactsPayload, wardrobeBaseSnapshot, exportWardrobeSets, exportResult.TextureAssets, exportResult.RendererAssets);
                GlbExtensionPatcher.PatchRootExtension(tempGlb, normalizedPath, ExtensionName, extension, exportResult.TextureAssets, exportPreviewImages);

                var report = BuildReportPayload(validation, sourceVariants, humanoid, normalizedPath, bakeAttempted, bakeSucceeded, dynamicsPayload, contactsPayload, wardrobeBaseSnapshot, exportWardrobeSets, exportResult.Textures, exportResult.RendererAssets);
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
