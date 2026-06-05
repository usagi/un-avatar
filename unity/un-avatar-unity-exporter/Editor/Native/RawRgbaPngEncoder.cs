using System;
using System.Runtime.InteropServices;
using Unity.Collections;
using Unity.Collections.LowLevel.Unsafe;
using UnityEngine;

namespace UNAvatar.UnityExporter
{
    public static class RawRgbaPngEncoder
    {
        public static byte[] Encode(Texture2D texture)
        {
            if (texture == null)
            {
                throw new ArgumentNullException(nameof(texture));
            }
            if (texture.format != TextureFormat.RGBA32)
            {
                return texture.EncodeToPNG();
            }

            try
            {
                return EncodeFpng(texture.GetRawTextureData<byte>(), texture.width, texture.height);
            }
            catch (DllNotFoundException)
            {
                return texture.EncodeToPNG();
            }
            catch (EntryPointNotFoundException)
            {
                return texture.EncodeToPNG();
            }
            catch (FpngEncodeException)
            {
                return texture.EncodeToPNG();
            }
        }

        public static byte[] Encode(byte[] unityOrderRgba, int width, int height)
        {
            try
            {
                return EncodeNativeFpngOnly(unityOrderRgba, width, height);
            }
            catch (DllNotFoundException)
            {
                return EncodeUnity(unityOrderRgba, width, height);
            }
            catch (EntryPointNotFoundException)
            {
                return EncodeUnity(unityOrderRgba, width, height);
            }
            catch (FpngEncodeException)
            {
                return EncodeUnity(unityOrderRgba, width, height);
            }
        }

        public static byte[] EncodeNativeFpngOnly(byte[] unityOrderRgba, int width, int height)
        {
            return EncodeFpng(unityOrderRgba, width, height);
        }

        private static byte[] EncodeUnity(byte[] unityOrderRgba, int width, int height)
        {
            var texture = new Texture2D(width, height, TextureFormat.RGBA32, false, false);
            try
            {
                texture.LoadRawTextureData(unityOrderRgba);
                texture.Apply(false, false);
                return texture.EncodeToPNG();
            }
            finally
            {
                UnityEngine.Object.DestroyImmediate(texture);
            }
        }

        private static byte[] EncodeFpng(byte[] unityOrderRgba, int width, int height)
        {
            if (unityOrderRgba == null)
            {
                throw new ArgumentNullException(nameof(unityOrderRgba));
            }
            if (width <= 0 || height <= 0)
            {
                throw new ArgumentOutOfRangeException(nameof(width), "Invalid RAW RGBA PNG encode dimensions.");
            }
            var expectedLength = checked(width * height * 4);
            if (unityOrderRgba.Length != expectedLength)
            {
                throw new ArgumentException("Invalid RAW RGBA PNG encode input.", nameof(unityOrderRgba));
            }

            var handle = GCHandle.Alloc(unityOrderRgba, GCHandleType.Pinned);
            try
            {
                IntPtr data;
                int size;
                var result = unavatar_fpng_encode_rgba32(
                    handle.AddrOfPinnedObject(),
                    width,
                    height,
                    out data,
                    out size);
                if (result != 0)
                {
                    throw new FpngEncodeException("fpng native encoder failed: " + result);
                }
                if (data == IntPtr.Zero || size <= 0)
                {
                    throw new FpngEncodeException("fpng native encoder returned no data.");
                }

                try
                {
                    var bytes = new byte[size];
                    Marshal.Copy(data, bytes, 0, size);
                    return bytes;
                }
                finally
                {
                    unavatar_fpng_free(data);
                }
            }
            finally
            {
                handle.Free();
            }
        }

        private static unsafe byte[] EncodeFpng(NativeArray<byte> unityOrderRgba, int width, int height)
        {
            if (width <= 0 || height <= 0)
            {
                throw new ArgumentOutOfRangeException(nameof(width), "Invalid RAW RGBA PNG encode dimensions.");
            }
            var expectedLength = checked(width * height * 4);
            if (!unityOrderRgba.IsCreated || unityOrderRgba.Length != expectedLength)
            {
                throw new ArgumentException("Invalid RAW RGBA PNG encode input.", nameof(unityOrderRgba));
            }

            IntPtr data;
            int size;
            var result = unavatar_fpng_encode_rgba32(
                (IntPtr)NativeArrayUnsafeUtility.GetUnsafeReadOnlyPtr(unityOrderRgba),
                width,
                height,
                out data,
                out size);
            if (result != 0)
            {
                throw new FpngEncodeException("fpng native encoder failed: " + result);
            }
            if (data == IntPtr.Zero || size <= 0)
            {
                throw new FpngEncodeException("fpng native encoder returned no data.");
            }

            try
            {
                var bytes = new byte[size];
                Marshal.Copy(data, bytes, 0, size);
                return bytes;
            }
            finally
            {
                unavatar_fpng_free(data);
            }
        }

        [DllImport("unavatar_fpng", CallingConvention = CallingConvention.Cdecl)]
        private static extern int unavatar_fpng_encode_rgba32(
            IntPtr rgba,
            int width,
            int height,
            out IntPtr png,
            out int pngSize);

        [DllImport("unavatar_fpng", CallingConvention = CallingConvention.Cdecl)]
        private static extern void unavatar_fpng_free(IntPtr png);

        private sealed class FpngEncodeException : Exception
        {
            public FpngEncodeException(string message)
                : base(message)
            {
            }
        }
    }
}
