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
            using (new EditorGUI.DisabledScope(IsCurrentToBaseOnlyExportMode()))
            {
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
            }
        }
    }
}
