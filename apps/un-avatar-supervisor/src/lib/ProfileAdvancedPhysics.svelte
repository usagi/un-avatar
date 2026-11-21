<script lang="ts">
 import { locale } from 'svelte-i18n';
 import ProfileNumberInputField from './ProfileNumberInputField.svelte';
 import ProfileToggleField from './ProfileToggleField.svelte';
 import type { MotionSetting, ProfileSettingValue, DynamicsColliderAugmentOverrideSetting } from './profileTypes';
 export let setting: MotionSetting;
 export let busy = false;
 export let onUpdateSettingValue: (field: string, value: ProfileSettingValue) => void | Promise<void>;
 $: ja = ($locale ?? 'ja-JP').startsWith('ja');
 const surface = 'physics.dynamics.solver.surface_constraints.';
 const mesh = 'physics.dynamics.solver.mesh_cloth_assist.';
 const collider = 'physics.dynamics.solver.collider_augment_overrides';
 const fields = [
  ['topology_max_edge_distance_m','接続辺の最大長 (m)','Maximum connected edge (m)',.001,.2,.001],
  ['topology_max_mean_edge_distance_m','接続辺の平均長の上限 (m)','Maximum mean edge (m)',.001,.2,.001],
  ['spatial_max_distance_m','近接点の最大距離 (m)','Maximum nearby point distance (m)',.001,.1,.001],
  ['topology_stiffness','接続辺の拘束の強さ','Connected edge stiffness',0,1,.01],
  ['spatial_stiffness','近接点の拘束の強さ','Nearby point stiffness',0,1,.01],
  ['min_edge_count','最小の辺数','Minimum edge count',1,64,1],
 ] as const;
 const meshFields = [
  ['body_dominance_threshold','身体の影響を判定する閾値','Body dominance threshold',.05,.99],
  ['min_existing_dynamic_weight','既存の揺れの最小ウェイト','Minimum existing dynamic weight',0,.95],
  ['max_assist_weight','補助ウェイトの上限','Maximum assist weight',0,.95],
 ] as const;
 $: assist = setting.dynamics_mesh_cloth_assist ?? {enabled:false,body_dominance_threshold:.55,min_existing_dynamic_weight:.05,seed_missing_dynamic_influence:true,max_assist_weight:.35,mesh_path_contains:[]};
 const lines = (value: string) => value.split(/\r?\n/).map(v=>v.trim()).filter(Boolean);
 function persistRules(rules: DynamicsColliderAugmentOverrideSetting[]) {
  // Svelte stores may proxy nested arrays; the save queue freezes its input.
  void onUpdateSettingValue(collider, rules.map(rule=>({name:rule.name,source_id_contains:[...rule.source_id_contains],collider_path_contains:[...rule.collider_path_contains]})));
 }
 function updateRule(index: number, patch: Partial<DynamicsColliderAugmentOverrideSetting>) {
  persistRules(setting.dynamics_collider_augment_overrides.map((rule,i)=>i===index?{...rule,...patch}:{...rule}));
 }
</script>

