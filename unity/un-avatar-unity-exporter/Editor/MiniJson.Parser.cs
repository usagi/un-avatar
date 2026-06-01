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
        private sealed class Parser
        {
            private readonly string json;
            private int index;

            public Parser(string json)
            {
                this.json = json ?? "";
            }

            public object Parse()
            {
                var value = ParseValue();
                SkipWhitespace();
                return value;
            }

            private object ParseValue()
            {
                SkipWhitespace();
                if (index >= json.Length)
                {
                    throw new InvalidDataException("Unexpected end of JSON.");
                }

                switch (json[index])
                {
                    case '{': return ParseObject();
                    case '[': return ParseArray();
                    case '"': return ParseString();
                    case 't': Expect("true"); return true;
                    case 'f': Expect("false"); return false;
                    case 'n': Expect("null"); return null;
                    default: return ParseNumber();
                }
            }

            private Dictionary<string, object> ParseObject()
            {
                Expect('{');
                var result = new Dictionary<string, object>();
                SkipWhitespace();
                if (TryConsume('}'))
                {
                    return result;
                }
                while (true)
                {
                    var key = ParseString();
                    SkipWhitespace();
                    Expect(':');
                    result[key] = ParseValue();
                    SkipWhitespace();
                    if (TryConsume('}'))
                    {
                        return result;
                    }
                    Expect(',');
                }
            }

            private List<object> ParseArray()
            {
                Expect('[');
                var result = new List<object>();
                SkipWhitespace();
                if (TryConsume(']'))
                {
                    return result;
                }
                while (true)
                {
                    result.Add(ParseValue());
                    SkipWhitespace();
                    if (TryConsume(']'))
                    {
                        return result;
                    }
                    Expect(',');
                }
            }

            private string ParseString()
            {
                Expect('"');
                var sb = new StringBuilder();
                while (index < json.Length)
                {
                    var c = json[index++];
                    if (c == '"')
                    {
                        return sb.ToString();
                    }
                    if (c != '\\')
                    {
                        sb.Append(c);
                        continue;
                    }
                    if (index >= json.Length)
                    {
                        throw new InvalidDataException("Invalid JSON string escape.");
                    }
                    var escaped = json[index++];
                    switch (escaped)
                    {
                        case '"': sb.Append('"'); break;
                        case '\\': sb.Append('\\'); break;
                        case '/': sb.Append('/'); break;
                        case 'b': sb.Append('\b'); break;
                        case 'f': sb.Append('\f'); break;
                        case 'n': sb.Append('\n'); break;
                        case 'r': sb.Append('\r'); break;
                        case 't': sb.Append('\t'); break;
                        case 'u':
                            if (index + 4 > json.Length)
                            {
                                throw new InvalidDataException("Invalid JSON unicode escape.");
                            }
                            var hex = json.Substring(index, 4);
                            sb.Append((char)int.Parse(hex, NumberStyles.HexNumber, CultureInfo.InvariantCulture));
                            index += 4;
                            break;
                        default:
                            throw new InvalidDataException("Invalid JSON string escape.");
                    }
                }
                throw new InvalidDataException("Unterminated JSON string.");
            }

            private double ParseNumber()
            {
                var start = index;
                if (json[index] == '-')
                {
                    index++;
                }
                while (index < json.Length && char.IsDigit(json[index]))
                {
                    index++;
                }
                if (index < json.Length && json[index] == '.')
                {
                    index++;
                    while (index < json.Length && char.IsDigit(json[index]))
                    {
                        index++;
                    }
                }
                if (index < json.Length && (json[index] == 'e' || json[index] == 'E'))
                {
                    index++;
                    if (index < json.Length && (json[index] == '+' || json[index] == '-'))
                    {
                        index++;
                    }
                    while (index < json.Length && char.IsDigit(json[index]))
                    {
                        index++;
                    }
                }
                return double.Parse(json.Substring(start, index - start), CultureInfo.InvariantCulture);
            }

            private void SkipWhitespace()
            {
                while (index < json.Length && char.IsWhiteSpace(json[index]))
                {
                    index++;
                }
            }

            private bool TryConsume(char expected)
            {
                SkipWhitespace();
                if (index < json.Length && json[index] == expected)
                {
                    index++;
                    return true;
                }
                return false;
            }

            private void Expect(char expected)
            {
                SkipWhitespace();
                if (index >= json.Length || json[index] != expected)
                {
                    throw new InvalidDataException("Expected `" + expected + "` in JSON.");
                }
                index++;
            }

            private void Expect(string expected)
            {
                if (index + expected.Length > json.Length || json.Substring(index, expected.Length) != expected)
                {
                    throw new InvalidDataException("Expected `" + expected + "` in JSON.");
                }
                index += expected.Length;
            }
        }
    }
}
