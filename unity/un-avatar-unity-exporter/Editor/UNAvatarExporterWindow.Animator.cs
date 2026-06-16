using System;
using System.Collections.Generic;
using UnityEditor;
using UnityEditor.Animations;
using UnityEngine;

namespace UNAvatar.UnityExporter
{
    public sealed partial class UNAvatarExporterWindow
    {
        private const int MaxAnimatorExportControllers = 8;
        private const int MaxAnimatorExportLayersPerController = 32;
        private const int MaxAnimatorExportStatesPerLayer = 192;
        private const int MaxAnimatorExportBindingsPerClip = 32;

        private Dictionary<string, object> BuildAnimatorPayload(GameObject root)
        {
            var controllers = new List<object>();
            var seen = new HashSet<int>();
            if (root != null)
            {
                var rootAnimator = root.GetComponent<Animator>();
                if (rootAnimator != null)
                {
                    AddAnimatorControllerPayload(
                        root.transform,
                        rootAnimator.runtimeAnimatorController as AnimatorController,
                        "rootAnimator",
                        "",
                        controllers,
                        seen);
                }

                var components = root.GetComponentsInChildren<Component>(true);
                AddVrcDescriptorAnimatorControllers(root.transform, components, controllers, seen);
            }

            return new Dictionary<string, object>
            {
                ["schemaVersion"] = "0.1-preview",
                ["controllerCount"] = controllers.Count,
                ["controllers"] = controllers,
                ["enabledActionIds"] = new List<object>()
            };
        }

        private void AddVrcDescriptorAnimatorControllers(
            Transform root,
            Component[] components,
            List<object> controllers,
            HashSet<int> seen)
        {
            var descriptor = FirstVrcAvatarDescriptor(components);
            var baseLayers = descriptor != null ? ReadMember(descriptor.GetType(), descriptor, "baseAnimationLayers") as Array : null;
            if (baseLayers == null)
            {
                return;
            }
            foreach (var layer in baseLayers)
            {
                if (layer == null)
                {
                    continue;
                }
                var layerType = ReadMember(layer.GetType(), layer, "type")?.ToString() ?? "";
                var controller = ReadMember(layer.GetType(), layer, "animatorController") as AnimatorController;
                AddAnimatorControllerPayload(root, controller, "vrcDescriptor", layerType, controllers, seen);
            }
        }

        private void AddAnimatorControllerPayload(
            Transform root,
            AnimatorController controller,
            string source,
            string layerType,
            List<object> controllers,
            HashSet<int> seen)
        {
            if (controllers.Count >= MaxAnimatorExportControllers)
            {
                return;
            }
            if (controller == null || !seen.Add(controller.GetInstanceID()))
            {
                return;
            }

            var parameters = new List<object>();
            foreach (var parameter in controller.parameters)
            {
                parameters.Add(new Dictionary<string, object>
                {
                    ["name"] = parameter.name ?? "",
                    ["type"] = parameter.type.ToString(),
                    ["defaultBool"] = parameter.defaultBool,
                    ["defaultFloat"] = parameter.defaultFloat,
                    ["defaultInt"] = parameter.defaultInt
                });
            }

            var layers = new List<object>();
            foreach (var layer in controller.layers)
            {
                if (layers.Count >= MaxAnimatorExportLayersPerController)
                {
                    break;
                }
                layers.Add(AnimatorLayerToJson(root, layer));
            }

            var json = UnityObjectReferenceHeaderToJson(controller);
            json["source"] = source ?? "";
            json["vrcLayerType"] = layerType ?? "";
            json["parameterCount"] = parameters.Count;
            json["parameters"] = parameters;
            json["layerCount"] = layers.Count;
            json["layers"] = layers;
            controllers.Add(json);
        }

        private Dictionary<string, object> AnimatorLayerToJson(Transform root, AnimatorControllerLayer layer)
        {
            var states = new List<object>();
            var anyStateTransitions = new List<object>();
            if (layer.stateMachine != null)
            {
                AddAnimatorStateMachineStates(root, layer.stateMachine, "", states, anyStateTransitions);
            }
            var anyStateDestinationNames = new HashSet<string>();
            foreach (var transitionObject in anyStateTransitions)
            {
                if (transitionObject is Dictionary<string, object> transition &&
                    transition.TryGetValue("destinationState", out var destination) &&
                    destination is string destinationName &&
                    !string.IsNullOrEmpty(destinationName))
                {
                    anyStateDestinationNames.Add(destinationName);
                }
            }
            if (anyStateDestinationNames.Count > 0)
            {
                states.RemoveAll(stateObject =>
                {
                    if (stateObject is not Dictionary<string, object> state ||
                        !state.TryGetValue("name", out var name) ||
                        name is not string stateName)
                    {
                        return true;
                    }
                    return !anyStateDestinationNames.Contains(stateName);
                });
            }
            if (states.Count > MaxAnimatorExportStatesPerLayer)
            {
                states.RemoveRange(MaxAnimatorExportStatesPerLayer, states.Count - MaxAnimatorExportStatesPerLayer);
            }
            return new Dictionary<string, object>
            {
                ["name"] = layer.name ?? "",
                ["stateCount"] = states.Count,
                ["states"] = states,
                ["anyStateTransitionCount"] = anyStateTransitions.Count,
                ["anyStateTransitions"] = anyStateTransitions
            };
        }

