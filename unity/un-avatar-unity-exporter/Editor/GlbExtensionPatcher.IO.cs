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
        private static List<GlbChunk> ReadChunks(byte[] bytes)
        {
            var magic = ReadUInt32(bytes, 0);
            var version = ReadUInt32(bytes, 4);
            if (magic != GlbMagic || version != 2)
            {
                throw new InvalidDataException("Expected GLB version 2.");
            }

            var chunks = new List<GlbChunk>();
            var offset = 12;
            while (offset + 8 <= bytes.Length)
            {
                var length = checked((int)ReadUInt32(bytes, offset));
                var type = ReadUInt32(bytes, offset + 4);
                offset += 8;
                if (offset + length > bytes.Length)
                {
                    throw new InvalidDataException("GLB chunk exceeds file size.");
                }
                var data = new byte[length];
                Buffer.BlockCopy(bytes, offset, data, 0, length);
                chunks.Add(new GlbChunk { Type = type, Data = data });
                offset += length;
            }
            return chunks;
        }

        private static bool TryGetRootList(Dictionary<string, object> map, string key, out List<object> value)
        {
            value = null;
            if (!map.TryGetValue(key, out var raw))
            {
                return false;
            }
            value = raw as List<object>;
            return value != null;
        }

        private static float ReadRootFloat(Dictionary<string, object> map, string key, float fallback)
        {
            if (!map.TryGetValue(key, out var value))
            {
                return fallback;
            }
            if (value is double d)
            {
                return (float)d;
            }
            if (value is float f)
            {
                return f;
            }
            if (value is int i)
            {
                return i;
            }
            return fallback;
        }
    }
}
