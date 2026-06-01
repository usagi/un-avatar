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
    public sealed partial class UNAvatarExporterWindow
    {
        private List<WardrobePreviewImageDraft> PreviewImagesForExport(List<WardrobeSetDraft> exportWardrobeSets = null)
        {
            var previews = new List<WardrobePreviewImageDraft>();
            if (basePreviewImages != null)
            {
                previews.AddRange(basePreviewImages.Where(image => image != null));
            }
            var sets = exportWardrobeSets ?? WardrobeSetsForExport();
            foreach (var set in sets)
            {
                if (set.previewImages == null)
                {
                    continue;
                }
                previews.AddRange(set.previewImages.Where(image => image != null));
            }
            return previews;
        }

        private void RegenerateWardrobePreviewImagesForExport()
        {
            if (avatarRoot == null)
            {
                return;
            }

            var previewBounds = CalculateWardrobePreviewBoundsForExport();
            basePreviewImages = CapturePreviewImagesForState("base", null, previewBounds);
            for (var i = 0; i < capturedWardrobeSets.Count; i++)
            {
                capturedWardrobeSets[i].previewImages = CapturePreviewImagesForState(capturedWardrobeSets[i].id, capturedWardrobeSets[i], previewBounds);
            }
        }

        private List<WardrobePreviewImageDraft> CapturePreviewImagesForState(string label, WardrobeSetDraft set, Bounds previewBounds)
        {
            GameObject previewClone = null;
            try
            {
                previewClone = CreateWardrobePreviewClone(label);
                if (set == null)
                {
                    ApplyBaseStateToRoot(previewClone);
                }
                else
                {
                    ApplyWardrobeSetStateToRoot(previewClone, set);
                }
                PrepareWardrobePreviewRenderers(previewClone);
                var previews = WardrobePreviewCapture.Capture(previewClone, previewBounds, CurrentPreviewCaptureOptions());
                AssignPreviewStateDigest(previews, label, previewClone);
                return previews;
            }
            finally
            {
                if (previewClone != null)
                {
                    DestroyImmediate(previewClone);
                }
            }
        }

        private GameObject CreateWardrobePreviewClone(string label)
        {
            var previewClone = Instantiate(avatarRoot);
            previewClone.name = avatarRoot.name + " (UNAvatar Preview Capture " + (label ?? "state") + ")";
            previewClone.hideFlags = HideFlags.HideAndDontSave;
            previewClone.SetActive(true);
            return previewClone;
        }

        private static void PrepareWardrobePreviewRenderers(GameObject root)
        {
            foreach (var skinned in root.GetComponentsInChildren<SkinnedMeshRenderer>(true))
            {
                skinned.updateWhenOffscreen = true;
                skinned.forceMatrixRecalculationPerRender = true;
            }
        }

        private static void AssignPreviewStateDigest(List<WardrobePreviewImageDraft> previews, string label, GameObject root)
        {
            var digest = WardrobePreviewStateDigest(label, root);
            var details = WardrobePreviewStateDetails(root);
            foreach (var preview in previews ?? new List<WardrobePreviewImageDraft>())
            {
                if (preview != null)
                {
                    preview.stateDigest = digest;
                    preview.stateDetails = details;
                }
            }
        }

        private static string WardrobePreviewStateDigest(string label, GameObject root)
        {
            if (root == null)
            {
                return label + "|missing-root";
            }

            var renderers = root.GetComponentsInChildren<Renderer>(true);
            var active = renderers
                .Where(renderer => renderer != null && renderer.enabled && renderer.gameObject.activeInHierarchy)
                .Select(renderer => VariantExtractor.TransformPath(root.transform, renderer.transform))
                .OrderBy(path => path, StringComparer.Ordinal)
                .ToList();
            var probes = new[]
            {
                "Color  1",
                "Color  13",
                "add-belt",
                "Maid",
                "Outer"
            };
            var states = probes.Select(path => path + "=" + ProbePathState(root, path));
            using (var sha = SHA256.Create())
            {
                var joined = string.Join("\n", active);
                var hash = sha.ComputeHash(Encoding.UTF8.GetBytes(joined));
                var sb = new StringBuilder(hash.Length * 2);
                foreach (var b in hash)
                {
                    sb.Append(b.ToString("x2", CultureInfo.InvariantCulture));
                }
                return $"{label}|activeRenderers={active.Count}|activeHash={sb}|{string.Join(",", states)}";
            }
        }

        private static List<string> WardrobePreviewStateDetails(GameObject root)
        {
            if (root == null)
            {
                return new List<string>();
            }

            return root.GetComponentsInChildren<Renderer>(true)
                .Where(renderer => renderer != null && renderer.enabled && renderer.gameObject.activeInHierarchy)
                .OrderBy(renderer => VariantExtractor.TransformPath(root.transform, renderer.transform), StringComparer.Ordinal)
                .Select(renderer =>
                {
                    var path = VariantExtractor.TransformPath(root.transform, renderer.transform);
                    var layerName = LayerMask.LayerToName(renderer.gameObject.layer);
                    var bounds = renderer.bounds;
                    var materials = string.Join(",",
                        (renderer.sharedMaterials ?? Array.Empty<Material>())
                            .Where(material => material != null)
                            .Select(material => material.name
                                + "/" + (material.shader != null ? material.shader.name : "<no-shader>")
                                + "/rq" + material.renderQueue.ToString(CultureInfo.InvariantCulture)));
                    return path
                        + "|layer=" + renderer.gameObject.layer.ToString(CultureInfo.InvariantCulture) + ":" + layerName
                        + "|boundsCenter=" + Vec3String(bounds.center)
                        + "|boundsSize=" + Vec3String(bounds.size)
                        + "|materials=" + materials;
                })
                .ToList();
        }

        private static string Vec3String(Vector3 value)
        {
            return value.x.ToString("R", CultureInfo.InvariantCulture)
                + "," + value.y.ToString("R", CultureInfo.InvariantCulture)
                + "," + value.z.ToString("R", CultureInfo.InvariantCulture);
        }

        private Bounds CalculateWardrobePreviewBoundsForExport()
        {
            var bounds = CalculateWardrobePreviewBoundsForState(null);
            foreach (var set in capturedWardrobeSets)
            {
                var setBounds = CalculateWardrobePreviewBoundsForState(set);
                if (bounds.size == Vector3.zero)
                {
                    bounds = setBounds;
                }
                else if (setBounds.size != Vector3.zero)
                {
                    bounds.Encapsulate(setBounds);
                }
            }
            return bounds;
        }

        private Bounds CalculateWardrobePreviewBoundsForState(WardrobeSetDraft set)
        {
            GameObject previewClone = null;
            try
            {
                previewClone = CreateWardrobePreviewClone(set == null ? "base-bounds" : set.id + "-bounds");
                if (set == null)
                {
                    ApplyBaseStateToRoot(previewClone);
                }
                else
                {
                    ApplyWardrobeSetStateToRoot(previewClone, set);
                }
                PrepareWardrobePreviewRenderers(previewClone);
                return WardrobePreviewCapture.CalculateVisibleBounds(previewClone);
            }
            finally
            {
                if (previewClone != null)
                {
                    DestroyImmediate(previewClone);
                }
            }
        }

        private List<WardrobeSetDraft> WardrobeSetsForExport()
        {
            if (!hasBaseSnapshot)
            {
                return capturedWardrobeSets;
            }

            var sets = new List<WardrobeSetDraft>(capturedWardrobeSets.Count);
            foreach (var set in capturedWardrobeSets)
            {
                if (set.capturedSnapshot == null || set.capturedSnapshot.nodes.Count == 0)
                {
                    sets.Add(set);
                    continue;
                }

                var rebased = WardrobeSnapshotCapture.Diff(baseSnapshot, set.capturedSnapshot, set.displayName);
                rebased.id = set.id;
                rebased.displayName = set.displayName;
                rebased.source = set.source + "_export_rebased";
                rebased.capturedSnapshot = set.capturedSnapshot;
                rebased.previewImages = ClonePreviewImages(set.previewImages);
                sets.Add(rebased);
            }
            return sets;
        }

        private static List<WardrobePreviewImageDraft> ClonePreviewImages(List<WardrobePreviewImageDraft> previews)
        {
            return (previews ?? new List<WardrobePreviewImageDraft>())
                .Select(WardrobePreviewCapture.ClonePreview)
                .Where(image => image != null)
                .ToList();
        }
    }
}
