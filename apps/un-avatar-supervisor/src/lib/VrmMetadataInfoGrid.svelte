<script lang="ts">
  import { _ } from "svelte-i18n";
  import { metadataList, type VrmMetadataInfo } from "./vrmMetadata";

  export let metadata: VrmMetadataInfo;
</script>

<dl class="vrm-metadata-grid">
  <dt>{$_("vrm_metadata.fields.file")}</dt>
  <dd>{metadata.file_name}</dd>
  {#if metadata.authors.length > 0}
    <dt>{$_("vrm_metadata.fields.author")}</dt>
    <dd>{metadataList(metadata.authors)}</dd>
  {/if}
  {#if metadata.copyright_information}
    <dt>{$_("vrm_metadata.fields.copyright")}</dt>
    <dd>{metadata.copyright_information}</dd>
  {/if}
  {#if metadata.contact_information}
    <dt>{$_("vrm_metadata.fields.contact")}</dt>
    <dd>{metadata.contact_information}</dd>
  {/if}
  {#if metadata.references.length > 0}
    <dt>{$_("vrm_metadata.fields.reference")}</dt>
    <dd>{metadataList(metadata.references)}</dd>
  {/if}
  {#if metadata.license_name}
    <dt>{$_("vrm_metadata.fields.license")}</dt>
    <dd>{metadata.license_name}</dd>
  {/if}
  {#if metadata.other_permission_url}
    <dt>{$_("vrm_metadata.fields.permission_url")}</dt>
    <dd>{metadata.other_permission_url}</dd>
  {/if}
  {#if metadata.other_license_url}
    <dt>{$_("vrm_metadata.fields.license_url")}</dt>
    <dd>{metadata.other_license_url}</dd>
  {/if}
</dl>
