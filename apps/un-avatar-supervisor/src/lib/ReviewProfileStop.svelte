<script lang="ts">
 import { _ } from "svelte-i18n";
 // Capture identities before confirmation; never follow later profile selection.
 let {targets, profileName, onStop, disabled = false}: {
  targets: {id:number}[]; profileName:string; onStop:(id:number)=>Promise<unknown>; disabled?:boolean;
 } = $props();
 let pending = $state<{name:string; ids:number[]} | null>(null);
 let stopping = $state(false);
 let result = $state<{failed:boolean; name:string; ids:number[]} | null>(null);
 async function confirm() {
  if (!pending || stopping || disabled) return;
  const snapshot=pending;
  stopping=true;
  const failed:number[]=[];
  for(const id of snapshot.ids){try{await onStop(id);}catch{failed.push(id);}}
  pending=failed.length ? {name:snapshot.name,ids:failed} : null;
  result={failed:failed.length>0,name:snapshot.name,ids:failed.length?failed:snapshot.ids};
  stopping=false;
 }
</script>
<div class="review-profile-stop">
 <button disabled={disabled || stopping || !targets.length} onclick={() => {pending={name:profileName,ids:targets.map(t=>t.id)};result=null;}}>{$_("supervisor_operations.stop_profile", {values:{count:targets.length}})}</button>
 {#if pending}
  <section aria-label={$_("supervisor_operations.stop_confirmation")}>
   <strong>{pending.name}</strong>
   <p>{$_("supervisor_operations.stop_targets", {values:{ids:pending.ids.map(id=>"#"+id).join(", ")}})}</p>
   <button disabled={disabled || stopping} onclick={() => void confirm()}>{$_("supervisor_operations.stop_confirm")}</button>
   <button disabled={stopping} onclick={() => pending=null}>{$_("supervisor_operations.cancel")}</button>
  </section>
 {/if}
 {#if result}<p role="status">{$_(result.failed ? "supervisor_operations.stop_failed" : "supervisor_operations.stop_requested", {values:{name:result.name,ids:result.ids.map(id=>"#"+id).join(", ")}})}</p>{/if}
</div>
<style>
 section{border:1px solid var(--border);padding:8px;margin:6px 0;border-radius:8px}
 p{font-size:12px;overflow-wrap:anywhere}button{white-space:normal;font:inherit;max-width:100%;margin:4px 0}
</style>
