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
        private Dictionary<string, object> BuildModularAvatarPayload(GameObject root, List<UnavatarTextureAssetRecord> textureAssets)
        {
            var componentObjects = root != null ? root.GetComponentsInChildren<Component>(true) : null;
            var components = new List<object>(componentObjects != null ? componentObjects.Length : 0);
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

            foreach (var component in componentObjects)
            {
                if (!IsModularAvatarComponent(component))
                {
                    continue;
                }
                components.Add(BuildModularAvatarComponentPayload(root.transform, component, textureAssets));
            }
            AddVrcAvatarDescriptorExpressionMenuComponents(root.transform, componentObjects, components);

            return new Dictionary<string, object>
            {
                ["schemaVersion"] = "0.1-preview",
                ["available"] = ModularAvatarBridge.IsAvailable,
                ["componentCount"] = components.Count,
                ["components"] = components
            };
        }

        private void AddVrcAvatarDescriptorExpressionMenuComponents(Transform root, Component[] componentObjects, List<object> components)
        {
            var descriptor = FirstVrcAvatarDescriptor(componentObjects);
            if (descriptor == null)
            {
                return;
            }
            var menu = ReadMember(descriptor.GetType(), descriptor, "expressionsMenu") as UnityEngine.Object ??
                ReadMember(descriptor.GetType(), descriptor, "expressionMenu") as UnityEngine.Object;
            if (menu == null || !IsVrcExpressionsMenuObject(menu))
            {
                return;
            }
            var visited = new HashSet<int>();
            var addedControlCount = 0;
            AddVrcExpressionsMenuSyntheticControls(
                root,
                descriptor,
                menu,
                "VRC Menu",
                0,
                visited,
                components,
                ref addedControlCount,
                1024);
        }

        private static Component FirstVrcAvatarDescriptor(Component[] componentObjects)
        {
            if (componentObjects == null)
            {
                return null;
            }
            foreach (var component in componentObjects)
            {
                if (component == null)
                {
                    continue;
                }
                var type = component.GetType();
                if (type.Name == "VRCAvatarDescriptor" || type.FullName == "VRC.SDK3.Avatars.Components.VRCAvatarDescriptor")
                {
                    return component;
                }
            }
            return null;
        }

        private void AddVrcExpressionsMenuSyntheticControls(
            Transform root,
            Component descriptor,
            UnityEngine.Object menu,
            string parentPath,
            int depth,
            HashSet<int> visited,
            List<object> components,
            ref int addedControlCount,
            int limit)
        {
            if (menu == null || depth > 4 || addedControlCount >= limit)
            {
                return;
            }
            var instanceId = menu.GetInstanceID();
            if (!visited.Add(instanceId))
            {
                return;
            }
            var controls = VrcExpressionsMenuControls(menu);
            for (var index = 0; index < controls.Count && addedControlCount < limit; index++)
            {
                var control = controls[index];
                var controlJson = VrcExpressionsMenuControlToJson(root, control, depth, visited);
                var label = controlJson.TryGetValue("name", out var nameValue) ? nameValue?.ToString() ?? "" : "";
                if (string.IsNullOrWhiteSpace(label))
                {
                    label = "control:" + index.ToString(CultureInfo.InvariantCulture);
                }
                var hierarchyPath = string.IsNullOrEmpty(parentPath) ? label : parentPath + "/" + label;
                components.Add(new Dictionary<string, object>
                {
                    ["type"] = "VRC.SDK3.Avatars.ScriptableObjects.VRCExpressionsMenu.Control",
                    ["shortType"] = "VRCExpressionsMenuControl",
                    ["supportKind"] = "metadata",
                    ["target"] = TransformTargetJson(root, descriptor.transform),
                    ["enabled"] = true,
                    ["fields"] = new Dictionary<string, object>
                    {
                        ["Control"] = controlJson,
                        ["hierarchyPath"] = hierarchyPath,
                        ["siblingIndex"] = index,
                        ["menuSource"] = "VRCAvatarDescriptor.expressionsMenu"
                    }
                });
                addedControlCount++;
                var controlType = controlJson.TryGetValue("type", out var typeValue) ? typeValue?.ToString() ?? "" : "";
                var subMenu = control != null ? ReadMember(control.GetType(), control, "subMenu") as UnityEngine.Object : null;
                if (controlType == "SubMenu" && subMenu != null && IsVrcExpressionsMenuObject(subMenu))
                {
                    AddVrcExpressionsMenuSyntheticControls(root, descriptor, subMenu, hierarchyPath, depth + 1, visited, components, ref addedControlCount, limit);
                }
            }
            visited.Remove(instanceId);
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

        private Dictionary<string, object> BuildModularAvatarComponentPayload(
            Transform root,
            Component component,
            List<UnavatarTextureAssetRecord> textureAssets)
        {
            var type = component.GetType();
            var payload = new Dictionary<string, object>
            {
                ["type"] = type.FullName ?? type.Name,
                ["shortType"] = type.Name,
                ["supportKind"] = ModularAvatarComponentSupportKind(type.Name),
                ["target"] = TransformTargetJson(root, component.transform),
                ["enabled"] = !(component is Behaviour behaviour) || behaviour.enabled,
                ["fields"] = BuildModularAvatarComponentFields(root, component, textureAssets)
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

        private Dictionary<string, object> BuildModularAvatarComponentFields(
            Transform root,
            Component component,
            List<UnavatarTextureAssetRecord> textureAssets)
        {
            var fields = SerializableModularAvatarFields(component.GetType());
            var json = new Dictionary<string, object>(fields.Length);
            foreach (var field in fields)
            {
                var value = SafeGetField(field, component);
                if (!json.ContainsKey(field.Name))
                {
                    json[field.Name] = ModularAvatarValueToJson(root, component, value, 0);
                }
            }
            AddModularAvatarMaskTextureFields(root, component, textureAssets, json);
            return json;
        }

        private static FieldInfo[] SerializableModularAvatarFields(Type type)
        {
            var fields = new List<FieldInfo>();
            for (var current = type; current != null && current != typeof(object); current = current.BaseType)
            {
                foreach (var field in current.GetFields(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance | BindingFlags.DeclaredOnly))
                {
                    if (field.IsStatic || field.IsNotSerialized)
                    {
                        continue;
                    }
                    if (!field.IsPublic && !Attribute.IsDefined(field, typeof(SerializeField), true))
                    {
                        continue;
                    }
                    fields.Add(field);
                }
            }
            return fields.ToArray();
        }

        private static List<Texture> CollectModularAvatarMaskTextures(GameObject root)
        {
            var textures = new List<Texture>();
            if (root == null)
            {
                return textures;
            }
            var seen = new HashSet<int>();
            foreach (var component in root.GetComponentsInChildren<Component>(true))
            {
                if (component == null || component.GetType().Name != "VertexFilterByMaskComponent")
                {
                    continue;
                }
                var texture = ReadMember(component.GetType(), component, "m_maskTexture") as Texture;
                if (texture != null && seen.Add(texture.GetInstanceID()))
                {
                    textures.Add(texture);
                }
            }
            return textures;
        }

        private void AddModularAvatarMaskTextureFields(
            Transform root,
            Component component,
            List<UnavatarTextureAssetRecord> textureAssets,
            Dictionary<string, object> fields)
        {
            var type = component.GetType();
            if (type.Name != "VertexFilterByMaskComponent")
            {
                return;
            }
            var texture = ReadMember(type, component, "m_maskTexture") as Texture;
            fields["m_materialIndex"] = ReadMember(type, component, "m_materialIndex") is int materialIndex ? materialIndex : 0;
            fields["m_deleteMode"] = ReadMember(type, component, "m_deleteMode")?.ToString() ?? "";
            fields["m_maskTexture"] = texture != null ? ModularAvatarObjectReferenceToJson(root, texture) : null;
            fields["maskTextureAssetId"] = TextureAssetIdFor(texture, textureAssets);
        }

        private static string TextureAssetIdFor(Texture texture, List<UnavatarTextureAssetRecord> textureAssets)
        {
            if (texture == null || textureAssets == null)
            {
                return "";
            }
            var assetPath = UnityEditor.AssetDatabase.GetAssetPath(texture);
            if (string.IsNullOrEmpty(assetPath))
            {
                return "";
            }
            foreach (var asset in textureAssets)
            {
                if (asset != null && string.Equals(asset.AssetPath, assetPath, StringComparison.Ordinal))
                {
                    return asset.Id ?? "";
                }
            }
            return "";
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
            if (value is Bounds bounds)
            {
                return new Dictionary<string, object>
                {
                    ["center"] = FloatList(bounds.center.x, bounds.center.y, bounds.center.z),
                    ["extents"] = FloatList(bounds.extents.x, bounds.extents.y, bounds.extents.z),
                    ["size"] = FloatList(bounds.size.x, bounds.size.y, bounds.size.z)
                };
            }
            if (value is UnityEngine.Object unityObject)
            {
                return ModularAvatarObjectReferenceToJson(root, unityObject);
            }
            if (IsVrcExpressionsMenuControlObject(value))
            {
                return VrcExpressionsMenuControlToJson(root, value, depth, new HashSet<int>());
            }
            if (type.FullName == "nadena.dev.modular_avatar.core.PortableMenuControl")
            {
                return PortableMenuControlToJson(root, value, depth);
            }
            if (type.FullName == "nadena.dev.modular_avatar.core.BlendshapeBinding")
            {
                return BlendshapeBindingToJson(root, owner, value);
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

        private object BlendshapeBindingToJson(Transform root, Component owner, object binding)
        {
            var type = binding.GetType();
            var referenceMesh = ReadMember(type, binding, "ReferenceMesh");
            var remapCurve = ReadMember(type, binding, "RemapCurve") as AnimationCurve;
            return new Dictionary<string, object>
            {
                ["referenceMesh"] = referenceMesh != null ? AvatarObjectReferenceToJson(root, owner, referenceMesh) : null,
                ["blendshape"] = ReadMember(type, binding, "Blendshape") as string ?? "",
                ["localBlendshape"] = ReadMember(type, binding, "LocalBlendshape") as string ?? "",
                ["remapCurve"] = AnimationCurveToJson(remapCurve)
            };
        }

        private static object AnimationCurveToJson(AnimationCurve curve)
        {
            if (curve == null)
            {
                return null;
            }
            var keys = new List<object>(curve.keys != null ? curve.keys.Length : 0);
            if (curve.keys != null)
            {
                foreach (var key in curve.keys)
                {
                    keys.Add(new Dictionary<string, object>
                    {
                        ["time"] = key.time,
                        ["value"] = key.value,
                        ["inTangent"] = key.inTangent,
                        ["outTangent"] = key.outTangent
                    });
                }
            }
            return new Dictionary<string, object>
            {
                ["keyCount"] = keys.Count,
                ["keys"] = keys
            };
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
            var json = UnityObjectReferenceHeaderToJson(obj);
            AddVrcExpressionsMenuSummary(json, root, obj, 0, new HashSet<int>());
            return json;
        }

        private static Dictionary<string, object> UnityObjectReferenceHeaderToJson(UnityEngine.Object obj)
        {
            var json = new Dictionary<string, object>
            {
                ["name"] = obj.name ?? "",
                ["type"] = obj.GetType().FullName ?? obj.GetType().Name
            };
            var assetPath = UnityEditor.AssetDatabase.GetAssetPath(obj);
            if (!string.IsNullOrEmpty(assetPath))
            {
                json["assetPath"] = assetPath;
                var guid = UnityEditor.AssetDatabase.AssetPathToGUID(assetPath);
                if (!string.IsNullOrEmpty(guid))
                {
                    json["assetGuid"] = guid;
                }
            }
            return json;
        }

        private void AddVrcExpressionsMenuSummary(
            Dictionary<string, object> json,
            Transform root,
            UnityEngine.Object obj,
            int depth,
            HashSet<int> visited)
        {
            if (obj == null || !IsVrcExpressionsMenuObject(obj))
            {
                return;
            }
            json["controlCount"] = VrcExpressionsMenuControls(obj).Count;
            json["controls"] = VrcExpressionsMenuControlsToJson(root, obj, depth, visited);
        }

        private static bool IsVrcExpressionsMenuObject(UnityEngine.Object obj)
        {
            var fullName = obj != null ? obj.GetType().FullName ?? "" : "";
            return fullName == "VRC.SDK3.Avatars.ScriptableObjects.VRCExpressionsMenu" ||
                fullName.EndsWith(".VRCExpressionsMenu", StringComparison.Ordinal);
        }

        private static bool IsVrcExpressionsMenuControlObject(object obj)
        {
            var type = obj != null ? obj.GetType() : null;
            if (type == null)
            {
                return false;
            }
            var fullName = type.FullName ?? "";
            return fullName == "VRC.SDK3.Avatars.ScriptableObjects.VRCExpressionsMenu+Control" ||
                (type.Name == "Control" &&
                    (type.DeclaringType?.FullName == "VRC.SDK3.Avatars.ScriptableObjects.VRCExpressionsMenu" ||
                        (type.DeclaringType?.FullName ?? "").EndsWith(".VRCExpressionsMenu", StringComparison.Ordinal)));
        }

        private List<object> VrcExpressionsMenuControlsToJson(Transform root, UnityEngine.Object menu, int depth, HashSet<int> visited)
        {
            var controls = new List<object>();
            if (menu == null || depth > 4)
            {
                return controls;
            }
            var instanceId = menu.GetInstanceID();
            if (!visited.Add(instanceId))
            {
                return controls;
            }
            foreach (var control in VrcExpressionsMenuControls(menu))
            {
                if (controls.Count >= 64)
                {
                    break;
                }
                controls.Add(VrcExpressionsMenuControlToJson(root, control, depth, visited));
            }
            visited.Remove(instanceId);
            return controls;
        }

        private Dictionary<string, object> VrcExpressionsMenuControlToJson(Transform root, object control, int depth, HashSet<int> visited)
        {
            var type = control != null ? control.GetType() : null;
            var subMenu = type != null ? ReadMember(type, control, "subMenu") as UnityEngine.Object : null;
            var json = new Dictionary<string, object>
            {
                ["name"] = type != null ? ReadMember(type, control, "name")?.ToString() ?? "" : "",
                ["type"] = type != null ? ReadMember(type, control, "type")?.ToString() ?? "" : "",
                ["parameter"] = VrcExpressionControlParameterName(type != null ? ReadMember(type, control, "parameter") : null),
                ["subParameters"] = VrcExpressionControlSubParameters(
                    type != null ? ReadMember(type, control, "subParameters") as IEnumerable : null),
                ["value"] = type != null ? NumberToFloat(ReadMember(type, control, "value"), 0.0f) : 0.0f,
                ["subMenu"] = subMenu != null ? VrcExpressionsMenuSubMenuReferenceToJson(root, subMenu, depth + 1, visited) : null
            };
            return json;
        }

        private Dictionary<string, object> PortableMenuControlToJson(Transform root, object control, int depth)
        {
            var type = control.GetType();
            var subMenu = ReadMember(type, control, "VRChatSubMenu") as UnityEngine.Object;
            var json = new Dictionary<string, object>
            {
                ["type"] = ReadMember(type, control, "Type")?.ToString() ?? "",
                ["parameter"] = ReadMember(type, control, "Parameter")?.ToString() ?? "",
                ["subParameters"] = PortableStringListToJson(ReadMember(type, control, "SubParameters") as IEnumerable),
                ["value"] = NumberToFloat(ReadMember(type, control, "Value"), 0.0f),
                ["subMenu"] = subMenu != null ? VrcExpressionsMenuSubMenuReferenceToJson(root, subMenu, depth + 1, new HashSet<int>()) : null
            };
            return json;
        }

        private static List<object> PortableStringListToJson(IEnumerable values)
        {
            var result = new List<object>();
            if (values == null)
            {
                return result;
            }
            foreach (var value in values)
            {
                if (result.Count >= 16)
                {
                    break;
                }
                result.Add(value?.ToString() ?? "");
            }
            return result;
        }

        private object VrcExpressionsMenuSubMenuReferenceToJson(Transform root, UnityEngine.Object subMenu, int depth, HashSet<int> visited)
        {
            var json = UnityObjectReferenceHeaderToJson(subMenu);
            if (depth <= 4 && IsVrcExpressionsMenuObject(subMenu))
            {
                json["controlCount"] = VrcExpressionsMenuControls(subMenu).Count;
                json["controls"] = VrcExpressionsMenuControlsToJson(root, subMenu, depth, visited);
            }
            return json;
        }

        private static IList VrcExpressionsMenuControls(UnityEngine.Object menu)
        {
            if (menu == null)
            {
                return Array.Empty<object>();
            }
            var controls = ReadMember(menu.GetType(), menu, "controls") as IList;
            return controls ?? Array.Empty<object>();
        }

        private static string VrcExpressionControlParameterName(object parameter)
        {
            if (parameter == null)
            {
                return "";
            }
            return ReadMember(parameter.GetType(), parameter, "name")?.ToString() ?? "";
        }

        private static List<object> VrcExpressionControlSubParameters(IEnumerable subParameters)
        {
            var result = new List<object>();
            if (subParameters == null)
            {
                return result;
            }
            foreach (var subParameter in subParameters)
            {
                if (result.Count >= 16)
                {
                    break;
                }
                if (subParameter == null)
                {
                    continue;
                }
                result.Add(new Dictionary<string, object>
                {
                    ["name"] = VrcExpressionControlParameterName(subParameter)
                });
            }
            return result;
        }

        private static float NumberToFloat(object value, float fallback)
        {
            if (value == null)
            {
                return fallback;
            }
            try
            {
                return Convert.ToSingle(value, CultureInfo.InvariantCulture);
            }
            catch
            {
                return fallback;
            }
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
