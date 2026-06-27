using System;
using System.Collections.Generic;
using System.Globalization;
using UnityEngine;
using UnityEngine.Animations;

namespace UNAvatar.UnityExporter
{
    public sealed partial class UNAvatarExporterWindow
    {
        private List<object> BuildNodeConstraintsPayload(GameObject rootObject)
        {
            var payload = new List<object>();
            if (rootObject == null)
            {
                return payload;
            }

            var sourceIdCounts = new Dictionary<string, int>(StringComparer.Ordinal);
            foreach (var constraint in rootObject.GetComponentsInChildren<ParentConstraint>(true))
            {
                if (constraint == null || !constraint.enabled || !constraint.constraintActive || constraint.weight <= 0.0f)
                {
                    continue;
                }
                var item = BuildParentConstraintPayload(rootObject.transform, constraint, sourceIdCounts);
                if (item != null)
                {
                    payload.Add(item);
                }
            }
            return payload;
        }

        private static Dictionary<string, object> BuildParentConstraintPayload(
            Transform root,
            ParentConstraint constraint,
            Dictionary<string, int> sourceIdCounts)
        {
            var sources = new List<object>();
            for (var i = 0; i < constraint.sourceCount; i++)
            {
                var source = constraint.GetSource(i);
                if (source.sourceTransform == null || source.weight <= 0.0f)
                {
                    continue;
                }
                sources.Add(new Dictionary<string, object>
                {
                    ["node"] = TransformTargetJson(root, source.sourceTransform),
                    ["weight"] = source.weight,
                    ["translationOffset"] = Vector3List(constraint.GetTranslationOffset(i)),
                    ["rotationOffset"] = Vector3List(constraint.GetRotationOffset(i))
                });
            }
            if (sources.Count == 0)
            {
                return null;
            }

            return new Dictionary<string, object>
            {
                ["id"] = BuildUnityConstraintSourceId(root, constraint.transform, "parent", sourceIdCounts),
                ["kind"] = "parent",
                ["source"] = "unity_parent_constraint",
                ["target"] = TransformTargetJson(root, constraint.transform),
                ["weight"] = constraint.weight,
                ["translateX"] = AxisHas(constraint.translationAxis, Axis.X),
                ["translateY"] = AxisHas(constraint.translationAxis, Axis.Y),
                ["translateZ"] = AxisHas(constraint.translationAxis, Axis.Z),
                ["rotateX"] = AxisHas(constraint.rotationAxis, Axis.X),
                ["rotateY"] = AxisHas(constraint.rotationAxis, Axis.Y),
                ["rotateZ"] = AxisHas(constraint.rotationAxis, Axis.Z),
                ["translationAtRest"] = Vector3List(constraint.translationAtRest),
                ["rotationAtRest"] = Vector3List(constraint.rotationAtRest),
                ["sources"] = sources
            };
        }

        private static string BuildUnityConstraintSourceId(
            Transform root,
            Transform target,
            string kind,
            Dictionary<string, int> sourceIdCounts)
        {
            var baseId = "unity_constraint:" + kind + ":" + VariantExtractor.TransformPath(root, target);
            if (!sourceIdCounts.TryGetValue(baseId, out var count))
            {
                count = 0;
            }
            count++;
            sourceIdCounts[baseId] = count;
            return count == 1 ? baseId : baseId + ":" + count.ToString(CultureInfo.InvariantCulture);
        }

        private static bool AxisHas(Axis axis, Axis flag)
        {
            return (axis & flag) != 0;
        }

        private static List<object> Vector3List(Vector3 value)
        {
            return new List<object> { value.x, value.y, value.z };
        }
    }
}
