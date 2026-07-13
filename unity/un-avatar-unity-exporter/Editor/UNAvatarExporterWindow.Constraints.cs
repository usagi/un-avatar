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
            foreach (var transform in rootObject.GetComponentsInChildren<Transform>(true))
            {
                foreach (var component in transform.GetComponents<Component>())
                {
                    Dictionary<string, object> item = null;
                    if (component is ParentConstraint parent
                        && parent.enabled && parent.constraintActive && parent.weight > 0.0f)
                    {
                        item = BuildParentConstraintPayload(rootObject.transform, parent, sourceIdCounts);
                    }
                    else if (component is AimConstraint aim
                        && aim.enabled && aim.constraintActive && aim.weight > 0.0f)
                    {
                        item = BuildAimConstraintPayload(rootObject.transform, aim, sourceIdCounts);
                    }
                    else if (component is RotationConstraint rotation
                        && rotation.enabled && rotation.constraintActive && rotation.weight > 0.0f)
                    {
                        item = BuildRotationConstraintPayload(rootObject.transform, rotation, sourceIdCounts);
                    }
                    else if (component is PositionConstraint position
                        && position.enabled && position.constraintActive && position.weight > 0.0f)
                    {
                        item = BuildPositionConstraintPayload(rootObject.transform, position, sourceIdCounts);
                    }
                    else if (component is ScaleConstraint scale
                        && scale.enabled && scale.constraintActive && scale.weight > 0.0f)
                    {
                        item = BuildScaleConstraintMetadata(rootObject.transform, scale, sourceIdCounts);
                    }
                    else if (component is LookAtConstraint lookAt
                        && lookAt.enabled && lookAt.constraintActive && lookAt.weight > 0.0f)
                    {
                        item = BuildLookAtConstraintMetadata(rootObject.transform, lookAt, sourceIdCounts);
                    }
                    if (item != null)
                    {
                        payload.Add(item);
                    }
                }
            }
            return payload;
        }

        private static List<object> BuildConstraintSources(Transform root, IConstraint constraint)
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
                    ["weight"] = source.weight
                });
            }
            return sources;
        }

        private static Dictionary<string, object> BuildAimConstraintPayload(
            Transform root,
            AimConstraint constraint,
            Dictionary<string, int> sourceIdCounts)
        {
            var sources = BuildConstraintSources(root, constraint);
            if (sources.Count == 0)
            {
                return null;
            }
            var item = new Dictionary<string, object>
            {
                ["id"] = BuildUnityConstraintSourceId(root, constraint.transform, "aim", sourceIdCounts),
                ["kind"] = "unity_aim",
                ["source"] = "unity_aim_constraint",
                ["target"] = TransformTargetJson(root, constraint.transform),
                ["weight"] = constraint.weight,
                ["aimVector"] = GltfVector3List(constraint.aimVector),
                ["upVector"] = GltfVector3List(constraint.upVector),
                ["worldUpType"] = UnityWorldUpTypeName(constraint.worldUpType),
                ["worldUpVector"] = GltfVector3List(constraint.worldUpVector),
                ["rotateX"] = AxisHas(constraint.rotationAxis, Axis.X),
                ["rotateY"] = AxisHas(constraint.rotationAxis, Axis.Y),
                ["rotateZ"] = AxisHas(constraint.rotationAxis, Axis.Z),
                ["rotationAtRestQuaternion"] = GltfQuaternionList(Quaternion.Euler(constraint.rotationAtRest)),
                ["rotationOffsetQuaternion"] = GltfQuaternionList(Quaternion.Euler(constraint.rotationOffset)),
                ["sources"] = sources
            };
            if (constraint.worldUpObject != null)
            {
                item["worldUpObject"] = TransformTargetJson(root, constraint.worldUpObject);
            }
            return item;
        }

        private static Dictionary<string, object> BuildRotationConstraintPayload(
            Transform root,
            RotationConstraint constraint,
            Dictionary<string, int> sourceIdCounts)
        {
            var sources = BuildConstraintSources(root, constraint);
            if (sources.Count == 0)
            {
                return null;
            }
            return new Dictionary<string, object>
            {
                ["id"] = BuildUnityConstraintSourceId(root, constraint.transform, "rotation", sourceIdCounts),
                ["kind"] = "unity_rotation",
                ["source"] = "unity_rotation_constraint",
                ["target"] = TransformTargetJson(root, constraint.transform),
                ["weight"] = constraint.weight,
                ["rotateX"] = AxisHas(constraint.rotationAxis, Axis.X),
                ["rotateY"] = AxisHas(constraint.rotationAxis, Axis.Y),
                ["rotateZ"] = AxisHas(constraint.rotationAxis, Axis.Z),
                ["rotationAtRestQuaternion"] = GltfQuaternionList(Quaternion.Euler(constraint.rotationAtRest)),
                ["rotationOffsetQuaternion"] = GltfQuaternionList(Quaternion.Euler(constraint.rotationOffset)),
                ["sources"] = sources
            };
        }

        private static Dictionary<string, object> BuildPositionConstraintPayload(
            Transform root,
            PositionConstraint constraint,
            Dictionary<string, int> sourceIdCounts)
        {
            var sources = BuildConstraintSources(root, constraint);
            if (sources.Count == 0)
            {
                return null;
            }
            return new Dictionary<string, object>
            {
                ["id"] = BuildUnityConstraintSourceId(root, constraint.transform, "position", sourceIdCounts),
                ["kind"] = "unity_position",
                ["source"] = "unity_position_constraint",
                ["target"] = TransformTargetJson(root, constraint.transform),
                ["weight"] = constraint.weight,
                ["translateX"] = AxisHas(constraint.translationAxis, Axis.X),
                ["translateY"] = AxisHas(constraint.translationAxis, Axis.Y),
                ["translateZ"] = AxisHas(constraint.translationAxis, Axis.Z),
                ["translationAtRest"] = GltfVector3List(constraint.translationAtRest),
                ["translationOffset"] = GltfVector3List(constraint.translationOffset),
                ["sources"] = sources
            };
        }

        private static Dictionary<string, object> BuildScaleConstraintMetadata(
            Transform root,
            ScaleConstraint constraint,
            Dictionary<string, int> sourceIdCounts)
        {
            var sources = BuildConstraintSources(root, constraint);
            if (sources.Count == 0)
            {
                return null;
            }
            return new Dictionary<string, object>
            {
                ["id"] = BuildUnityConstraintSourceId(root, constraint.transform, "scale", sourceIdCounts),
                ["kind"] = "unity_scale",
                ["source"] = "unity_scale_constraint",
                ["runtimeSupport"] = "unsupported",
                ["target"] = TransformTargetJson(root, constraint.transform),
                ["weight"] = constraint.weight,
                ["scaleX"] = AxisHas(constraint.scalingAxis, Axis.X),
                ["scaleY"] = AxisHas(constraint.scalingAxis, Axis.Y),
                ["scaleZ"] = AxisHas(constraint.scalingAxis, Axis.Z),
                ["scaleAtRest"] = Vector3List(constraint.scaleAtRest),
                ["scaleOffset"] = Vector3List(constraint.scaleOffset),
                ["sources"] = sources
            };
        }

        private static Dictionary<string, object> BuildLookAtConstraintMetadata(
            Transform root,
            LookAtConstraint constraint,
            Dictionary<string, int> sourceIdCounts)
        {
            var sources = BuildConstraintSources(root, constraint);
            if (sources.Count == 0)
            {
                return null;
            }
            var item = new Dictionary<string, object>
            {
                ["id"] = BuildUnityConstraintSourceId(root, constraint.transform, "look_at", sourceIdCounts),
                ["kind"] = "unity_look_at",
                ["source"] = "unity_look_at_constraint",
                ["runtimeSupport"] = "unsupported",
                ["target"] = TransformTargetJson(root, constraint.transform),
                ["weight"] = constraint.weight,
                ["roll"] = constraint.roll,
                ["useUpObject"] = constraint.useUpObject,
                ["rotationAtRestQuaternion"] = GltfQuaternionList(Quaternion.Euler(constraint.rotationAtRest)),
                ["rotationOffsetQuaternion"] = GltfQuaternionList(Quaternion.Euler(constraint.rotationOffset)),
                ["sources"] = sources
            };
            if (constraint.worldUpObject != null)
            {
                item["worldUpObject"] = TransformTargetJson(root, constraint.worldUpObject);
            }
            return item;
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

        private static List<object> GltfVector3List(Vector3 value)
        {
            return new List<object> { -value.x, value.y, value.z };
        }

        private static List<object> GltfQuaternionList(Quaternion value)
        {
            return new List<object> { value.x, -value.y, -value.z, value.w };
        }

        private static string UnityWorldUpTypeName(AimConstraint.WorldUpType value)
        {
            switch (value)
            {
                case AimConstraint.WorldUpType.SceneUp: return "scene_up";
                case AimConstraint.WorldUpType.ObjectUp: return "object_up";
                case AimConstraint.WorldUpType.ObjectRotationUp: return "object_rotation_up";
                case AimConstraint.WorldUpType.Vector: return "vector";
                case AimConstraint.WorldUpType.None: return "none";
                default: return "none";
            }
        }
    }
}
