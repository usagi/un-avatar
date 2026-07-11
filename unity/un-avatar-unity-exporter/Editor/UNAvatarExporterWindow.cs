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
    public sealed partial class UNAvatarExporterWindow : EditorWindow
    {
        private const string ExtensionName = "UN_avatar";
        private const string SpecVersion = "0.1-preview";
        private const string ExporterBuildMarker = "2026-06-29-untoon-matcap-rim-payload";
        private const int BaseSelectionIndex = -2;
        internal const string DeveloperModePrefKey = "UNAvatar.UnityExporter.DeveloperMode";

        [SerializeField] private GameObject avatarRoot;
        [SerializeField] private string exportPath = "";
        [SerializeField] private UNAvatarExportMode exportMode = UNAvatarExportMode.Wardrobe;
        [SerializeField] private bool forceIncludeInactiveObjects = true;
        [SerializeField] private bool hasBaseSnapshot = false;
        [SerializeField] private string wardrobeSetName = "New Outfit";
        [SerializeField] private WardrobeSnapshotDraft baseSnapshot = new WardrobeSnapshotDraft();
        [SerializeField] private List<WardrobePreviewImageDraft> basePreviewImages = new List<WardrobePreviewImageDraft>();
        [SerializeField] private bool hasImportedBaseOperations = false;
        [SerializeField] private List<WardrobeOperationDraft> importedBaseOperations = new List<WardrobeOperationDraft>();
        [SerializeField] private List<WardrobeSetDraft> capturedWardrobeSets = new List<WardrobeSetDraft>();
        [SerializeField] private int selectedWardrobeSetIndex = -1;
        [SerializeField] private bool useHighQualitySampleRender = true;
        [SerializeField] private bool useAntiAliasingForSampleImage = false;
        [SerializeField] private bool developerMode = false;
        [SerializeField] private bool showDeveloperDiagnostics = false;

        private Vector2 scroll;
        private string lastSummary = "";
        private string developerDiagnosticsText = "";
        private string developerDiagnosticsFilePath = "";

        internal static bool IsDeveloperModeEnabled => EditorPrefs.GetBool(DeveloperModePrefKey, false);

        private void OnEnable()
        {
            developerMode = IsDeveloperModeEnabled;
            MigrateExportMode();
        }

        [MenuItem("Tools/U.N. Avatar/Export .unavatar")]
        public static void Open()
        {
            var window = GetWindow<UNAvatarExporterWindow>("U.N. Avatar Exporter");
            window.minSize = new Vector2(520, 420);
            window.Show();
        }

        private void OnGUI()
        {
            TryAutoAssignAvatarRoot(false);

            scroll = EditorGUILayout.BeginScrollView(scroll);
            EditorGUILayout.LabelField("Export Target", EditorStyles.boldLabel);
            using (new EditorGUILayout.HorizontalScope())
            {
                avatarRoot = (GameObject)EditorGUILayout.ObjectField("Avatar Root", avatarRoot, typeof(GameObject), true);
                using (new EditorGUI.DisabledScope(avatarRoot != null))
                {
                    if (GUILayout.Button("Auto", GUILayout.Width(64)))
                    {
                        TryAutoAssignAvatarRoot(true);
                    }
                }
            }
            exportPath = EditorGUILayout.TextField("Output", exportPath);
            using (new EditorGUILayout.HorizontalScope())
            {
                GUILayout.FlexibleSpace();
                if (GUILayout.Button("Browse", GUILayout.Width(96)))
                {
                    var initialDirectory = ResolveInitialExportDirectory(exportPath);
                    var initialName = ResolveInitialExportName(exportPath);
                    var selected = EditorUtility.SaveFilePanel("Export .unavatar", initialDirectory, initialName, "unavatar");
                    if (!string.IsNullOrEmpty(selected))
                    {
                        exportPath = EnsureUnavatarExtension(selected);
                        GUI.FocusControl(null);
                        Repaint();
                    }
                }
            }

            EditorGUILayout.Space(8);
            EditorGUILayout.LabelField("Export Settings", EditorStyles.boldLabel);
            MigrateExportMode();
            exportMode = DrawExportModePopup(exportMode);
            if (IsCurrentToBaseOnlyExportMode())
            {
                EditorGUILayout.HelpBox("Current to Base Only writes the current scene state as Base and ignores captured wardrobe sets for this export. Saved wardrobe settings are kept.", MessageType.Info);
            }
            forceIncludeInactiveObjects = true;

            if (GUILayout.Button("Restore from .unavatar", GUILayout.Height(22)))
            {
                ImportCapturedSetsFromUnavatar();
            }

            DrawWardrobeCaptureGui();

            EditorGUILayout.Space(8);
            EditorGUILayout.LabelField("3. Export", EditorStyles.boldLabel);
            using (new EditorGUILayout.HorizontalScope())
            {
                if (developerMode && GUILayout.Button("Validate", GUILayout.Height(28)))
                {
                    lastSummary = ValidateSelection().ToDisplayText();
                }
                if (GUILayout.Button("Export", GUILayout.Height(28)))
                {
                    ExportSelected();
                }
            }

            EditorGUILayout.Space(8);
            EditorGUILayout.LabelField("Report", EditorStyles.boldLabel);
            EditorGUILayout.HelpBox(string.IsNullOrWhiteSpace(lastSummary) ? "No validation run yet." : lastSummary, MessageType.Info);

            EditorGUILayout.Space(8);
            EditorGUILayout.LabelField("---");
            var nextDeveloperMode = EditorGUILayout.ToggleLeft("Developer mode", developerMode);
            if (nextDeveloperMode != developerMode)
            {
                developerMode = nextDeveloperMode;
                EditorPrefs.SetBool(DeveloperModePrefKey, developerMode);
            }
            if (developerMode)
            {
                EditorGUILayout.HelpBox("Developer mode is for release-gated diagnostics and benchmarks.", MessageType.Info);

                DrawPngBenchmarkDeveloperControls();

                showDeveloperDiagnostics = EditorGUILayout.Foldout(showDeveloperDiagnostics, "Diagnostics", true);
                if (showDeveloperDiagnostics)
                {
                    EditorGUILayout.LabelField("Exporter build marker", ExporterBuildMarker);
                    if (GUILayout.Button("Refresh Diagnostics", GUILayout.Height(22)))
                    {
                        developerDiagnosticsText = BuildDeveloperDiagnostics();
                        developerDiagnosticsFilePath = "";
                    }
                    if (GUILayout.Button("Diagnose Skin Export", GUILayout.Height(22)))
                    {
                        SetDeveloperDiagnostics("skin-export", BuildSkinExportDiagnostics());
                    }
                    if (GUILayout.Button("Diagnose PhysBone Export", GUILayout.Height(22)))
                    {
                        SetDeveloperDiagnostics("physbone-export", BuildPhysBoneExportDiagnostics());
                    }
                    using (new EditorGUILayout.HorizontalScope())
                    {
                        if (GUILayout.Button("Clear Diagnostics", GUILayout.Height(22)))
                        {
                            developerDiagnosticsText = "";
                            developerDiagnosticsFilePath = "";
                        }
                        using (new EditorGUI.DisabledScope(string.IsNullOrEmpty(developerDiagnosticsFilePath)))
                        {
                            if (GUILayout.Button("Open Diagnostics File", GUILayout.Height(22)))
                            {
                                EditorUtility.RevealInFinder(developerDiagnosticsFilePath);
                            }
                        }
                    }
                    if (!string.IsNullOrEmpty(developerDiagnosticsText))
                    {
                        EditorGUILayout.TextArea(developerDiagnosticsText, GUILayout.MinHeight(180));
                    }
                }
            }
            EditorGUILayout.EndScrollView();
        }

        private void SetDeveloperDiagnostics(string label, string text)
        {
            developerDiagnosticsFilePath = WriteDeveloperDiagnosticsFile(label, text);
            developerDiagnosticsText = BuildDeveloperDiagnosticsPreview(text, developerDiagnosticsFilePath);
            Debug.Log("UNAvatar diagnostics written: " + developerDiagnosticsFilePath);
        }

        private static string WriteDeveloperDiagnosticsFile(string label, string text)
        {
            var safeLabel = string.IsNullOrEmpty(label) ? "diagnostics" : label.Replace(Path.DirectorySeparatorChar, '_').Replace(Path.AltDirectorySeparatorChar, '_');
            var directory = Path.Combine(Path.GetTempPath(), "UNAvatarDiagnostics");
            Directory.CreateDirectory(directory);
            var path = Path.Combine(directory, safeLabel + "-" + DateTime.Now.ToString("yyyyMMdd-HHmmss", CultureInfo.InvariantCulture) + ".txt");
            File.WriteAllText(path, text ?? "", Encoding.UTF8);
            return path;
        }

        private static string BuildDeveloperDiagnosticsPreview(string text, string filePath)
        {
            const int MaxPreviewCharacters = 20000;
            var source = text ?? "";
            var builder = new StringBuilder();
            builder.AppendLine("Full diagnostics written to:");
            builder.AppendLine(filePath ?? "");
            builder.AppendLine();
            builder.AppendLine("Preview:");
            if (source.Length > MaxPreviewCharacters)
            {
                builder.AppendLine(source.Substring(0, MaxPreviewCharacters));
                builder.AppendLine("...(truncated; open diagnostics file for full output)");
            }
            else
            {
                builder.AppendLine(source);
            }
            return builder.ToString();
        }

        private bool IsCurrentToBaseOnlyExportMode()
        {
            return exportMode == UNAvatarExportMode.CurrentToBaseOnly;
        }

        private static UNAvatarExportMode DrawExportModePopup(UNAvatarExportMode current)
        {
            var selected = current == UNAvatarExportMode.CurrentToBaseOnly ? 1 : 0;
            selected = EditorGUILayout.Popup("Export Mode", selected, new[] { "Wardrobe", "Current to Base Only" });
            return selected == 1 ? UNAvatarExportMode.CurrentToBaseOnly : UNAvatarExportMode.Wardrobe;
        }

        private void MigrateExportMode()
        {
            if ((int)exportMode == 2)
            {
                exportMode = UNAvatarExportMode.Wardrobe;
            }
        }
    }
}
