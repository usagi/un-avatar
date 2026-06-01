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
    internal static partial class MiniJson
    {
        public static object Deserialize(string json)
        {
            return new Parser(json).Parse();
        }

        public static string Serialize(object value)
        {
            var sb = new StringBuilder();
            WriteValue(sb, value);
            return sb.ToString();
        }

        public static string EscapeString(string value)
        {
            if (string.IsNullOrEmpty(value))
            {
                return value ?? "";
            }
            var firstEscaped = -1;
            for (var i = 0; i < value.Length; i++)
            {
                var c = value[i];
                if (c == '"' || c == '\\' || c < 0x20)
                {
                    firstEscaped = i;
                    break;
                }
            }
            if (firstEscaped < 0)
            {
                return value;
            }

            var sb = new StringBuilder();
            if (firstEscaped > 0)
            {
                sb.Append(value, 0, firstEscaped);
            }
            for (var i = firstEscaped; i < value.Length; i++)
            {
                var c = value[i];
                switch (c)
                {
                    case '"': sb.Append("\\\""); break;
                    case '\\': sb.Append("\\\\"); break;
                    case '\b': sb.Append("\\b"); break;
                    case '\f': sb.Append("\\f"); break;
                    case '\n': sb.Append("\\n"); break;
                    case '\r': sb.Append("\\r"); break;
                    case '\t': sb.Append("\\t"); break;
                    default:
                        if (c < 0x20)
                        {
                            sb.Append("\\u");
                            sb.Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        }
                        else
                        {
                            sb.Append(c);
                        }
                        break;
                }
            }
            return sb.ToString();
        }
    }
}
