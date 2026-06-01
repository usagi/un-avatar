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
    internal static partial class GlbExtensionPatcher
    {
        private static void WriteGlb(string path, List<GlbChunk> chunks)
        {
            var totalLength = 12 + chunks.Sum(c => 8 + c.Data.Length);
            using (var stream = File.Create(path))
            using (var writer = new BinaryWriter(stream))
            {
                writer.Write(GlbMagic);
                writer.Write((uint)2);
                writer.Write((uint)totalLength);
                foreach (var chunk in chunks)
                {
                    writer.Write((uint)chunk.Data.Length);
                    writer.Write(chunk.Type);
                    writer.Write(chunk.Data);
                }
            }
        }

        private static byte[] Pad(byte[] data, byte value)
        {
            var paddedLength = (data.Length + 3) & ~3;
            if (paddedLength == data.Length)
            {
                return data;
            }
            var padded = new byte[paddedLength];
            Buffer.BlockCopy(data, 0, padded, 0, data.Length);
            for (var i = data.Length; i < padded.Length; i++)
            {
                padded[i] = value;
            }
            return padded;
        }

        private static uint ReadUInt32(byte[] bytes, int offset)
        {
            return BitConverter.ToUInt32(bytes, offset);
        }

        private sealed class GlbChunk
        {
            public uint Type;
            public byte[] Data;
        }
    }
}
