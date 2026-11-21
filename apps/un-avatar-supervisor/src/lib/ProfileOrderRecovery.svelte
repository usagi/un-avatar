<script lang="ts">
 import {tick} from 'svelte';
 let errorElement=$state<HTMLParagraphElement|null>(null);
 import {invoke} from '@tauri-apps/api/core';
 import {_} from 'svelte-i18n';
 export type OrderRecoveryEntry={name:string;revision:string|null;files:string[];state:string;error:string|null};
 let {busy,onRestore}:{busy:boolean;onRestore:(root:string,entry:OrderRecoveryEntry)=>Promise<void>}=$props();
 let history=$state<{root:string;entries:OrderRecoveryEntry[]}|null>(null),selected=$state<OrderRecoveryEntry|null>(null),loading=$state(false),restoring=$state(false),error=$state(''),result=$state('');
 const canRestore=(entry:OrderRecoveryEntry)=>!!entry.revision&&!entry.error&&['partial','applied'].includes(entry.state);
 async function load(){if(busy||loading||restoring)return;loading=true;error='';selected=null;try{history=await invoke('list_profile_order_recovery');}catch(e){history=null;error=String(e);}finally{loading=false;}}
 async function restore(){if(busy||restoring||!history||!selected||!canRestore(selected))return;restoring=true;error='';result='';try{await onRestore(history.root,selected);selected=null;history=null;result=$_("profiles.order_recovery.done");}catch(e){if(String(e).startsWith('profile_order_committed_refresh_failed:')){selected=null;history=null;error=$_("profiles.order_draft.committed");}else error=$_("profiles.order_recovery.failed")+" "+String(e);}finally{restoring=false;if(error){await tick();errorElement?.focus({preventScroll:true});errorElement?.scrollIntoView({block:'nearest'});}}}
</script>
<details class="panel order-recovery" ontoggle={event=>{if(event.currentTarget.open)void load();}}>
 <summary>{$_("profiles.order_recovery.title")}</summary><p>{$_("profiles.order_recovery.help")}</p>
 <button disabled={busy||loading||restoring} onclick={()=>void load()}>{$_("profiles.order_recovery.reload")}</button>
 {#if error}<p role="alert" tabindex="-1" bind:this={errorElement}>{error}</p>{/if}{#if result}<p role="status">{result}</p>{/if}
 {#if history}<p class="identity">{history.root}</p>{#if !history.entries.length}<p>{$_("profiles.order_recovery.empty")}</p>{/if}
 {#each history.entries as entry}<article><strong>{$_(`profiles.order_recovery.${['original','applied','partial','conflict','invalid'].includes(entry.state)?entry.state:'invalid'}`)}</strong><p class="identity">{entry.name}</p><p>{entry.files.join(' / ')}</p>{#if entry.error}<p role="alert">{entry.error}</p>{/if}<button disabled={busy||loading||restoring||!canRestore(entry)} onclick={()=>selected=entry}>{$_("profiles.order_recovery.review")}</button></article>{/each}{/if}
 {#if selected}<section aria-label={$_("profiles.order_recovery.confirm")}><h3>{$_("profiles.order_recovery.confirm")}</h3><p class="identity">{history?.root} / {selected.name}</p><p>{selected.files.join(' / ')}</p><p>{$_("profiles.order_recovery.warning")}</p><button disabled={busy||restoring} onclick={()=>void restore()}>{$_("profiles.order_recovery.restore")}</button><button disabled={restoring} onclick={()=>selected=null}>{$_("profiles.order_recovery.cancel")}</button></section>{/if}
</details>
<style>.order-recovery{flex:0 0 auto;max-height:40vh;overflow:auto;padding:.8rem;margin-block:.5rem}.identity{overflow-wrap:anywhere;font-size:.8rem}summary{cursor:pointer;font-weight:600}article,section{border-top:1px solid var(--border);padding-block:.75rem}p{line-height:1.5}button{margin:.2rem}</style>


