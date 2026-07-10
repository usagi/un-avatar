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
        private List<object> BuildDynamicsPayload(GameObject root)
        {
            var payload = new List<object>();
            if (root == null)
            {
                return payload;
            }

            var sourceIdCounts = new Dictionary<string, int>();
            var components = root.GetComponentsInChildren<Component>(true);
            foreach (var component in components)
            {
                if (!IsVrcPhysBoneComponent(component))
                {
                    continue;
                }
                var sourceId = BuildVrcPhysBoneSourceId(root.transform, component, sourceIdCounts);
                payload.Add(BuildVrcPhysBonePayload(root.transform, component, sourceId));
            }
            return payload;
        }

        private List<object> BuildContactsPayload(GameObject root)
        {
            var payload = new List<object>();
            if (root == null)
            {
                return payload;
            }

            var sourceIdCounts = new Dictionary<string, int>();
            var components = root.GetComponentsInChildren<Component>(true);
            foreach (var component in components)
            {
                if (!IsVrcContactComponent(component))
                {
                    continue;
                }
                var sourceId = BuildVrcContactSourceId(root.transform, component, sourceIdCounts);
                payload.Add(BuildVrcContactPayload(root.transform, component, sourceId));
            }
            return payload;
        }

        private Dictionary<string, object> BuildDynamicsReportSummary(List<object> groups)
        {
            groups = groups ?? new List<object>();
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
                        ["ignoreTransforms"] = group.TryGetValue("ignoreTransforms", out var ignoreTransforms) ? ignoreTransforms : new List<object>(),
                        ["multiChildType"] = group.TryGetValue("multiChildType", out var multiChildType) ? multiChildType : "",
                        ["sourceColliderCount"] = SourceColliderCount(group),
                        ["sourceColliderSamples"] = SourceColliderSamples(group, 8),
                        ["stiffness"] = group.TryGetValue("stiffness", out var stiffness) ? stiffness : 0.0f,
                        ["spring"] = group.TryGetValue("spring", out var spring) ? spring : 0.0f,
                        ["pull"] = group.TryGetValue("pull", out var pull) ? pull : 0.0f,
                        ["gravity"] = group.TryGetValue("gravity", out var gravity) ? gravity : new List<object>(),
                        ["radius"] = group.TryGetValue("radius", out var radius) ? radius : 0.0f,
                        ["radiusCurveKeyCount"] = SourceCurveKeyCount(group, "radiusCurve"),
                        ["limit"] = SourceLimitSample(group),
                        ["interaction"] = SourceInteractionSample(group)
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

        private Dictionary<string, object> BuildContactsReportSummary(List<object> contacts)
        {
            contacts = contacts ?? new List<object>();
            var samples = new List<object>();
            for (var i = 0; i < contacts.Count && samples.Count < 32; i++)
            {
                if (contacts[i] is Dictionary<string, object> contact)
                {
                    samples.Add(new Dictionary<string, object>
                    {
                        ["id"] = contact.TryGetValue("id", out var id) ? id : "",
                        ["source"] = contact.TryGetValue("source", out var source) ? source : "",
                        ["component"] = contact.TryGetValue("component", out var component) ? component : null,
                        ["node"] = contact.TryGetValue("node", out var node) ? node : null,
                        ["kind"] = contact.TryGetValue("kind", out var kind) ? kind : "",
                        ["parameter"] = contact.TryGetValue("parameter", out var parameter) ? parameter : "",
                        ["collisionTags"] = contact.TryGetValue("collisionTags", out var tags) ? tags : new List<object>(),
                        ["shape"] = contact.TryGetValue("shape", out var shape) ? shape : "",
                        ["radius"] = contact.TryGetValue("radius", out var radius) ? radius : 0.0f,
                        ["height"] = contact.TryGetValue("height", out var height) ? height : 0.0f
                    });
                }
            }
            return new Dictionary<string, object>
            {
                ["contactCount"] = contacts.Count,
                ["sampleLimit"] = 32,
                ["samples"] = samples
            };
        }

        private static Dictionary<string, object> SourceLimitSample(Dictionary<string, object> group)
        {
            var sourceParams = SourceParams(group);
            return new Dictionary<string, object>
            {
                ["limitType"] = sourceParams != null && sourceParams.TryGetValue("limitType", out var limitType) ? limitType : "",
                ["limitRotation"] = sourceParams != null && sourceParams.TryGetValue("limitRotation", out var limitRotation) ? limitRotation : new List<object> { 0.0f, 0.0f, 0.0f },
                ["maxAngleX"] = sourceParams != null && sourceParams.TryGetValue("maxAngleX", out var maxAngleX) ? maxAngleX : 0.0f,
                ["maxAngleZ"] = sourceParams != null && sourceParams.TryGetValue("maxAngleZ", out var maxAngleZ) ? maxAngleZ : 0.0f,
                ["maxStretch"] = sourceParams != null && sourceParams.TryGetValue("maxStretch", out var maxStretch) ? maxStretch : 0.0f,
                ["maxStretchCurveKeyCount"] = SourceCurveKeyCount(group, "maxStretchCurve")
            };
        }

        private static int SourceCurveKeyCount(Dictionary<string, object> group, string curveName)
        {
            var sourceParams = SourceParams(group);
            if (sourceParams == null ||
                !sourceParams.TryGetValue(curveName, out var rawCurve) ||
                !(rawCurve is Dictionary<string, object> curve))
            {
                return 0;
            }
            return curve.TryGetValue("keyCount", out var keyCount) && keyCount is int intKeyCount ? intKeyCount : 0;
        }

        private static bool VrcPhysBoneNeedsTranslationWriteback(Dictionary<string, object> sourceParams)
        {
            return SourceFloatNonZero(sourceParams, "maxStretch") ||
                SourceFloatNonZero(sourceParams, "maxSquish") ||
                SourceFloatNonZero(sourceParams, "stretchMotion") ||
                SourceParamCurveKeyCount(sourceParams, "maxStretchCurve") > 0 ||
                SourceParamCurveKeyCount(sourceParams, "maxSquishCurve") > 0 ||
                SourceParamCurveKeyCount(sourceParams, "stretchMotionCurve") > 0;
        }

        private static bool SourceFloatNonZero(Dictionary<string, object> sourceParams, string name)
        {
            return sourceParams != null &&
                sourceParams.TryGetValue(name, out var value) &&
                value is float f &&
                Mathf.Abs(f) > 1e-6f;
        }

        private static int SourceParamCurveKeyCount(Dictionary<string, object> sourceParams, string curveName)
        {
            if (sourceParams == null ||
                !sourceParams.TryGetValue(curveName, out var rawCurve) ||
                !(rawCurve is Dictionary<string, object> curve))
            {
                return 0;
            }
            return curve.TryGetValue("keyCount", out var keyCount) && keyCount is int intKeyCount ? intKeyCount : 0;
        }

        private static Dictionary<string, object> SourceInteractionSample(Dictionary<string, object> group)
        {
            var sourceParams = SourceParams(group);
            return new Dictionary<string, object>
            {
                ["allowCollision"] = sourceParams != null && sourceParams.TryGetValue("allowCollision", out var allowCollision) ? allowCollision : false,
                ["allowGrabbing"] = sourceParams != null && sourceParams.TryGetValue("allowGrabbing", out var allowGrabbing) ? allowGrabbing : false,
                ["allowPosing"] = sourceParams != null && sourceParams.TryGetValue("allowPosing", out var allowPosing) ? allowPosing : false
            };
        }

        private static Dictionary<string, object> SourceParams(Dictionary<string, object> group)
        {
            if (!group.TryGetValue("sourceParams", out var rawSourceParams) || !(rawSourceParams is Dictionary<string, object> sourceParams))
            {
                return null;
            }
            return sourceParams;
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

        private static bool IsVrcContactComponent(Component component)
        {
            if (component == null)
            {
                return false;
            }
            var type = component.GetType();
            return IsVrcContactSenderComponent(type) || IsVrcContactReceiverComponent(type);
        }

        private static bool IsVrcContactSenderComponent(Type type)
        {
            return type.Name == "VRCContactSender" ||
                string.Equals(type.FullName, "VRC.SDK3.Dynamics.Contact.Components.VRCContactSender", StringComparison.Ordinal);
        }

        private static bool IsVrcContactReceiverComponent(Type type)
        {
            return type.Name == "VRCContactReceiver" ||
                string.Equals(type.FullName, "VRC.SDK3.Dynamics.Contact.Components.VRCContactReceiver", StringComparison.Ordinal);
        }

        private static string BuildVrcPhysBoneSourceId(Transform root, Component component, Dictionary<string, int> sourceIdCounts)
        {
            var baseId = "physbone:" + VariantExtractor.TransformPath(root, component.transform);
            if (!sourceIdCounts.TryGetValue(baseId, out var count))
            {
                count = 0;
            }
            count++;
            sourceIdCounts[baseId] = count;
            return count == 1 ? baseId : baseId + ":" + count.ToString(CultureInfo.InvariantCulture);
        }

        private static string BuildVrcContactSourceId(Transform root, Component component, Dictionary<string, int> sourceIdCounts)
        {
            var type = component.GetType();
            var kind = IsVrcContactReceiverComponent(type) ? "receiver" : "sender";
            var baseId = "contact:" + VariantExtractor.TransformPath(root, component.transform) + ":" + kind;
            if (!sourceIdCounts.TryGetValue(baseId, out var count))
            {
                count = 0;
            }
            count++;
            sourceIdCounts[baseId] = count;
            return count == 1 ? baseId : baseId + ":" + count.ToString(CultureInfo.InvariantCulture);
        }

        private Dictionary<string, object> BuildVrcContactPayload(Transform root, Component component, string sourceId)
        {
            var type = component.GetType();
            var rootTransform = ReadMember(type, component, "rootTransform") as Transform;
            if (rootTransform == null)
            {
                rootTransform = component.transform;
            }
            var isReceiver = IsVrcContactReceiverComponent(type);
            var kind = isReceiver ? "receiver" : "sender";
            return new Dictionary<string, object>
            {
                ["id"] = sourceId,
                ["source"] = isReceiver ? "vrc_contact_receiver" : "vrc_contact_sender",
                ["component"] = TransformTargetJson(root, component.transform),
                ["node"] = TransformTargetJson(root, rootTransform),
                ["kind"] = kind,
                ["parameter"] = isReceiver ? ReadMember(type, component, "parameter")?.ToString() ?? "" : "",
                ["collisionTags"] = StringsJson(ReadStringListMember(type, component, "collisionTags")),
                ["shape"] = ReadMember(type, component, "shapeType")?.ToString() ?? "",
                ["radius"] = ReadFloatMember(type, component, "radius", 0.0f),
                ["height"] = ReadFloatMember(type, component, "height", 0.0f),
                ["position"] = Vector3Json(ReadVector3Member(type, component, "position", Vector3.zero)),
                ["rotation"] = QuaternionJson(ReadQuaternionMember(type, component, "rotation", Quaternion.identity))
            };
        }

        private Dictionary<string, object> BuildVrcPhysBonePayload(Transform root, Component component, string sourceId)
        {
            var type = component.GetType();
            var rootTransform = ReadMember(type, component, "rootTransform") as Transform;
            if (rootTransform == null)
            {
                rootTransform = component.transform;
            }

            var pull = ReadFloatMember(type, component, "pull", 0.0f);
            var spring = ReadFloatMember(type, component, "spring", 0.0f);
            var stiffness = ReadFloatMember(type, component, "stiffness", 0.0f);
            var radius = ReadFloatMember(type, component, "radius", 0.02f);
            var gravity = ReadFloatMember(type, component, "gravity", 0.0f);
            var multiChildType = ReadMember(type, component, "multiChildType")?.ToString() ?? "";
            var sourceParams = BuildVrcPhysBoneSourceParams(root, type, component);

            var payload = new Dictionary<string, object>
            {
                ["id"] = sourceId,
                ["name"] = component.name ?? "",
                ["source"] = "vrc_physbone",
                ["enabled"] = !(component is Behaviour behaviour) || behaviour.enabled,
                ["component"] = TransformTargetJson(root, component.transform),
                ["roots"] = new List<object> { TransformTargetJson(root, rootTransform) },
                ["ignoreTransforms"] = TransformListTargetsJson(root, ReadTransformListMember(type, component, "ignoreTransforms")),
                ["multiChildType"] = multiChildType,
                ["pull"] = pull,
                ["spring"] = spring,
                ["stiffness"] = stiffness,
                ["drag"] = 0.4f,
                ["gravity"] = new List<object> { 0.0f, -gravity, 0.0f },
                ["radius"] = radius,
                ["sourceParams"] = sourceParams
            };
            if (VrcPhysBoneNeedsTranslationWriteback(sourceParams))
            {
                payload["writebackMode"] = "rotation_translation";
            }
            return payload;
        }

        private Dictionary<string, object> BuildVrcPhysBoneSourceParams(Transform root, Type type, Component component)
        {
            var sourceParams = new Dictionary<string, object>
            {
                ["pull"] = ReadFloatMember(type, component, "pull", 0.0f),
                ["spring"] = ReadFloatMember(type, component, "spring", 0.0f),
                ["momentum"] = ReadFloatMember(type, component, "momentum", 0.0f),
                ["stiffness"] = ReadFloatMember(type, component, "stiffness", 0.0f),
                ["integrationType"] = ReadMember(type, component, "integrationType")?.ToString() ?? "",
                ["gravity"] = ReadFloatMember(type, component, "gravity", 0.0f),
                ["gravityFalloff"] = ReadFloatMember(type, component, "gravityFalloff", 0.0f),
                ["immobile"] = ReadFloatMember(type, component, "immobile", 0.0f),
                ["immobileType"] = ReadMember(type, component, "immobileType")?.ToString() ?? "",
                ["radius"] = ReadFloatMember(type, component, "radius", 0.02f),
                ["pullCurve"] = ReadAnimationCurveMember(type, component, "pullCurve"),
                ["springCurve"] = ReadAnimationCurveMember(type, component, "springCurve"),
                ["momentumCurve"] = ReadAnimationCurveMember(type, component, "momentumCurve"),
                ["stiffnessCurve"] = ReadAnimationCurveMember(type, component, "stiffnessCurve"),
                ["gravityCurve"] = ReadAnimationCurveMember(type, component, "gravityCurve"),
                ["gravityFalloffCurve"] = ReadAnimationCurveMember(type, component, "gravityFalloffCurve"),
                ["immobileCurve"] = ReadAnimationCurveMember(type, component, "immobileCurve"),
                ["radiusCurve"] = ReadAnimationCurveMember(type, component, "radiusCurve"),
                ["endpointPosition"] = Vector3Json(ReadVector3Member(type, component, "endpointPosition", Vector3.zero)),
                ["multiChildType"] = ReadMember(type, component, "multiChildType")?.ToString() ?? "",
                ["maxStretch"] = ReadFloatMember(type, component, "maxStretch", 0.0f),
                ["limitType"] = ReadMember(type, component, "limitType")?.ToString() ?? "",
                ["limitRotation"] = Vector3Json(ReadVector3Member(type, component, "limitRotation", Vector3.zero)),
                ["maxAngleX"] = ReadFloatMember(type, component, "maxAngleX", 0.0f),
                ["maxAngleZ"] = ReadFloatMember(type, component, "maxAngleZ", 0.0f),
                ["maxAngleXCurve"] = ReadAnimationCurveMember(type, component, "maxAngleXCurve"),
                ["maxAngleZCurve"] = ReadAnimationCurveMember(type, component, "maxAngleZCurve"),
                ["maxStretchCurve"] = ReadAnimationCurveMember(type, component, "maxStretchCurve"),
                ["maxSquish"] = ReadFloatMember(type, component, "maxSquish", 0.0f),
                ["maxSquishCurve"] = ReadAnimationCurveMember(type, component, "maxSquishCurve"),
                ["stretchMotion"] = ReadFloatMember(type, component, "stretchMotion", 0.0f),
                ["stretchMotionCurve"] = ReadAnimationCurveMember(type, component, "stretchMotionCurve"),
                ["allowCollision"] = ReadBoolMember(type, component, "allowCollision", false),
                ["allowGrabbing"] = ReadBoolMember(type, component, "allowGrabbing", false),
                ["allowPosing"] = ReadBoolMember(type, component, "allowPosing", false),
                ["parameter"] = ReadStringMember(type, component, "parameter", ""),
                ["colliders"] = BuildVrcPhysBoneColliderPayloads(root, ReadComponentListMember(type, component, "colliders"))
            };
            AddOptionalSourceParam(sourceParams, type, component, "version");
            AddOptionalSourceParam(sourceParams, type, component, "ignoreOtherPhysBones");
            AddOptionalSourceParam(sourceParams, type, component, "isAnimated");
            return sourceParams;
        }

        private static int SourceColliderCount(Dictionary<string, object> group)
        {
            var sourceParams = SourceParams(group);
            if (sourceParams == null)
            {
                return 0;
            }
            if (!sourceParams.TryGetValue("colliders", out var rawColliders) || !(rawColliders is List<object> colliders))
            {
                return 0;
            }
            return colliders.Count;
        }

        private static List<object> SourceColliderSamples(Dictionary<string, object> group, int limit)
        {
            var samples = new List<object>();
            var sourceParams = SourceParams(group);
            if (sourceParams == null)
            {
                return samples;
            }
            if (!sourceParams.TryGetValue("colliders", out var rawColliders) || !(rawColliders is List<object> colliders))
            {
                return samples;
            }
            foreach (var collider in colliders)
            {
                if (samples.Count >= limit)
                {
                    break;
                }
                if (collider is Dictionary<string, object> colliderMap)
                {
                    samples.Add(new Dictionary<string, object>
                    {
                        ["component"] = colliderMap.TryGetValue("component", out var component) ? component : null,
                        ["root"] = colliderMap.TryGetValue("root", out var root) ? root : null,
                        ["shapeType"] = colliderMap.TryGetValue("shapeType", out var shapeType) ? shapeType : "",
                        ["radius"] = colliderMap.TryGetValue("radius", out var radius) ? radius : 0.0f,
                        ["height"] = colliderMap.TryGetValue("height", out var height) ? height : 0.0f,
                        ["insideBounds"] = colliderMap.TryGetValue("insideBounds", out var insideBounds) ? insideBounds : false,
                        ["bonesAsSphere"] = colliderMap.TryGetValue("bonesAsSphere", out var bonesAsSphere) ? bonesAsSphere : true
                    });
                }
            }
            return samples;
        }

        private List<object> BuildVrcPhysBoneColliderPayloads(Transform root, List<Component> colliders)
        {
            var json = new List<object>(colliders != null ? colliders.Count : 0);
            if (colliders == null)
            {
                return json;
            }
            foreach (var collider in colliders)
            {
                if (IsVrcPhysBoneColliderComponent(collider))
                {
                    json.Add(BuildVrcPhysBoneColliderPayload(root, collider));
                }
            }
            return json;
        }

        private static bool IsVrcPhysBoneColliderComponent(Component component)
        {
            if (component == null)
            {
                return false;
            }
            var type = component.GetType();
            return type.Name == "VRCPhysBoneCollider" ||
                string.Equals(type.FullName, "VRC.SDK3.Dynamics.PhysBone.Components.VRCPhysBoneCollider", StringComparison.Ordinal);
        }

        private Dictionary<string, object> BuildVrcPhysBoneColliderPayload(Transform root, Component collider)
        {
            var type = collider.GetType();
            var rootTransform = ReadMember(type, collider, "rootTransform") as Transform;
            if (rootTransform == null)
            {
                rootTransform = collider.transform;
            }
            var payload = new Dictionary<string, object>
            {
                ["component"] = TransformTargetJson(root, collider.transform),
                ["root"] = TransformTargetJson(root, rootTransform),
                ["shapeType"] = ReadMember(type, collider, "shapeType")?.ToString() ?? "",
                ["radius"] = ReadFloatMember(type, collider, "radius", 0.0f),
                ["height"] = ReadFloatMember(type, collider, "height", 0.0f),
                ["position"] = Vector3Json(ReadVector3Member(type, collider, "position", Vector3.zero)),
                ["rotation"] = QuaternionJson(ReadQuaternionMember(type, collider, "rotation", Quaternion.identity)),
                ["insideBounds"] = ReadBoolMember(type, collider, "insideBounds", false),
                ["bonesAsSphere"] = ReadBoolMember(type, collider, "bonesAsSphere", true)
            };
            AddOptionalSourceParam(payload, type, collider, "globalCollision");
            return payload;
        }

        private static List<object> TransformListTargetsJson(Transform root, List<Transform> transforms)
        {
            var json = new List<object>(transforms != null ? transforms.Count : 0);
            if (transforms == null)
            {
                return json;
            }
            foreach (var transform in transforms)
            {
                if (transform != null)
                {
                    json.Add(TransformTargetJson(root, transform));
                }
            }
            return json;
        }

        private static List<object> StringsJson(List<string> values)
        {
            var json = new List<object>(values != null ? values.Count : 0);
            if (values == null)
            {
                return json;
            }
            foreach (var value in values)
            {
                if (!string.IsNullOrEmpty(value))
                {
                    json.Add(value);
                }
            }
            return json;
        }

        private static List<object> Vector3Json(Vector3 value)
        {
            return new List<object> { value.x, value.y, value.z };
        }

        private static List<object> QuaternionJson(Quaternion value)
        {
            return new List<object> { value.x, value.y, value.z, value.w };
        }

        private static List<string> ReadStringListMember(Type type, object instance, string name)
        {
            var value = ReadMember(type, instance, name);
            var strings = new List<string>();
            if (value is IEnumerable enumerable)
            {
                foreach (var item in enumerable)
                {
                    var text = item != null ? item.ToString() : "";
                    if (!string.IsNullOrEmpty(text))
                    {
                        strings.Add(text);
                    }
                }
            }
            return strings;
        }

        private static List<Component> ReadComponentListMember(Type type, object instance, string name)
        {
            var value = ReadMember(type, instance, name);
            var components = new List<Component>();
            if (value is IEnumerable enumerable)
            {
                foreach (var item in enumerable)
                {
                    if (item is Component component)
                    {
                        components.Add(component);
                    }
                }
            }
            return components;
        }

        private static List<Transform> ReadTransformListMember(Type type, object instance, string name)
        {
            var value = ReadMember(type, instance, name);
            var transforms = new List<Transform>();
            if (value is IEnumerable enumerable)
            {
                foreach (var item in enumerable)
                {
                    if (item is Transform transform)
                    {
                        transforms.Add(transform);
                    }
                }
            }
            return transforms;
        }

        private static Vector3 ReadVector3Member(Type type, object instance, string name, Vector3 fallback)
        {
            var value = ReadMember(type, instance, name);
            return value is Vector3 vector && IsFinite(vector) ? vector : fallback;
        }

        private static Quaternion ReadQuaternionMember(Type type, object instance, string name, Quaternion fallback)
        {
            var value = ReadMember(type, instance, name);
            return value is Quaternion quaternion && IsFinite(quaternion) ? quaternion : fallback;
        }

        private static object ReadAnimationCurveMember(Type type, object instance, string name)
        {
            return AnimationCurveToJson(ReadMember(type, instance, name) as AnimationCurve);
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
                return SanitizeFloat(f, fallback);
            }
            if (value is double d)
            {
                return SanitizeFloat((float)d, fallback);
            }
            if (value is int i)
            {
                return SanitizeFloat(i, fallback);
            }
            if (value is long l)
            {
                return SanitizeFloat(l, fallback);
            }
            if (float.TryParse(value.ToString(), NumberStyles.Float, CultureInfo.InvariantCulture, out var parsed))
            {
                return SanitizeFloat(parsed, fallback);
            }
            return fallback;
        }

        private static float SanitizeFloat(float value, float fallback)
        {
            return float.IsNaN(value) || float.IsInfinity(value) ? fallback : value;
        }

        private static bool IsFinite(Vector3 value)
        {
            return IsFinite(value.x) && IsFinite(value.y) && IsFinite(value.z);
        }

        private static bool IsFinite(Quaternion value)
        {
            return IsFinite(value.x) && IsFinite(value.y) && IsFinite(value.z) && IsFinite(value.w);
        }

        private static bool IsFinite(float value)
        {
            return !float.IsNaN(value) && !float.IsInfinity(value);
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

        private static string ReadStringMember(Type type, object instance, string name, string fallback)
        {
            var value = ReadMember(type, instance, name);
            return value != null ? value.ToString() ?? fallback : fallback;
        }

        private static void AddOptionalSourceParam(Dictionary<string, object> target, Type type, object instance, string name)
        {
            if (target == null || type == null || instance == null || string.IsNullOrEmpty(name))
            {
                return;
            }
            var value = ReadMember(type, instance, name);
            if (value == null)
            {
                return;
            }
            switch (value)
            {
                case bool b:
                    target[name] = b;
                    break;
                case int i:
                    target[name] = i;
                    break;
                case long l:
                    target[name] = l;
                    break;
                case float f:
                    target[name] = SanitizeFloat(f, 0.0f);
                    break;
                case double d:
                    target[name] = SanitizeFloat((float)d, 0.0f);
                    break;
                case string s:
                    target[name] = s;
                    break;
                default:
                    target[name] = value.ToString() ?? "";
                    break;
            }
        }
    }
}