        private void AddAnimatorStateMachineStates(
            Transform root,
            AnimatorStateMachine stateMachine,
            string path,
            List<object> states,
            List<object> anyStateTransitions)
        {
            foreach (var transition in stateMachine.anyStateTransitions)
            {
                anyStateTransitions.Add(AnimatorTransitionToJson(transition));
            }
            foreach (var child in stateMachine.states)
            {
                if (states.Count >= MaxAnimatorExportStatesPerLayer)
                {
                    break;
                }
                if (child.state == null)
                {
                    continue;
                }
                var statePath = string.IsNullOrEmpty(path) ? child.state.name : path + "/" + child.state.name;
                states.Add(AnimatorStateToJson(root, child.state, statePath));
            }
            foreach (var childMachine in stateMachine.stateMachines)
            {
                if (states.Count >= MaxAnimatorExportStatesPerLayer)
                {
                    break;
                }
                if (childMachine.stateMachine == null)
                {
                    continue;
                }
                var childPath = string.IsNullOrEmpty(path) ? childMachine.stateMachine.name : path + "/" + childMachine.stateMachine.name;
                AddAnimatorStateMachineStates(root, childMachine.stateMachine, childPath, states, anyStateTransitions);
            }
        }

        private Dictionary<string, object> AnimatorStateToJson(Transform root, AnimatorState state, string statePath)
        {
            var transitions = new List<object>();
            foreach (var transition in state.transitions)
            {
                transitions.Add(AnimatorTransitionToJson(transition));
            }
            return new Dictionary<string, object>
            {
                ["name"] = state.name ?? "",
                ["path"] = statePath ?? "",
                ["motion"] = AnimatorMotionToJson(root, state.motion, 0),
                ["transitionCount"] = transitions.Count,
                ["transitions"] = transitions
            };
        }

        private object AnimatorMotionToJson(Transform root, Motion motion, int depth)
        {
            if (motion == null || depth > 4)
            {
                return null;
            }
            if (motion is AnimationClip clip)
            {
                return AnimatorClipToJson(root, clip);
            }
            if (motion is BlendTree blendTree)
            {
                var children = new List<object>();
                foreach (var child in blendTree.children)
                {
                    children.Add(AnimatorMotionToJson(root, child.motion, depth + 1));
                }
                var json = UnityObjectReferenceHeaderToJson(blendTree);
                json["motionType"] = "BlendTree";
                json["blendType"] = blendTree.blendType.ToString();
                json["blendParameter"] = blendTree.blendParameter ?? "";
                json["blendParameterY"] = blendTree.blendParameterY ?? "";
                json["childCount"] = children.Count;
                json["children"] = children;
                return json;
            }
            var fallback = UnityObjectReferenceHeaderToJson(motion);
            fallback["motionType"] = motion.GetType().Name;
            return fallback;
        }

        private Dictionary<string, object> AnimatorClipToJson(Transform root, AnimationClip clip)
        {
            var curveBindings = new List<object>();
            foreach (var binding in AnimationUtility.GetCurveBindings(clip))
            {
                if (curveBindings.Count >= MaxAnimatorExportBindingsPerClip)
                {
                    break;
                }
                if (!IsAnimatorActionCurveBinding(binding))
                {
                    continue;
                }
                var curve = AnimationUtility.GetEditorCurve(clip, binding);
                curveBindings.Add(AnimatorCurveBindingToJson(binding, curve));
            }

            var objectBindings = new List<object>();
            foreach (var binding in AnimationUtility.GetObjectReferenceCurveBindings(clip))
            {
                if (objectBindings.Count >= MaxAnimatorExportBindingsPerClip)
                {
                    break;
                }
                if (!IsAnimatorActionObjectReferenceBinding(binding))
                {
                    continue;
                }
                var keyframes = AnimationUtility.GetObjectReferenceCurve(clip, binding);
                objectBindings.Add(AnimatorObjectReferenceBindingToJson(binding, keyframes));
            }

            var json = UnityObjectReferenceHeaderToJson(clip);
            json["motionType"] = "AnimationClip";
            json["length"] = clip.length;
            json["curveBindingCount"] = curveBindings.Count;
            json["curveBindings"] = curveBindings;
            json["objectReferenceBindingCount"] = objectBindings.Count;
            json["objectReferenceBindings"] = objectBindings;
            return json;
        }

