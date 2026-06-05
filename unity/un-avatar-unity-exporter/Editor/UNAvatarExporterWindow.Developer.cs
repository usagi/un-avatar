using UnityEditor;
using UnityEngine;

namespace UNAvatar.UnityExporter
{
    public sealed partial class UNAvatarExporterWindow
    {
        private void DrawPngBenchmarkDeveloperControls()
        {
            EditorGUILayout.Space(4);
            EditorGUILayout.LabelField("PNG Encoder Benchmark", EditorStyles.boldLabel);

            var enabled = PngEncoderBenchmark.IsEnabled;
            var nextEnabled = EditorGUILayout.ToggleLeft("Enable PNG encoder benchmark", enabled);
            if (nextEnabled != enabled)
            {
                PngEncoderBenchmark.IsEnabled = nextEnabled;
            }

            using (new EditorGUI.DisabledScope(!PngEncoderBenchmark.IsEnabled))
            {
                if (GUILayout.Button("Run PNG Encoder Benchmark", GUILayout.Height(24)))
                {
                    PngEncoderBenchmark.RunMenu();
                }
            }
        }
    }
}
