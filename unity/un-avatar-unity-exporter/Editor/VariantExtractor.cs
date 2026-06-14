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
                ["operations"] = OperationsToJson()
            };
        }

        private List<object> OperationsToJson()
        {
            var json = new List<object>(Operations.Count);
            foreach (var operation in Operations)
            {
                json.Add(operation);
            }
            return json;
        }
    }

    internal static class VariantExtractor
    {
        public static List<VariantRecord> Extract(GameObject root, UNAvatarExportMode mode)
        {
            var variants = new List<VariantRecord>(1);
            if (root == null)
            {
                return variants;
            }

            variants.Add(MakeCurrentStateVariant(root));

            if (mode == UNAvatarExportMode.CurrentToBaseOnly)
            {
                return variants;
            }

            var components = root.GetComponentsInChildren<Component>(true);
            variants.AddRange(ExtractModularAvatarObjectToggles(root, components));
            variants.AddRange(ExtractModularAvatarMenuItems(root, components));
            return variants;
        }

        private static VariantRecord MakeCurrentStateVariant(GameObject root)
        {
            var renderers = root.GetComponentsInChildren<Renderer>(true);
            var variant = new VariantRecord
            {
                Id = "current-state",
                Name = "Current State",
                Source = "unity-active-state"
            };
            variant.Operations.Capacity = renderers.Length;
            foreach (var renderer in renderers)
            {
                variant.Operations.Add(new Dictionary<string, object>
                {
                    ["op"] = "nodeEnabled",
                    ["path"] = TransformPath(root.transform, renderer.transform),
                    ["visible"] = renderer.gameObject.activeSelf && renderer.enabled
                });
            }
            return variant;
        }

        private static IEnumerable<VariantRecord> ExtractModularAvatarObjectToggles(GameObject root, Component[] components)
        {
            var records = new List<VariantRecord>();
            foreach (var component in components)
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

        private static IEnumerable<VariantRecord> ExtractModularAvatarMenuItems(GameObject root, Component[] components)
        {
            var records = new List<VariantRecord>();
            foreach (var component in components)
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
            var parts = new List<string>();
            var current = target;
            var length = 0;
            while (current != null && current != root)
            {
                var name = current.name;
                parts.Add(name);
                length += (name != null ? name.Length : 0) + 1;
                current = current.parent;
            }
            if (parts.Count == 0)
            {
                return "";
            }

            var builder = new StringBuilder(Math.Max(0, length - 1));
            for (var i = parts.Count - 1; i >= 0; i--)
            {
                if (builder.Length > 0)
                {
                    builder.Append('/');
                }
                builder.Append(parts[i]);
            }
            return builder.ToString();
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
}

