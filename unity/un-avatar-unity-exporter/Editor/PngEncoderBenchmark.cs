using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using UnityEditor;
using UnityEngine;

namespace UNAvatar.UnityExporter
{
    internal static class PngEncoderBenchmark
    {
        private const string EnabledPrefKey = "UNAvatar.UnityExporter.PngEncoderBenchmarkEnabled";
        private const int WarmupIterations = 2;
        private const int TimedIterations = 8;

        public static bool IsEnabled
        {
            get => EditorPrefs.GetBool(EnabledPrefKey, false);
            set => EditorPrefs.SetBool(EnabledPrefKey, value);
        }

        private static bool CanRun => UNAvatarExporterWindow.IsDeveloperModeEnabled && IsEnabled;

        [MenuItem("Tools/U.N. Avatar/Benchmark PNG Encoders")]
        public static void RunMenu()
        {
            if (!CanRun)
            {
                UnityEngine.Debug.LogWarning("[U.N. Avatar] PNG encoder benchmark is disabled. Enable Developer mode in the exporter window first.");
                return;
            }

            var report = Run();
            var path = Path.Combine(Path.GetTempPath(), "un-avatar-png-encoder-benchmark.csv");
            File.WriteAllText(path, report, new UTF8Encoding(false));
            UnityEngine.Debug.Log("[U.N. Avatar] PNG encoder benchmark written to " + path + "\n" + report);
        }

        [MenuItem("Tools/U.N. Avatar/Benchmark PNG Encoders", true)]
        public static bool ValidateRunMenu()
        {
            return CanRun;
        }

        public static string Run()
        {
            var cases = new List<BenchmarkInput>
            {
                BenchmarkInput.Create(512, 512, "gradient"),
                BenchmarkInput.Create(512, 512, "noise"),
                BenchmarkInput.Create(1024, 1024, "gradient"),
                BenchmarkInput.Create(1024, 1024, "noise"),
                BenchmarkInput.Create(2048, 2048, "gradient"),
                BenchmarkInput.Create(2048, 2048, "noise")
            };

            var rows = new List<BenchmarkRow>(cases.Count * 4);
            foreach (var input in cases)
            {
                rows.Add(Measure(input, "Texture2D.EncodeToPNG", EncodeWithTexture2D));
                rows.Add(Measure(input, "ImageConversion.EncodeArrayToPNG(main)", EncodeWithImageConversion));
                rows.Add(Measure(input, "ImageConversion.EncodeArrayToPNG(worker)", EncodeWithImageConversionWorker));
                rows.Add(Measure(input, "fpng(native)", EncodeWithFpngNative));
            }
            return ToCsv(rows);
        }

        private static BenchmarkRow Measure(BenchmarkInput input, string encoder, Func<BenchmarkInput, byte[]> encode)
        {
            byte[] last = null;
            try
            {
                for (var i = 0; i < WarmupIterations; i++)
                {
                    last = encode(input);
                }

                var elapsed = new double[TimedIterations];
                var gcBefore = GC.CollectionCount(0);
                for (var i = 0; i < TimedIterations; i++)
                {
                    var sw = Stopwatch.StartNew();
                    last = encode(input);
                    sw.Stop();
                    elapsed[i] = sw.Elapsed.TotalMilliseconds;
                }
                Array.Sort(elapsed);
                var decodeResult = VerifyDecodedRgba(input, last);

                return new BenchmarkRow
                {
                    Encoder = encoder,
                    Width = input.Width,
                    Height = input.Height,
                    Pattern = input.Pattern,
                    Iterations = TimedIterations,
                    P50Ms = Percentile(elapsed, 0.50),
                    P90Ms = Percentile(elapsed, 0.90),
                    MinMs = elapsed[0],
                    MaxMs = elapsed[elapsed.Length - 1],
                    PngBytes = last != null ? last.Length : 0,
                    Gen0Collections = GC.CollectionCount(0) - gcBefore,
                    DecodeMatches = decodeResult.Matches,
                    DecodeError = decodeResult.Error,
                    Error = ""
                };
            }
            catch (Exception ex)
            {
                return new BenchmarkRow
                {
                    Encoder = encoder,
                    Width = input.Width,
                    Height = input.Height,
                    Pattern = input.Pattern,
                    Iterations = 0,
                    DecodeMatches = false,
                    Error = ex.GetType().Name + ": " + ex.Message
                };
            }
        }

