using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.Reflection;
using UnityEngine;

namespace UNAvatar.UnityExporter
{
    public sealed partial class UNAvatarExporterWindow
    {
        private Dictionary<string, object> BuildModularAvatarPayload(GameObject root)
        {
            var components = new List<object>();
            if (root == null)
            {
                return new Dictionary<string, object>
                {
                    ["schemaVersion"] = "0.1-preview",
                    ["available"] = ModularAvatarBridge.IsAvailable,
                    ["componentCount"] = 0,
                    ["components"] = components
                };
            }

            foreach (var component in root.GetComponentsInChildren<Component>(true))
            {
                if (!IsModularAvatarComponent(component))
                {
                    continue;
                }
                components.Add(BuildModularAvatarComponentPayload(root.transform, component));
            }

            return new Dictionary<string, object>
            {
                ["schemaVersion"] = "0.1-preview",
                ["available"] = ModularAvatarBridge.IsAvailable,
                ["componentCount"] = components.Count,
                ["components"] = components
            };
        }

        private static bool IsModularAvatarComponent(Component component)
        {
            if (component == null)
            {
                return false;
            }
            var type = component.GetType();
            var fullName = type.FullName ?? "";
            if (fullName.StartsWith("nadena.dev.modular_avatar.core.", StringComparison.Ordinal))
            {
                return true;
            }
            return type.Name == "MAMoveIndependently" || type.Name == "RemoveVertexColor";
        }

        private Dictionary<string, object> BuildModularAvatarComponentPayload(Transform root, Component component)
        {
            var type = component.GetType();
            var payload = new Dictionary<string, object>
            {
                ["type"] = type.FullName ?? type.Name,
                ["shortType"] = type.Name,
                ["target"] = TransformTargetJson(root, component.transform),
                ["enabled"] = !(component is Behaviour behaviour) || behaviour.enabled,
                ["fields"] = BuildModularAvatarComponentFields(root, component)
            };

            if (type.Name == "ModularAvatarMergeArmature")
            {
                payload["boneMappings"] = BuildMergeArmatureBoneMappings(root, component);
            }
            if (type.Name == "ModularAvatarBoneProxy")
            {
                payload["resolvedTarget"] = BuildBoneProxyResolvedTarget(root, component);
            }
            return payload;
        }

        private Dictionary<string, object> BuildModularAvatarComponentFields(Transform root, Component component)
        {
            var json = new Dictionary<string, object>();
            var fields = component.GetType().GetFields(BindingFlags.Public | BindingFlags.Instance);
            foreach (var field in fields)
            {
                if (field.IsStatic)
                {
                    continue;
                }
                var value = SafeGetField(field, component);
                json[field.Name] = ModularAvatarValueToJson(root, component, value, 0);
            }
            return json;
        }

        private static object SafeGetField(FieldInfo field, object instance)
        {
            try
            {
                return field.GetValue(instance);
            }
            catch
            {
                return null;
            }
        }

        private object BuildBoneProxyResolvedTarget(Transform root, Component component)
        {
            var property = component.GetType().GetProperty("target", BindingFlags.Public | BindingFlags.Instance);
            if (property == null)
            {
                return null;
            }
            try
            {
                return ModularAvatarObjectReferenceToJson(root, property.GetValue(component) as UnityEngine.Object);
            }
            catch
            {
                return null;
            }
        }

        private List<object> BuildMergeArmatureBoneMappings(Transform root, Component component)
        {
            var mappings = new List<object>();
            var method = component.GetType().GetMethod("GetBonesMapping", BindingFlags.Public | BindingFlags.Instance);
            if (method == null)
            {
                return mappings;
            }
            IEnumerable result;
            try
            {
                result = method.Invoke(component, null) as IEnumerable;
            }
            catch
            {
                return mappings;
            }
            if (result == null)
            {
                return mappings;
            }
            foreach (var pair in result)
            {
                var pairType = pair.GetType();
                var item1 = pairType.GetField("Item1");
                var item2 = pairType.GetField("Item2");
                var baseBone = item1 != null ? item1.GetValue(pair) as Transform : null;
                var mergeBone = item2 != null ? item2.GetValue(pair) as Transform : null;
                mappings.Add(new Dictionary<string, object>
                {
                    ["targetBone"] = TransformTargetJson(root, baseBone),
                    ["sourceBone"] = TransformTargetJson(root, mergeBone)
                });
            }
            return mappings;
        }

