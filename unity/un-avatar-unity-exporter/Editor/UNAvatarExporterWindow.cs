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
        private const string ExporterBuildMarker = "2026-06-02-modular-avatar-payload";
        private const int BaseSelectionIndex = -2;
        internal const string DeveloperModePrefKey = "UNAvatar.UnityExporter.DeveloperMode";

        [SerializeField] private GameObject avatarRoot;
        [SerializeField] private string exportPath = "";
        [SerializeField] private UNAvatarExportMode exportMode = UNAvatarExportMode.WardrobeBaked;
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

        internal static bool IsDeveloperModeEnabled => EditorPrefs.GetBool(DeveloperModePrefKey, false);

        private void OnEnable()
        {
            developerMode = IsDeveloperModeEnabled;
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
            exportMode = (UNAvatarExportMode)EditorGUILayout.EnumPopup("Export Mode", exportMode);
            forceIncludeInactiveObjects = true;

            DrawWardrobeCaptureGui();

            EditorGUILayout.Space(8);
            EditorGUILayout.LabelField("4. Export", EditorStyles.boldLabel);
            using (new EditorGUILayout.HorizontalScope())
            {
                if (GUILayout.Button("Validate", GUILayout.Height(28)))
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
                    }
                    if (!string.IsNullOrEmpty(developerDiagnosticsText))
                    {
                        EditorGUILayout.TextArea(developerDiagnosticsText, GUILayout.MinHeight(180));
                    }
                }
            }
            EditorGUILayout.EndScrollView();
        }
    }
}