<div class="advanced-physics">
 <details>
  <summary>{ja?'布の形を保つ拘束':'Cloth surface constraints'}</summary>
  <p>{ja?'近接する点と接続辺を拘束し、布の形を保ちます。全体の揺れを無効にしても設定値は保持します。':'Constrain nearby points and connected edges to retain cloth shape. Values remain when overall dynamics are disabled.'}</p>
  <div class="section-grid">
   <ProfileToggleField label={ja?'布の拘束を使う':'Enable surface constraints'} checked={setting.dynamics_surface_constraints_enabled} disabled={busy} onChange={v=>onUpdateSettingValue(surface+'enabled',v)} />
   {#each fields as [key,jp,en,min,max,step]}
    <ProfileNumberInputField label={ja?jp:en} value={setting[`dynamics_surface_constraint_${key}`]} {min} {max} {step} disabled={busy} onChange={v=>onUpdateSettingValue(surface+key,v)} />
   {/each}
  </div>
 </details>
 <details>
  <summary>{ja?'布メッシュの追従補助':'Cloth mesh follow assistance'}</summary>
  <p>{ja?'身体の動きに偏る布のウェイトを補助します。保存後の反映には、そのプロファイルのRendererの再起動が必要です。':'Assist cloth weights dominated by body motion. Applying saved changes requires restarting this profile’s Renderer.'}</p>
  <div class="section-grid">
   <ProfileToggleField label={ja?'追従補助を使う':'Enable mesh assistance'} checked={assist.enabled} disabled={busy} onChange={v=>onUpdateSettingValue(mesh+'enabled',v)} />
   {#each meshFields as [key,jp,en,min,max]}
    <ProfileNumberInputField label={ja?jp:en} value={assist[key]} {min} {max} step={.01} disabled={busy} onChange={v=>onUpdateSettingValue(mesh+key,v)} />
   {/each}
   <ProfileToggleField label={ja?'揺れの影響がない点にも補助を作る':'Seed missing dynamic influence'} checked={assist.seed_missing_dynamic_influence} disabled={busy} onChange={v=>onUpdateSettingValue(mesh+'seed_missing_dynamic_influence',v)} />
   <label><span>{ja?'対象メッシュの語句（1行に1つ）':'Mesh path terms (one per line)'}</span><textarea disabled={busy} value={assist.mesh_path_contains.join('\n')} onchange={e=>onUpdateSettingValue(mesh+'mesh_path_contains',lines(e.currentTarget.value))}></textarea><small>{ja?'空欄なら布カテゴリの語句を使います。':'Leave empty to use cloth category terms.'}</small></label>
  </div>
 </details>
 <details>
  <summary>{ja?'衝突相手を追加するルール':'Additional collision partners'} ({setting.dynamics_collider_augment_overrides.length})</summary>
  <p>{ja?'揺れる部位と衝突相手の語句を両方指定します。不完全なルールは保存されず、入力を保持します。':'Specify both source and collider terms. Incomplete rules are retained as drafts and cannot be saved.'}</p>
  {#each setting.dynamics_collider_augment_overrides as rule,index}
   <fieldset disabled={busy}>
    <legend>{ja?'ルール':'Rule'} {index+1}</legend>
    <label><span>{ja?'名前':'Name'}</span><input value={rule.name} onchange={e=>updateRule(index,{name:e.currentTarget.value})} /></label>
    <label><span>{ja?'揺れる部位の語句（1行に1つ）':'Source terms (one per line)'}</span><textarea value={rule.source_id_contains.join('\n')} onchange={e=>updateRule(index,{source_id_contains:lines(e.currentTarget.value)})}></textarea></label>
    <label><span>{ja?'衝突相手の語句（1行に1つ）':'Collider terms (one per line)'}</span><textarea value={rule.collider_path_contains.join('\n')} onchange={e=>updateRule(index,{collider_path_contains:lines(e.currentTarget.value)})}></textarea></label>
    <button type="button" onclick={()=>persistRules(setting.dynamics_collider_augment_overrides.filter((_,i)=>i!==index))}>{ja?'このルールを削除':'Remove this rule'}</button>
   </fieldset>
  {/each}
  <button type="button" disabled={busy} onclick={()=>persistRules([...setting.dynamics_collider_augment_overrides,{name:'',source_id_contains:[],collider_path_contains:[]}])}>{ja?'ルールを追加':'Add rule'}</button>
 </details>
</div>

<style>
 .advanced-physics { min-width:0; }
 .section-grid { grid-template-columns:repeat(auto-fit,minmax(min(100%,260px),1fr)); }
 .section-grid :global(label) { min-width:0; }
 .section-grid :global(input),.section-grid :global(textarea) { max-width:100%; box-sizing:border-box; }
 details { border-top:1px solid var(--border-color,#6667); padding:1rem 0; }
 summary { cursor:pointer; font-weight:600; }
 p,small { opacity:.85; }
 fieldset { min-width:0; margin:1rem 0; border:1px solid var(--border-color,#6667); border-radius:.6rem; display:grid; gap:.7rem; }
 label { display:grid; gap:.35rem; min-width:0; }
 input,textarea { width:100%; box-sizing:border-box; }
 textarea { min-height:4rem; resize:vertical; }
</style>
