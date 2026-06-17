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
    internal static partial class MiniJson
    {
        private static void WriteValue(StringBuilder sb, object value)
        {
            if (value == null)
            {
                sb.Append("null");
                return;
            }

            switch (value)
            {
                case string s:
                    sb.Append('"').Append(EscapeString(s)).Append('"');
                    break;
                case bool b:
                    sb.Append(b ? "true" : "false");
                    break;
                case byte _:
                case sbyte _:
                case short _:
                case ushort _:
                case int _:
                case uint _:
                case long _:
                case ulong _:
                case decimal _:
                    sb.Append(Convert.ToString(value, CultureInfo.InvariantCulture));
                    break;
                case float f:
                    WriteFiniteFloat(sb, f);
                    break;
                case double d:
                    WriteFiniteDouble(sb, d);
                    break;
                case IDictionary<string, object> map:
                    WriteObject(sb, map);
                    break;
                case IDictionary dictionary:
                    WriteDictionary(sb, dictionary);
                    break;
                case IEnumerable enumerable:
                    WriteArray(sb, enumerable);
                    break;
                default:
                    sb.Append('"').Append(EscapeString(Convert.ToString(value, CultureInfo.InvariantCulture))).Append('"');
                    break;
            }
        }

        private static void WriteFiniteFloat(StringBuilder sb, float value)
        {
            if (float.IsNaN(value) || float.IsInfinity(value))
            {
                sb.Append("null");
                return;
            }
            sb.Append(value.ToString(CultureInfo.InvariantCulture));
        }

        private static void WriteFiniteDouble(StringBuilder sb, double value)
        {
            if (double.IsNaN(value) || double.IsInfinity(value))
            {
                sb.Append("null");
                return;
            }
            sb.Append(value.ToString(CultureInfo.InvariantCulture));
        }

        private static void WriteObject(StringBuilder sb, IDictionary<string, object> map)
        {
            sb.Append('{');
            var first = true;
            foreach (var item in map)
            {
                if (!first)
                {
                    sb.Append(',');
                }
                first = false;
                sb.Append('"').Append(EscapeString(item.Key)).Append("\":");
                WriteValue(sb, item.Value);
            }
            sb.Append('}');
        }

        private static void WriteDictionary(StringBuilder sb, IDictionary map)
        {
            sb.Append('{');
            var first = true;
            foreach (DictionaryEntry item in map)
            {
                if (!first)
                {
                    sb.Append(',');
                }
                first = false;
                sb.Append('"').Append(EscapeString(Convert.ToString(item.Key, CultureInfo.InvariantCulture))).Append("\":");
                WriteValue(sb, item.Value);
            }
            sb.Append('}');
        }

        private static void WriteArray(StringBuilder sb, IEnumerable values)
        {
            sb.Append('[');
            var first = true;
            foreach (var item in values)
            {
                if (!first)
                {
                    sb.Append(',');
                }
                first = false;
                WriteValue(sb, item);
            }
            sb.Append(']');
        }
    }
}