        private static bool IsAnimatorActionCurveBinding(EditorCurveBinding binding)
        {
            var propertyName = binding.propertyName ?? "";
            if (propertyName == "m_IsActive" || propertyName == "m_Enabled")
            {
                return true;
            }
            if (propertyName.StartsWith("blendShape.", StringComparison.Ordinal))
            {
                return true;
            }
            return propertyName.StartsWith("material.", StringComparison.Ordinal) ||
                propertyName.StartsWith("m_Materials.", StringComparison.Ordinal);
        }

        private static bool IsAnimatorActionObjectReferenceBinding(EditorCurveBinding binding)
        {
            var propertyName = binding.propertyName ?? "";
            return propertyName.StartsWith("m_Materials.", StringComparison.Ordinal) ||
                propertyName.StartsWith("material.", StringComparison.Ordinal);
        }

        private static Dictionary<string, object> AnimatorCurveBindingToJson(EditorCurveBinding binding, AnimationCurve curve)
        {
            var json = AnimatorBindingHeaderToJson(binding);
            json["keyCount"] = curve != null && curve.keys != null ? curve.keys.Length : 0;
            if (curve != null && curve.keys != null && curve.keys.Length > 0)
            {
                var first = curve.keys[0];
                var last = curve.keys[curve.keys.Length - 1];
                json["firstValue"] = first.value;
                json["lastValue"] = last.value;
                if (AnimatorCurveConstantValue(curve, out var constant))
                {
                    json["constantValue"] = constant;
                }
            }
            return json;
        }

        private static Dictionary<string, object> AnimatorObjectReferenceBindingToJson(
            EditorCurveBinding binding,
            ObjectReferenceKeyframe[] keyframes)
        {
            var json = AnimatorBindingHeaderToJson(binding);
            json["keyCount"] = keyframes != null ? keyframes.Length : 0;
            if (keyframes != null && keyframes.Length > 0)
            {
                json["firstValue"] = keyframes[0].value != null ? UnityObjectReferenceHeaderToJson(keyframes[0].value) : null;
                json["lastValue"] = keyframes[keyframes.Length - 1].value != null
                    ? UnityObjectReferenceHeaderToJson(keyframes[keyframes.Length - 1].value)
                    : null;
            }
            return json;
        }

        private static Dictionary<string, object> AnimatorBindingHeaderToJson(EditorCurveBinding binding)
        {
            return new Dictionary<string, object>
            {
                ["path"] = binding.path ?? "",
                ["propertyName"] = binding.propertyName ?? "",
                ["type"] = binding.type != null ? binding.type.FullName ?? binding.type.Name : "",
                ["isPPtrCurve"] = binding.isPPtrCurve
            };
        }

        private static bool AnimatorCurveConstantValue(AnimationCurve curve, out float value)
        {
            value = 0.0f;
            if (curve == null || curve.keys == null || curve.keys.Length == 0)
            {
                return false;
            }
            value = curve.keys[0].value;
            for (var i = 1; i < curve.keys.Length; i++)
            {
                if (Mathf.Abs(curve.keys[i].value - value) > 0.0001f)
                {
                    return false;
                }
            }
            return true;
        }

        private static Dictionary<string, object> AnimatorTransitionToJson(AnimatorTransitionBase transition)
        {
            var conditions = new List<object>();
            foreach (var condition in transition.conditions)
            {
                conditions.Add(new Dictionary<string, object>
                {
                    ["parameter"] = condition.parameter ?? "",
                    ["mode"] = condition.mode.ToString(),
                    ["threshold"] = condition.threshold
                });
            }
            return new Dictionary<string, object>
            {
                ["name"] = transition.name ?? "",
                ["destinationState"] = transition.destinationState != null ? transition.destinationState.name ?? "" : "",
                ["destinationStateMachine"] = transition.destinationStateMachine != null ? transition.destinationStateMachine.name ?? "" : "",
                ["isExit"] = transition.isExit,
                ["conditionCount"] = conditions.Count,
                ["conditions"] = conditions
            };
        }
    }
}