        private static byte[] EncodeWithTexture2D(BenchmarkInput input)
        {
            var texture = new Texture2D(input.Width, input.Height, TextureFormat.RGBA32, false, false);
            try
            {
                texture.LoadRawTextureData(input.Rgba);
                texture.Apply(false, false);
                return texture.EncodeToPNG();
            }
            finally
            {
                UnityEngine.Object.DestroyImmediate(texture);
            }
        }

        private static byte[] EncodeWithImageConversion(BenchmarkInput input)
        {
            return ImageConversion.EncodeArrayToPNG(
                input.Rgba,
                UnityEngine.Experimental.Rendering.GraphicsFormat.R8G8B8A8_UNorm,
                (uint)input.Width,
                (uint)input.Height);
        }

        private static byte[] EncodeWithImageConversionWorker(BenchmarkInput input)
        {
            return Task.Run(() => EncodeWithImageConversion(input)).GetAwaiter().GetResult();
        }

        private static byte[] EncodeWithFpngNative(BenchmarkInput input)
        {
            var handle = GCHandle.Alloc(input.Rgba, GCHandleType.Pinned);
            try
            {
                IntPtr data;
                int size;
                var result = unavatar_fpng_encode_rgba32(
                    handle.AddrOfPinnedObject(),
                    input.Width,
                    input.Height,
                    out data,
                    out size);
                if (result != 0)
                {
                    throw new InvalidOperationException("fpng native encoder failed: " + result);
                }
                if (data == IntPtr.Zero || size <= 0)
                {
                    throw new InvalidOperationException("fpng native encoder returned no data.");
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

        [DllImport("unavatar_fpng", CallingConvention = CallingConvention.Cdecl)]
        private static extern int unavatar_fpng_encode_rgba32(
            IntPtr rgba,
            int width,
            int height,
            out IntPtr png,
            out int pngSize);

        [DllImport("unavatar_fpng", CallingConvention = CallingConvention.Cdecl)]
        private static extern void unavatar_fpng_free(IntPtr png);

        private static DecodeVerification VerifyDecodedRgba(BenchmarkInput input, byte[] png)
        {
            if (png == null || png.Length == 0)
            {
                return new DecodeVerification { Matches = false, Error = "encoder returned no PNG bytes" };
            }

            var texture = new Texture2D(2, 2, TextureFormat.RGBA32, false, false);
            try
            {
                if (!ImageConversion.LoadImage(texture, png, false))
                {
                    return new DecodeVerification { Matches = false, Error = "Unity PNG decoder rejected output" };
                }
                if (texture.width != input.Width || texture.height != input.Height)
                {
                    return new DecodeVerification
                    {
                        Matches = false,
                        Error = "decoded size " + texture.width.ToString(CultureInfo.InvariantCulture) + "x" + texture.height.ToString(CultureInfo.InvariantCulture)
                    };
                }

                var pixels = texture.GetPixels32();
                var expectedLength = checked(input.Width * input.Height);
                if (pixels.Length != expectedLength)
                {
                    return new DecodeVerification { Matches = false, Error = "decoded pixel count " + pixels.Length.ToString(CultureInfo.InvariantCulture) };
                }

                for (var pixelIndex = 0; pixelIndex < pixels.Length; pixelIndex++)
                {
                    var byteIndex = pixelIndex * 4;
                    var pixel = pixels[pixelIndex];
                    if (pixel.r != input.Rgba[byteIndex] ||
                        pixel.g != input.Rgba[byteIndex + 1] ||
                        pixel.b != input.Rgba[byteIndex + 2] ||
                        pixel.a != input.Rgba[byteIndex + 3])
                    {
                        return new DecodeVerification
                        {
                            Matches = false,
                            Error = "first mismatch at pixel " + pixelIndex.ToString(CultureInfo.InvariantCulture)
                        };
                    }
                }

                return new DecodeVerification { Matches = true, Error = "" };
            }
            catch (Exception ex)
            {
                return new DecodeVerification { Matches = false, Error = ex.GetType().Name + ": " + ex.Message };
            }
            finally
            {
                UnityEngine.Object.DestroyImmediate(texture);
            }
        }

        private static double Percentile(double[] sorted, double p)
        {
            if (sorted == null || sorted.Length == 0)
            {
                return 0.0;
            }
            var index = (int)Math.Round((sorted.Length - 1) * p);
            index = Math.Max(0, Math.Min(sorted.Length - 1, index));
            return sorted[index];
        }

        private static string ToCsv(List<BenchmarkRow> rows)
        {
            var sb = new StringBuilder();
            sb.AppendLine("encoder,width,height,pattern,iterations,p50_ms,p90_ms,min_ms,max_ms,png_bytes,gen0_collections,decode_matches,decode_error,error");
            foreach (var row in rows)
            {
                sb.Append(Escape(row.Encoder)).Append(',')
                    .Append(row.Width.ToString(CultureInfo.InvariantCulture)).Append(',')
                    .Append(row.Height.ToString(CultureInfo.InvariantCulture)).Append(',')
                    .Append(Escape(row.Pattern)).Append(',')
                    .Append(row.Iterations.ToString(CultureInfo.InvariantCulture)).Append(',')
                    .Append(row.P50Ms.ToString("F3", CultureInfo.InvariantCulture)).Append(',')
                    .Append(row.P90Ms.ToString("F3", CultureInfo.InvariantCulture)).Append(',')
                    .Append(row.MinMs.ToString("F3", CultureInfo.InvariantCulture)).Append(',')
                    .Append(row.MaxMs.ToString("F3", CultureInfo.InvariantCulture)).Append(',')
                    .Append(row.PngBytes.ToString(CultureInfo.InvariantCulture)).Append(',')
                    .Append(row.Gen0Collections.ToString(CultureInfo.InvariantCulture)).Append(',')
                    .Append(row.DecodeMatches ? "true" : "false").Append(',')
                    .Append(Escape(row.DecodeError)).Append(',')
                    .Append(Escape(row.Error)).AppendLine();
            }
            return sb.ToString();
        }

        private static string Escape(string value)
        {
            value = value ?? "";
            return value.IndexOfAny(new[] { ',', '"', '\n', '\r' }) < 0
                ? value
                : "\"" + value.Replace("\"", "\"\"") + "\"";
        }

        private sealed class BenchmarkInput
        {
            public int Width;
            public int Height;
            public string Pattern;
            public byte[] Rgba;

            public static BenchmarkInput Create(int width, int height, string pattern)
            {
                var bytes = new byte[checked(width * height * 4)];
                if (pattern == "noise")
                {
                    FillNoise(bytes, width, height);
                }
                else
                {
                    FillGradient(bytes, width, height);
                }
                return new BenchmarkInput
                {
                    Width = width,
                    Height = height,
                    Pattern = pattern,
                    Rgba = bytes
                };
            }

            private static void FillGradient(byte[] bytes, int width, int height)
            {
                var index = 0;
                for (var y = 0; y < height; y++)
                {
                    for (var x = 0; x < width; x++)
                    {
                        bytes[index++] = (byte)(x * 255 / Math.Max(1, width - 1));
                        bytes[index++] = (byte)(y * 255 / Math.Max(1, height - 1));
                        bytes[index++] = (byte)((x + y) * 255 / Math.Max(1, width + height - 2));
                        bytes[index++] = 255;
                    }
                }
            }

            private static void FillNoise(byte[] bytes, int width, int height)
            {
                var state = 0x12345678u ^ (uint)width ^ ((uint)height << 16);
                for (var i = 0; i < bytes.Length; i += 4)
                {
                    state = state * 1664525u + 1013904223u;
                    bytes[i] = (byte)(state >> 24);
                    bytes[i + 1] = (byte)(state >> 16);
                    bytes[i + 2] = (byte)(state >> 8);
                    bytes[i + 3] = 255;
                }
            }
        }

        private sealed class BenchmarkRow
        {
            public string Encoder;
            public int Width;
            public int Height;
            public string Pattern;
            public int Iterations;
            public double P50Ms;
            public double P90Ms;
            public double MinMs;
            public double MaxMs;
            public int PngBytes;
            public int Gen0Collections;
            public bool DecodeMatches;
            public string DecodeError;
            public string Error;
        }

        private sealed class DecodeVerification
        {
            public bool Matches;
            public string Error;
        }
    }
}
