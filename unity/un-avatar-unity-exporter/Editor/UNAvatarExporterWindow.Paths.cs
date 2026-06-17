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
    public sealed partial class UNAvatarExporterWindow
    {
        private static void SetActiveRecursive(Transform root, bool active)
        {
            root.gameObject.SetActive(active);
            for (var i = 0; i < root.childCount; i++)
            {
                SetActiveRecursive(root.GetChild(i), active);
            }
        }

        private static string EnsureUnavatarExtension(string path)
        {
            if (string.Equals(Path.GetExtension(path), ".unavatar", StringComparison.OrdinalIgnoreCase))
            {
                return path;
            }
            return Path.ChangeExtension(path, ".unavatar");
        }

        private string ResolveInitialExportDirectory(string currentPath)
        {
            if (!string.IsNullOrWhiteSpace(currentPath))
            {
                try
                {
                    var directory = Path.GetDirectoryName(currentPath);
                    if (!string.IsNullOrWhiteSpace(directory) && Directory.Exists(directory))
                    {
                        return directory;
                    }
                }
                catch (ArgumentException)
                {
                }
            }

            var projectRoot = Directory.GetParent(Application.dataPath);
            return projectRoot != null ? projectRoot.FullName : Application.dataPath;
        }

        private string ResolveInitialExportName(string currentPath)
        {
            if (!string.IsNullOrWhiteSpace(currentPath))
            {
                try
                {
                    var fileName = Path.GetFileNameWithoutExtension(currentPath);
                    if (!string.IsNullOrWhiteSpace(fileName))
                    {
                        return fileName;
                    }
                }
                catch (ArgumentException)
                {
                }
            }

            return avatarRoot != null ? SanitizeFileName(avatarRoot.name) : "avatar";
        }

        private static string SanitizeFileName(string value)
        {
            var invalid = Path.GetInvalidFileNameChars();
            var chars = value.ToCharArray();
            for (var i = 0; i < chars.Length; i++)
            {
                for (var j = 0; j < invalid.Length; j++)
                {
                    if (chars[i] == invalid[j])
                    {
                        chars[i] = '_';
                        break;
                    }
                }
            }
            var sanitized = new string(chars).Trim();
            return string.IsNullOrEmpty(sanitized) ? "avatar" : sanitized;
        }
    }
}
