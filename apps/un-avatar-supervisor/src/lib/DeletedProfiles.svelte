<script lang="ts">
 import { invoke } from "@tauri-apps/api/core";
 import { _ } from "svelte-i18n";
 type Entry = { archive_id: string; profile_id: string; name: string };
 let { busy, existingIds, onRestore }: { busy: boolean; existingIds: string[]; onRestore: (id: string) => Promise<void> } = $props();
 let entries = $state<Entry[]>([]), loading = $state(false), restoring = $state<string | null>(null), error = $state(""), loadError = $state(""), result = $state("");
 async function load() {
  if (loading || restoring) return;
  loading=true; loadError="";
  try { entries=await invoke<Entry[]>("list_deleted_avatar_settings"); }
  catch (e) { entries=[]; loadError=String(e); } finally { loading=false; }
 }
 async function restore(entry: Entry) {
  if (busy || loading || restoring || existingIds.includes(entry.profile_id)) return;
  restoring=entry.archive_id;error="";result="";
  try { await onRestore(entry.archive_id); result=$_("profiles.archive.restored", { values: { name: entry.name } }); }
  catch (e) { error=String(e); } finally { restoring=null; }
 }
</script>
<details class="panel archived-profiles" ontoggle={(event) => { if (event.currentTarget.open) void load(); }}>
 <summary>{$_("profiles.archive.title")}</summary>
 <p>{$_("profiles.archive.description")}</p>
 <button disabled={busy || loading || !!restoring} onclick={() => void load()}>{$_("profiles.archive.refresh")}</button>
 {#if loadError}<p role="alert">{loadError}</p>{/if}
 {#if error}<p role="alert">{error}</p>{/if}
 {#if result}<p role="status">{result}</p>{/if}
 {#if loading}<p role="status">{$_("profiles.archive.loading")}</p>{:else if !loadError && entries.length === 0}<p>{$_("profiles.archive.empty")}</p>{/if}
 <ul>{#each entries as entry (entry.archive_id)}
 <li><div><strong>{entry.name}</strong><small>{entry.profile_id}</small><small>{entry.archive_id}</small></div>
 {#if existingIds.includes(entry.profile_id)}<span>{$_("profiles.archive.exists")}</span>{/if}
 <button disabled={busy || loading || !!restoring || existingIds.includes(entry.profile_id)} onclick={() => void restore(entry)}>{$_("profiles.archive.restore")}</button></li>
 {/each}</ul>
</details>
<style>
 .archived-profiles{flex:0 0 auto;max-height:40vh;overflow:auto}
 .archived-profiles{margin-block:.75rem;padding:.8rem}summary{cursor:pointer;font-weight:600}p{line-height:1.5}ul{list-style:none;padding:0}li{display:flex;align-items:center;gap:.75rem;flex-wrap:wrap;padding:.6rem 0;border-top:1px solid currentColor}li>div{flex:1;min-width:0}small{display:block;overflow-wrap:anywhere;opacity:.7}button:focus-visible,summary:focus-visible{outline:2px solid currentColor;outline-offset:3px}
</style>
