using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace UNAvatar.UnityExporter
{
    internal static class HumanoidExtractor
    {
        public static Dictionary<string, string> Extract(GameObject root)
        {
            var result = new Dictionary<string, string>();
            if (root == null)
            {
                return result;
            }

            var animator = root.GetComponentInChildren<Animator>(true);
            if (animator == null || !animator.isHuman)
            {
                return result;
            }

            foreach (HumanBodyBones bone in Enum.GetValues(typeof(HumanBodyBones)))
            {
                if (bone == HumanBodyBones.LastBone)
                {
                    continue;
                }
                var transform = animator.GetBoneTransform(bone);
                if (transform != null && transform.IsChildOf(root.transform))
                {
                    result[bone.ToString()] = VariantExtractor.TransformPath(root.transform, transform);
                }
            }
            return result;
        }
    }
}