        private object ModularAvatarValueToJson(Transform root, Component owner, object value, int depth)
        {
            if (value == null)
            {
                return null;
            }
            if (depth > 3)
            {
                return value.ToString();
            }

            var type = value.GetType();
            if (type.IsEnum)
            {
                return value.ToString();
            }
            if (value is string || value is bool || value is int || value is long || value is float || value is double)
            {
                return value;
            }
            if (value is Vector2 vector2)
            {
                return FloatList(vector2.x, vector2.y);
            }
            if (value is Vector3 vector3)
            {
                return FloatList(vector3.x, vector3.y, vector3.z);
            }
            if (value is Vector4 vector4)
            {
                return FloatList(vector4.x, vector4.y, vector4.z, vector4.w);
            }
            if (value is Color color)
            {
                return FloatList(color.r, color.g, color.b, color.a);
            }
            if (value is UnityEngine.Object unityObject)
            {
                return ModularAvatarObjectReferenceToJson(root, unityObject);
            }
            if (type.FullName == "nadena.dev.modular_avatar.core.AvatarObjectReference")
            {
                return AvatarObjectReferenceToJson(root, owner, value);
            }
            if (value is IEnumerable enumerable && !(value is string))
            {
                var list = new List<object>();
                foreach (var item in enumerable)
                {
                    if (list.Count >= 64)
                    {
                        break;
                    }
                    list.Add(ModularAvatarValueToJson(root, owner, item, depth + 1));
                }
                return list;
            }

            return value.ToString();
        }

        private object AvatarObjectReferenceToJson(Transform root, Component owner, object reference)
        {
            var json = new Dictionary<string, object>();
            var referencePath = ReadMember(reference.GetType(), reference, "referencePath") as string;
            json["referencePath"] = referencePath ?? "";
            var directTarget = ReadMember(reference.GetType(), reference, "targetObject") as UnityEngine.Object;
            json["targetObject"] = ModularAvatarObjectReferenceToJson(root, directTarget);

            var get = reference.GetType().GetMethod("Get", BindingFlags.Public | BindingFlags.Instance, null, new[] { typeof(Component) }, null);
            if (get != null)
            {
                try
                {
                    json["resolvedTarget"] = ModularAvatarObjectReferenceToJson(root, get.Invoke(reference, new object[] { owner }) as UnityEngine.Object);
                }
                catch
                {
                    json["resolvedTarget"] = null;
                }
            }
            return json;
        }

        private object ModularAvatarObjectReferenceToJson(Transform root, UnityEngine.Object obj)
        {
            if (obj == null)
            {
                return null;
            }
            if (obj is GameObject gameObject)
            {
                return TransformTargetJson(root, gameObject.transform);
            }
            if (obj is Transform transform)
            {
                return TransformTargetJson(root, transform);
            }
            return new Dictionary<string, object>
            {
                ["name"] = obj.name ?? "",
                ["type"] = obj.GetType().FullName ?? obj.GetType().Name
            };
        }

        private static Dictionary<string, object> TransformTargetJson(Transform root, Transform target)
        {
            if (root == null || target == null)
            {
                return new Dictionary<string, object>
                {
                    ["nodeId"] = "",
                    ["path"] = "",
                    ["name"] = ""
                };
            }
            return new Dictionary<string, object>
            {
                ["nodeId"] = WardrobeSnapshotCapture.NodeIdFor(root, target),
                ["path"] = VariantExtractor.TransformPath(root, target),
                ["name"] = target.name ?? ""
            };
        }

        private static List<object> FloatList(params float[] values)
        {
            var list = new List<object>(values.Length);
            foreach (var value in values)
            {
                list.Add(value);
            }
            return list;
        }

        private static object ReadMember(Type type, object instance, string name)
        {
            var property = type.GetProperty(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            if (property != null)
            {
                try
                {
                    return property.GetValue(instance);
                }
                catch
                {
                    return null;
                }
            }
            var field = type.GetField(name, BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance);
            if (field != null)
            {
                try
                {
                    return field.GetValue(instance);
                }
                catch
                {
                    return null;
                }
            }
            return null;
        }
    }
}
