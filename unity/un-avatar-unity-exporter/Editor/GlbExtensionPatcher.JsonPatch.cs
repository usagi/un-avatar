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
    internal static partial class GlbExtensionPatcher
    {
        private static int ExistingArrayLength(string json, string propertyName)
        {
            var keyIndex = json.IndexOf("\"" + propertyName + "\"", StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                return 0;
            }
            var colon = json.IndexOf(':', keyIndex);
            var arrayStart = json.IndexOf('[', colon);
            var arrayEnd = FindMatchingBracket(json, arrayStart);
            var inner = json.Substring(arrayStart + 1, arrayEnd - arrayStart - 1).Trim();
            if (inner.Length == 0)
            {
                return 0;
            }
            var count = 1;
            var depth = 0;
            var inString = false;
            var escaped = false;
            for (var i = 0; i < inner.Length; i++)
            {
                var c = inner[i];
                if (inString)
                {
                    if (escaped)
                    {
                        escaped = false;
                    }
                    else if (c == '\\')
                    {
                        escaped = true;
                    }
                    else if (c == '"')
                    {
                        inString = false;
                    }
                    continue;
                }
                if (c == '"')
                {
                    inString = true;
                }
                else if (c == '[' || c == '{')
                {
                    depth++;
                }
                else if (c == ']' || c == '}')
                {
                    depth--;
                }
                else if (c == ',' && depth == 0)
                {
                    count++;
                }
            }
            return count;
        }

        private static string AppendRootArrayItems(string json, string propertyName, List<string> items)
        {
            if (items == null || items.Count == 0)
            {
                return json;
            }
            var joinedItems = string.Join(",", items);
            var keyIndex = json.IndexOf("\"" + propertyName + "\"", StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                return InsertRootProperty(json, "\"" + propertyName + "\":[" + joinedItems + "]");
            }
            var colon = json.IndexOf(':', keyIndex);
            var arrayStart = json.IndexOf('[', colon);
            var arrayEnd = FindMatchingBracket(json, arrayStart);
            var existing = json.Substring(arrayStart + 1, arrayEnd - arrayStart - 1).Trim();
            var replacement = existing.Length == 0
                ? "[" + joinedItems + "]"
                : "[" + existing + "," + joinedItems + "]";
            return json.Substring(0, arrayStart) + replacement + json.Substring(arrayEnd + 1);
        }

        private static string UpdatePrimaryBufferByteLength(string json, int byteLength)
        {
            var buffersIndex = json.IndexOf("\"buffers\"", StringComparison.Ordinal);
            if (buffersIndex < 0)
            {
                return InsertRootProperty(json, "\"buffers\":[{\"byteLength\":" + byteLength.ToString(CultureInfo.InvariantCulture) + "}]");
            }
            var byteLengthIndex = json.IndexOf("\"byteLength\"", buffersIndex, StringComparison.Ordinal);
            if (byteLengthIndex < 0)
            {
                return json;
            }
            var colon = json.IndexOf(':', byteLengthIndex);
            var valueStart = colon + 1;
            while (valueStart < json.Length && char.IsWhiteSpace(json[valueStart]))
            {
                valueStart++;
            }
            var valueEnd = valueStart;
            while (valueEnd < json.Length && char.IsDigit(json[valueEnd]))
            {
                valueEnd++;
            }
            return json.Substring(0, valueStart) + byteLength.ToString(CultureInfo.InvariantCulture) + json.Substring(valueEnd);
        }

        private static string PatchRootJson(string json, string extensionName, Dictionary<string, object> payload)
        {
            json = AddExtensionUsed(json, extensionName);
            var extensionJson = MiniJson.Serialize(payload);
            var property = "\"" + MiniJson.EscapeString(extensionName) + "\":" + extensionJson;
            var extensionsIndex = json.IndexOf("\"extensions\"", StringComparison.Ordinal);
            if (extensionsIndex < 0)
            {
                return InsertRootProperty(json, "\"extensions\":{" + property + "}");
            }

            var colon = json.IndexOf(':', extensionsIndex);
            var objectStart = json.IndexOf('{', colon);
            var objectEnd = FindMatchingBrace(json, objectStart);
            var existing = json.Substring(objectStart + 1, objectEnd - objectStart - 1).Trim();
            var replacement = existing.Length == 0 ? "{" + property + "}" : "{" + existing + "," + property + "}";
            return json.Substring(0, objectStart) + replacement + json.Substring(objectEnd + 1);
        }

        private static string AddExtensionUsed(string json, string extensionName)
        {
            if (json.Contains("\"" + extensionName + "\""))
            {
                return json;
            }

            var keyIndex = json.IndexOf("\"extensionsUsed\"", StringComparison.Ordinal);
            if (keyIndex < 0)
            {
                return InsertRootProperty(json, "\"extensionsUsed\":[\"" + MiniJson.EscapeString(extensionName) + "\"]");
            }

            var colon = json.IndexOf(':', keyIndex);
            var arrayStart = json.IndexOf('[', colon);
            var arrayEnd = FindMatchingBracket(json, arrayStart);
            var existing = json.Substring(arrayStart + 1, arrayEnd - arrayStart - 1).Trim();
            var replacement = existing.Length == 0
                ? "[\"" + MiniJson.EscapeString(extensionName) + "\"]"
                : "[" + existing + ",\"" + MiniJson.EscapeString(extensionName) + "\"]";
            return json.Substring(0, arrayStart) + replacement + json.Substring(arrayEnd + 1);
        }

        private static string InsertRootProperty(string json, string property)
        {
            var end = json.LastIndexOf('}');
            if (end < 0)
            {
                throw new InvalidDataException("GLB JSON root is not an object.");
            }
            var before = json.Substring(0, end).TrimEnd();
            var separator = before.EndsWith("{", StringComparison.Ordinal) ? "" : ",";
            return before + separator + property + json.Substring(end);
        }

        private static int FindMatchingBrace(string text, int openIndex)
        {
            return FindMatching(text, openIndex, '{', '}');
        }

        private static int FindMatchingBracket(string text, int openIndex)
        {
            return FindMatching(text, openIndex, '[', ']');
        }

        private static int FindMatching(string text, int openIndex, char open, char close)
        {
            if (openIndex < 0 || text[openIndex] != open)
            {
                throw new InvalidDataException("JSON delimiter was not found.");
            }
            var depth = 0;
            var inString = false;
            var escaped = false;
            for (var i = openIndex; i < text.Length; i++)
            {
                var c = text[i];
                if (inString)
                {
                    if (escaped)
                    {
                        escaped = false;
                    }
                    else if (c == '\\')
                    {
                        escaped = true;
                    }
                    else if (c == '"')
                    {
                        inString = false;
                    }
                    continue;
                }

                if (c == '"')
                {
                    inString = true;
                }
                else if (c == open)
                {
                    depth++;
                }
                else if (c == close)
                {
                    depth--;
                    if (depth == 0)
                    {
                        return i;
                    }
                }
            }
            throw new InvalidDataException("Matching JSON delimiter was not found.");
        }
    }
}
