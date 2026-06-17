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
    public sealed partial class UNAvatarExporterWindow
    {
        private List<WardrobePreviewImageDraft> PreviewImagesForExport(List<WardrobeSetDraft> exportWardrobeSets = null)
        {
            var sets = exportWardrobeSets ?? WardrobeSetsForExport();
            var previews = new List<WardrobePreviewImageDraft>(CountPreviewImages(basePreviewImages, sets));
            if (basePreviewImages != null)
            {
                foreach (var image in basePreviewImages)
                {
                    if (image != null)
                    {
                        previews.Add(image);
                    }
                }
            }
            foreach (var set in sets)
            {
                if (set.previewImages == null)
                {
                    continue;
                }
                foreach (var image in set.previewImages)
                {
                    if (image != null)
                    {
                        previews.Add(image);
                    }
                }
            }
            return previews;
        }

        private static int CountPreviewImages(List<WardrobePreviewImageDraft> baseImages, List<WardrobeSetDraft> sets)
        {
            var count = baseImages != null ? baseImages.Count : 0;
            if (sets == null)
            {
                return count;
            }
            foreach (var set in sets)
            {
                if (set != null && set.previewImages != null)
                {
                    count += set.previewImages.Count;
                }
            }
            return count;
        }

        private void RegenerateWardrobePreviewImagesForExport()
        {
            if (avatarRoot == null)
            {
                return;
            }

            GameObject previewClone = null;
            try
            {
                previewClone = CreateWardrobePreviewClone("shared");
                PrepareWardrobePreviewRenderers(previewClone);

                var previewBounds = CalculateWardrobePreviewBoundsForExport(previewClone);
                if (IsCurrentToBaseOnlyExportMode())
                {
                    basePreviewImages = WardrobePreviewCapture.Capture(previewClone, previewBounds, CurrentPreviewCaptureOptions());
                    AssignPreviewStateDigest(basePreviewImages, "base", previewClone);
                    return;
                }

                basePreviewImages = CapturePreviewImagesForState(previewClone, "base", null, previewBounds);
                for (var i = 0; i < capturedWardrobeSets.Count; i++)
                {
                    capturedWardrobeSets[i].previewImages = CapturePreviewImagesForState(previewClone, capturedWardrobeSets[i].id, capturedWardrobeSets[i], previewBounds);
                }
            }
            finally
            {
                if (previewClone != null)
                {
                    DestroyImmediate(previewClone);
                }
            }
        }

        private List<WardrobePreviewImageDraft> CapturePreviewImagesForState(GameObject previewClone, string label, WardrobeSetDraft set, Bounds previewBounds)
        {
            ApplyPreviewStateToRoot(previewClone, set);
            var previews = WardrobePreviewCapture.Capture(previewClone, previewBounds, CurrentPreviewCaptureOptions());
            AssignPreviewStateDigest(previews, label, previewClone);
            return previews;
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
            if (previews == null)
            {
                return;
            }
            foreach (var preview in previews)
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

            var active = ActiveRendererPaths(root);
            var probes = WardrobePreviewProbePaths;
            var pathLookup = BuildProbePathLookup(root);
            var states = new List<string>(probes.Length);
            foreach (var path in probes)
            {
                states.Add(path + "=" + ProbePathState(pathLookup, path));
            }
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

            var details = new List<string>();
            foreach (var renderer in root.GetComponentsInChildren<Renderer>(true))
            {
                if (renderer == null || !renderer.enabled || !renderer.gameObject.activeInHierarchy)
                {
                    continue;
                }
                var path = VariantExtractor.TransformPath(root.transform, renderer.transform);
                var layerName = LayerMask.LayerToName(renderer.gameObject.layer);
                var bounds = renderer.bounds;
                var materials = MaterialSummary(renderer.sharedMaterials);
                details.Add(path
                    + "|layer=" + renderer.gameObject.layer.ToString(CultureInfo.InvariantCulture) + ":" + layerName
                    + "|boundsCenter=" + Vec3String(bounds.center)
                    + "|boundsSize=" + Vec3String(bounds.size)
                    + "|materials=" + materials);
            }
            details.Sort(StringComparer.Ordinal);
            return details;
        }

        private static List<string> ActiveRendererPaths(GameObject root)
        {
            var active = new List<string>();
            foreach (var renderer in root.GetComponentsInChildren<Renderer>(true))
            {
                if (renderer != null && renderer.enabled && renderer.gameObject.activeInHierarchy)
                {
                    active.Add(VariantExtractor.TransformPath(root.transform, renderer.transform));
                }
            }
            active.Sort(StringComparer.Ordinal);
            return active;
        }

        private static string MaterialSummary(Material[] materials)
        {
            if (materials == null || materials.Length == 0)
            {
                return "";
            }
            var parts = new List<string>(materials.Length);
            foreach (var material in materials)
            {
                if (material == null)
                {
                    continue;
                }
                parts.Add(material.name
                    + "/" + (material.shader != null ? material.shader.name : "<no-shader>")
                    + "/rq" + material.renderQueue.ToString(CultureInfo.InvariantCulture));
            }
            return string.Join(",", parts);
        }

        private static string Vec3String(Vector3 value)
        {
            return value.x.ToString("R", CultureInfo.InvariantCulture)
                + "," + value.y.ToString("R", CultureInfo.InvariantCulture)
                + "," + value.z.ToString("R", CultureInfo.InvariantCulture);
        }

        private Bounds CalculateWardrobePreviewBoundsForExport(GameObject previewClone)
        {
            var bounds = CalculateWardrobePreviewBoundsForState(previewClone, null);
            foreach (var set in capturedWardrobeSets)
            {
                var setBounds = CalculateWardrobePreviewBoundsForState(previewClone, set);
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

        private Bounds CalculateWardrobePreviewBoundsForState(GameObject previewClone, WardrobeSetDraft set)
        {
            ApplyPreviewStateToRoot(previewClone, set);
            return WardrobePreviewCapture.CalculateVisibleBounds(previewClone);
        }

        private void ApplyPreviewStateToRoot(GameObject previewClone, WardrobeSetDraft set)
        {
            if (set == null)
            {
                ApplyBaseStateToRoot(previewClone);
            }
            else
            {
                ApplyWardrobeSetStateToRoot(previewClone, set);
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
                    sets.Add(CloneWardrobeSetForExport(set));
                    continue;
                }

                var rebased = WardrobeSnapshotCapture.Diff(baseSnapshot, set.capturedSnapshot, set.displayName, avatarRoot);
                rebased.id = WardrobeSnapshotCapture.NormalizeWardrobeSetId(set.id, set.displayName);
                rebased.displayName = set.displayName;
                rebased.source = set.source + "_export_rebased";
                rebased.capturedSnapshot = set.capturedSnapshot;
                rebased.previewImages = ClonePreviewImages(set.previewImages);
                sets.Add(rebased);
            }
            return sets;
        }

        private WardrobeSetDraft CloneWardrobeSetForExport(WardrobeSetDraft source)
        {
            if (source == null)
            {
                return null;
            }
            return new WardrobeSetDraft
            {
                id = WardrobeSnapshotCapture.NormalizeWardrobeSetId(source.id, source.displayName),
                displayName = source.displayName,
                source = source.source,
                assetGroups = source.assetGroups != null ? new List<string>(source.assetGroups) : new List<string>(),
                assetGroupOwnershipHints = WardrobeSetDraft.CloneHints(source.assetGroupOwnershipHints),
                operations = CloneWardrobeSetOperations(source.operations),
                previewImages = ClonePreviewImages(source.previewImages),
                capturedSnapshot = source.capturedSnapshot
            };
        }

        private static List<WardrobePreviewImageDraft> ClonePreviewImages(List<WardrobePreviewImageDraft> previews)
        {
            var cloned = new List<WardrobePreviewImageDraft>(previews != null ? previews.Count : 0);
            if (previews == null)
            {
                return cloned;
            }
            foreach (var preview in previews)
            {
                var image = WardrobePreviewCapture.ClonePreview(preview);
                if (image != null)
                {
                    cloned.Add(image);
                }
            }
            return cloned;
        }
    }
}
