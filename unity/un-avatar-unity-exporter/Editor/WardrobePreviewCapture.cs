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
    internal sealed class WardrobePreviewCaptureOptions
    {
        public bool HighQualityRender;
        public bool AntiAliasing;

        public string RenderMode => HighQualityRender ? "high_quality_camera_render" : "standard_camera_render";
        public int AntiAliasingSamples => AntiAliasing ? 8 : 1;
    }

    internal static class WardrobePreviewCapture
    {
        private const int PreviewSize = 1024;
        private const float PreviewFovY = 52.6357f;
        private const float PreviewNear = 0.05f;
        private const float PreviewFar = 100.0f;
        private const int PreviewCaptureLayer = 31;

        public static List<WardrobePreviewImageDraft> Capture(GameObject root)
        {
            return Capture(root, CalculateVisibleBounds(root), new WardrobePreviewCaptureOptions());
        }

        public static List<WardrobePreviewImageDraft> Capture(GameObject root, WardrobePreviewCaptureOptions options)
        {
            return Capture(root, CalculateVisibleBounds(root), options);
        }

        public static List<WardrobePreviewImageDraft> Capture(GameObject root, Bounds bounds)
        {
            return Capture(root, bounds, new WardrobePreviewCaptureOptions());
        }

        public static List<WardrobePreviewImageDraft> Capture(GameObject root, Bounds bounds, WardrobePreviewCaptureOptions options)
        {
            var result = new List<WardrobePreviewImageDraft>();
            if (root == null)
            {
                return result;
            }
            options = options ?? new WardrobePreviewCaptureOptions();

            if (bounds.size == Vector3.zero)
            {
                bounds = new Bounds(root.transform.position + Vector3.up, Vector3.one * 2.0f);
            }

            var center = bounds.center;
            var radius = Mathf.Max(bounds.extents.magnitude, 0.5f);
            var distance = radius / Mathf.Sin(PreviewFovY * Mathf.Deg2Rad * 0.5f);
            distance *= 0.56f;

            using (new WardrobePreviewLayerScope(root, PreviewCaptureLayer))
            {
                result.Add(CaptureView(root, "front", center + new Vector3(0.0f, radius * 0.08f, distance), center, Vector3.up, options));
                result.Add(CaptureView(root, "back", center + new Vector3(0.0f, radius * 0.08f, -distance), center, Vector3.up, options));
                result.Add(CaptureView(root, "side", center + new Vector3(distance, radius * 0.08f, 0.0f), center, Vector3.up, options));
                result.Add(CaptureView(root, "top", center + new Vector3(0.0f, distance, 0.0f), center, Vector3.back, options));
                result.Add(CaptureView(root, "threeQuarterTop", center + new Vector3(distance * 0.58f, distance * 0.48f, distance * 0.58f), center, Vector3.up, options));
                return result;
            }
        }

        public static WardrobePreviewImageDraft ClonePreview(WardrobePreviewImageDraft source)
        {
            if (source == null)
            {
                return null;
            }
            return new WardrobePreviewImageDraft
            {
                id = source.id,
                view = source.view,
                width = source.width,
                height = source.height,
                mimeType = source.mimeType,
                pixelFormat = source.pixelFormat,
                colorSpace = source.colorSpace,
                renderMode = source.renderMode,
                antiAliasingSamples = source.antiAliasingSamples,
                stateDigest = source.stateDigest,
                stateDetails = source.stateDetails != null ? new List<string>(source.stateDetails) : new List<string>(),
                fovYDegrees = source.fovYDegrees,
                nearClip = source.nearClip,
                farClip = source.farClip,
                cameraPosition = source.cameraPosition,
                cameraRotationEuler = source.cameraRotationEuler,
                target = source.target,
                bufferView = source.bufferView,
                pngBytes = source.pngBytes != null ? new List<byte>(source.pngBytes) : new List<byte>()
            };
        }

        private static WardrobePreviewImageDraft CaptureView(
            GameObject root,
            string view,
            Vector3 position,
            Vector3 target,
            Vector3 up,
            WardrobePreviewCaptureOptions options)
        {
            var cameraObject = new GameObject("UNAvatar Wardrobe Preview Camera");
            cameraObject.hideFlags = HideFlags.HideAndDontSave;
            var camera = cameraObject.AddComponent<Camera>();
            var oldActive = RenderTexture.active;
            var renderTexture = RenderTexture.GetTemporary(
                PreviewSize,
                PreviewSize,
                24,
                RenderTextureFormat.ARGB32,
                RenderTextureReadWrite.sRGB,
                Mathf.Max(1, options.AntiAliasingSamples));
            try
            {
                camera.transform.position = position;
                camera.transform.LookAt(target, up);
                camera.fieldOfView = PreviewFovY;
                camera.nearClipPlane = PreviewNear;
                camera.farClipPlane = PreviewFar;
                camera.aspect = 1.0f;
                camera.allowHDR = options.HighQualityRender;
                camera.allowMSAA = options.AntiAliasing;
                camera.clearFlags = CameraClearFlags.SolidColor;
                camera.backgroundColor = new Color(0, 0, 0, 0);
                camera.cullingMask = 1 << PreviewCaptureLayer;
                camera.targetTexture = renderTexture;
                using (new WardrobePreviewQualityScope(options))
                {
                    camera.Render();
                }

                RenderTexture.active = renderTexture;
                var texture = new Texture2D(PreviewSize, PreviewSize, TextureFormat.RGBA32, false, false);
                try
                {
                    texture.ReadPixels(new Rect(0, 0, PreviewSize, PreviewSize), 0, 0);
                    texture.Apply(false, false);
                    return new WardrobePreviewImageDraft
                    {
                        id = view,
                        view = view,
                        width = PreviewSize,
                        height = PreviewSize,
                        mimeType = "image/png",
                        pixelFormat = "RGBA8",
                        colorSpace = "sRGB",
                        renderMode = options.RenderMode,
                        antiAliasingSamples = options.AntiAliasingSamples,
                        fovYDegrees = PreviewFovY,
                        nearClip = PreviewNear,
                        farClip = PreviewFar,
                        cameraPosition = camera.transform.position,
                        cameraRotationEuler = camera.transform.rotation.eulerAngles,
                        target = target,
                        pngBytes = texture.EncodeToPNG().ToList()
                    };
                }
                finally
                {
                    UnityEngine.Object.DestroyImmediate(texture);
                }
            }
            finally
            {
                camera.targetTexture = null;
                RenderTexture.active = oldActive;
                RenderTexture.ReleaseTemporary(renderTexture);
                UnityEngine.Object.DestroyImmediate(cameraObject);
            }
        }

        private sealed class WardrobePreviewLayerScope : IDisposable
        {
            private readonly List<Transform> transforms;
            private readonly List<int> layers;

            public WardrobePreviewLayerScope(GameObject root, int layer)
            {
                transforms = root != null
                    ? root.GetComponentsInChildren<Transform>(true).ToList()
                    : new List<Transform>();
                layers = transforms.Select(transform => transform.gameObject.layer).ToList();
                foreach (var transform in transforms)
                {
                    transform.gameObject.layer = layer;
                }
            }

            public void Dispose()
            {
                for (var i = 0; i < transforms.Count; i++)
                {
                    if (transforms[i] != null)
                    {
                        transforms[i].gameObject.layer = layers[i];
                    }
                }
            }
        }

        private sealed class WardrobePreviewQualityScope : IDisposable
        {
            private readonly bool applied;
            private readonly int oldAntiAliasing;
            private readonly int oldPixelLightCount;
            private readonly ShadowQuality oldShadows;
            private readonly ShadowResolution oldShadowResolution;
            private readonly ShadowProjection oldShadowProjection;
            private readonly float oldShadowDistance;
            private readonly float oldLodBias;
            private readonly int oldMaximumLodLevel;

            public WardrobePreviewQualityScope(WardrobePreviewCaptureOptions options)
            {
                options = options ?? new WardrobePreviewCaptureOptions();
                if (!options.HighQualityRender)
                {
                    return;
                }
                applied = true;
                oldAntiAliasing = QualitySettings.antiAliasing;
                oldPixelLightCount = QualitySettings.pixelLightCount;
                oldShadows = QualitySettings.shadows;
                oldShadowResolution = QualitySettings.shadowResolution;
                oldShadowProjection = QualitySettings.shadowProjection;
                oldShadowDistance = QualitySettings.shadowDistance;
                oldLodBias = QualitySettings.lodBias;
                oldMaximumLodLevel = QualitySettings.maximumLODLevel;

                QualitySettings.antiAliasing = options.AntiAliasing ? options.AntiAliasingSamples : 0;
                QualitySettings.pixelLightCount = Mathf.Max(QualitySettings.pixelLightCount, 8);
                QualitySettings.shadows = ShadowQuality.All;
                QualitySettings.shadowResolution = ShadowResolution.VeryHigh;
                QualitySettings.shadowProjection = ShadowProjection.StableFit;
                QualitySettings.shadowDistance = Mathf.Max(QualitySettings.shadowDistance, 100.0f);
                QualitySettings.lodBias = Mathf.Max(QualitySettings.lodBias, 2.0f);
                QualitySettings.maximumLODLevel = 0;
            }

            public void Dispose()
            {
                if (!applied)
                {
                    return;
                }
                QualitySettings.antiAliasing = oldAntiAliasing;
                QualitySettings.pixelLightCount = oldPixelLightCount;
                QualitySettings.shadows = oldShadows;
                QualitySettings.shadowResolution = oldShadowResolution;
                QualitySettings.shadowProjection = oldShadowProjection;
                QualitySettings.shadowDistance = oldShadowDistance;
                QualitySettings.lodBias = oldLodBias;
                QualitySettings.maximumLODLevel = oldMaximumLodLevel;
            }
        }

        public static Bounds CalculateVisibleBounds(GameObject root)
        {
            var renderers = root.GetComponentsInChildren<Renderer>(true)
                .Where(renderer => renderer.enabled && renderer.gameObject.activeInHierarchy)
                .ToList();
            if (renderers.Count == 0)
            {
                return new Bounds(root.transform.position, Vector3.zero);
            }

            var bounds = renderers[0].bounds;
            for (var i = 1; i < renderers.Count; i++)
            {
                bounds.Encapsulate(renderers[i].bounds);
            }
            return bounds;
        }
    }
}

