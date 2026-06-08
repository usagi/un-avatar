using System;
using System.Collections.Generic;
using System.Globalization;
using System.Reflection;
using UnityEngine;

namespace UNAvatar.UnityExporter
{
    public sealed partial class UNAvatarExporterWindow
    {
        private List<object> BuildDynamicsPayload(GameObject root)
        {
            var payload = new List<object>();
            if (root == null)
            {
                return payload;
            }

            var components = root.GetComponentsInChildren<Component>(true);
            foreach (var component in components)
            {
                if (!IsVrcPhysBoneComponent(component))
                {
                    continue;
                }
                if (component is Behaviour behaviour && (!behaviour.enabled || !behaviour.gameObject.activeInHierarchy))
                {
                    continue;
                }
                payload.Add(BuildVrcPhysBonePayload(root.transform, component));
            }
            return payload;
        }

        private Dictionary<string, object> BuildDynamicsReportSummary(GameObject root)
        {
            var groups = BuildDynamicsPayload(root);
            var samples = new List<object>();
            for (var i = 0; i < groups.Count && samples.Count < 32; i++)
            {
                if (groups[i] is Dictionary<string, object> group)
                {
                    samples.Add(new Dictionary<string, object>
                    {
                        ["id"] = group.TryGetValue("id", out var id) ? id : "",
                        ["source"] = group.TryGetValue("source", out var source) ? source : "",
                        ["component"] = group.TryGetValue("component", out var component) ? component : null,
                        ["roots"] = group.TryGetValue("roots", out var roots) ? roots : new List<object>(),
                        ["stiffness"] = group.TryGetValue("stiffness", out var stiffness) ? stiffness : 0.0f,
                        ["spring"] = group.TryGetValue("spring", out var spring) ? spring : 0.0f,
                        ["pull"] = group.TryGetValue("pull", out var pull) ? pull : 0.0f,
                        ["gravity"] = group.TryGetValue("gravity", out var gravity) ? gravity : new List<object>(),
                        ["radius"] = group.TryGetValue("radius", out var radius) ? radius : 0.0f
                    });
                }
            }
            return new Dictionary<string, object>
            {
                ["groupCount"] = groups.Count,
                ["sampleLimit"] = 32,
                ["samples"] = samples
            };
        }

        private static bool IsVrcPhysBoneComponent(Component component)
        {
            if (component == null)
            {
                return false;
            }
            var type = component.GetType();
            return type.Name == "VRCPhysBone" ||
                string.Equals(type.FullName, "VRC.SDK3.Dynamics.PhysBone.Components.VRCPhysBone", StringComparison.Ordinal);
        }

        private Dictionary<string, object> BuildVrcPhysBonePayload(Transform root, Component component)
        {
            var type = component.GetType();
            var rootTransform = ReadMember(type, component, "rootTransform") as Transform;
            if (rootTransform == null)
            {
                rootTransform = component.transform;
            }

            var pull = ReadFloatMember(type, component, "pull", 0.0f);
            var spring = ReadFloatMember(type, component, "spring", 0.0f);
            var stiffness = ReadFloatMember(type, component, "stiffness", spring > 0.0f ? spring : pull);
            var radius = ReadFloatMember(type, component, "radius", 0.02f);
            var gravity = ReadFloatMember(type, component, "gravity", 0.0f);

            return new Dictionary<string, object>
            {
                ["id"] = "physbone:" + VariantExtractor.TransformPath(root, component.transform),
                ["name"] = component.name ?? "",
                ["source"] = "vrc_physbone",
                ["enabled"] = !(component is Behaviour behaviour) || behaviour.enabled,
                ["component"] = TransformTargetJson(root, component.transform),
                ["roots"] = new List<object> { TransformTargetJson(root, rootTransform) },
                ["pull"] = pull,
                ["spring"] = spring,
                ["stiffness"] = stiffness,
                ["drag"] = 0.4f,
                ["gravity"] = new List<object> { 0.0f, -gravity, 0.0f },
                ["radius"] = radius,
                ["sourceParams"] = BuildVrcPhysBoneSourceParams(type, component)
            };
        }

        private static Dictionary<string, object> BuildVrcPhysBoneSourceParams(Type type, Component component)
        {
            return new Dictionary<string, object>
            {
                ["pull"] = ReadFloatMember(type, component, "pull", 0.0f),
                ["spring"] = ReadFloatMember(type, component, "spring", 0.0f),
                ["stiffness"] = ReadFloatMember(type, component, "stiffness", 0.0f),
                ["gravity"] = ReadFloatMember(type, component, "gravity", 0.0f),
                ["gravityFalloff"] = ReadFloatMember(type, component, "gravityFalloff", 0.0f),
                ["immobile"] = ReadFloatMember(type, component, "immobile", 0.0f),
                ["radius"] = ReadFloatMember(type, component, "radius", 0.02f),
                ["maxStretch"] = ReadFloatMember(type, component, "maxStretch", 0.0f),
                ["limitType"] = ReadMember(type, component, "limitType")?.ToString() ?? "",
                ["maxAngleX"] = ReadFloatMember(type, component, "maxAngleX", 0.0f),
                ["maxAngleZ"] = ReadFloatMember(type, component, "maxAngleZ", 0.0f),
                ["allowCollision"] = ReadBoolMember(type, component, "allowCollision", false),
                ["allowGrabbing"] = ReadBoolMember(type, component, "allowGrabbing", false),
                ["allowPosing"] = ReadBoolMember(type, component, "allowPosing", false)
            };
        }

        private static float ReadFloatMember(Type type, object instance, string name, float fallback)
        {
            var value = ReadMember(type, instance, name);
            if (value == null)
            {
                return fallback;
            }
            if (value is float f)
            {
                return f;
            }
            if (value is double d)
            {
                return (float)d;
            }
            if (value is int i)
            {
                return i;
            }
            if (value is long l)
            {
                return l;
            }
            if (float.TryParse(value.ToString(), NumberStyles.Float, CultureInfo.InvariantCulture, out var parsed))
            {
                return parsed;
            }
            return fallback;
        }

        private static bool ReadBoolMember(Type type, object instance, string name, bool fallback)
        {
            var value = ReadMember(type, instance, name);
            if (value == null)
            {
                return fallback;
            }
            if (value is bool b)
            {
                return b;
            }
            if (bool.TryParse(value.ToString(), out var parsed))
            {
                return parsed;
            }
            return fallback;
        }
    }
}
