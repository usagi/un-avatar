using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace UNAvatar.UnityExporter
{
    internal enum UNAvatarExportMode
    {
        AllVariantsInOne,
        CurrentStateOnly,
        SplitVariants
    }

    [Serializable]
    internal sealed class WardrobeTargetDraft
    {
        public string nodeId;
        public string path;

        public Dictionary<string, object> ToJson()
        {
            return new Dictionary<string, object>
            {
                ["nodeId"] = nodeId ?? "",
                ["path"] = path ?? ""
            };
        }
    }

    [Serializable]
    internal sealed class WardrobeOperationDraft
    {
        public string type;
        public WardrobeTargetDraft target = new WardrobeTargetDraft();
        public string name;
        public bool boolValue;
        public float floatValue;

        public Dictionary<string, object> ToJson()
        {
            var json = new Dictionary<string, object>
            {
                ["type"] = type ?? "",
                ["target"] = target != null ? target.ToJson() : new Dictionary<string, object>()
            };
            if (!string.IsNullOrEmpty(name))
            {
                json["name"] = name;
            }
            if (type == "blendShapeWeight")
            {
                json["value"] = floatValue;
            }
            else
            {
                json["visible"] = boolValue;
            }
            return json;
        }
    }

    [Serializable]
    internal sealed class WardrobeSetDraft
    {
        public string id;
        public string displayName;
        public string source = "unity_capture_diff";
        public List<string> assetGroups = new List<string>();
        public List<WardrobeOperationDraft> operations = new List<WardrobeOperationDraft>();
        public WardrobeSnapshotDraft capturedSnapshot;

        public Dictionary<string, object> ToJson(bool isDefault)
        {
            var json = new Dictionary<string, object>
            {
                ["id"] = id ?? "",
                ["displayName"] = displayName ?? "",
                ["source"] = source ?? "",
                ["default"] = isDefault,
                ["assetGroups"] = assetGroups.Cast<object>().ToList(),
                ["operations"] = operations.Select(op => op.ToJson()).Cast<object>().ToList()
            };
            return json;
        }
    }

    [Serializable]
    internal sealed class WardrobeSnapshotDraft
    {
        public string rootName;
        public List<NodeStateDraft> nodes = new List<NodeStateDraft>();
        public List<RendererStateDraft> renderers = new List<RendererStateDraft>();
        public List<BlendShapeStateDraft> blendShapes = new List<BlendShapeStateDraft>();
    }

    [Serializable]
    internal sealed class NodeStateDraft
    {
        public string nodeId;
        public string path;
        public bool activeSelf;
        public bool visible;
    }

    [Serializable]
    internal sealed class RendererStateDraft
    {
        public string nodeId;
        public string path;
        public bool enabled;
    }

    [Serializable]
    internal sealed class BlendShapeStateDraft
    {
        public string nodeId;
        public string path;
        public string name;
        public float weight;
    }

    [Serializable]
    internal sealed class WardrobeCaptureSessionDraft
    {
        public string schema = "network.usagi.un-avatar.unity-exporter.wardrobe-capture";
        public string schemaVersion = "0.1-preview";
        public string avatarRootName;
        public string setName;
        public bool hasBaseSnapshot;
        public WardrobeSnapshotDraft baseSnapshot = new WardrobeSnapshotDraft();
        public List<WardrobeSetDraft> sets = new List<WardrobeSetDraft>();
    }

    internal sealed class TextureDiagnostic
    {
        public string Name;
        public string AssetPath;
        public string Extension;
        public long ByteLength;
    }

    internal sealed class ExportedTextureRecord
    {
        public string Name;
        public string AssetPath;
        public string SourceExtension;
        public string SourceMimeType;
        public long SourceByteLength;
        public string OutputMimeType;
        public int OutputByteLength;
        public string ExportMode;
        public string FallbackReason;

        public Dictionary<string, object> ToJson()
        {
            return new Dictionary<string, object>
            {
                ["name"] = Name ?? "",
                ["assetPath"] = AssetPath ?? "",
                ["sourceExtension"] = SourceExtension ?? "",
                ["sourceMimeType"] = SourceMimeType ?? "",
                ["sourceByteLength"] = SourceByteLength,
                ["outputMimeType"] = OutputMimeType ?? "",
                ["outputByteLength"] = OutputByteLength,
                ["exportMode"] = ExportMode ?? "",
                ["fallbackReason"] = FallbackReason ?? ""
            };
        }
    }

    internal sealed class UnavatarTextureAssetRecord
    {
        public string Id;
        public string Name;
        public string AssetPath;
        public string MimeType;
        public string SourceExtension;
        public string SourcePixelFormat;
        public string ColorSpace;
        public string Channels;
        public int Width;
        public int Height;
        public byte[] Bytes;
        public int BufferView = -1;

        public Dictionary<string, object> ToJson()
        {
            var json = new Dictionary<string, object>
            {
                ["id"] = Id ?? "",
                ["name"] = Name ?? "",
                ["assetPath"] = AssetPath ?? "",
                ["mimeType"] = MimeType ?? "",
                ["sourceExtension"] = SourceExtension ?? "",
                ["sourcePixelFormat"] = SourcePixelFormat ?? "",
                ["colorSpace"] = ColorSpace ?? "linear",
                ["channels"] = Channels ?? "",
                ["byteLength"] = Bytes != null ? Bytes.Length : 0
            };
            if (Width > 0)
            {
                json["width"] = Width;
            }
            if (Height > 0)
            {
                json["height"] = Height;
            }
            if (BufferView >= 0)
            {
                json["bufferView"] = BufferView;
            }
            return json;
        }
    }

    public sealed class UNAvatarExporterWindow : EditorWindow
    {
        private const string ExtensionName = "UN_avatar";
        private const string SpecVersion = "0.1-preview";
        private const int BaseSelectionIndex = -2;

        [SerializeField] private GameObject avatarRoot;
        [SerializeField] private string exportPath = "";
        [SerializeField] private UNAvatarExportMode exportMode = UNAvatarExportMode.AllVariantsInOne;
        [SerializeField] private bool forceIncludeInactiveObjects = true;
        [SerializeField] private bool hasBaseSnapshot = false;
        [SerializeField] private string wardrobeSetName = "New Outfit";
        [SerializeField] private WardrobeSnapshotDraft baseSnapshot = new WardrobeSnapshotDraft();
        [SerializeField] private bool hasImportedBaseOperations = false;
        [SerializeField] private List<WardrobeOperationDraft> importedBaseOperations = new List<WardrobeOperationDraft>();
        [SerializeField] private List<WardrobeSetDraft> capturedWardrobeSets = new List<WardrobeSetDraft>();
        [SerializeField] private int selectedWardrobeSetIndex = -1;
        [SerializeField] private bool developerMode = false;

        private Vector2 scroll;
        private string lastSummary = "";
        private string developerDiagnosticsText = "";

        [MenuItem("Tools/U.N. Avatar/Export .unavatar")]
        public static void Open()
        {
            var window = GetWindow<UNAvatarExporterWindow>("U.N. Avatar Exporter");
            window.minSize = new Vector2(520, 420);
            window.Show();
        }

        private void OnGUI()
        {
            scroll = EditorGUILayout.BeginScrollView(scroll);
            EditorGUILayout.LabelField("Export Target", EditorStyles.boldLabel);
            avatarRoot = (GameObject)EditorGUILayout.ObjectField("Avatar Root", avatarRoot, typeof(GameObject), true);
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
            developerMode = EditorGUILayout.ToggleLeft("Developer mode", developerMode);
            if (developerMode)
            {
                EditorGUILayout.HelpBox(
                    "Developer mode enables diagnostic logs and experimental tools while the exporter is under development.",
                    MessageType.Warning);
                using (new EditorGUI.DisabledScope(true))
                {
                    EditorGUILayout.Toggle("Force Enable All Before Bake", true);
                }
                EditorGUILayout.LabelField("Debug Hints", EditorStyles.boldLabel);
                developerDiagnosticsText = BuildDeveloperDiagnostics();
                EditorGUILayout.TextArea(developerDiagnosticsText, GUILayout.MinHeight(180));
            }
            EditorGUILayout.EndScrollView();
        }

        private string BuildDeveloperDiagnostics()
        {
            if (avatarRoot == null)
            {
                return "Avatar Root is missing.";
            }

            var renderers = avatarRoot.GetComponentsInChildren<Renderer>(true);
            var materials = renderers
                .SelectMany(renderer => renderer.sharedMaterials)
                .Where(material => material != null)
                .Distinct()
                .ToList();
            var textures = CollectMaterialTextures(materials);
            var sourceTextures = textures
                .Where(texture => !string.IsNullOrEmpty(texture.AssetPath))
                .ToList();
            var generatedTextures = textures.Count - sourceTextures.Count;
            var byExtension = sourceTextures
                .GroupBy(texture => texture.Extension)
                .OrderByDescending(group => group.Sum(texture => texture.ByteLength))
                .Select(group => $"{group.Key}: {group.Count()} files, {FormatBytes(group.Sum(texture => texture.ByteLength))}");
            var largest = sourceTextures
                .OrderByDescending(texture => texture.ByteLength)
                .Take(8)
                .Select(texture => $"{FormatBytes(texture.ByteLength)}  {texture.Name}  ({texture.Extension})");
            var fallbackExtensions = sourceTextures
                .Where(texture => !IsV01DirectTextureSource(texture.AssetPath))
                .GroupBy(texture => texture.Extension)
                .OrderByDescending(group => group.Sum(texture => texture.ByteLength))
                .Select(group => $"{group.Key}: {group.Count()} files, {FormatBytes(group.Sum(texture => texture.ByteLength))}");

            var lines = new List<string>
            {
                $"Renderers: {renderers.Length}",
                $"Materials: {materials.Count}",
                $"Distinct material textures: {textures.Count}",
                $"Source-backed textures: {sourceTextures.Count}",
                $"Generated/fallback textures: {generatedTextures}",
                "",
                "Source texture bytes by extension:",
                byExtension.Any() ? string.Join("\n", byExtension) : "(none)",
                "",
                "Largest source textures:",
                largest.Any() ? string.Join("\n", largest) : "(none)",
                "",
                "Source extensions that will use PNG fallback in v0.1:",
                fallbackExtensions.Any() ? string.Join("\n", fallbackExtensions) : "(none)",
                "",
                "Hints:",
                "JPG/JPEG source bytes are usually worth preserving.",
                "Large PNG normal/mask textures may be smaller after exporter PNG fallback or later optimizer transcode.",
                "If generated/fallback textures are high, export time will include GPU readback and PNG encode."
            };
            return string.Join("\n", lines);
        }

        private static bool IsV01DirectTextureSource(string path)
        {
            var extension = Path.GetExtension(path).ToLowerInvariant();
            return extension == ".png" || extension == ".jpg" || extension == ".jpeg";
        }

        private static List<TextureDiagnostic> CollectMaterialTextures(List<Material> materials)
        {
            var byKey = new Dictionary<string, TextureDiagnostic>(StringComparer.Ordinal);
            foreach (var material in materials)
            {
                string[] texturePropertyNames;
                try
                {
                    texturePropertyNames = material.GetTexturePropertyNames();
                }
                catch
                {
                    texturePropertyNames = Array.Empty<string>();
                }

                foreach (var propertyName in texturePropertyNames)
                {
                    var texture = material.GetTexture(propertyName);
                    if (texture == null)
                    {
                        continue;
                    }

                    var assetPath = AssetDatabase.GetAssetPath(texture);
                    var key = string.IsNullOrEmpty(assetPath) ? "texture:" + texture.GetInstanceID().ToString(CultureInfo.InvariantCulture) : "asset:" + assetPath;
                    if (byKey.ContainsKey(key))
                    {
                        continue;
                    }

                    var byteLength = 0L;
                    var extension = "(generated)";
                    if (!string.IsNullOrEmpty(assetPath))
                    {
                        extension = Path.GetExtension(assetPath).ToLowerInvariant();
                        var fullPath = Path.IsPathRooted(assetPath)
                            ? assetPath
                            : Path.Combine(Directory.GetCurrentDirectory(), assetPath);
                        if (File.Exists(fullPath))
                        {
                            byteLength = new FileInfo(fullPath).Length;
                        }
                    }

                    byKey[key] = new TextureDiagnostic
                    {
                        Name = texture.name,
                        AssetPath = assetPath,
                        Extension = string.IsNullOrEmpty(extension) ? "(none)" : extension,
                        ByteLength = byteLength
                    };
                }
            }

            return byKey.Values.ToList();
        }

        private static string FormatBytes(long bytes)
        {
            if (bytes >= 1024L * 1024L)
            {
                return (bytes / 1024.0 / 1024.0).ToString("0.0", CultureInfo.InvariantCulture) + " MB";
            }
            if (bytes >= 1024L)
            {
                return (bytes / 1024.0).ToString("0.0", CultureInfo.InvariantCulture) + " KB";
            }
            return bytes.ToString(CultureInfo.InvariantCulture) + " B";
        }

        private ExportValidation ValidateSelection()
        {
            var validation = new ExportValidation();
            validation.ModularAvatarInstalled = ModularAvatarBridge.IsAvailable;
            validation.AvatarRootSet = avatarRoot != null;
            validation.OutputPathSet = !string.IsNullOrWhiteSpace(exportPath);

            if (avatarRoot != null)
            {
                validation.RendererCount = avatarRoot.GetComponentsInChildren<Renderer>(true).Length;
                validation.SkinnedMeshRendererCount = avatarRoot.GetComponentsInChildren<SkinnedMeshRenderer>(true).Length;
                validation.MaterialCount = avatarRoot.GetComponentsInChildren<Renderer>(true)
                    .SelectMany(r => r.sharedMaterials)
                    .Where(m => m != null)
                    .Distinct()
                    .Count();
                validation.VariantCount = VariantExtractor.Extract(avatarRoot, exportMode).Count;
                validation.WardrobeSetCount = capturedWardrobeSets.Count;
                validation.HumanoidBoneCount = HumanoidExtractor.Extract(avatarRoot).Count;
            }

            return validation;
        }

        private void DrawWardrobeCaptureGui()
        {
            EditorGUILayout.Space(8);
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
                    if (GUILayout.Toggle(baseSelected, "Base", "Button", GUILayout.Height(22)) && !baseSelected)
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
                    if (GUILayout.Toggle(selected, set.displayName, "Button") && !selected)
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
            hasBaseSnapshot = true;
            hasImportedBaseOperations = false;
            importedBaseOperations.Clear();
            selectedWardrobeSetIndex = BaseSelectionIndex;
            lastSummary = $"Captured base: {baseSnapshot.nodes.Count} nodes, {baseSnapshot.renderers.Count} renderers, {baseSnapshot.blendShapes.Count} blendshapes.";
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
            capturedWardrobeSets.Add(set);
            selectedWardrobeSetIndex = capturedWardrobeSets.Count - 1;
            lastSummary = $"Captured wardrobe set `{set.displayName}`: {set.operations.Count} operations.";
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
            capturedWardrobeSets[selectedWardrobeSetIndex] = updated;
            lastSummary = $"Updated wardrobe set `{updated.displayName}`: {updated.operations.Count} operations.";
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
                hasImportedBaseOperations = imported.hasBaseOperations;
                hasBaseSnapshot = false;
                baseSnapshot = new WardrobeSnapshotDraft();
                capturedWardrobeSets = imported.sets;
                selectedWardrobeSetIndex = hasImportedBaseOperations ? BaseSelectionIndex : capturedWardrobeSets.Count > 0 ? 0 : -1;
                wardrobeSetName = capturedWardrobeSets.Count > 0 ? capturedWardrobeSets[capturedWardrobeSets.Count - 1].displayName : wardrobeSetName;
                lastSummary = $"Imported wardrobe sets from .unavatar: {capturedWardrobeSets.Count} sets. Base operations: {importedBaseOperations.Count}.";
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

            ApplyWardrobeOperations(CurrentBaseOperations());
            selectedWardrobeSetIndex = BaseSelectionIndex;
            lastSummary = "Applied base wardrobe state to scene.";
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

            ApplyWardrobeOperations(CurrentBaseOperations());
            ApplyWardrobeOperations(capturedWardrobeSets[selectedWardrobeSetIndex].operations);
            lastSummary = "Applied wardrobe set `" + capturedWardrobeSets[selectedWardrobeSetIndex].displayName + "` to scene.";
            SceneView.RepaintAll();
        }

        private List<WardrobeOperationDraft> CurrentBaseOperations()
        {
            return hasBaseSnapshot
                ? WardrobeSnapshotCapture.BaseOperations(baseSnapshot)
                : hasImportedBaseOperations
                ? importedBaseOperations.Select(WardrobeSnapshotCapture.CloneOperation).ToList()
                : new List<WardrobeOperationDraft>();
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
                lastSummary = "Capture Base or imported Base operations are missing.";
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
                    lastSummary = "Capture Base or imported Base operations are missing.";
                }
                return false;
            }
            return true;
        }

        private void ApplyWardrobeOperations(IEnumerable<WardrobeOperationDraft> operations)
        {
            ApplyWardrobeOperationsToRoot(avatarRoot, operations);
        }

        private static void ApplyWardrobeOperationsToRoot(GameObject root, IEnumerable<WardrobeOperationDraft> operations)
        {
            if (root == null || operations == null)
            {
                return;
            }

            var nodes = root.GetComponentsInChildren<Transform>(true)
                .ToDictionary(transform => WardrobeSnapshotCapture.NodeIdFor(root.transform, transform), transform => transform);
            var nodesByPath = root.GetComponentsInChildren<Transform>(true)
                .GroupBy(transform => VariantExtractor.TransformPath(root.transform, transform))
                .ToDictionary(group => group.Key, group => group.First());

            foreach (var operation in operations)
            {
                if (operation == null || operation.target == null)
                {
                    continue;
                }
                var transform = default(Transform);
                if (!string.IsNullOrEmpty(operation.target.nodeId))
                {
                    nodes.TryGetValue(operation.target.nodeId, out transform);
                }
                if (transform == null && !string.IsNullOrEmpty(operation.target.path))
                {
                    nodesByPath.TryGetValue(operation.target.path, out transform);
                }
                if (transform == null)
                {
                    continue;
                }

                if (operation.type == "subtreeEnabled" || operation.type == "subtreeVisibility")
                {
                    if (operation.boolValue)
                    {
                        transform.gameObject.SetActive(true);
                    }
                    else
                    {
                        transform.gameObject.SetActive(false);
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
                            skinned.SetBlendShapeWeight(index, operation.floatValue);
                        }
                    }
                }
            }
        }

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

                if (forceIncludeInactiveObjects && exportMode != UNAvatarExportMode.CurrentStateOnly)
                {
                    SetActiveRecursive(clone.transform, true);
                }
                ApplyWardrobeOperationsToRoot(clone, CurrentBaseOperations());

                var bakeAttempted = ModularAvatarBridge.IsAvailable;
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
                var bakedBaseSnapshot = WardrobeSnapshotCapture.Capture(clone);
                // Per-set Modular Avatar baking is too risky for the preview exporter: some VRC avatar
                // projects can crash Unity during repeated bake/active-state mutation. Keep the exported
                // model baked, but store wardrobe sets as authored capture diffs until the bake path is hardened.
                List<WardrobeSetDraft> bakedWardrobeSets = null;

                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Exporting GLB", 0.55f);
                var glbName = SanitizeFileName(avatarRoot.name);
                var exportResult = MinimalGltfExporter.ExportGlb(clone, tempDir, glbName, ReferencedMorphTargetNamesForExport());
                var tempGlb = exportResult.Path;

                EditorUtility.DisplayProgressBar("U.N. Avatar Export", "Patching UN_avatar extension", 0.8f);
                var extension = BuildExtensionPayload(sourceVariants, humanoid, bakeAttempted, bakeSucceeded, clone, bakedBaseSnapshot, bakedWardrobeSets, exportResult.TextureAssets);
                GlbExtensionPatcher.PatchRootExtension(tempGlb, normalizedPath, ExtensionName, extension, exportResult.TextureAssets);

                var report = BuildReportPayload(validation, sourceVariants, humanoid, normalizedPath, bakeAttempted, bakeSucceeded, bakedBaseSnapshot, bakedWardrobeSets, exportResult.Textures);
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

        private Dictionary<string, object> BuildExtensionPayload(
            List<VariantRecord> variants,
            Dictionary<string, string> humanoid,
            bool bakeAttempted,
            bool bakeSucceeded,
            GameObject registryRoot,
            WardrobeSnapshotDraft exportBaseSnapshot,
            List<WardrobeSetDraft> exportWardrobeSets,
            List<UnavatarTextureAssetRecord> textureAssets)
        {
            return new Dictionary<string, object>
            {
                ["specVersion"] = SpecVersion,
                ["generator"] = "U.N. Avatar Unity Exporter 0.1.0-preview",
                ["manifest"] = new Dictionary<string, object>
                {
                    ["name"] = avatarRoot != null ? avatarRoot.name : "",
                    ["sourceType"] = "vrc_unity_prefab",
                    ["exportMode"] = exportMode.ToString(),
                    ["createdUtc"] = DateTime.UtcNow.ToString("O", CultureInfo.InvariantCulture)
                },
                ["humanoid"] = humanoid,
                ["nodes"] = BuildNodeRegistryPayload(registryRoot),
                ["textureAssets"] = (textureAssets ?? new List<UnavatarTextureAssetRecord>())
                    .Select(asset => asset.ToJson())
                    .Cast<object>()
                    .ToList(),
                ["variants"] = variants.Select(v => v.ToJson()).ToList<object>(),
                ["wardrobe"] = BuildWardrobePayload(variants, exportBaseSnapshot, exportWardrobeSets),
                ["provenance"] = new Dictionary<string, object>
                {
                    ["unityVersion"] = Application.unityVersion,
                    ["sourceName"] = avatarRoot != null ? avatarRoot.name : "",
                    ["redistributionAllowed"] = false
                },
                ["unityExporter"] = new Dictionary<string, object>
                {
                    ["bakeModularAvatar"] = true,
                    ["modularAvatarInstalled"] = ModularAvatarBridge.IsAvailable,
                    ["modularAvatarBakeAttempted"] = bakeAttempted,
                    ["modularAvatarBakeSucceeded"] = bakeSucceeded,
                    ["forceIncludeInactiveObjects"] = forceIncludeInactiveObjects,
                    ["gltfWriter"] = "built-in"
                }
            };
        }

        private HashSet<string> ReferencedMorphTargetNamesForExport()
        {
            var names = new HashSet<string>(StringComparer.Ordinal);
            foreach (var operation in capturedWardrobeSets.SelectMany(set => set.operations))
            {
                if (operation.type == "blendShapeWeight" && !string.IsNullOrWhiteSpace(operation.name))
                {
                    names.Add(operation.name);
                }
            }
            foreach (var operation in CurrentBaseOperations())
            {
                if (operation.type == "blendShapeWeight" && !string.IsNullOrWhiteSpace(operation.name) && Math.Abs(operation.floatValue) > 0.001f)
                {
                    names.Add(operation.name);
                }
            }
            return names;
        }

        private Dictionary<string, object> BuildReportPayload(
            ExportValidation validation,
            List<VariantRecord> variants,
            Dictionary<string, string> humanoid,
            string output,
            bool bakeAttempted,
            bool bakeSucceeded,
            WardrobeSnapshotDraft exportBaseSnapshot,
            List<WardrobeSetDraft> exportWardrobeSets,
            List<ExportedTextureRecord> exportedTextures)
        {
            exportedTextures = exportedTextures ?? new List<ExportedTextureRecord>();
            var fallbackTextures = exportedTextures
                .Where(texture => texture.ExportMode == "png_fallback")
                .ToList();
            var textureSourceBytesByExtension = exportedTextures
                .Where(texture => !string.IsNullOrEmpty(texture.SourceExtension))
                .GroupBy(texture => texture.SourceExtension)
                .OrderByDescending(group => group.Sum(texture => texture.SourceByteLength))
                .Select(group => new Dictionary<string, object>
                {
                    ["extension"] = group.Key,
                    ["count"] = group.Count(),
                    ["sourceByteLength"] = group.Sum(texture => texture.SourceByteLength)
                })
                .Cast<object>()
                .ToList();

            return new Dictionary<string, object>
            {
                ["schema"] = "network.usagi.un-avatar.unity-exporter.report",
                ["schemaVersion"] = "0.1-preview",
                ["output"] = output,
                ["unityVersion"] = Application.unityVersion,
                ["avatarRoot"] = avatarRoot != null ? avatarRoot.name : "",
                ["exportMode"] = exportMode.ToString(),
                ["validation"] = validation.ToJson(),
                ["humanoidBoneCount"] = humanoid.Count,
                ["variantCount"] = variants.Count,
                ["variants"] = variants.Select(v => v.ToJson()).ToList<object>(),
                ["wardrobeSetCount"] = capturedWardrobeSets.Count,
                ["wardrobe"] = BuildWardrobePayload(variants, exportBaseSnapshot, exportWardrobeSets),
                ["bake"] = new Dictionary<string, object>
                {
                    ["modularAvatarInstalled"] = ModularAvatarBridge.IsAvailable,
                    ["attempted"] = bakeAttempted,
                    ["succeeded"] = bakeSucceeded
                },
                ["textures"] = new Dictionary<string, object>
                {
                    ["count"] = exportedTextures.Count,
                    ["fallbackCount"] = fallbackTextures.Count,
                    ["sourceBytesByExtension"] = textureSourceBytesByExtension,
                    ["fallbacks"] = fallbackTextures
                        .OrderByDescending(texture => texture.SourceByteLength)
                        .Select(texture => texture.ToJson())
                        .Cast<object>()
                        .ToList(),
                    ["items"] = exportedTextures
                        .Select(texture => texture.ToJson())
                        .Cast<object>()
                        .ToList()
                },
                ["unsupported"] = new List<object>
                {
                    "Full FX Animator evaluation",
                    "Full Poiyomi material reproduction",
                    "Full VRC contacts/interactions"
                }
            };
        }

        private Dictionary<string, object> BuildWardrobePayload(
            List<VariantRecord> variants,
            WardrobeSnapshotDraft exportBaseSnapshot = null,
            List<WardrobeSetDraft> exportWardrobeSets = null)
        {
            var hasExportBaseSnapshot = exportBaseSnapshot != null && exportBaseSnapshot.nodes.Count > 0;
            var baseOperations = hasExportBaseSnapshot
                ? WardrobeSnapshotCapture.BaseOperations(exportBaseSnapshot)
                : hasBaseSnapshot
                ? WardrobeSnapshotCapture.BaseOperations(baseSnapshot)
                : importedBaseOperations.Select(WardrobeSnapshotCapture.CloneOperation).ToList();
            var sets = new List<object>
            {
                new WardrobeSetDraft
                {
                    id = "base",
                    displayName = "Base",
                    source = hasExportBaseSnapshot ? "unity_baked_capture_base" : hasBaseSnapshot ? "unity_capture_base" : hasImportedBaseOperations ? "imported_unavatar_base" : "implicit_current_state",
                    operations = baseOperations
                }.ToJson(true)
            };

            var nonBaseSets = exportWardrobeSets ?? capturedWardrobeSets;
            foreach (var set in nonBaseSets)
            {
                sets.Add(set.ToJson(false));
            }

            if (nonBaseSets.Count == 0 && variants != null)
            {
                foreach (var variant in variants.Where(v => v.Id != "current-state"))
                {
                    sets.Add(new Dictionary<string, object>
                    {
                        ["id"] = "candidate-" + variant.Id,
                        ["displayName"] = variant.Name,
                        ["source"] = variant.Source,
                        ["default"] = false,
                        ["assetGroups"] = new List<object>(),
                        ["operations"] = variant.Operations.Cast<object>().ToList()
                    });
                }
            }

            return new Dictionary<string, object>
            {
                ["baseSet"] = "base",
                ["captureBase"] = hasExportBaseSnapshot ? SnapshotSummary(exportBaseSnapshot) : hasBaseSnapshot ? SnapshotSummary(baseSnapshot) : new Dictionary<string, object>(),
                ["sets"] = sets
            };
        }

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
                    if (forceIncludeInactiveObjects && exportMode != UNAvatarExportMode.CurrentStateOnly)
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

        private static void SetActiveRecursive(Transform root, bool active)
        {
            root.gameObject.SetActive(active);
            for (var i = 0; i < root.childCount; i++)
            {
                SetActiveRecursive(root.GetChild(i), active);
            }
        }

        private static string EnsureUnavatarExtension(string path)
        {
            if (string.Equals(Path.GetExtension(path), ".unavatar", StringComparison.OrdinalIgnoreCase))
            {
                return path;
            }
            return Path.ChangeExtension(path, ".unavatar");
        }

        private string ResolveInitialExportDirectory(string currentPath)
        {
            if (!string.IsNullOrWhiteSpace(currentPath))
            {
                try
                {
                    var directory = Path.GetDirectoryName(currentPath);
                    if (!string.IsNullOrWhiteSpace(directory) && Directory.Exists(directory))
                    {
                        return directory;
                    }
                }
                catch (ArgumentException)
                {
                }
            }

            var projectRoot = Directory.GetParent(Application.dataPath);
            return projectRoot != null ? projectRoot.FullName : Application.dataPath;
        }

        private string ResolveInitialExportName(string currentPath)
        {
            if (!string.IsNullOrWhiteSpace(currentPath))
            {
                try
                {
                    var fileName = Path.GetFileNameWithoutExtension(currentPath);
                    if (!string.IsNullOrWhiteSpace(fileName))
                    {
                        return fileName;
                    }
                }
                catch (ArgumentException)
                {
                }
            }

            return avatarRoot != null ? SanitizeFileName(avatarRoot.name) : "avatar";
        }

        private string ResolveDraftFileName()
        {
            var name = ResolveInitialExportName(exportPath);
            return string.IsNullOrWhiteSpace(name) ? "avatar-wardrobe-capture" : name + ".wardrobe-capture";
        }

        private static string SanitizeFileName(string value)
        {
            var invalid = Path.GetInvalidFileNameChars();
            var chars = value.Select(c => invalid.Contains(c) ? '_' : c).ToArray();
            var sanitized = new string(chars).Trim();
            return string.IsNullOrEmpty(sanitized) ? "avatar" : sanitized;
        }
    }

    internal sealed class ExportValidation
    {
        public bool ModularAvatarInstalled;
        public bool AvatarRootSet;
        public bool OutputPathSet;
        public int RendererCount;
        public int SkinnedMeshRendererCount;
        public int MaterialCount;
        public int VariantCount;
        public int WardrobeSetCount;
        public int HumanoidBoneCount;

        public bool CanExport => AvatarRootSet && OutputPathSet;

        public string ToDisplayText()
        {
            var lines = new List<string>
            {
                "Built-in GLB writer: available",
                "Modular Avatar: " + (ModularAvatarInstalled ? "installed" : "not detected"),
                "Avatar root: " + (AvatarRootSet ? "set" : "missing"),
                "Output path: " + (OutputPathSet ? "set" : "missing"),
                "Renderers: " + RendererCount,
                "Skinned meshes: " + SkinnedMeshRendererCount,
                "Materials: " + MaterialCount,
                "Variants: " + VariantCount,
                "Wardrobe sets: " + WardrobeSetCount,
                "Humanoid bones: " + HumanoidBoneCount,
                "Can export: " + CanExport
            };
            return string.Join("\n", lines);
        }

        public Dictionary<string, object> ToJson()
        {
            return new Dictionary<string, object>
            {
                ["gltfWriter"] = "built-in",
                ["modularAvatarInstalled"] = ModularAvatarInstalled,
                ["avatarRootSet"] = AvatarRootSet,
                ["outputPathSet"] = OutputPathSet,
                ["rendererCount"] = RendererCount,
                ["skinnedMeshRendererCount"] = SkinnedMeshRendererCount,
                ["materialCount"] = MaterialCount,
                ["variantCount"] = VariantCount,
                ["wardrobeSetCount"] = WardrobeSetCount,
                ["humanoidBoneCount"] = HumanoidBoneCount,
                ["canExport"] = CanExport
            };
        }
    }

    internal static class ModularAvatarBridge
    {
        private const string ProcessorTypeName = "nadena.dev.modular_avatar.core.editor.AvatarProcessor";

        public static bool IsAvailable => FindType(ProcessorTypeName) != null;

        public static bool TryBake(GameObject root, out string error)
        {
            error = "";
            var type = FindType(ProcessorTypeName);
            if (type == null)
            {
                error = "Modular Avatar AvatarProcessor was not found.";
                return false;
            }
            var method = type.GetMethod("ProcessAvatar", BindingFlags.Public | BindingFlags.Static, null, new[] { typeof(GameObject) }, null);
            if (method == null)
            {
                error = "Modular Avatar ProcessAvatar(GameObject) was not found.";
                return false;
            }
            try
            {
                method.Invoke(null, new object[] { root });
                return true;
            }
            catch (TargetInvocationException ex)
            {
                error = ex.InnerException != null ? ex.InnerException.Message : ex.Message;
                return false;
            }
            catch (Exception ex)
            {
                error = ex.Message;
                return false;
            }
        }

        private static Type FindType(string fullName)
        {
            foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies())
            {
                var type = assembly.GetType(fullName, false);
                if (type != null)
                {
                    return type;
                }
            }
            return null;
        }
    }

    internal sealed class VariantRecord
    {
        public string Id;
        public string Name;
        public string Source;
        public readonly List<Dictionary<string, object>> Operations = new List<Dictionary<string, object>>();

        public Dictionary<string, object> ToJson()
        {
            return new Dictionary<string, object>
            {
                ["id"] = Id,
                ["name"] = Name,
                ["source"] = Source,
                ["operations"] = Operations.Cast<object>().ToList()
            };
        }
    }

    internal static class VariantExtractor
    {
        public static List<VariantRecord> Extract(GameObject root, UNAvatarExportMode mode)
        {
            var variants = new List<VariantRecord>();
            if (root == null)
            {
                return variants;
            }

            variants.Add(MakeCurrentStateVariant(root));

            if (mode == UNAvatarExportMode.CurrentStateOnly)
            {
                return variants;
            }

            variants.AddRange(ExtractModularAvatarObjectToggles(root));
            variants.AddRange(ExtractModularAvatarMenuItems(root));
            return variants;
        }

        private static VariantRecord MakeCurrentStateVariant(GameObject root)
        {
            var variant = new VariantRecord
            {
                Id = "current-state",
                Name = "Current State",
                Source = "unity-active-state"
            };
            foreach (var renderer in root.GetComponentsInChildren<Renderer>(true))
            {
                variant.Operations.Add(new Dictionary<string, object>
                {
                    ["op"] = "nodeEnabled",
                    ["path"] = TransformPath(root.transform, renderer.transform),
                    ["visible"] = renderer.gameObject.activeInHierarchy && renderer.enabled
                });
            }
            return variant;
        }

        private static IEnumerable<VariantRecord> ExtractModularAvatarObjectToggles(GameObject root)
        {
            var records = new List<VariantRecord>();
            foreach (var component in root.GetComponentsInChildren<Component>(true))
            {
                if (component == null || component.GetType().FullName != "nadena.dev.modular_avatar.core.ModularAvatarObjectToggle")
                {
                    continue;
                }

                var record = new VariantRecord
                {
                    Id = "ma-object-toggle-" + records.Count.ToString(CultureInfo.InvariantCulture),
                    Name = component.gameObject.name,
                    Source = "modular-avatar-object-toggle"
                };

                var objects = component.GetType().GetProperty("Objects", BindingFlags.Public | BindingFlags.Instance)?.GetValue(component) as IEnumerable;
                if (objects != null)
                {
                    foreach (var item in objects)
                    {
                        var itemType = item.GetType();
                        var active = ReadBool(itemType, item, "Active", true);
                        var reference = ReadMember(itemType, item, "Object");
                        var target = ResolveAvatarObjectReference(reference, component);
                        if (target != null && target.transform.IsChildOf(root.transform))
                        {
                            record.Operations.Add(new Dictionary<string, object>
                            {
                                ["op"] = "nodeEnabled",
                                ["path"] = TransformPath(root.transform, target.transform),
                                ["visible"] = active
                            });
                        }
                    }
                }

                if (record.Operations.Count > 0)
                {
                    records.Add(record);
                }
            }
            return records;
        }

        private static IEnumerable<VariantRecord> ExtractModularAvatarMenuItems(GameObject root)
        {
            var records = new List<VariantRecord>();
            foreach (var component in root.GetComponentsInChildren<Component>(true))
            {
                if (component == null || component.GetType().FullName != "nadena.dev.modular_avatar.core.ModularAvatarMenuItem")
                {
                    continue;
                }

                var label = ReadString(component.GetType(), component, "label", "");
                var portable = component.GetType().GetProperty("PortableControl", BindingFlags.Public | BindingFlags.Instance)?.GetValue(component);
                var portableType = portable != null ? portable.GetType() : null;
                var controlType = portableType != null ? ReadAny(portableType, portable, "Type")?.ToString() : "";
                var parameter = portableType != null ? ReadAny(portableType, portable, "Parameter")?.ToString() : "";
                var value = portableType != null ? ReadAny(portableType, portable, "Value") : null;

                records.Add(new VariantRecord
                {
                    Id = "ma-menu-item-" + records.Count.ToString(CultureInfo.InvariantCulture),
                    Name = string.IsNullOrWhiteSpace(label) ? component.gameObject.name : label,
                    Source = "modular-avatar-menu-item",
                    Operations =
                    {
                        new Dictionary<string, object>
                        {
                            ["op"] = "metadata",
                            ["path"] = TransformPath(root.transform, component.transform),
                            ["controlType"] = controlType ?? "",
                            ["parameter"] = parameter ?? "",
                            ["value"] = value != null ? Convert.ToString(value, CultureInfo.InvariantCulture) : ""
                        }
                    }
                });
            }
            return records;
        }

        private static GameObject ResolveAvatarObjectReference(object reference, Component owner)
        {
            if (reference == null)
            {
                return null;
            }
            var method = reference.GetType().GetMethod("Get", BindingFlags.Public | BindingFlags.Instance, null, new[] { typeof(Component) }, null);
            if (method == null)
            {
                return null;
            }
            try
            {
                return method.Invoke(reference, new object[] { owner }) as GameObject;
            }
            catch
            {
                return null;
            }
        }

        public static string TransformPath(Transform root, Transform target)
        {
            if (root == target)
            {
                return "";
            }
            var parts = new Stack<string>();
            var current = target;
            while (current != null && current != root)
            {
                parts.Push(current.name);
                current = current.parent;
            }
            return string.Join("/", parts.ToArray());
        }

        private static object ReadAny(Type type, object instance, string name)
        {
            return ReadMember(type, instance, name);
        }

        private static object ReadMember(Type type, object instance, string name)
        {
            var property = type.GetProperty(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            if (property != null)
            {
                return property.GetValue(instance);
            }
            var field = type.GetField(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            return field != null ? field.GetValue(instance) : null;
        }

        private static bool ReadBool(Type type, object instance, string name, bool fallback)
        {
            var value = ReadMember(type, instance, name);
            return value is bool b ? b : fallback;
        }

        private static string ReadString(Type type, object instance, string name, string fallback)
        {
            return ReadMember(type, instance, name) as string ?? fallback;
        }
    }

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
                    visible = IsNodeVisible(transform)
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

            CompressVisibilityOperations(set);
            return set;
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
            CompressVisibilityOperations(operations);
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
            var normalized = new string((value ?? "outfit")
                .Trim()
                .ToLowerInvariant()
                .Select(c => char.IsLetterOrDigit(c) ? c : '-')
                .ToArray());
            while (normalized.Contains("--"))
            {
                normalized = normalized.Replace("--", "-");
            }
            normalized = normalized.Trim('-');
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

        private static bool IsNodeVisible(Transform transform)
        {
            return transform.gameObject.activeInHierarchy;
        }

        public static string NodeIdFor(Transform root, Transform target)
        {
            return "node_" + HashStablePath(StableTransformPath(root, target)).ToString("x16", CultureInfo.InvariantCulture);
        }

        private static string StableTransformPath(Transform root, Transform target)
        {
            if (root == target)
            {
                return root.name + "[0]";
            }
            var parts = new Stack<string>();
            var current = target;
            while (current != null)
            {
                parts.Push(current.name + "[" + SiblingIndex(current).ToString(CultureInfo.InvariantCulture) + "]");
                if (current == root)
                {
                    break;
                }
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

    internal sealed class ImportedWardrobeDraft
    {
        public bool hasBaseOperations;
        public List<WardrobeOperationDraft> baseOperations = new List<WardrobeOperationDraft>();
        public List<WardrobeSetDraft> sets = new List<WardrobeSetDraft>();
    }

    internal static class UnavatarWardrobeImporter
    {
        public static ImportedWardrobeDraft Import(string path)
        {
            var json = GlbExtensionPatcher.ReadRootJson(path);
            var extensionJson = GlbExtensionPatcher.ExtractRootExtensionJson(json, "UN_avatar");
            var extension = MiniJson.Deserialize(extensionJson) as Dictionary<string, object>;
            if (extension == null || !TryGetMap(extension, "wardrobe", out var wardrobe))
            {
                throw new InvalidDataException("UN_avatar.wardrobe was not found.");
            }

            var result = new ImportedWardrobeDraft();
            if (!TryGetList(wardrobe, "sets", out var sets))
            {
                return result;
            }
            var baseSetId = ReadString(wardrobe, "baseSet", "base");

            foreach (var item in sets)
            {
                var map = item as Dictionary<string, object>;
                if (map == null)
                {
                    continue;
                }

                var set = ReadSet(map);
                if (string.Equals(set.id, baseSetId, StringComparison.Ordinal) ||
                    string.Equals(set.id, "base", StringComparison.OrdinalIgnoreCase) ||
                    ReadBool(map, "default", false))
                {
                    result.hasBaseOperations = true;
                    result.baseOperations = set.operations;
                    continue;
                }
                result.sets.Add(set);
            }

            return result;
        }

        private static WardrobeSetDraft ReadSet(Dictionary<string, object> map)
        {
            var set = new WardrobeSetDraft
            {
                id = ReadString(map, "id", ""),
                displayName = ReadString(map, "displayName", ReadString(map, "name", "Imported Set")),
                source = "imported_unavatar"
            };

            if (TryGetList(map, "assetGroups", out var assetGroups))
            {
                foreach (var group in assetGroups)
                {
                    var text = group as string;
                    if (!string.IsNullOrWhiteSpace(text))
                    {
                        set.assetGroups.Add(text);
                    }
                }
            }

            if (TryGetList(map, "operations", out var operations))
            {
                foreach (var item in operations)
                {
                    var opMap = item as Dictionary<string, object>;
                    if (opMap == null)
                    {
                        continue;
                    }
                    set.operations.Add(ReadOperation(opMap));
                }
            }

            return set;
        }

        private static WardrobeOperationDraft ReadOperation(Dictionary<string, object> map)
        {
            var operation = new WardrobeOperationDraft
            {
                type = ReadString(map, "type", ReadString(map, "op", "")),
                name = ReadString(map, "name", "")
            };
            if (TryGetMap(map, "target", out var target))
            {
                operation.target = new WardrobeTargetDraft
                {
                    nodeId = ReadString(target, "nodeId", ""),
                    path = ReadString(target, "path", "")
                };
            }
            else
            {
                operation.target = new WardrobeTargetDraft
                {
                    nodeId = ReadString(map, "nodeId", ""),
                    path = ReadString(map, "path", "")
                };
            }
            operation.boolValue = ReadBool(map, "visible", false);
            operation.floatValue = ReadFloat(map, "value", 0.0f);
            return operation;
        }

        private static bool TryGetMap(Dictionary<string, object> map, string key, out Dictionary<string, object> value)
        {
            value = null;
            if (!map.TryGetValue(key, out var raw))
            {
                return false;
            }
            value = raw as Dictionary<string, object>;
            return value != null;
        }

        private static bool TryGetList(Dictionary<string, object> map, string key, out List<object> value)
        {
            value = null;
            if (!map.TryGetValue(key, out var raw))
            {
                return false;
            }
            value = raw as List<object>;
            return value != null;
        }

        private static string ReadString(Dictionary<string, object> map, string key, string fallback)
        {
            return map.TryGetValue(key, out var value) && value is string text ? text : fallback;
        }

        private static bool ReadBool(Dictionary<string, object> map, string key, bool fallback)
        {
            return map.TryGetValue(key, out var value) && value is bool b ? b : fallback;
        }

        private static float ReadFloat(Dictionary<string, object> map, string key, float fallback)
        {
            if (!map.TryGetValue(key, out var value))
            {
                return fallback;
            }
            if (value is double d)
            {
                return (float)d;
            }
            if (value is float f)
            {
                return f;
            }
            if (value is int i)
            {
                return i;
            }
            return fallback;
        }
    }

    internal static class HumanoidExtractor
    {
        public static Dictionary<string, string> Extract(GameObject root)
        {
            var result = new Dictionary<string, string>();
            if (root == null)
            {
                return result;
            }

            var animator = root.GetComponentInChildren<Animator>(true);
            if (animator == null || !animator.isHuman)
            {
                return result;
            }

            foreach (HumanBodyBones bone in Enum.GetValues(typeof(HumanBodyBones)))
            {
                if (bone == HumanBodyBones.LastBone)
                {
                    continue;
                }
                var transform = animator.GetBoneTransform(bone);
                if (transform != null && transform.IsChildOf(root.transform))
                {
                    result[bone.ToString()] = VariantExtractor.TransformPath(root.transform, transform);
                }
            }
            return result;
        }
    }

    internal static class MinimalGltfExporter
    {
        private const uint JsonChunkType = 0x4E4F534A;
        private const uint BinChunkType = 0x004E4942;
        private const uint GlbMagic = 0x46546C67;

        public sealed class ExportResult
        {
            public string Path;
            public List<ExportedTextureRecord> Textures = new List<ExportedTextureRecord>();
            public List<UnavatarTextureAssetRecord> TextureAssets = new List<UnavatarTextureAssetRecord>();
        }

        public static ExportResult ExportGlb(GameObject root, string directory, string fileName, HashSet<string> morphTargetNames)
        {
            var writer = new Writer(root, morphTargetNames);
            var path = Path.Combine(directory, fileName + ".glb");
            writer.Export(path);
            return new ExportResult
            {
                Path = path,
                Textures = writer.ExportedTextures,
                TextureAssets = writer.TextureAssets
            };
        }

        private sealed class Writer
        {
            private readonly GameObject root;
            private readonly BinaryBuffer buffer = new BinaryBuffer();
            private readonly HashSet<string> morphTargetNames;
            private readonly Dictionary<Transform, int> nodeIndices = new Dictionary<Transform, int>();
            private readonly Dictionary<Material, int> materialIndices = new Dictionary<Material, int>();
            private readonly Dictionary<Texture, int> textureIndices = new Dictionary<Texture, int>();
            private readonly Dictionary<Texture, UnavatarTextureAssetRecord> textureAssetIndices = new Dictionary<Texture, UnavatarTextureAssetRecord>();
            private int defaultMaterialIndex = -1;
            private readonly List<object> nodes = new List<object>();
            private readonly List<object> meshes = new List<object>();
            private readonly List<object> skins = new List<object>();
            private readonly List<object> accessors = new List<object>();
            private readonly List<object> bufferViews = new List<object>();
            private readonly List<object> materials = new List<object>();
            private readonly List<object> images = new List<object>();
            private readonly List<object> textures = new List<object>();
            private readonly List<object> samplers = new List<object>();
            private readonly List<ExportedTextureRecord> exportedTextures = new List<ExportedTextureRecord>();
            private readonly List<UnavatarTextureAssetRecord> textureAssets = new List<UnavatarTextureAssetRecord>();

            public List<ExportedTextureRecord> ExportedTextures => exportedTextures;
            public List<UnavatarTextureAssetRecord> TextureAssets => textureAssets;

            public Writer(GameObject root, HashSet<string> morphTargetNames)
            {
                this.root = root;
                this.morphTargetNames = morphTargetNames ?? new HashSet<string>(StringComparer.Ordinal);
                samplers.Add(new Dictionary<string, object>
                {
                    ["magFilter"] = 9729,
                    ["minFilter"] = 9987,
                    ["wrapS"] = 10497,
                    ["wrapT"] = 10497
                });
            }

            public void Export(string path)
            {
                BuildNodeTree(root.transform);
                AttachRenderers(root.transform);

                var gltf = new Dictionary<string, object>
                {
                    ["asset"] = new Dictionary<string, object>
                    {
                        ["version"] = "2.0",
                        ["generator"] = "U.N. Avatar Unity Exporter built-in GLB writer 0.1.0-preview"
                    },
                    ["scene"] = 0,
                    ["scenes"] = new List<object>
                    {
                        new Dictionary<string, object>
                        {
                            ["name"] = root.name,
                            ["nodes"] = new List<object> { 0 }
                        }
                    },
                    ["nodes"] = nodes,
                    ["meshes"] = meshes,
                    ["accessors"] = accessors,
                    ["bufferViews"] = bufferViews,
                    ["materials"] = materials
                };

                if (skins.Count > 0)
                {
                    gltf["skins"] = skins;
                }
                if (images.Count > 0)
                {
                    gltf["images"] = images;
                    gltf["textures"] = textures;
                    gltf["samplers"] = samplers;
                }
                if (buffer.Length > 0)
                {
                    gltf["buffers"] = new List<object>
                    {
                        new Dictionary<string, object>
                        {
                            ["byteLength"] = buffer.Length
                        }
                    };
                }

                WriteGlb(path, MiniJson.Serialize(gltf), buffer.ToArray());
            }

            private void BuildNodeTree(Transform transform)
            {
                var index = nodes.Count;
                nodeIndices[transform] = index;
                var isExportRoot = transform == root.transform;
                var translation = isExportRoot ? Vector3.zero : transform.localPosition;
                var node = new Dictionary<string, object>
                {
                    ["name"] = transform.name,
                    ["translation"] = FloatArray(translation.x, translation.y, translation.z),
                    ["rotation"] = FloatArray(transform.localRotation.x, transform.localRotation.y, transform.localRotation.z, transform.localRotation.w),
                    ["scale"] = FloatArray(transform.localScale.x, transform.localScale.y, transform.localScale.z)
                };
                nodes.Add(node);

                var children = new List<object>();
                for (var i = 0; i < transform.childCount; i++)
                {
                    var child = transform.GetChild(i);
                    BuildNodeTree(child);
                    children.Add(nodeIndices[child]);
                }
                if (children.Count > 0)
                {
                    node["children"] = children;
                }
            }

            private void AttachRenderers(Transform transform)
            {
                foreach (var meshRenderer in transform.GetComponents<MeshRenderer>())
                {
                    var filter = transform.GetComponent<MeshFilter>();
                    if (filter != null && filter.sharedMesh != null)
                    {
                        var node = (Dictionary<string, object>)nodes[nodeIndices[transform]];
                        var mesh = ExportMesh(filter.sharedMesh, meshRenderer.sharedMaterials, null);
                        if (mesh >= 0) node["mesh"] = mesh;
                    }
                }

                foreach (var skinned in transform.GetComponents<SkinnedMeshRenderer>())
                {
                    if (skinned.sharedMesh == null)
                    {
                        continue;
                    }
                    var node = (Dictionary<string, object>)nodes[nodeIndices[transform]];
                    var mesh = ExportMesh(skinned.sharedMesh, skinned.sharedMaterials, skinned);
                    if (mesh >= 0) node["mesh"] = mesh;
                    var skin = ExportSkin(skinned);
                    if (skin >= 0)
                    {
                        node["skin"] = skin;
                    }
                }

                for (var i = 0; i < transform.childCount; i++)
                {
                    AttachRenderers(transform.GetChild(i));
                }
            }

            private int ExportMesh(Mesh mesh, Material[] sourceMaterials, SkinnedMeshRenderer skinned)
            {
                var vertices = mesh.vertices;
                if (vertices == null || vertices.Length == 0)
                {
                    return -1;
                }

                var normals = mesh.normals;
                var tangents = mesh.tangents;
                var uv = mesh.uv;
                var colors = mesh.colors;
                var boneWeights = skinned != null ? mesh.boneWeights : null;

                var positionAccessor = AddVec3Accessor(vertices, true);
                var normalAccessor = normals != null && normals.Length == vertices.Length ? AddVec3Accessor(normals, false) : -1;
                var tangentAccessor = tangents != null && tangents.Length == vertices.Length ? AddVec4Accessor(tangents) : -1;
                var uvAccessor = uv != null && uv.Length == vertices.Length ? AddVec2Accessor(uv) : -1;
                var colorAccessor = colors != null && colors.Length == vertices.Length ? AddColorAccessor(colors) : -1;
                var jointsAccessor = boneWeights != null && boneWeights.Length == vertices.Length ? AddJointsAccessor(boneWeights) : -1;
                var weightsAccessor = boneWeights != null && boneWeights.Length == vertices.Length ? AddWeightsAccessor(boneWeights) : -1;
                var morphTargets = BuildMorphTargets(mesh, vertices.Length);
                var morphWeights = skinned != null && morphTargets.Count > 0 ? BuildMorphWeights(mesh, skinned, morphTargets) : new List<object>();

                var primitives = new List<object>();
                for (var submesh = 0; submesh < mesh.subMeshCount; submesh++)
                {
                    var indices = mesh.GetIndices(submesh);
                    if (indices == null || indices.Length == 0)
                    {
                        continue;
                    }

                    var attributes = new Dictionary<string, object> { ["POSITION"] = positionAccessor };
                    if (normalAccessor >= 0) attributes["NORMAL"] = normalAccessor;
                    if (tangentAccessor >= 0) attributes["TANGENT"] = tangentAccessor;
                    if (uvAccessor >= 0) attributes["TEXCOORD_0"] = uvAccessor;
                    if (colorAccessor >= 0) attributes["COLOR_0"] = colorAccessor;
                    if (jointsAccessor >= 0 && weightsAccessor >= 0)
                    {
                        attributes["JOINTS_0"] = jointsAccessor;
                        attributes["WEIGHTS_0"] = weightsAccessor;
                    }
                    var targets = new List<object>();
                    foreach (var target in morphTargets)
                    {
                        targets.Add(target.ToJson());
                    }

                    var material = sourceMaterials != null && submesh < sourceMaterials.Length ? sourceMaterials[submesh] : null;
                    var primitive = new Dictionary<string, object>
                    {
                        ["attributes"] = attributes,
                        ["indices"] = AddIndicesAccessor(indices),
                        ["material"] = ExportMaterial(material),
                        ["mode"] = 4
                    };
                    if (targets.Count > 0)
                    {
                        primitive["targets"] = targets;
                    }
                    primitives.Add(primitive);
                }
                if (primitives.Count == 0)
                {
                    return -1;
                }

                var gltfMesh = new Dictionary<string, object>
                {
                    ["name"] = mesh.name,
                    ["primitives"] = primitives
                };
                if (morphWeights.Count > 0)
                {
                    gltfMesh["weights"] = morphWeights;
                }
                if (morphTargets.Count > 0)
                {
                    gltfMesh["extras"] = new Dictionary<string, object>
                    {
                        ["targetNames"] = morphTargets.Select(target => (object)target.Name).ToList()
                    };
                }
                meshes.Add(gltfMesh);
                return meshes.Count - 1;
            }

            private List<MorphTargetRecord> BuildMorphTargets(Mesh mesh, int vertexCount)
            {
                var targets = new List<MorphTargetRecord>();
                if (mesh == null || mesh.blendShapeCount <= 0 || vertexCount <= 0)
                {
                    return targets;
                }

                for (var i = 0; i < mesh.blendShapeCount; i++)
                {
                    var name = mesh.GetBlendShapeName(i);
                    if (morphTargetNames.Count > 0 && !morphTargetNames.Contains(name))
                    {
                        continue;
                    }
                    var frameCount = mesh.GetBlendShapeFrameCount(i);
                    if (frameCount <= 0)
                    {
                        continue;
                    }
                    var deltaVertices = new Vector3[vertexCount];
                    var deltaNormals = new Vector3[vertexCount];
                    var deltaTangents = new Vector3[vertexCount];
                    mesh.GetBlendShapeFrameVertices(i, frameCount - 1, deltaVertices, deltaNormals, deltaTangents);
                    var record = new MorphTargetRecord
                    {
                        Name = name,
                        PositionAccessor = AddVec3Accessor(deltaVertices, false),
                        NormalAccessor = HasAnyNonZero(deltaNormals) ? AddVec3Accessor(deltaNormals, false) : -1
                    };
                    targets.Add(record);
                }
                return targets;
            }

            private static List<object> BuildMorphWeights(Mesh mesh, SkinnedMeshRenderer skinned, List<MorphTargetRecord> morphTargets)
            {
                var weights = new List<object>();
                foreach (var target in morphTargets)
                {
                    var index = mesh.GetBlendShapeIndex(target.Name);
                    weights.Add(index >= 0 ? Mathf.Clamp01(skinned.GetBlendShapeWeight(index) / 100.0f) : 0.0f);
                }
                return weights;
            }

            private static bool HasAnyNonZero(Vector3[] values)
            {
                if (values == null)
                {
                    return false;
                }
                for (var i = 0; i < values.Length; i++)
                {
                    if (values[i].sqrMagnitude > 0.0f)
                    {
                        return true;
                    }
                }
                return false;
            }

            private int ExportSkin(SkinnedMeshRenderer renderer)
            {
                var bones = renderer.bones;
                if (bones == null || bones.Length == 0)
                {
                    return -1;
                }

                var joints = new List<object>();
                foreach (var bone in bones)
                {
                    if (bone == null || !nodeIndices.TryGetValue(bone, out var nodeIndex))
                    {
                        return -1;
                    }
                    joints.Add(nodeIndex);
                }

                var bindposes = renderer.sharedMesh != null ? renderer.sharedMesh.bindposes : null;
                var matrices = new List<Matrix4x4>();
                for (var i = 0; i < bones.Length; i++)
                {
                    matrices.Add(bindposes != null && i < bindposes.Length ? bindposes[i] : Matrix4x4.identity);
                }

                var skin = new Dictionary<string, object>
                {
                    ["joints"] = joints,
                    ["inverseBindMatrices"] = AddMat4Accessor(matrices)
                };
                if (renderer.rootBone != null && nodeIndices.TryGetValue(renderer.rootBone, out var skeleton))
                {
                    skin["skeleton"] = skeleton;
                }
                skins.Add(skin);
                return skins.Count - 1;
            }

            private int ExportMaterial(Material material)
            {
                if (material == null)
                {
                    return ExportDefaultMaterial();
                }
                if (materialIndices.TryGetValue(material, out var existing))
                {
                    return existing;
                }

                var baseColor = ReadColor(material, "_BaseColor", ReadColor(material, "_Color", Color.white));
                var pbr = new Dictionary<string, object>
                {
                    ["baseColorFactor"] = FloatArray(baseColor.r, baseColor.g, baseColor.b, baseColor.a),
                    ["metallicFactor"] = ReadFloat(material, "_Metallic", 0.0f),
                    ["roughnessFactor"] = 1.0f - ReadFloat(material, "_Glossiness", 0.5f)
                };

                var mainTex = ReadTexture(material, "_BaseMap") ?? ReadTexture(material, "_MainTex");
                if (mainTex != null)
                {
                    var textureIndex = ExportTexture(mainTex);
                    if (textureIndex >= 0)
                    {
                        pbr["baseColorTexture"] = new Dictionary<string, object> { ["index"] = textureIndex };
                    }
                }

                var gltfMaterial = new Dictionary<string, object>
                {
                    ["name"] = material.name,
                    ["pbrMetallicRoughness"] = pbr,
                    ["doubleSided"] = true
                };
                var normalTexture = ReadTexture(material, "_BumpMap") ?? ReadTexture(material, "_NormalMap");
                if (normalTexture != null)
                {
                    var normalTextureIndex = ExportTexture(normalTexture);
                    if (normalTextureIndex >= 0)
                    {
                        gltfMaterial["normalTexture"] = new Dictionary<string, object>
                        {
                            ["index"] = normalTextureIndex,
                            ["scale"] = ReadFloat(material, "_BumpScale", 1.0f)
                        };
                    }
                }
                var emissionTexture = ReadTexture(material, "_EmissionMap") ?? ReadTexture(material, "_EmissionTex");
                var emissionColor = ReadColor(material, "_EmissionColor", Color.black);
                if (emissionTexture != null)
                {
                    var emissionTextureIndex = ExportTexture(emissionTexture);
                    if (emissionTextureIndex >= 0)
                    {
                        gltfMaterial["emissiveTexture"] = new Dictionary<string, object> { ["index"] = emissionTextureIndex };
                    }
                }
                if (emissionColor.maxColorComponent > 0.0f)
                {
                    gltfMaterial["emissiveFactor"] = FloatArray(emissionColor.r, emissionColor.g, emissionColor.b);
                }
                if (baseColor.a < 0.999f || material.renderQueue >= 3000)
                {
                    gltfMaterial["alphaMode"] = "BLEND";
                }
                if (material.HasProperty("_Cutoff"))
                {
                    gltfMaterial["alphaCutoff"] = ReadFloat(material, "_Cutoff", 0.5f);
                }
                var unAvatarMaterial = BuildUnAvatarMaterialExtras(material);
                if (unAvatarMaterial != null)
                {
                    gltfMaterial["extras"] = new Dictionary<string, object>
                    {
                        ["UN_avatar_material"] = unAvatarMaterial
                    };
                }

                materials.Add(gltfMaterial);
                var index = materials.Count - 1;
                materialIndices[material] = index;
                return index;
            }

            private int ExportDefaultMaterial()
            {
                if (defaultMaterialIndex >= 0)
                {
                    return defaultMaterialIndex;
                }
                materials.Add(new Dictionary<string, object>
                {
                    ["name"] = "Default",
                    ["pbrMetallicRoughness"] = new Dictionary<string, object>
                    {
                        ["baseColorFactor"] = FloatArray(1, 1, 1, 1),
                        ["metallicFactor"] = 0,
                        ["roughnessFactor"] = 0.5
                    },
                    ["doubleSided"] = true
                });
                defaultMaterialIndex = materials.Count - 1;
                return defaultMaterialIndex;
            }

            private Dictionary<string, object> BuildUnAvatarMaterialExtras(Material material)
            {
                var shaderName = material.shader != null ? material.shader.name : "";
                var lowerShader = shaderName.ToLowerInvariant();
                var looksToon = lowerShader.Contains("liltoon") || lowerShader.Contains("mtoon") || material.HasProperty("_ShadeColor") || material.HasProperty("_ShadeTex");
                if (!looksToon)
                {
                    return null;
                }

                var mtoon = new Dictionary<string, object>();
                var shadeColor = ReadColor(material, "_ShadeColor", new Color(0.97f, 0.97f, 0.97f, 1.0f));
                mtoon["shadeColorFactor"] = FloatArray(shadeColor.r, shadeColor.g, shadeColor.b);
                AddTextureIndex(mtoon, "shadeMultiplyTextureIndex", ReadTexture(material, "_ShadeTex") ?? ReadTexture(material, "_1st_ShadeMap"));
                mtoon["shadingShiftFactor"] = ReadFloat(material, "_ShadeShift", ReadFloat(material, "_ShadowBorder", 0.0f));
                mtoon["shadingToonyFactor"] = 1.0f - Mathf.Clamp01(ReadFloat(material, "_ShadowBlur", 0.0f));

                var matcapColor = ReadColor(material, "_MatCapColor", Color.white);
                mtoon["matcapFactor"] = FloatArray(matcapColor.r, matcapColor.g, matcapColor.b);
                AddTextureIndex(mtoon, "matcapTextureIndex", ReadTexture(material, "_MatCapTex") ?? ReadTexture(material, "_MatcapTex"));

                var rimColor = ReadColor(material, "_RimColor", Color.black);
                mtoon["parametricRimColorFactor"] = FloatArray(rimColor.r, rimColor.g, rimColor.b);
                mtoon["parametricRimFresnelPowerFactor"] = ReadFloat(material, "_RimFresnelPower", 5.0f);
                mtoon["rimLightingMixFactor"] = ReadFloat(material, "_RimEnableLighting", 1.0f);
                AddTextureIndex(mtoon, "rimMultiplyTextureIndex", ReadTexture(material, "_RimColorTex"));
                AddTextureIndex(mtoon, "reflectionCubeTextureIndex", ReadTexture(material, "_ReflectionCubeTex"));

                var outlineWidth = ReadFloat(material, "_OutlineWidth", 0.0f);
                mtoon["outlineWidthMode"] = outlineWidth > 0.0f ? "world_coordinates" : "none";
                mtoon["outlineWidthFactor"] = outlineWidth;
                var outlineColor = ReadColor(material, "_OutlineColor", Color.black);
                mtoon["outlineColorFactor"] = FloatArray(outlineColor.r, outlineColor.g, outlineColor.b);
                mtoon["outlineLightingMixFactor"] = ReadFloat(material, "_OutlineEnableLighting", 1.0f);
                AddTextureIndex(mtoon, "outlineWidthMultiplyTextureIndex", ReadTexture(material, "_OutlineWidthMask"));

                mtoon["transparentWithZWrite"] = ReadFloat(material, "_ZWrite", 0.0f) > 0.5f || ReadFloat(material, "_ZWriteMode", 0.0f) > 0.5f;

                return new Dictionary<string, object>
                {
                    ["sourceShader"] = shaderName,
                    ["family"] = lowerShader.Contains("liltoon") ? "liltoon" : lowerShader.Contains("mtoon") ? "mtoon" : "toon",
                    ["unMaterialModel"] = "UNToon",
                    ["mtoon"] = mtoon
                };
            }

            private void AddTextureIndex(Dictionary<string, object> dst, string key, Texture texture)
            {
                if (texture == null)
                {
                    return;
                }
                var textureIndex = ExportTexture(texture);
                if (textureIndex >= 0)
                {
                    dst[key] = textureIndex;
                    return;
                }
                var asset = ExportUnavatarTextureAsset(texture);
                if (asset != null)
                {
                    dst[key + "Asset"] = asset.Id;
                }
            }

            private int ExportTexture(Texture texture)
            {
                if (texture == null)
                {
                    return -1;
                }
                if (textureIndices.TryGetValue(texture, out var existing))
                {
                    return existing;
                }

                string fallbackReason;
                var encoded = TryReadSourceTextureBytes(texture, out fallbackReason);
                if (encoded == null && IsUnavatarExtensionOnlyTexture(MimeTypeFromPath(AssetDatabase.GetAssetPath(texture))))
                {
                    return -1;
                }
                if (encoded == null)
                {
                    encoded = EncodeTexturePng(texture, fallbackReason);
                }
                if (encoded == null || encoded.Bytes == null || encoded.Bytes.Length == 0)
                {
                    return -1;
                }

                var view = AddBufferView(encoded.Bytes);
                images.Add(new Dictionary<string, object>
                {
                    ["name"] = texture.name,
                    ["bufferView"] = view,
                    ["mimeType"] = encoded.MimeType
                });
                exportedTextures.Add(new ExportedTextureRecord
                {
                    Name = texture.name,
                    AssetPath = encoded.AssetPath,
                    SourceExtension = encoded.SourceExtension,
                    SourceMimeType = encoded.SourceMimeType,
                    SourceByteLength = encoded.SourceByteLength,
                    OutputMimeType = encoded.MimeType,
                    OutputByteLength = encoded.Bytes.Length,
                    ExportMode = encoded.ExportMode,
                    FallbackReason = encoded.FallbackReason
                });
                textures.Add(new Dictionary<string, object>
                {
                    ["sampler"] = 0,
                    ["source"] = images.Count - 1
                });
                var index = textures.Count - 1;
                textureIndices[texture] = index;
                return index;
            }

            private sealed class EncodedTexture
            {
                public byte[] Bytes;
                public string MimeType;
                public string AssetPath;
                public string SourceExtension;
                public string SourceMimeType;
                public long SourceByteLength;
                public string ExportMode;
                public string FallbackReason;

                public EncodedTexture(byte[] bytes, string mimeType)
                {
                    Bytes = bytes;
                    MimeType = mimeType;
                }
            }

            private static EncodedTexture TryReadSourceTextureBytes(Texture texture, out string fallbackReason)
            {
                fallbackReason = "";
                var assetPath = AssetDatabase.GetAssetPath(texture);
                if (string.IsNullOrEmpty(assetPath))
                {
                    fallbackReason = "generated_or_runtime_texture";
                    return null;
                }

                var mimeType = GltfImageMimeTypeFromPath(assetPath);
                if (string.IsNullOrEmpty(mimeType))
                {
                    fallbackReason = "unsupported_source_mime";
                    return null;
                }

                var fullPath = Path.IsPathRooted(assetPath)
                    ? assetPath
                    : Path.Combine(Directory.GetCurrentDirectory(), assetPath);
                if (!File.Exists(fullPath))
                {
                    fallbackReason = "source_file_not_found";
                    return null;
                }

                try
                {
                    var bytes = File.ReadAllBytes(fullPath);
                    if (bytes.Length <= 0)
                    {
                        fallbackReason = "empty_source_file";
                        return null;
                    }
                    return new EncodedTexture(bytes, mimeType)
                    {
                        AssetPath = assetPath,
                        SourceExtension = Path.GetExtension(assetPath).ToLowerInvariant(),
                        SourceMimeType = mimeType,
                        SourceByteLength = bytes.Length,
                        ExportMode = "source",
                        FallbackReason = ""
                    };
                }
                catch (Exception ex)
                {
                    Debug.LogWarning("[U.N. Avatar] Source texture read failed for " + texture.name + ": " + ex.Message);
                    fallbackReason = "source_read_failed";
                    return null;
                }
            }

            private static string MimeTypeFromPath(string path)
            {
                var extension = Path.GetExtension(path).ToLowerInvariant();
                switch (extension)
                {
                    case ".png":
                        return "image/png";
                    case ".jpg":
                    case ".jpeg":
                        return "image/jpeg";
                    case ".exr":
                        return "image/exr";
                    default:
                        return null;
                }
            }

            private static string GltfImageMimeTypeFromPath(string path)
            {
                var extension = Path.GetExtension(path).ToLowerInvariant();
                switch (extension)
                {
                    case ".png":
                        return "image/png";
                    case ".jpg":
                    case ".jpeg":
                        return "image/jpeg";
                    default:
                        return null;
                }
            }

            private UnavatarTextureAssetRecord ExportUnavatarTextureAsset(Texture texture)
            {
                if (textureAssetIndices.TryGetValue(texture, out var existing))
                {
                    return existing;
                }
                var assetPath = AssetDatabase.GetAssetPath(texture);
                if (string.IsNullOrEmpty(assetPath))
                {
                    return null;
                }
                var mimeType = MimeTypeFromPath(assetPath);
                if (!IsUnavatarExtensionOnlyTexture(mimeType))
                {
                    return null;
                }
                var fullPath = Path.IsPathRooted(assetPath)
                    ? assetPath
                    : Path.Combine(Directory.GetCurrentDirectory(), assetPath);
                if (!File.Exists(fullPath))
                {
                    return null;
                }

                try
                {
                    var bytes = File.ReadAllBytes(fullPath);
                    if (bytes.Length == 0)
                    {
                        return null;
                    }
                    var metadata = TextureAssetMetadata.FromTexture(texture, assetPath, bytes);
                    var asset = new UnavatarTextureAssetRecord
                    {
                        Id = "texture-asset-" + textureAssets.Count.ToString(CultureInfo.InvariantCulture),
                        Name = texture.name,
                        AssetPath = assetPath,
                        MimeType = mimeType,
                        SourceExtension = Path.GetExtension(assetPath).ToLowerInvariant(),
                        SourcePixelFormat = metadata.SourcePixelFormat,
                        ColorSpace = metadata.ColorSpace,
                        Channels = metadata.Channels,
                        Width = metadata.Width,
                        Height = metadata.Height,
                        Bytes = bytes
                    };
                    textureAssets.Add(asset);
                    textureAssetIndices[texture] = asset;
                    exportedTextures.Add(new ExportedTextureRecord
                    {
                        Name = texture.name,
                        AssetPath = assetPath,
                        SourceExtension = asset.SourceExtension,
                        SourceMimeType = mimeType,
                        SourceByteLength = bytes.Length,
                        OutputMimeType = mimeType,
                        OutputByteLength = bytes.Length,
                        ExportMode = "unavatar_source_asset",
                        FallbackReason = ""
                    });
                    return asset;
                }
                catch (Exception ex)
                {
                    Debug.LogWarning("[U.N. Avatar] Source texture asset read failed for " + texture.name + ": " + ex.Message);
                    return null;
                }
            }

            private static bool IsUnavatarExtensionOnlyTexture(string mimeType)
            {
                return mimeType == "image/exr";
            }

            private sealed class TextureAssetMetadata
            {
                public string SourcePixelFormat = "";
                public string ColorSpace = "linear";
                public string Channels = "";
                public int Width;
                public int Height;

                public static TextureAssetMetadata FromTexture(Texture texture, string assetPath, byte[] bytes)
                {
                    var extension = Path.GetExtension(assetPath).ToLowerInvariant();
                    if (extension == ".exr")
                    {
                        var exr = TryReadExrMetadata(bytes);
                        if (exr != null)
                        {
                            return exr;
                        }
                        return new TextureAssetMetadata
                        {
                            SourcePixelFormat = "unknown_float",
                            ColorSpace = "linear",
                            Channels = ""
                        };
                    }

                    var pixelFormat = SourcePixelFormatHintFromTexture(texture, assetPath);
                    return new TextureAssetMetadata
                    {
                        SourcePixelFormat = pixelFormat,
                        ColorSpace = "linear",
                        Channels = ChannelsHintFromPixelFormat(pixelFormat),
                        Width = texture != null ? texture.width : 0,
                        Height = texture != null ? texture.height : 0
                    };
                }

                private static TextureAssetMetadata TryReadExrMetadata(byte[] bytes)
                {
                    try
                    {
                        if (bytes == null || bytes.Length < 12 || BitConverter.ToUInt32(bytes, 0) != 20000630u)
                        {
                            return null;
                        }

                        var offset = 8;
                        var width = 0;
                        var height = 0;
                        var channelNames = new List<string>();
                        var pixelTypes = new List<int>();

                        while (offset < bytes.Length)
                        {
                            var name = ReadNullTerminatedAscii(bytes, ref offset);
                            if (name == null)
                            {
                                return null;
                            }
                            if (name.Length == 0)
                            {
                                break;
                            }
                            var type = ReadNullTerminatedAscii(bytes, ref offset);
                            if (type == null || offset + 4 > bytes.Length)
                            {
                                return null;
                            }
                            var size = BitConverter.ToInt32(bytes, offset);
                            offset += 4;
                            if (size < 0 || offset + size > bytes.Length)
                            {
                                return null;
                            }

                            if (name == "channels" && type == "chlist")
                            {
                                ReadExrChannels(bytes, offset, size, channelNames, pixelTypes);
                            }
                            else if (name == "dataWindow" && type == "box2i" && size >= 16)
                            {
                                var minX = BitConverter.ToInt32(bytes, offset);
                                var minY = BitConverter.ToInt32(bytes, offset + 4);
                                var maxX = BitConverter.ToInt32(bytes, offset + 8);
                                var maxY = BitConverter.ToInt32(bytes, offset + 12);
                                width = Math.Max(0, maxX - minX + 1);
                                height = Math.Max(0, maxY - minY + 1);
                            }

                            offset += size;
                        }

                        var channels = CanonicalChannels(channelNames);
                        var pixelFormat = PixelFormatFromExrChannels(channels, pixelTypes);
                        return new TextureAssetMetadata
                        {
                            SourcePixelFormat = pixelFormat,
                            ColorSpace = "linear",
                            Channels = channels,
                            Width = width,
                            Height = height
                        };
                    }
                    catch
                    {
                        return null;
                    }
                }

                private static void ReadExrChannels(byte[] bytes, int start, int size, List<string> channelNames, List<int> pixelTypes)
                {
                    var offset = start;
                    var end = start + size;
                    while (offset < end)
                    {
                        var channelName = ReadNullTerminatedAscii(bytes, ref offset);
                        if (channelName == null || channelName.Length == 0)
                        {
                            break;
                        }
                        if (offset + 16 > end)
                        {
                            break;
                        }
                        var pixelType = BitConverter.ToInt32(bytes, offset);
                        offset += 16;
                        channelNames.Add(channelName);
                        pixelTypes.Add(pixelType);
                    }
                }

                private static string ReadNullTerminatedAscii(byte[] bytes, ref int offset)
                {
                    if (offset >= bytes.Length)
                    {
                        return null;
                    }
                    var start = offset;
                    while (offset < bytes.Length && bytes[offset] != 0)
                    {
                        offset++;
                    }
                    if (offset >= bytes.Length)
                    {
                        return null;
                    }
                    var value = Encoding.ASCII.GetString(bytes, start, offset - start);
                    offset++;
                    return value;
                }

                private static string CanonicalChannels(List<string> channelNames)
                {
                    if (channelNames == null || channelNames.Count == 0)
                    {
                        return "";
                    }
                    var names = new HashSet<string>(channelNames.Select(c => c.ToUpperInvariant()));
                    if (names.SetEquals(new[] { "R", "G", "B", "A" }))
                    {
                        return "rgba";
                    }
                    if (names.SetEquals(new[] { "R", "G", "B" }))
                    {
                        return "rgb";
                    }
                    if (names.SetEquals(new[] { "R", "G" }))
                    {
                        return "rg";
                    }
                    if (names.SetEquals(new[] { "R" }) || names.SetEquals(new[] { "Y" }))
                    {
                        return "r";
                    }
                    return "";
                }

                private static string PixelFormatFromExrChannels(string channels, List<int> pixelTypes)
                {
                    if (string.IsNullOrEmpty(channels) || pixelTypes == null || pixelTypes.Count == 0)
                    {
                        return "unknown_float";
                    }
                    var distinctTypes = new HashSet<int>(pixelTypes);
                    if (distinctTypes.Count != 1)
                    {
                        return "unknown_float";
                    }

                    string suffix;
                    switch (pixelTypes[0])
                    {
                        case 0:
                            suffix = "32U";
                            break;
                        case 1:
                            suffix = "16F";
                            break;
                        case 2:
                            suffix = "32F";
                            break;
                        default:
                            return "unknown_float";
                    }
                    return channels.ToUpperInvariant() + suffix;
                }
            }

            private static string SourcePixelFormatHintFromTexture(Texture texture, string assetPath)
            {
                var extension = Path.GetExtension(assetPath).ToLowerInvariant();
                if (extension == ".exr")
                {
                    return "unknown_float";
                }
                if (texture != null && texture.graphicsFormat.ToString().IndexOf("16", StringComparison.Ordinal) >= 0)
                {
                    return texture.graphicsFormat.ToString();
                }
                return "";
            }

            private static string ChannelsHintFromPixelFormat(string pixelFormat)
            {
                if (string.IsNullOrEmpty(pixelFormat))
                {
                    return "";
                }
                var upper = pixelFormat.ToUpperInvariant();
                if (upper.StartsWith("RGBA", StringComparison.Ordinal))
                {
                    return "rgba";
                }
                if (upper.StartsWith("RGB", StringComparison.Ordinal))
                {
                    return "rgb";
                }
                if (upper.StartsWith("RG", StringComparison.Ordinal))
                {
                    return "rg";
                }
                if (upper.StartsWith("R", StringComparison.Ordinal))
                {
                    return "r";
                }
                return "";
            }

            private static EncodedTexture EncodeTexturePng(Texture texture, string fallbackReason)
            {
                var assetPath = AssetDatabase.GetAssetPath(texture);
                var sourceExtension = string.IsNullOrEmpty(assetPath) ? "" : Path.GetExtension(assetPath).ToLowerInvariant();
                var sourceMimeType = string.IsNullOrEmpty(assetPath) ? "" : MimeTypeFromPath(assetPath) ?? "";
                var sourceByteLength = 0L;
                if (!string.IsNullOrEmpty(assetPath))
                {
                    var fullPath = Path.IsPathRooted(assetPath)
                        ? assetPath
                        : Path.Combine(Directory.GetCurrentDirectory(), assetPath);
                    if (File.Exists(fullPath))
                    {
                        sourceByteLength = new FileInfo(fullPath).Length;
                    }
                }

                var oldActive = RenderTexture.active;
                var temporary = RenderTexture.GetTemporary(texture.width, texture.height, 0, RenderTextureFormat.ARGB32, RenderTextureReadWrite.sRGB);
                try
                {
                    Graphics.Blit(texture, temporary);
                    RenderTexture.active = temporary;
                    var readable = new Texture2D(texture.width, texture.height, TextureFormat.RGBA32, false);
                    readable.ReadPixels(new Rect(0, 0, texture.width, texture.height), 0, 0);
                    readable.Apply();
                    var png = readable.EncodeToPNG();
                    UnityEngine.Object.DestroyImmediate(readable);
                    return new EncodedTexture(png, "image/png")
                    {
                        AssetPath = assetPath,
                        SourceExtension = sourceExtension,
                        SourceMimeType = sourceMimeType,
                        SourceByteLength = sourceByteLength,
                        ExportMode = "png_fallback",
                        FallbackReason = string.IsNullOrEmpty(fallbackReason) ? "source_bytes_unavailable" : fallbackReason
                    };
                }
                catch (Exception ex)
                {
                    Debug.LogWarning("[U.N. Avatar] Texture export failed for " + texture.name + ": " + ex.Message);
                    return null;
                }
                finally
                {
                    RenderTexture.active = oldActive;
                    RenderTexture.ReleaseTemporary(temporary);
                }
            }

            private int AddVec3Accessor(Vector3[] values, bool minMax)
            {
                var bytes = new byte[values.Length * 12];
                var min = new Vector3(float.PositiveInfinity, float.PositiveInfinity, float.PositiveInfinity);
                var max = new Vector3(float.NegativeInfinity, float.NegativeInfinity, float.NegativeInfinity);
                for (var i = 0; i < values.Length; i++)
                {
                    WriteFloat(bytes, i * 12, values[i].x);
                    WriteFloat(bytes, i * 12 + 4, values[i].y);
                    WriteFloat(bytes, i * 12 + 8, values[i].z);
                    min = Vector3.Min(min, values[i]);
                    max = Vector3.Max(max, values[i]);
                }
                var view = AddBufferView(bytes);
                var accessor = Accessor(view, values.Length, 5126, "VEC3");
                if (minMax)
                {
                    accessor["min"] = FloatArray(min.x, min.y, min.z);
                    accessor["max"] = FloatArray(max.x, max.y, max.z);
                }
                accessors.Add(accessor);
                return accessors.Count - 1;
            }

            private int AddVec4Accessor(Vector4[] values)
            {
                var bytes = new byte[values.Length * 16];
                for (var i = 0; i < values.Length; i++)
                {
                    WriteFloat(bytes, i * 16, values[i].x);
                    WriteFloat(bytes, i * 16 + 4, values[i].y);
                    WriteFloat(bytes, i * 16 + 8, values[i].z);
                    WriteFloat(bytes, i * 16 + 12, values[i].w);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5126, "VEC4"));
                return accessors.Count - 1;
            }

            private int AddVec2Accessor(Vector2[] values)
            {
                var bytes = new byte[values.Length * 8];
                for (var i = 0; i < values.Length; i++)
                {
                    WriteFloat(bytes, i * 8, values[i].x);
                    WriteFloat(bytes, i * 8 + 4, values[i].y);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5126, "VEC2"));
                return accessors.Count - 1;
            }

            private int AddColorAccessor(Color[] values)
            {
                var bytes = new byte[values.Length * 16];
                for (var i = 0; i < values.Length; i++)
                {
                    WriteFloat(bytes, i * 16, values[i].r);
                    WriteFloat(bytes, i * 16 + 4, values[i].g);
                    WriteFloat(bytes, i * 16 + 8, values[i].b);
                    WriteFloat(bytes, i * 16 + 12, values[i].a);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5126, "VEC4"));
                return accessors.Count - 1;
            }

            private int AddJointsAccessor(BoneWeight[] values)
            {
                var bytes = new byte[values.Length * 8];
                for (var i = 0; i < values.Length; i++)
                {
                    WriteUShort(bytes, i * 8, values[i].boneIndex0);
                    WriteUShort(bytes, i * 8 + 2, values[i].boneIndex1);
                    WriteUShort(bytes, i * 8 + 4, values[i].boneIndex2);
                    WriteUShort(bytes, i * 8 + 6, values[i].boneIndex3);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5123, "VEC4"));
                return accessors.Count - 1;
            }

            private int AddWeightsAccessor(BoneWeight[] values)
            {
                var bytes = new byte[values.Length * 16];
                for (var i = 0; i < values.Length; i++)
                {
                    WriteFloat(bytes, i * 16, values[i].weight0);
                    WriteFloat(bytes, i * 16 + 4, values[i].weight1);
                    WriteFloat(bytes, i * 16 + 8, values[i].weight2);
                    WriteFloat(bytes, i * 16 + 12, values[i].weight3);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Length, 5126, "VEC4"));
                return accessors.Count - 1;
            }

            private int AddMat4Accessor(List<Matrix4x4> values)
            {
                var bytes = new byte[values.Count * 64];
                for (var i = 0; i < values.Count; i++)
                {
                    var offset = i * 64;
                    var m = values[i];
                    WriteFloat(bytes, offset, m.m00);
                    WriteFloat(bytes, offset + 4, m.m10);
                    WriteFloat(bytes, offset + 8, m.m20);
                    WriteFloat(bytes, offset + 12, m.m30);
                    WriteFloat(bytes, offset + 16, m.m01);
                    WriteFloat(bytes, offset + 20, m.m11);
                    WriteFloat(bytes, offset + 24, m.m21);
                    WriteFloat(bytes, offset + 28, m.m31);
                    WriteFloat(bytes, offset + 32, m.m02);
                    WriteFloat(bytes, offset + 36, m.m12);
                    WriteFloat(bytes, offset + 40, m.m22);
                    WriteFloat(bytes, offset + 44, m.m32);
                    WriteFloat(bytes, offset + 48, m.m03);
                    WriteFloat(bytes, offset + 52, m.m13);
                    WriteFloat(bytes, offset + 56, m.m23);
                    WriteFloat(bytes, offset + 60, m.m33);
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, values.Count, 5126, "MAT4"));
                return accessors.Count - 1;
            }

            private int AddIndicesAccessor(int[] indices)
            {
                var useUshort = indices.All(i => i >= 0 && i <= ushort.MaxValue);
                var bytes = new byte[indices.Length * (useUshort ? 2 : 4)];
                for (var i = 0; i < indices.Length; i++)
                {
                    if (useUshort)
                    {
                        WriteUShort(bytes, i * 2, indices[i]);
                    }
                    else
                    {
                        WriteUInt(bytes, i * 4, (uint)indices[i]);
                    }
                }
                var view = AddBufferView(bytes);
                accessors.Add(Accessor(view, indices.Length, useUshort ? 5123 : 5125, "SCALAR"));
                return accessors.Count - 1;
            }

            private int AddBufferView(byte[] bytes)
            {
                var offset = buffer.Append(bytes);
                bufferViews.Add(new Dictionary<string, object>
                {
                    ["buffer"] = 0,
                    ["byteOffset"] = offset,
                    ["byteLength"] = bytes.Length
                });
                return bufferViews.Count - 1;
            }

            private static Dictionary<string, object> Accessor(int bufferView, int count, int componentType, string type)
            {
                return new Dictionary<string, object>
                {
                    ["bufferView"] = bufferView,
                    ["byteOffset"] = 0,
                    ["componentType"] = componentType,
                    ["count"] = count,
                    ["type"] = type
                };
            }

            private static Color ReadColor(Material material, string property, Color fallback)
            {
                return material.HasProperty(property) ? material.GetColor(property) : fallback;
            }

            private static float ReadFloat(Material material, string property, float fallback)
            {
                return material.HasProperty(property) ? material.GetFloat(property) : fallback;
            }

            private static Texture ReadTexture(Material material, string property)
            {
                return material.HasProperty(property) ? material.GetTexture(property) : null;
            }

            private static void WriteGlb(string path, string json, byte[] bin)
            {
                var jsonBytes = Pad(Encoding.UTF8.GetBytes(json), 0x20);
                var binBytes = Pad(bin ?? Array.Empty<byte>(), 0x00);
                var total = 12 + 8 + jsonBytes.Length + (binBytes.Length > 0 ? 8 + binBytes.Length : 0);
                using (var stream = File.Create(path))
                using (var writer = new BinaryWriter(stream))
                {
                    writer.Write(GlbMagic);
                    writer.Write((uint)2);
                    writer.Write((uint)total);
                    writer.Write((uint)jsonBytes.Length);
                    writer.Write(JsonChunkType);
                    writer.Write(jsonBytes);
                    if (binBytes.Length > 0)
                    {
                        writer.Write((uint)binBytes.Length);
                        writer.Write(BinChunkType);
                        writer.Write(binBytes);
                    }
                }
            }

            private static List<object> FloatArray(params float[] values)
            {
                return values.Select(v => (object)v).ToList();
            }

            private static byte[] Pad(byte[] data, byte value)
            {
                var length = (data.Length + 3) & ~3;
                if (length == data.Length)
                {
                    return data;
                }
                var padded = new byte[length];
                Buffer.BlockCopy(data, 0, padded, 0, data.Length);
                for (var i = data.Length; i < padded.Length; i++)
                {
                    padded[i] = value;
                }
                return padded;
            }

            private static void WriteFloat(byte[] bytes, int offset, float value)
            {
                Buffer.BlockCopy(BitConverter.GetBytes(value), 0, bytes, offset, 4);
            }

            private static void WriteUShort(byte[] bytes, int offset, int value)
            {
                Buffer.BlockCopy(BitConverter.GetBytes((ushort)value), 0, bytes, offset, 2);
            }

            private static void WriteUInt(byte[] bytes, int offset, uint value)
            {
                Buffer.BlockCopy(BitConverter.GetBytes(value), 0, bytes, offset, 4);
            }
        }

        private sealed class MorphTargetRecord
        {
            public string Name;
            public int PositionAccessor;
            public int NormalAccessor;

            public Dictionary<string, object> ToJson()
            {
                var json = new Dictionary<string, object>();
                if (PositionAccessor >= 0)
                {
                    json["POSITION"] = PositionAccessor;
                }
                if (NormalAccessor >= 0)
                {
                    json["NORMAL"] = NormalAccessor;
                }
                return json;
            }
        }

        private sealed class BinaryBuffer
        {
            private readonly List<byte> bytes = new List<byte>();

            public int Length => bytes.Count;

            public int Append(byte[] data)
            {
                while ((bytes.Count & 3) != 0)
                {
                    bytes.Add(0);
                }
                var offset = bytes.Count;
                bytes.AddRange(data);
                while ((bytes.Count & 3) != 0)
                {
                    bytes.Add(0);
                }
                return offset;
            }

            public byte[] ToArray()
            {
                return bytes.ToArray();
            }
        }
    }

    internal static class GlbExtensionPatcher
    {
        private const uint GlbMagic = 0x46546C67;
        private const uint JsonChunkType = 0x4E4F534A;

        public static string ReadRootJson(string glbPath)
        {
            var bytes = File.ReadAllBytes(glbPath);
            if (bytes.Length < 20)
            {
                throw new InvalidDataException("GLB is too small.");
            }
            var magic = ReadUInt32(bytes, 0);
            var version = ReadUInt32(bytes, 4);
            if (magic != GlbMagic || version != 2)
            {
                throw new InvalidDataException("Expected GLB version 2.");
            }

            var offset = 12;
            while (offset + 8 <= bytes.Length)
            {
                var length = checked((int)ReadUInt32(bytes, offset));
                var type = ReadUInt32(bytes, offset + 4);
                offset += 8;
                if (offset + length > bytes.Length)
                {
                    throw new InvalidDataException("GLB chunk exceeds file size.");
                }
                if (type == JsonChunkType)
                {
                    return Encoding.UTF8.GetString(bytes, offset, length).TrimEnd('\0', ' ', '\t', '\r', '\n');
                }
                offset += length;
            }

            throw new InvalidDataException("GLB JSON chunk was not found.");
        }

        public static string ExtractRootExtensionJson(string json, string extensionName)
        {
            var extensionsIndex = json.IndexOf("\"extensions\"", StringComparison.Ordinal);
            if (extensionsIndex < 0)
            {
                throw new InvalidDataException("Root extensions object was not found.");
            }
            var colon = json.IndexOf(':', extensionsIndex);
            var objectStart = json.IndexOf('{', colon);
            var objectEnd = FindMatchingBrace(json, objectStart);
            var extensionKey = "\"" + MiniJson.EscapeString(extensionName) + "\"";
            var keyIndex = json.IndexOf(extensionKey, objectStart, objectEnd - objectStart, StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                throw new InvalidDataException(extensionName + " extension was not found.");
            }
            var extensionColon = json.IndexOf(':', keyIndex);
            var extensionStart = json.IndexOf('{', extensionColon);
            var extensionEnd = FindMatchingBrace(json, extensionStart);
            return json.Substring(extensionStart, extensionEnd - extensionStart + 1);
        }

        public static void PatchRootExtension(
            string sourceGlb,
            string destinationGlb,
            string extensionName,
            Dictionary<string, object> payload,
            List<UnavatarTextureAssetRecord> textureAssets = null)
        {
            var bytes = File.ReadAllBytes(sourceGlb);
            if (bytes.Length < 20)
            {
                throw new InvalidDataException("GLB is too small.");
            }
            var magic = ReadUInt32(bytes, 0);
            var version = ReadUInt32(bytes, 4);
            if (magic != GlbMagic || version != 2)
            {
                throw new InvalidDataException("Expected GLB version 2.");
            }

            var chunks = new List<GlbChunk>();
            var offset = 12;
            while (offset + 8 <= bytes.Length)
            {
                var length = checked((int)ReadUInt32(bytes, offset));
                var type = ReadUInt32(bytes, offset + 4);
                offset += 8;
                if (offset + length > bytes.Length)
                {
                    throw new InvalidDataException("GLB chunk exceeds file size.");
                }
                var data = new byte[length];
                Buffer.BlockCopy(bytes, offset, data, 0, length);
                chunks.Add(new GlbChunk { Type = type, Data = data });
                offset += length;
            }

            var jsonChunk = chunks.FirstOrDefault(c => c.Type == JsonChunkType);
            if (jsonChunk == null)
            {
                throw new InvalidDataException("GLB JSON chunk was not found.");
            }

            var binChunk = chunks.FirstOrDefault(c => c.Type == 0x004E4942);
            if (binChunk == null)
            {
                binChunk = new GlbChunk { Type = 0x004E4942, Data = Array.Empty<byte>() };
                chunks.Add(binChunk);
            }

            var json = Encoding.UTF8.GetString(jsonChunk.Data).TrimEnd('\0', ' ', '\t', '\r', '\n');
            if (textureAssets != null && textureAssets.Count > 0)
            {
                json = AppendTextureAssetBufferViews(json, binChunk, textureAssets);
                payload["textureAssets"] = textureAssets
                    .Select(asset => asset.ToJson())
                    .Cast<object>()
                    .ToList();
            }
            json = PatchRootJson(json, extensionName, payload);
            jsonChunk.Data = Pad(Encoding.UTF8.GetBytes(json), 0x20);

            WriteGlb(destinationGlb, chunks);
        }

        private static string AppendTextureAssetBufferViews(string json, GlbChunk binChunk, List<UnavatarTextureAssetRecord> textureAssets)
        {
            if (textureAssets == null || textureAssets.Count == 0)
            {
                return json;
            }

            var bin = new List<byte>(binChunk.Data ?? Array.Empty<byte>());
            var viewJson = new List<string>();
            foreach (var asset in textureAssets)
            {
                if (asset == null || asset.Bytes == null || asset.Bytes.Length == 0)
                {
                    continue;
                }
                while ((bin.Count & 3) != 0)
                {
                    bin.Add(0);
                }
                var byteOffset = bin.Count;
                bin.AddRange(asset.Bytes);
                while ((bin.Count & 3) != 0)
                {
                    bin.Add(0);
                }
                asset.BufferView = ExistingArrayLength(json, "bufferViews") + viewJson.Count;
                viewJson.Add("{\"buffer\":0,\"byteOffset\":" + byteOffset.ToString(CultureInfo.InvariantCulture) + ",\"byteLength\":" + asset.Bytes.Length.ToString(CultureInfo.InvariantCulture) + "}");
            }
            binChunk.Data = Pad(bin.ToArray(), 0x00);
            if (viewJson.Count == 0)
            {
                return UpdatePrimaryBufferByteLength(json, binChunk.Data.Length);
            }
            json = AppendRootArrayItems(json, "bufferViews", viewJson);
            json = UpdatePrimaryBufferByteLength(json, binChunk.Data.Length);
            return json;
        }

        private static int ExistingArrayLength(string json, string propertyName)
        {
            var keyIndex = json.IndexOf("\"" + propertyName + "\"", StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                return 0;
            }
            var colon = json.IndexOf(':', keyIndex);
            var arrayStart = json.IndexOf('[', colon);
            var arrayEnd = FindMatchingBracket(json, arrayStart);
            var inner = json.Substring(arrayStart + 1, arrayEnd - arrayStart - 1).Trim();
            if (inner.Length == 0)
            {
                return 0;
            }
            var count = 1;
            var depth = 0;
            var inString = false;
            var escaped = false;
            for (var i = 0; i < inner.Length; i++)
            {
                var c = inner[i];
                if (inString)
                {
                    if (escaped)
                    {
                        escaped = false;
                    }
                    else if (c == '\\')
                    {
                        escaped = true;
                    }
                    else if (c == '"')
                    {
                        inString = false;
                    }
                    continue;
                }
                if (c == '"')
                {
                    inString = true;
                }
                else if (c == '[' || c == '{')
                {
                    depth++;
                }
                else if (c == ']' || c == '}')
                {
                    depth--;
                }
                else if (c == ',' && depth == 0)
                {
                    count++;
                }
            }
            return count;
        }

        private static string AppendRootArrayItems(string json, string propertyName, List<string> items)
        {
            if (items == null || items.Count == 0)
            {
                return json;
            }
            var keyIndex = json.IndexOf("\"" + propertyName + "\"", StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                return InsertRootProperty(json, "\"" + propertyName + "\":[" + string.Join(",", items) + "]");
            }
            var colon = json.IndexOf(':', keyIndex);
            var arrayStart = json.IndexOf('[', colon);
            var arrayEnd = FindMatchingBracket(json, arrayStart);
            var existing = json.Substring(arrayStart + 1, arrayEnd - arrayStart - 1).Trim();
            var replacement = existing.Length == 0
                ? "[" + string.Join(",", items) + "]"
                : "[" + existing + "," + string.Join(",", items) + "]";
            return json.Substring(0, arrayStart) + replacement + json.Substring(arrayEnd + 1);
        }

        private static string UpdatePrimaryBufferByteLength(string json, int byteLength)
        {
            var buffersIndex = json.IndexOf("\"buffers\"", StringComparison.Ordinal);
            if (buffersIndex < 0)
            {
                return InsertRootProperty(json, "\"buffers\":[{\"byteLength\":" + byteLength.ToString(CultureInfo.InvariantCulture) + "}]");
            }
            var byteLengthIndex = json.IndexOf("\"byteLength\"", buffersIndex, StringComparison.Ordinal);
            if (byteLengthIndex < 0)
            {
                return json;
            }
            var colon = json.IndexOf(':', byteLengthIndex);
            var valueStart = colon + 1;
            while (valueStart < json.Length && char.IsWhiteSpace(json[valueStart]))
            {
                valueStart++;
            }
            var valueEnd = valueStart;
            while (valueEnd < json.Length && char.IsDigit(json[valueEnd]))
            {
                valueEnd++;
            }
            return json.Substring(0, valueStart) + byteLength.ToString(CultureInfo.InvariantCulture) + json.Substring(valueEnd);
        }

        private static string PatchRootJson(string json, string extensionName, Dictionary<string, object> payload)
        {
            json = AddExtensionUsed(json, extensionName);
            var extensionJson = MiniJson.Serialize(payload);
            var property = "\"" + MiniJson.EscapeString(extensionName) + "\":" + extensionJson;
            var extensionsIndex = json.IndexOf("\"extensions\"", StringComparison.Ordinal);
            if (extensionsIndex < 0)
            {
                return InsertRootProperty(json, "\"extensions\":{" + property + "}");
            }

            var colon = json.IndexOf(':', extensionsIndex);
            var objectStart = json.IndexOf('{', colon);
            var objectEnd = FindMatchingBrace(json, objectStart);
            var existing = json.Substring(objectStart + 1, objectEnd - objectStart - 1).Trim();
            var replacement = existing.Length == 0 ? "{" + property + "}" : "{" + existing + "," + property + "}";
            return json.Substring(0, objectStart) + replacement + json.Substring(objectEnd + 1);
        }

        private static string AddExtensionUsed(string json, string extensionName)
        {
            if (json.Contains("\"" + extensionName + "\""))
            {
                return json;
            }

            var keyIndex = json.IndexOf("\"extensionsUsed\"", StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                return InsertRootProperty(json, "\"extensionsUsed\":[\"" + MiniJson.EscapeString(extensionName) + "\"]");
            }

            var colon = json.IndexOf(':', keyIndex);
            var arrayStart = json.IndexOf('[', colon);
            var arrayEnd = FindMatchingBracket(json, arrayStart);
            var existing = json.Substring(arrayStart + 1, arrayEnd - arrayStart - 1).Trim();
            var replacement = existing.Length == 0
                ? "[\"" + MiniJson.EscapeString(extensionName) + "\"]"
                : "[" + existing + ",\"" + MiniJson.EscapeString(extensionName) + "\"]";
            return json.Substring(0, arrayStart) + replacement + json.Substring(arrayEnd + 1);
        }

        private static string InsertRootProperty(string json, string property)
        {
            var end = json.LastIndexOf('}');
            if (end < 0)
            {
                throw new InvalidDataException("GLB JSON root is not an object.");
            }
            var before = json.Substring(0, end).TrimEnd();
            var separator = before.EndsWith("{", StringComparison.Ordinal) ? "" : ",";
            return before + separator + property + json.Substring(end);
        }

        private static int FindMatchingBrace(string text, int openIndex)
        {
            return FindMatching(text, openIndex, '{', '}');
        }

        private static int FindMatchingBracket(string text, int openIndex)
        {
            return FindMatching(text, openIndex, '[', ']');
        }

        private static int FindMatching(string text, int openIndex, char open, char close)
        {
            if (openIndex < 0 || text[openIndex] != open)
            {
                throw new InvalidDataException("JSON delimiter was not found.");
            }
            var depth = 0;
            var inString = false;
            var escaped = false;
            for (var i = openIndex; i < text.Length; i++)
            {
                var c = text[i];
                if (inString)
                {
                    if (escaped)
                    {
                        escaped = false;
                    }
                    else if (c == '\\')
                    {
                        escaped = true;
                    }
                    else if (c == '"')
                    {
                        inString = false;
                    }
                    continue;
                }

                if (c == '"')
                {
                    inString = true;
                }
                else if (c == open)
                {
                    depth++;
                }
                else if (c == close)
                {
                    depth--;
                    if (depth == 0)
                    {
                        return i;
                    }
                }
            }
            throw new InvalidDataException("Matching JSON delimiter was not found.");
        }

        private static void WriteGlb(string path, List<GlbChunk> chunks)
        {
            var totalLength = 12 + chunks.Sum(c => 8 + c.Data.Length);
            using (var stream = File.Create(path))
            using (var writer = new BinaryWriter(stream))
            {
                writer.Write(GlbMagic);
                writer.Write((uint)2);
                writer.Write((uint)totalLength);
                foreach (var chunk in chunks)
                {
                    writer.Write((uint)chunk.Data.Length);
                    writer.Write(chunk.Type);
                    writer.Write(chunk.Data);
                }
            }
        }

        private static byte[] Pad(byte[] data, byte value)
        {
            var paddedLength = (data.Length + 3) & ~3;
            if (paddedLength == data.Length)
            {
                return data;
            }
            var padded = new byte[paddedLength];
            Buffer.BlockCopy(data, 0, padded, 0, data.Length);
            for (var i = data.Length; i < padded.Length; i++)
            {
                padded[i] = value;
            }
            return padded;
        }

        private static uint ReadUInt32(byte[] bytes, int offset)
        {
            return BitConverter.ToUInt32(bytes, offset);
        }

        private sealed class GlbChunk
        {
            public uint Type;
            public byte[] Data;
        }
    }

    internal static class MiniJson
    {
        public static object Deserialize(string json)
        {
            return new Parser(json).Parse();
        }

        public static string Serialize(object value)
        {
            var sb = new StringBuilder();
            WriteValue(sb, value);
            return sb.ToString();
        }

        public static string EscapeString(string value)
        {
            var sb = new StringBuilder();
            foreach (var c in value)
            {
                switch (c)
                {
                    case '"': sb.Append("\\\""); break;
                    case '\\': sb.Append("\\\\"); break;
                    case '\b': sb.Append("\\b"); break;
                    case '\f': sb.Append("\\f"); break;
                    case '\n': sb.Append("\\n"); break;
                    case '\r': sb.Append("\\r"); break;
                    case '\t': sb.Append("\\t"); break;
                    default:
                        if (c < 0x20)
                        {
                            sb.Append("\\u");
                            sb.Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        }
                        else
                        {
                            sb.Append(c);
                        }
                        break;
                }
            }
            return sb.ToString();
        }

        private static void WriteValue(StringBuilder sb, object value)
        {
            if (value == null)
            {
                sb.Append("null");
                return;
            }

            switch (value)
            {
                case string s:
                    sb.Append('"').Append(EscapeString(s)).Append('"');
                    break;
                case bool b:
                    sb.Append(b ? "true" : "false");
                    break;
                case byte _:
                case sbyte _:
                case short _:
                case ushort _:
                case int _:
                case uint _:
                case long _:
                case ulong _:
                case float _:
                case double _:
                case decimal _:
                    sb.Append(Convert.ToString(value, CultureInfo.InvariantCulture));
                    break;
                case IDictionary<string, object> map:
                    WriteObject(sb, map);
                    break;
                case IDictionary dictionary:
                    WriteDictionary(sb, dictionary);
                    break;
                case IEnumerable enumerable:
                    WriteArray(sb, enumerable);
                    break;
                default:
                    sb.Append('"').Append(EscapeString(Convert.ToString(value, CultureInfo.InvariantCulture))).Append('"');
                    break;
            }
        }

        private static void WriteObject(StringBuilder sb, IDictionary<string, object> map)
        {
            sb.Append('{');
            var first = true;
            foreach (var item in map)
            {
                if (!first)
                {
                    sb.Append(',');
                }
                first = false;
                sb.Append('"').Append(EscapeString(item.Key)).Append("\":");
                WriteValue(sb, item.Value);
            }
            sb.Append('}');
        }

        private static void WriteDictionary(StringBuilder sb, IDictionary map)
        {
            sb.Append('{');
            var first = true;
            foreach (DictionaryEntry item in map)
            {
                if (!first)
                {
                    sb.Append(',');
                }
                first = false;
                sb.Append('"').Append(EscapeString(Convert.ToString(item.Key, CultureInfo.InvariantCulture))).Append("\":");
                WriteValue(sb, item.Value);
            }
            sb.Append('}');
        }

        private static void WriteArray(StringBuilder sb, IEnumerable values)
        {
            sb.Append('[');
            var first = true;
            foreach (var item in values)
            {
                if (!first)
                {
                    sb.Append(',');
                }
                first = false;
                WriteValue(sb, item);
            }
            sb.Append(']');
        }

        private sealed class Parser
        {
            private readonly string json;
            private int index;

            public Parser(string json)
            {
                this.json = json ?? "";
            }

            public object Parse()
            {
                var value = ParseValue();
                SkipWhitespace();
                return value;
            }

            private object ParseValue()
            {
                SkipWhitespace();
                if (index >= json.Length)
                {
                    throw new InvalidDataException("Unexpected end of JSON.");
                }

                switch (json[index])
                {
                    case '{': return ParseObject();
                    case '[': return ParseArray();
                    case '"': return ParseString();
                    case 't': Expect("true"); return true;
                    case 'f': Expect("false"); return false;
                    case 'n': Expect("null"); return null;
                    default: return ParseNumber();
                }
            }

            private Dictionary<string, object> ParseObject()
            {
                Expect('{');
                var result = new Dictionary<string, object>();
                SkipWhitespace();
                if (TryConsume('}'))
                {
                    return result;
                }
                while (true)
                {
                    var key = ParseString();
                    SkipWhitespace();
                    Expect(':');
                    result[key] = ParseValue();
                    SkipWhitespace();
                    if (TryConsume('}'))
                    {
                        return result;
                    }
                    Expect(',');
                }
            }

            private List<object> ParseArray()
            {
                Expect('[');
                var result = new List<object>();
                SkipWhitespace();
                if (TryConsume(']'))
                {
                    return result;
                }
                while (true)
                {
                    result.Add(ParseValue());
                    SkipWhitespace();
                    if (TryConsume(']'))
                    {
                        return result;
                    }
                    Expect(',');
                }
            }

            private string ParseString()
            {
                Expect('"');
                var sb = new StringBuilder();
                while (index < json.Length)
                {
                    var c = json[index++];
                    if (c == '"')
                    {
                        return sb.ToString();
                    }
                    if (c != '\\')
                    {
                        sb.Append(c);
                        continue;
                    }
                    if (index >= json.Length)
                    {
                        throw new InvalidDataException("Invalid JSON string escape.");
                    }
                    var escaped = json[index++];
                    switch (escaped)
                    {
                        case '"': sb.Append('"'); break;
                        case '\\': sb.Append('\\'); break;
                        case '/': sb.Append('/'); break;
                        case 'b': sb.Append('\b'); break;
                        case 'f': sb.Append('\f'); break;
                        case 'n': sb.Append('\n'); break;
                        case 'r': sb.Append('\r'); break;
                        case 't': sb.Append('\t'); break;
                        case 'u':
                            if (index + 4 > json.Length)
                            {
                                throw new InvalidDataException("Invalid JSON unicode escape.");
                            }
                            var hex = json.Substring(index, 4);
                            sb.Append((char)int.Parse(hex, NumberStyles.HexNumber, CultureInfo.InvariantCulture));
                            index += 4;
                            break;
                        default:
                            throw new InvalidDataException("Invalid JSON string escape.");
                    }
                }
                throw new InvalidDataException("Unterminated JSON string.");
            }

            private double ParseNumber()
            {
                var start = index;
                if (json[index] == '-')
                {
                    index++;
                }
                while (index < json.Length && char.IsDigit(json[index]))
                {
                    index++;
                }
                if (index < json.Length && json[index] == '.')
                {
                    index++;
                    while (index < json.Length && char.IsDigit(json[index]))
                    {
                        index++;
                    }
                }
                if (index < json.Length && (json[index] == 'e' || json[index] == 'E'))
                {
                    index++;
                    if (index < json.Length && (json[index] == '+' || json[index] == '-'))
                    {
                        index++;
                    }
                    while (index < json.Length && char.IsDigit(json[index]))
                    {
                        index++;
                    }
                }
                return double.Parse(json.Substring(start, index - start), CultureInfo.InvariantCulture);
            }

            private void SkipWhitespace()
            {
                while (index < json.Length && char.IsWhiteSpace(json[index]))
                {
                    index++;
                }
            }

            private bool TryConsume(char expected)
            {
                SkipWhitespace();
                if (index < json.Length && json[index] == expected)
                {
                    index++;
                    return true;
                }
                return false;
            }

            private void Expect(char expected)
            {
                SkipWhitespace();
                if (index >= json.Length || json[index] != expected)
                {
                    throw new InvalidDataException("Expected `" + expected + "` in JSON.");
                }
                index++;
            }

            private void Expect(string expected)
            {
                if (index + expected.Length > json.Length || json.Substring(index, expected.Length) != expected)
                {
                    throw new InvalidDataException("Expected `" + expected + "` in JSON.");
                }
                index += expected.Length;
            }
        }
    }
}
