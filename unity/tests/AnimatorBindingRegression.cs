using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using UnityEditor;
using UnityEngine;
using UNAvatar.UnityExporter;

// Copy into Assets/Editor of an isolated Unity 2022.3 project containing the exporter.
// Run with -batchmode -nographics -executeMethod AnimatorBindingRegression.Run.
public static class AnimatorBindingRegression
{
    [Serializable]
    public sealed class CaseResult
    {
        public string name;
        public int expectedFloat;
        public int actualFloat;
        public int expectedObject;
        public int actualObject;
        public bool valuesMatch;
        public bool passed;
    }

    [Serializable]
    public sealed class Report
    {
        public bool passed;
        public string error;
        public List<CaseResult> cases = new List<CaseResult>();
    }

    public static void Run()
    {
        var report = new Report();
        UNAvatarExporterWindow window = null;
        GameObject root = null;
        try
        {
            window = ScriptableObject.CreateInstance<UNAvatarExporterWindow>();
            root = new GameObject("BindingRegression");
            var method = typeof(UNAvatarExporterWindow).GetMethod("AnimatorClipToJson", BindingFlags.NonPublic | BindingFlags.Instance);
            if (method == null) throw new InvalidOperationException("AnimatorClipToJson not found");
            var material = new Material(Shader.Find("Hidden/InternalErrorShader"));
            var clip = new AnimationClip { name = "more-than-32-bindings" };
            for (int i = 0; i < 40; i++)
            {
                var binding = EditorCurveBinding.FloatCurve("Body", typeof(SkinnedMeshRenderer), "blendShape.Face" + i);
                AnimationUtility.SetEditorCurve(clip, binding, AnimationCurve.Constant(0, 1, i + 0.25f));
                var objectBinding = EditorCurveBinding.PPtrCurve("Part" + i, typeof(MeshRenderer), "m_Materials.Array.data[0]");
                AnimationUtility.SetObjectReferenceCurve(clip, objectBinding, new[] { new ObjectReferenceKeyframe { time = 0, value = material } });
            }
            // Unsupported transform animation must remain excluded.
            AnimationUtility.SetEditorCurve(clip, EditorCurveBinding.FloatCurve("Body", typeof(Transform), "m_LocalPosition.x"), AnimationCurve.Constant(0, 1, 2));
            Check(report, window, root, method, clip);
            UnityEngine.Object.DestroyImmediate(clip);
            UnityEngine.Object.DestroyImmediate(material);

            // Optional private fixtures stay outside the repository.
            foreach (var name in new[] { "Lunary_smile1", "Lunary_smile2", "Lunary_sad1" })
            {
                var fixture = AssetDatabase.LoadAssetAtPath<AnimationClip>("Assets/Clips/" + name + ".anim");
                if (fixture != null) Check(report, window, root, method, fixture);
            }
            report.passed = report.cases.Count > 0 && report.cases.All(c => c.passed);
        }
        catch (Exception error)
        {
            report.error = error.ToString();
        }
        finally
        {
            if (root != null) UnityEngine.Object.DestroyImmediate(root);
            if (window != null) UnityEngine.Object.DestroyImmediate(window);
            File.WriteAllText("animator-binding-regression.json", JsonUtility.ToJson(report, true));
            Debug.Log("Animator binding regression passed=" + report.passed);
            EditorApplication.Exit(report.passed ? 0 : 1);
        }
    }

    private static void Check(Report report, UNAvatarExporterWindow window, GameObject root, MethodInfo method, AnimationClip clip)
    {
        var exported = (Dictionary<string, object>)method.Invoke(window, new object[] { root.transform, clip });
        var floats = (List<object>)exported["curveBindings"];
        var objects = (List<object>)exported["objectReferenceBindings"];
        var supported = AnimationUtility.GetCurveBindings(clip).Where(b => b.propertyName.StartsWith("blendShape.", StringComparison.Ordinal)).ToArray();
        var sourceObjects = AnimationUtility.GetObjectReferenceCurveBindings(clip);
        bool match = true;
        foreach (var binding in supported)
        {
            var row = floats.Cast<Dictionary<string, object>>().FirstOrDefault(r => (string)r["path"] == binding.path && (string)r["propertyName"] == binding.propertyName);
            var curve = AnimationUtility.GetEditorCurve(clip, binding);
            match &= row != null && curve.keys.Length > 0 && row.ContainsKey("firstValue") && row.ContainsKey("lastValue") &&
                Math.Abs(Convert.ToSingle(row["firstValue"]) - curve.keys[0].value) < 0.0001f &&
                Math.Abs(Convert.ToSingle(row["lastValue"]) - curve.keys[curve.keys.Length - 1].value) < 0.0001f;
        }
        var result = new CaseResult {
            name = clip.name, expectedFloat = supported.Length, actualFloat = floats.Count,
            expectedObject = sourceObjects.Length, actualObject = objects.Count, valuesMatch = match
        };
        result.passed = result.expectedFloat == result.actualFloat && result.expectedObject == result.actualObject && match &&
            Convert.ToInt32(exported["curveBindingCount"]) == floats.Count && Convert.ToInt32(exported["objectReferenceBindingCount"]) == objects.Count;
        report.cases.Add(result);
        if (result.passed && AssetDatabase.Contains(clip))
        {
            // Keep private fixture output local for importer/Renderer integration.
            var jsonType = typeof(UNAvatarExporterWindow).Assembly.GetType("UNAvatar.UnityExporter.MiniJson", true);
            var serialize = jsonType.GetMethod("Serialize", BindingFlags.Public | BindingFlags.Static);
            Directory.CreateDirectory("exported-clips");
            File.WriteAllText(Path.Combine("exported-clips", clip.name + ".json"), (string)serialize.Invoke(null, new object[] { exported }));
        }
        Debug.Log(clip.name + " float=" + result.actualFloat + "/" + result.expectedFloat + " object=" + result.actualObject + "/" + result.expectedObject);
    }
}
