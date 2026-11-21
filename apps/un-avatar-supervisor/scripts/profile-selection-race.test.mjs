import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test, {after} from 'node:test';
import {snapshot} from 'svelte/internal/client';
// Extracted functions do not pass through the Svelte compiler. Use its real
// snapshot runtime for the rune that production compilation lowers.
globalThis.__unTestSnapshot=snapshot;
after(()=>{delete globalThis.__unTestSnapshot;});
import ts from 'typescript';
const source=await readFile(new URL('../src/App.svelte',import.meta.url),'utf8');
function selectionSaveHarness(){
 const pending=[];const invoke=(cmd,args)=>new Promise((resolve,reject)=>pending.push({cmd,args,resolve,reject}));
 const code=`let lastSelectionSaveQueue=Promise.resolve(),backendAppSettingsReady=true,appSettings={last_selected_setting_id:'a',theme_mode:'dark'},message='';
 const hasTauriRuntime=()=>true;const saveAppSettings=()=>{};const appSettingsStorageKey='',legacyThemeModeStorageKey='';
 ${body('persistLastSelectedSettingId')}
 return {save:persistLastSelectedSettingId,state:()=>({appSettings,message})};`;
 return {...new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke),pending};
}
test('remembered selection serializes pending B and final A',async()=>{
 const h=selectionSaveHarness();const b=h.save('b');await Promise.resolve();
 const a=h.save('a');await Promise.resolve();assert.equal(h.pending.length,1);
 h.pending[0].resolve();await b;await Promise.resolve();
 assert.equal(h.pending[1].args.value,'a');h.pending[1].resolve();await a;
 assert.equal(h.state().appSettings.last_selected_setting_id,'a');
 assert.equal(h.state().appSettings.theme_mode,'dark');
});
test('remembered selection failure stays visible and next save still runs',async()=>{
 const h=selectionSaveHarness();const b=h.save('b');const c=h.save('c');await Promise.resolve();
 h.pending[0].reject(Error('storage unavailable'));await b;await Promise.resolve();
 assert.equal(h.state().appSettings.last_selected_setting_id,'a');
 assert.match(h.state().message,/storage unavailable/);
 h.pending[1].resolve();await c;assert.equal(h.state().appSettings.last_selected_setting_id,'c');
});
function body(name){
 let start=source.indexOf(`\tasync function ${name}(`);
 if(start<0)start=source.indexOf(`\tfunction ${name}(`);
 assert.ok(start>=0,name);
 return source.slice(start,source.indexOf('\n\t}',start)+3).replaceAll('$state.snapshot', 'globalThis.__unTestSnapshot');
}
function harness(){
 const pending=[];const invoke=(cmd,args)=>new Promise((resolve,reject)=>pending.push({cmd,args,resolve,reject}));
 const code=`let profileBatchSequence=0,profileBatchDrafts=[];
 let selectedSettingId='a',selectedSetting={id:'a',manifest_path:'a.toml'},avatarSettings=[selectedSetting,{id:'b',manifest_path:'b.toml'}],launchTargetId='b',busy=false,message='';
 function compareAvatarSettings(a,b){return a.id.localeCompare(b.id);}
 function pickInitialLaunchTargetId(current){return current;}
 function hasTauriRuntime(){return true;}function $_(s){return s;}
 function restartableRendererForField(){return null;}function queueRendererRestart(){}
 function isCoalescibleProfileSettingValue(){return false;}
 const pendingProfileSettingUpdates=new Map();let failedProfileSettingUpdates=[]; const profileMatchDrafts={},profileColliderDrafts={}; const DYNAMICS_MATCH_OVERRIDE_FIELD="physics.dynamics.solver.match_overrides";
 const profileSettingUpdateCoalesceMs=100;const window={setTimeout:()=>1,clearTimeout:()=>{}};
 const avatarSettingBySelectedId=id=>avatarSettings.find(s=>s.id===id);
 const avatarSettingByIdOrManifestPath=id=>avatarSettings.find(s=>s.id===id||s.manifest_path===id);
 const canApplyWithoutRestart=()=>true;
 ${body('profileSettingUpdateKey')} ${body('scheduleCoalescedProfileSettingUpdate')} ${body('flushCoalescedProfileSettingUpdate')}
 async function applyRuntimeProfileUpdate(){} async function applyRuntimeProfileUpdates(){}
 function looksLikeUnavatarPath(){return false;}function bumpProfileIconRevision(){}
 const unavatarProfileIconCrop={imageDataUrl:'data:image/png;base64,test',zoom:1,offsetX:0,offsetY:0};
 ${body('replaceAvatarSetting')} ${body('updateSettingValue')} ${body('browseSettingPath')} ${body('saveAvatarPath')} ${body('applyProfileUpdates')} ${body('applySelectedAvatarThumbnail')} ${body('saveUnavatarProfileIconCrop')}
 return {readRevision:r=>{selectedSetting.revision=r;},failures:()=>failedProfileSettingUpdates,retry:key=>flushCoalescedProfileSettingUpdate(key),applyProfileUpdates,applySelectedAvatarThumbnail,saveUnavatarProfileIconCrop,updateSettingValue,browseSettingPath,saveAvatarPath,selectB:()=>{selectedSettingId='b';selectedSetting=avatarSettings[1];},state:()=>({selectedSettingId,avatarSettings,launchTargetId})};`;
 return {...new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke),pending};
}
function coalescedHarness(){
 const requests=[];const invoke=(cmd,args)=>new Promise((resolve,reject)=>requests.push({cmd,args,resolve,reject}));
 const code=`let selectedSettingId='a',avatarSettings=[{id:'a',manifest_path:'a.toml'},{id:'b',manifest_path:'b.toml'}],message='';
 const pendingProfileSettingUpdates=new Map();let failedProfileSettingUpdates=[]; const profileMatchDrafts={},profileColliderDrafts={}; const DYNAMICS_MATCH_OVERRIDE_FIELD="physics.dynamics.solver.match_overrides";
 const profileSettingUpdateCoalesceMs=100;const window={setTimeout:()=>1,clearTimeout:()=>{}};
 const avatarSettingBySelectedId=id=>avatarSettings.find(s=>s.id===id);const canApplyWithoutRestart=()=>true;
 let runtimeFails=false;const $_=s=>s;const replaceAvatarSetting=()=>{};const applyRuntimeProfileUpdate=async()=>{if(runtimeFails)throw Error('runtime unavailable');};const queueRendererRestart=()=>{};
 ${body('profileSettingUpdateKey')} ${body('scheduleCoalescedProfileSettingUpdate')} ${body('flushCoalescedProfileSettingUpdate')}
 return {readRevision:r=>avatarSettings[0].revision=r,schedule:(v)=>scheduleCoalescedProfileSettingUpdate(avatarSettings[0],'camera_fov',v),flush:()=>flushCoalescedProfileSettingUpdate('a\\ncamera_fov'),state:()=>pendingProfileSettingUpdates.get('a\\ncamera_fov'),selectB:()=>selectedSettingId='b',failures:()=>failedProfileSettingUpdates,failRuntime:()=>runtimeFails=true};`;
 return {...new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke),requests};
}
test('queued vector save snapshots input instead of following later array mutation',async()=>{
 const h=coalescedHarness();const vector=[1,2,3];h.schedule(vector);vector[0]=99;
 const save=h.flush();assert.deepEqual(h.requests[0].args.value,[1,2,3]);
 h.requests[0].reject(Error('offline'));await save;
 assert.deepEqual(h.state().value,[1,2,3]);
 const retry=h.flush();assert.deepEqual(h.requests[1].args.value,[1,2,3]);
 h.requests[1].resolve({id:'a',manifest_path:'a.toml'});await retry;
});
test('failed numeric save keeps the submitted value for an explicit retry to the original profile',async()=>{
 const h=coalescedHarness();h.schedule(42);const save=h.flush();h.selectB();h.requests[0].reject(Error('disk unavailable'));await save;
 assert.equal(h.state()?.value,42,'failed value must not be discarded');
 const retry=h.flush();assert.deepEqual(h.requests[1].args,{settingId:'a',field:'camera_fov',value:42});
 h.requests[1].resolve({id:'a',manifest_path:'a.toml'});await retry;assert.equal(h.state(),undefined);
});
test('older save failure preserves newer numeric input without presenting an obsolete retry',async()=>{
 const h=coalescedHarness();h.schedule(42);const old=h.flush();h.schedule(55);h.requests[0].reject(Error('old save failed'));await old;
 assert.equal(h.state().value,55);assert.deepEqual(h.failures(),[]);
 const latest=h.flush();assert.equal(h.requests[1].args.value,55);h.requests[1].reject(Error('latest save failed'));await latest;
 assert.equal(h.failures()[0].value,55);assert.equal(h.state().timer,null,'failed save must not retry automatically');
});
test('editing again replaces a retained failed value and clears its obsolete retry notice',async()=>{
 const h=coalescedHarness();h.schedule(42);const old=h.flush();h.requests[0].reject(Error('failed'));await old;
 assert.equal(h.failures().length,1);h.schedule(60);assert.deepEqual(h.failures(),[]);
 const next=h.flush();assert.equal(h.requests[1].args.value,60);h.requests[1].resolve({id:'a',manifest_path:'a.toml'});await next;
 assert.equal(h.state(),undefined);
});
test('runtime apply failure after successful persistence is not misreported as an unsaved value',async()=>{
 const h=coalescedHarness();h.failRuntime();h.schedule(42);const save=h.flush();h.requests[0].resolve({id:'a',manifest_path:'a.toml'});await save;
 assert.equal(h.state(),undefined);assert.deepEqual(h.failures(),[]);
});
test('camera partial apply reports failed renderer identities alongside successful count',async()=>{
 const calls=[];let failState=true;
 const invoke=async(cmd,args)=>{calls.push({cmd,args});if(failState&&cmd==='set_renderer_camera_state'&&args.id===2)throw Error('connection lost');};
 const code=`let runtimeCommandFailures=[];const runtimeCommandGenerations=new Map();${body('applyRendererCommand')}
 let message='saved';const fieldSetStartsWith=(fs,p)=>fs.some(f=>f.startsWith(p));
 const $_=(key,options)=>JSON.stringify({key,...options});
 const setAppliedMessage=async(count)=>{if(count)message='applied '+count;};
 ${body('applyRuntimeCameraUpdate')}
 return {applyRuntimeCameraUpdate,message:()=>message,failures:()=>runtimeCommandFailures,otherMessage:()=>{message="unrelated saved";}};`;
 const h=new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke);
 await h.applyRuntimeCameraUpdate(['camera.diagonal_fov_deg'],{id:'a',name:'Avatar A',camera_locked:false,camera_diagonal_fov_deg:42},[{id:1},{id:2}]);
 assert.match(h.message(),/camera_apply_partial/);assert.match(h.message(),/#2/);assert.match(h.message(),/connection lost/);
 assert.equal(h.failures().length,1);assert.equal(h.failures()[0].rendererId,2);
 h.otherMessage();assert.equal(h.failures().length,1);
 await h.applyRuntimeCameraUpdate(['camera.locked'],{id:'a',name:'Avatar A',camera_locked:true},[{id:2,name:'Second'}]);
 assert.equal(h.failures().length,1);assert.equal(h.failures()[0].command,'set_renderer_camera_state');
 failState=false;await h.applyRuntimeCameraUpdate(['camera.diagonal_fov_deg'],{id:'a',name:'Avatar A',camera_locked:true,camera_diagonal_fov_deg:45},[{id:2,name:'Second'}]);
 assert.equal(h.failures().length,0);

 assert.equal(calls.filter(x=>x.cmd==='set_renderer_camera_state').length,3);
 assert.equal(calls.filter(x=>x.cmd==='update_avatar_setting_value').length,0);
});
for (const failOld of [false,true]) test(`profile runtime apply stays ordered and independent (earlier failure=${failOld})`,async()=>{
 const calls=[];let release;const gate=new Promise(resolve=>release=resolve);
 const code=`const profileRuntimeApplyTails=new Map();
 const fieldSetHas=()=>true,isRuntimeWindowField=()=>true;
 const rendererForSetting=s=>({id:s.id,transparent:true});const renderersForSetting=s=>[{id:s.id}];
 const withDeferredRuntimeRefresh=fn=>fn();
 const applyRuntimeWindowUpdate=async(s,r)=>{calls.push(['start',s.id,s.version,r.id]);if(s.version===1){await gate;if(failOld)throw Error('old renderer apply failed');}};
 const applyRuntimeBackgroundColor=async()=>{},applyRuntimeMotionUpdate=async()=>{},applyRuntimePhysicsUpdate=async()=>{},applyRuntimeAvatarEffectsUpdate=async()=>{},applyRuntimePostEffectsUpdate=async()=>{},applyRuntimeEnvironmentUpdate=async()=>{};
 const applyRuntimeCameraUpdate=async(fs,s,rs)=>calls.push(['camera',s.id,s.version,rs[0].id]);
 ${body('applyRuntimeProfileUpdates')}
 return applyRuntimeProfileUpdates;`;
 const apply=new Function('calls','gate','failOld',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(calls,gate,failOld);
 const old=apply([['camera.radius',1]],{id:'a',version:1},{id:'a'});
 const checkedOld=failOld?assert.rejects(old,/old renderer apply failed/):old;
 const newer=apply([['camera.radius',2]],{id:'a',version:2},{id:'a'});
 const other=apply([['camera.radius',3]],{id:'b',version:3},{id:'b'});
 await other;
 assert.ok(calls.some(c=>c[0]==='camera'&&c[1]==='b'));
 assert.ok(!calls.some(c=>c[1]==='a'&&c[2]===2),'new snapshot must wait for older apply');
 release();await Promise.all([checkedOld,newer]);
 assert.deepEqual(calls.filter(c=>c[0]==='camera'&&c[1]==='a').map(c=>c[2]),failOld?[2]:[1,2]);
});
for (const [fn,fields,expected] of [
 ['applyRuntimeAvatarEffectsUpdate',['effects.avatar.outline.width','effects.avatar.contact_shadow.enabled'],['set_renderer_avatar_outline','set_renderer_contact_shadow']],
 ['applyRuntimePostEffectsUpdate',['effects.post.ssao.enabled','effects.post.bloom.enabled'],['set_renderer_ssao','set_renderer_bloom']],
 ['applyRuntimeEnvironmentUpdate',['environment.color.exposure','environment.lighting.intensity'],['set_renderer_environment_color','set_renderer_lighting']],
 ['applyRuntimeMotionUpdate',['motion.apply_vmc_root_translation','motion.look_at.enabled','motion.vmc_udp.enabled'],['set_renderer_apply_vmc_root_translation','set_renderer_look_at','set_renderer_motion_receivers']],
 ['applyRuntimePhysicsUpdate',['dynamics_enabled','debug.show_bone_colliders'],['set_renderer_dynamics','set_renderer_show_bone_colliders']],
]) {
 test(`${fn} applies both families in a saved batch`,async()=>{
  const calls=[];
  const code=`const fieldSetStartsWith=(fs,p)=>fs.some(f=>f.startsWith(p));const fieldSetIncludes=(fs,f)=>fs.includes(f);const fieldSetIncludesAny=(fs,keys)=>keys.some(k=>fs.includes(k));const $_=s=>s;const setAppliedMessage=async()=>{};const refreshRendererRuntimeViewAfterApply=async()=>{};
  const motionLookAtFields=['motion.look_at.enabled','motion.look_at.clamp_deg'],motionReceiverFields=['motion.vmc_udp.enabled'];
  const DYNAMICS_ENABLED_FIELD='dynamics_enabled',DYNAMICS_OVERRIDE_FIELD_PREFIX='physics.dynamics.solver.',DYNAMICS_BONE_COLLIDER_FIELD_PREFIX='physics.bone_colliders.';
  const rendererLookAtPayload=()=>({}),rendererMotionReceiversPayload=()=>({});
  const rendererAvatarOutlinePayload=()=>({}),rendererContactShadowPayload=()=>({}),rendererSsaoPayload=()=>({}),rendererBloomPayload=()=>({}),rendererEnvironmentColorPayload=()=>({}),rendererLightingPayload=()=>({});
  ${body(fn)} return ${fn};`;
  const apply=new Function('applyRendererCommand',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(async(targets,command)=>{calls.push(command);return targets.length;});
  await apply(fields,{id:'a'},[{id:1},{id:2}]);assert.deepEqual(calls,expected);
 });
}
test('runtime command failures survive unrelated success and ignore stale completion',async()=>{
 const requests=[];const invoke=(cmd,args)=>new Promise((resolve,reject)=>requests.push({cmd,args,resolve,reject}));
 const code=`let runtimeCommandFailures=[];const runtimeCommandGenerations=new Map();${body('applyRendererCommand')} return {applyRendererCommand,failures:()=>runtimeCommandFailures};`;
 const h=new Function('invoke','console',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke,{warn(){}});
 const targets=[{id:1,name:'A'},{id:2,name:'A'}];
 const first=h.applyRendererCommand(targets,'set_renderer_ssao',()=>({}));await Promise.resolve();requests[0].resolve();requests[1].reject(Error('offline'));assert.equal(await first,1);
 assert.equal(h.failures()[0].rendererId,2);
 const other=h.applyRendererCommand(targets,'set_renderer_bloom',()=>({}));await Promise.resolve();requests[2].resolve();requests[3].resolve();await other;assert.equal(h.failures().length,1);
 const old=h.applyRendererCommand([targets[1]],'set_renderer_ssao',()=>({}));
 const newer=h.applyRendererCommand([targets[1]],'set_renderer_ssao',()=>({}));
 await Promise.resolve();requests[5].resolve();await newer;assert.equal(h.failures().length,0);
 requests[4].reject(Error('obsolete failure'));await old;assert.equal(h.failures().length,0);
});
for(const operation of ['field','icon','avatar']){
 test(`${operation} save updates originating profile without selecting it again`,async()=>{
  const h=harness();
  const task=operation==='field'?h.updateSettingValue('name','A edited'):operation==='icon'?h.browseSettingPath('icon_path','icon'):h.saveAvatarPath('avatar.vrm',null,'a');
  h.selectB();
  if(operation==='icon'){h.pending[0].resolve('icon.png');await Promise.resolve();}
  const request=h.pending.at(-1);assert.equal(request.args.settingId,'a');
  request.resolve({id:'a',manifest_path:'a.toml',name:'A edited'});await task;
  assert.equal(h.state().selectedSettingId,'b');
  assert.equal(h.state().avatarSettings.find(s=>s.id==='a').name,'A edited');
  assert.equal(h.state().launchTargetId,'b');
 });
}

for(const operation of ['preset','thumbnail','crop']){
 test(`${operation} update retains current selection while updating original profile`,async()=>{
  const h=harness();
  const task=operation==='preset'?h.applyProfileUpdates([['name','A edited']]):operation==='thumbnail'?h.applySelectedAvatarThumbnail({settingId:'a',avatarPath:'a.vrm'}):h.saveUnavatarProfileIconCrop('a',null);
  h.selectB();assert.equal(h.pending[0].args.settingId,'a');
  h.pending[0].resolve({id:'a',manifest_path:'a.toml',name:'A edited'});await task;
  assert.equal(h.state().selectedSettingId,'b');
  assert.equal(h.state().avatarSettings.find(s=>s.id==='a').name,'A edited');
 });
}

function deletionHarness(refreshError=false){
 const pending=[];const invoke=(cmd,args)=>new Promise((resolve,reject)=>pending.push({cmd,args,resolve,reject}));
 const code=`let selectedSettingId='a',busy=false,message='',refreshes=0;const pendingProfileSettingUpdates=new Map();const profileBatchDrafts=[];
 let avatarSettings=[{id:'a',name:'A'},{id:'b',name:'B'}];
 const avatarSettingBySelectedId=id=>avatarSettings.find(s=>s.id===id);
 const hasTauriRuntime=()=>true;const $_=s=>s;
 async function refreshAll(){refreshes++;if (${refreshError}) throw Error('refresh unavailable');}
 ${body('deleteSetting')}
 return {deleteSetting,batchEdit(settingId,state){profileBatchDrafts.push({settingId,state});},pendingEdit:(id,inFlight=false)=>pendingProfileSettingUpdates.set(id,{settingId:id,inFlight}),select:id=>selectedSettingId=id,state:()=>({selectedSettingId,busy,message,refreshes,avatarSettings})};`;
 return {...new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke),pending};
}
test('deleting A does not clear B selected while deletion is pending',async()=>{
 const h=deletionHarness();const task=h.deleteSetting('a');h.select('b');
 assert.deepEqual(h.pending[0].args,{settingId:'a'});h.pending[0].resolve();await task;
 assert.equal(h.state().selectedSettingId,'b');assert.equal(h.state().refreshes,1);assert.equal(h.state().busy,false);
 assert.deepEqual(h.state().avatarSettings,[{id:'b',name:'B'}]);
});
test('deleting the still selected profile clears that deleted selection',async()=>{
 const h=deletionHarness();const task=h.deleteSetting('a');h.pending[0].resolve();await task;
 assert.equal(h.state().selectedSettingId,null);assert.equal(h.state().refreshes,1);
});
test('rejected deletion retains selection and reports the failure without retry',async()=>{
 const h=deletionHarness();const task=h.deleteSetting('a');h.select('b');h.pending[0].reject(Error('deletion refused'));await task;
 assert.equal(h.state().selectedSettingId,'b');assert.match(h.state().message,/deletion refused/);
 assert.equal(h.state().refreshes,0);assert.equal(h.pending.length,1);assert.equal(h.state().busy,false);
 assert.deepEqual(h.state().avatarSettings,[{id:'a',name:'A'},{id:'b',name:'B'}]);
});

test('acknowledged deletion remains removed even when subsequent refresh fails',async()=>{
 const h=deletionHarness(true);const task=h.deleteSetting('a');h.select('b');h.pending[0].resolve();await task;
 assert.deepEqual(h.state().avatarSettings,[{id:'b',name:'B'}]);
 assert.equal(h.state().selectedSettingId,'b');assert.equal(h.state().busy,false);
 assert.match(h.state().message,/refresh unavailable/);assert.equal(h.pending.length,1);
});

function restorationHarness(){
 const pending=[];const invoke=(cmd,args)=>new Promise((resolve,reject)=>pending.push({cmd,args,resolve,reject}));
 const code=`let selectedSettingId='b',launchTargetId='b',avatarSettings=[{id:'b'}],busy=false;const $_=s=>s;
 ${body('restoreArchivedSetting')}
 return {restoreArchivedSetting,state:()=>({selectedSettingId,launchTargetId,avatarSettings,busy})};`;
 return {...new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke),pending};
}
test('restoration refreshes available profiles without changing editing or launch target',async()=>{
 const h=restorationHarness();const task=h.restoreArchivedSetting('archive-a');
 assert.equal(h.pending[0].cmd,'restore_avatar_setting');assert.deepEqual(h.pending[0].args,{archiveId:'archive-a'});
 h.pending[0].resolve();await Promise.resolve();assert.equal(h.pending[1].cmd,'list_avatar_settings');h.pending[1].resolve([{id:'a'},{id:'b'}]);await task;
 assert.deepEqual(h.state(),{selectedSettingId:'b',launchTargetId:'b',avatarSettings:[{id:'a'},{id:'b'}],busy:false});
});
test('restoration conflict keeps existing profiles and does not retry',async()=>{
 const h=restorationHarness();const task=h.restoreArchivedSetting('archive-a');h.pending[0].reject(Error('profile exists'));
 await assert.rejects(task,/profile exists/);assert.equal(h.pending.length,1);assert.equal(h.state().busy,false);assert.deepEqual(h.state().avatarSettings,[{id:'b'}]);
});
test('list failure after successful restoration is reported separately',async()=>{
 const h=restorationHarness();const task=h.restoreArchivedSetting('archive-a');h.pending[0].resolve();await Promise.resolve();h.pending[1].reject(Error('list unavailable'));
 await assert.rejects(task,/profiles.archive.refresh_failed/);assert.equal(h.pending.length,2);assert.equal(h.state().selectedSettingId,'b');assert.equal(h.state().busy,false);
});

for(const inFlight of [false,true]) test(`delete refuses pending numeric edit (inFlight=${inFlight}) without discarding it`,async()=>{
 const h=deletionHarness();h.pendingEdit('a',inFlight);await h.deleteSetting('a');assert.equal(h.pending.length,0);assert.equal(h.state().selectedSettingId,'a');assert.match(h.state().message,/pending_edits/);
});
test('pending edit on a different profile does not block deleting the requested profile',async()=>{
 const h=deletionHarness();h.pendingEdit('b',true);const task=h.deleteSetting('a');assert.equal(h.pending[0].args.settingId,'a');h.pending[0].resolve();await task;
});

test('renderer lookup follows stable profile identity after rename and rejects a different ID at the old path',()=>{
 const code=`const renderers=[{id:1,pid:11,state:'Running',profile_id:'a',manifest_path:'old.toml'},{id:2,pid:12,state:'Running',profile_id:'b',manifest_path:'new.toml'},{id:3,pid:13,state:'Running',manifest_path:'new.toml'}];
 const avatarSettingById=new Map([['a',{id:'a',manifest_path:'new.toml'}]]);
 const sameNormalizedPath=(a,b)=>a?.toLowerCase()===b?.toLowerCase();const settingForManifestPath=p=>({id:'legacy',manifest_path:p});
 ${body('isLiveRenderer')}${body('renderersForSetting')}${body('settingForRenderer')}
 return {renderersForSetting,settingForRenderer,renderers};`;
 const h=new Function(ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)();
 assert.deepEqual(h.renderersForSetting({id:'a',manifest_path:'new.toml'}).map(r=>r.id),[1,3]);
 assert.equal(h.settingForRenderer(h.renderers[0]).id,'a');assert.equal(h.settingForRenderer(h.renderers[1]),null);
});

function creationHarness(){
 const pending=[];const invoke=(cmd,args)=>new Promise((resolve,reject)=>pending.push({cmd,args,resolve,reject}));
 const code=`const pendingProfileSettingUpdates=new Map();const profileBatchDrafts=[];let busy=false,message='',selectedSettingId='a',activeTab='settings',launchTargetId='running-a',avatarSettings=[{id:'a'},{id:'b',note:'unsaved'}];
 const hasTauriRuntime=()=>true;const $_=s=>s;const compareAvatarSettings=(a,b)=>a.id.localeCompare(b.id);let editorNavigationGeneration=0;const traceFrontendEvent=()=>{};${body('setActiveTab')}${body('selectProfileForEditing')}
 ${body('duplicateSetting')} let creatingProfile=false; ${body('newSetting')}
 return {duplicateSetting,newSetting,batchEdit(settingId,state){profileBatchDrafts.push({settingId,state});},pendingEdit(id,inFlight=false){pendingProfileSettingUpdates.set(id,{settingId:id,inFlight});},navigate(id,tab){selectProfileForEditing(id);setActiveTab(tab,"test");},state:()=>({busy,message,selectedSettingId,activeTab,avatarSettings,launchTargetId})};`;
 return {...new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke),pending};
}
for(const name of ['duplicateSetting','newSetting']){
 test(`${name} retains another editor and launch target without full refresh`,async()=>{
  const h=creationHarness();const run=h[name]('a');h.navigate('b','settings');h.pending[0].resolve({id:'new',name:'New'});await run;
  assert.equal(h.pending.length,1);assert.equal(h.state().selectedSettingId,'b');assert.equal(h.state().avatarSettings.find(s=>s.id==='b').note,'unsaved');assert.equal(h.state().launchTargetId,'running-a');
 });
 test(`${name} preserves navigation to logs`,async()=>{
  const h=creationHarness();const run=h[name]('a');h.navigate('a','logs');h.pending[0].resolve({id:'new',name:'New'});await run;
  assert.equal(h.state().activeTab,'logs');assert.equal(h.state().selectedSettingId,'a');
 });
 test(`${name} selects new target when context unchanged and rejects overlapping operation`,async()=>{
  const h=creationHarness();const run=h[name]('a');await h[name]('a');assert.equal(h.pending.length,1);
  h.pending[0].resolve({id:'new',name:'New'});await run;assert.equal(h.state().selectedSettingId,'new');assert.equal(h.state().busy,false);
 });
}

function refreshHarness(){
 const pending=[];const invoke=(cmd)=>cmd==='list_avatar_settings'?new Promise((resolve,reject)=>pending.push({resolve,reject})):Promise.resolve([]);
 const code=`let refreshAllRequest=0,avatarSettings=[{id:'a'}],pendingProfileSettingUpdates=new Map(),activeTab='settings',renderers=[],runtimeStatuses={},notifications=[],nativeNotificationStatus=null,rendererCacheStatus=null,selectedSettingId='a',launchTargetId='a',appSettings={},loadedProfileDetailKey='a',message='';
 const traceFrontendEvent=()=>{},traceAsync=(_n,fn)=>fn(),hasTauriRuntime=()=>true,refreshRendererRuntimeStatuses=async()=>{},refreshRendererCacheStatus=()=>{},reconcileSelectedRendererId=()=>{};
 const pickInitialSelectedSettingId=(id,_last,list)=>list.some(p=>p.id===id)?id:list[0]?.id??null,pickInitialLaunchTargetId=pickInitialSelectedSettingId;
 ${body('refreshAll')}
 return {refreshAll,add(){avatarSettings=[...avatarSettings,{id:'new'}];selectedSettingId='new';},pendingEdit(){pendingProfileSettingUpdates.set('a',{});avatarSettings[0].note='unsaved';},state:()=>({avatarSettings,selectedSettingId,message})};`;
 return {...new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke),pending};
}
const settle=()=>new Promise(resolve=>setImmediate(resolve));
test('old full refresh cannot remove a newly created profile or move its selection',async()=>{
 const h=refreshHarness();const run=h.refreshAll();await settle();h.add();h.pending[0].resolve([{id:'a'}]);await run;
 assert.equal(h.state().selectedSettingId,'new');assert.equal(h.state().avatarSettings.length,2);
});
test('full refresh retains pending profile edits',async()=>{
 const h=refreshHarness();const run=h.refreshAll();await settle();h.pendingEdit();h.pending[0].resolve([{id:'a',note:'old'}]);await run;
 assert.equal(h.state().avatarSettings[0].note,'unsaved');
});
test('newest full refresh wins over older list results and errors',async()=>{
 const h=refreshHarness();const old=h.refreshAll();await settle();const recent=h.refreshAll();await settle();
 h.pending[1].resolve([{id:'a',note:'latest'}]);await recent;h.pending[0].reject(Error('obsolete'));await old;
 assert.equal(h.state().avatarSettings[0].note,'latest');assert.equal(h.state().message,'');
});

for(const name of ['duplicateSetting','newSetting']){
 for(const destination of [['b','settings'],['a','logs']]){
  test(`${name} does not steal focus after a navigation round trip via ${destination}`,async()=>{
   const h=creationHarness();const run=h[name]('a');
   h.navigate(...destination);h.navigate('a','settings');
   h.pending[0].resolve({id:'new',name:'New'});await run;
   assert.equal(h.state().selectedSettingId,'a');assert.equal(h.state().activeTab,'settings');
   assert.ok(h.state().avatarSettings.some(s=>s.id==='new'));
   assert.equal(h.state().launchTargetId,'running-a');
  });
 }
}

for (const kind of ['Vrm','Unavatar']) {
 const variable=kind==='Vrm'?'vrmMetadataModal':'unavatarMetadataModal';
 function metadataHarness(){
  let resolve,reject;const saved=new Promise((ok,fail)=>{resolve=ok;reject=fail;});
  const calls=[];
  const code=`const original={settingId:'a',expectedRevision:'dialog-open-revision',pendingPath:'a.vrm',rendererToRestart:null,metadata:{thumbnail_data_url:'data:preview'}};
   let ${variable}=original,busy=false,message='',useThumbnailForProfileIconOnAccept=true;
   const unavatarProfileIconCrop={enabled:true,imageDataUrl:'data:preview'};const $_=s=>s;
   const saveAvatarPath=(...args)=>{calls.push(args);return saved};
   const applySelectedAvatarThumbnail=async(options)=>{calls.push(['icon-revision',options.expectedRevision]);throw Error('icon failed')};
   const saveUnavatarProfileIconCrop=async(id,renderer,revision)=>{calls.push(['icon-revision',revision]);throw Error('icon failed')};
   ${body(`accept${kind}MetadataAndUse`)}
   return {run:accept${kind}MetadataAndUse,replace:()=>${variable}={settingId:'b',pendingPath:'b.vrm'},state:()=>({modal:${variable},busy,message}),original};`;
  return {...new Function('saved','calls',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(saved,calls),resolve,reject,calls};
 }
 test(`${kind} path save failure retains original confirmation and target`,async()=>{
  const h=metadataHarness(),task=h.run();h.reject(Error('save refused'));await task;
  assert.deepEqual(h.calls[0],['a.vrm',null,'a','dialog-open-revision']);assert.equal(h.state().modal,h.original);
  assert.equal(h.state().busy,false);assert.match(h.state().message,/save refused/);
 });
 if(kind==='Vrm')test('VRM icon retry does not repeat saved avatar path or adopt another revision',async()=>{
  const h=metadataHarness(),task=h.run();h.resolve({id:'a',revision:'path-saved'});await task;
  await h.run();assert.equal(h.calls.length,3);assert.deepEqual(h.calls[2],['icon-revision','path-saved']);
  assert.equal(h.state().modal.settingId,'a');assert.equal(h.state().modal.avatarPathSaved,true);
 });
 test(`${kind} old save failure cannot replace a newer confirmation`,async()=>{
  const h=metadataHarness(),task=h.run();h.replace();h.reject(Error('save refused'));await task;
  assert.equal(h.state().modal.settingId,'b');
 });
 test(`${kind} icon failure after successful path save does not reopen path confirmation`,async()=>{
  const h=metadataHarness(),task=h.run();h.resolve({id:'a',revision:'path-saved'});await task;assert.deepEqual(h.calls[1],['icon-revision','path-saved']);
  if(kind==='Vrm'){assert.equal(h.state().modal.avatarPathSaved,true);assert.equal(h.state().modal.expectedRevision,'path-saved');assert.match(h.state().modal.saveError,/icon failed/);}else {assert.equal(h.state().modal.pendingPath,null);assert.equal(h.state().modal.iconSelectionOnly,true);assert.equal(h.state().modal.settingId,'a');assert.equal(h.state().modal.expectedRevision,'path-saved');}
  assert.equal(h.state().busy,false);assert.match(h.state().message,/icon failed/);
 });
}

for(const value of ['Renamed profile',false]) test(`immediate save retains ${typeof value} for retry after switching profile`,async()=>{
 const h=harness();const field=typeof value==='string'?'profile.display_name':'profile.allow_multiple_renderers';
 const save=h.updateSettingValue(field,value);h.selectB();h.pending[0].reject(Error('disk locked'));await save;
 assert.equal(h.failures().length,1);assert.equal(h.failures()[0].value,value);
 const retry=h.retry(h.failures()[0].key);
 assert.deepEqual(h.pending[1].args,{settingId:'a',field,value});
 h.pending[1].resolve({id:'a',manifest_path:'a.toml'});await retry;
 assert.equal(h.failures().length,0);assert.equal(h.state().selectedSettingId,'b');
});


test('coalesced retry keeps the edit-start revision after a newer profile read',async()=>{
 const h=coalescedHarness();h.readRevision('read-old');h.schedule(42);h.readRevision('read-new');
 const save=h.flush();assert.equal(h.requests[0].args.expectedRevision,'read-old');h.requests[0].reject(Error('profile_conflict'));await save;
 const retry=h.flush();assert.equal(h.requests[1].args.expectedRevision,'read-old');h.requests[1].reject(Error('profile_conflict'));await retry;
 assert.equal(h.failures()[0].value,42);
 h.schedule(55);const fresh=h.flush();assert.equal(h.requests[2].args.expectedRevision,'read-new');h.requests[2].resolve({id:'a',revision:'saved'});await fresh;
 assert.equal(h.failures().length,0);
});

for (const format of ['Vrm', 'Unavatar']) {
 test(`${format} metadata wait retains revision captured before another profile update`,async()=>{
  let finish; const metadata=new Promise(resolve=>{finish=resolve});
  const code=`let selectedSetting={id:'a',revision:'opened'},vrmMetadataModal=null,unavatarMetadataModal=null,unavatarProfileIconCrop={},useThumbnailForProfileIconOnAccept=false,message='';
   const $_=s=>s;const looksLikeUnavatarPath=()=>${format==='Unavatar'};
   const readUnavatarMetadataForPath=()=>metadata,readVrmMetadataForPath=()=>metadata;
   const firstUnavatarPreviewDataUrl=()=>null;const saveAvatarPath=()=>{throw Error('unexpected immediate save')};
   ${body('requestAvatarPathUpdate')}
   return {run:()=>requestAvatarPathUpdate('avatar.vrm',null),change:()=>{selectedSetting.revision='external';selectedSetting={id:'b',revision:'b'}},modal:()=>${format==='Vrm'?'vrmMetadataModal':'unavatarMetadataModal'}};`;
  const h=new Function('metadata',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(metadata);
  const work=h.run();h.change();finish({});await work;
  assert.equal(h.modal().settingId,'a');assert.equal(h.modal().expectedRevision,'opened');
 });
}
test('avatar path save sends the dialog revision rather than the currently selected target',async()=>{
 const h=harness();h.selectB();const task=h.saveAvatarPath('chosen.vrm',null,'a','opened');
 assert.deepEqual(h.pending[0].args,{settingId:'a',expectedRevision:'opened',field:'avatar_path',value:'chosen.vrm'});
 h.pending[0].resolve({id:'a',manifest_path:'a.toml',revision:'saved'});await task;
 assert.equal(h.state().selectedSettingId,'b');
});

test('icon picker captures revision before waiting and retains rejected path for original target retry',async()=>{
 const h=harness();h.readRevision('picker-open');const choosing=h.browseSettingPath('icon_path','icon');
 h.readRevision('external-edit');h.selectB();h.pending[0].resolve('chosen-icon.png');await Promise.resolve();
 const request=h.pending[1];assert.equal(request.cmd,'update_avatar_setting_value');
 assert.deepEqual(request.args,{settingId:'a',expectedRevision:'picker-open',field:'icon_path',value:'chosen-icon.png'});
 request.reject(Error('profile_conflict'));await choosing;
 assert.equal(h.failures()[0].value,'chosen-icon.png');assert.equal(h.failures()[0].settingId,'a');
 const retry=h.retry(h.failures()[0].key);
 assert.deepEqual(h.pending[2].args,request.args);
 h.pending[2].reject(Error('profile_conflict'));await retry;
 assert.equal(h.state().selectedSettingId,'b');assert.equal(h.failures().length,1);
});

for(const operation of ['thumbnail','crop']) test(`${operation} save transmits explicit captured revision and original target`,async()=>{
 const h=harness();h.selectB();
 const task=operation==='thumbnail'?h.applySelectedAvatarThumbnail({settingId:'a',avatarPath:'a.vrm',expectedRevision:'captured'}):h.saveUnavatarProfileIconCrop('a',null,'captured');
 assert.equal(h.pending[0].args.settingId,'a');assert.equal(h.pending[0].args.expectedRevision,'captured');
 h.pending[0].resolve({id:'a',manifest_path:'a.toml',revision:'saved'});await task;
 assert.equal(h.state().selectedSettingId,'b');
});
for(const format of ['Vrm','Unavatar']) test(`${format} metadata review cannot bind old asset to newly selected profile`,async()=>{
 let finish;const metadata=new Promise(resolve=>{finish=resolve});
 const code=`let selectedSetting={id:'a',avatar_path:'a.vrm',revision:'opened'},busy=false,message='',vrmMetadataModal=null,unavatarMetadataModal=null,unavatarProfileIconCrop={};
 const hasTauriRuntime=()=>true, $_=s=>s, looksLikeUnavatarPath=()=>${format==='Unavatar'},firstUnavatarPreviewDataUrl=()=>null;
 const readUnavatarMetadataForPath=()=>metadata,readVrmMetadataForPath=()=>metadata;
 ${body('reviewSelectedAvatarMetadata')}
 return {run:reviewSelectedAvatarMetadata,change:()=>selectedSetting={id:'b',avatar_path:'b.vrm',revision:'new'},modal:()=>${format==='Vrm'?'vrmMetadataModal':'unavatarMetadataModal'}};`;
 const h=new Function('metadata',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(metadata);
 const task=h.run();h.change();finish({});await task;
 assert.equal(h.modal().settingId,'a');assert.equal(h.modal().expectedRevision,'opened');
});

test('crop save completion does not close a newer profile confirmation',async()=>{
 let finish;const saving=new Promise(resolve=>{finish=resolve});
 const code=`let unavatarMetadataModal={settingId:'a',expectedRevision:'original',rendererToRestart:null},unavatarProfileIconCrop={enabled:true,imageDataUrl:'crop'},busy=false,message='';
 const $_=s=>s;const saveUnavatarProfileIconCrop=()=>saving;
 ${body('saveUnavatarProfileIconFromModal')}
 return {run:saveUnavatarProfileIconFromModal,change:()=>unavatarMetadataModal={settingId:'b'},modal:()=>unavatarMetadataModal};`;
 const h=new Function('saving',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(saving);
 const task=h.run();h.change();finish({});await task;assert.equal(h.modal().settingId,'b');
});

test('delete request retains explicit confirmation revision and rejects without removing target',async()=>{
 const h=deletionHarness();const task=h.deleteSetting('a','confirmation-revision');
 assert.deepEqual(h.pending[0].args,{settingId:'a',expectedRevision:'confirmation-revision'});
 h.pending[0].reject(Error('profile_conflict'));await task;
 assert.ok(h.state().avatarSettings.some(s=>s.id==='a'));
 assert.match(h.state().message,/profile_conflict/);
});
test('delete hold captures displayed revision before profile list changes',()=>{
 const calls=[];let tick;
 const code=`let busy=false,deleteHoldTargetId=null,deleteHoldProgress=0,deleteHoldStartedAt=0,deleteHoldTimer=null;const deleteHoldDurationMs=100;
 let revision='before';const avatarSettingBySelectedId=()=>({revision});const performance={now:()=>0};
 const cancelDeleteHold=()=>{};const window={requestAnimationFrame:f=>{setTick(f);return 1}};
 const deleteSetting=(...args)=>calls.push(args);
 ${body('startDeleteHold')}
 return {start:startDeleteHold,change:()=>{revision='after';performance.now=()=>100;}};`;
 const h=new Function('calls','setTick',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(calls,f=>tick=f);
 h.start('a');h.change();tick();assert.deepEqual(calls,[['a','before']]);
});

for (const moveAt of ['none','save','apply']) test(`dynamics group addition preserves navigation during ${moveAt}`,async()=>{
 let finishSave,finishApply;const saveGate=new Promise(resolve=>finishSave=resolve),applyGate=new Promise(resolve=>finishApply=resolve);
 const requests=[],applied=[];
 const code=`let selectedSettingId='a',activeTab='renderers',editorNavigationGeneration=0,message='';
 let avatarSettings=[{id:'b',manifest_path:'b.toml',revision:'r1',dynamics_group_overrides:[]}],launchTargetId='a';
 const hasTauriRuntime=()=>true,settingForRenderer=()=>avatarSettings[0],$_=key=>key;
 const compareAvatarSettings=(a,b)=>a.id.localeCompare(b.id),pickInitialLaunchTargetId=current=>current;
 const traceFrontendEvent=()=>{};
 ${body('replaceAvatarSetting')}
 ${body('selectProfileForEditing')}
 ${body('setActiveTab')}
 ${body('addRendererDynamicsGroupOverride')}
 return {add:()=>addRendererDynamicsGroupOverride({id:7,manifest_path:'b.toml'},'physbone:hair',null),
 move:()=>{selectProfileForEditing('c');setActiveTab('renderers','user');},
 state:()=>({selectedSettingId,activeTab,avatarSettings})};`;
 const h=new Function('invoke','applyRuntimeProfileUpdate',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(
  async(cmd,args)=>{requests.push({cmd,args});await saveGate;return {id:'b',manifest_path:'b.toml',revision:'r2',dynamics_group_overrides:args.value};},
  async(field,value,setting)=>{applied.push(setting.id);await applyGate;}
 );
 const pending=h.add();assert.equal(requests[0].args.settingId,'b');
 if(moveAt==='save')h.move();
 finishSave();await new Promise(resolve=>setImmediate(resolve));
 if(moveAt==='apply')h.move();
 finishApply();await pending;
 assert.deepEqual(applied,['b']);
 assert.equal(h.state().avatarSettings[0].dynamics_group_overrides[0].source_id,'physbone:hair');
 assert.equal(h.state().selectedSettingId,moveAt==='none'?'b':'c');
 assert.equal(h.state().activeTab,moveAt==='none'?'settings':'renderers');
});

for (const moveAt of ['none','save','apply']) test(`dynamics match addition preserves navigation during ${moveAt}`,async()=>{
 let finishSave,finishApply;const saveGate=new Promise(resolve=>finishSave=resolve),applyGate=new Promise(resolve=>finishApply=resolve);
 const requests=[],applied=[];
 const code=`let selectedSettingId='a',activeTab='renderers',editorNavigationGeneration=0,message='';
 let avatarSettings=[{id:'b',manifest_path:'b.toml',revision:'r1',dynamics_match_overrides:[]}],launchTargetId='a';
 const hasTauriRuntime=()=>true,settingForRenderer=()=>avatarSettings[0],$_=key=>key;
 const compareAvatarSettings=(a,b)=>a.id.localeCompare(b.id),pickInitialLaunchTargetId=current=>current;
 const traceFrontendEvent=()=>{}; const DYNAMICS_MATCH_OVERRIDE_FIELD='physics.dynamics.solver.match_overrides';
 ${body('replaceAvatarSetting')}
 ${body('selectProfileForEditing')}
 ${body('setActiveTab')}
 ${body('addRendererDynamicsMatchOverride')}
 return {add:()=>addRendererDynamicsMatchOverride({id:7,manifest_path:'b.toml'},{source_id:'physbone:hair'}),
 move:()=>{selectProfileForEditing('c');setActiveTab('renderers','user');},
 state:()=>({selectedSettingId,activeTab,avatarSettings})};`;
 const h=new Function('invoke','applyRuntimeProfileUpdate',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(
  async(cmd,args)=>{requests.push({cmd,args});await saveGate;return {id:'b',manifest_path:'b.toml',revision:'r2',dynamics_match_overrides:args.value};},
  async(field,value,setting)=>{applied.push(setting.id);await applyGate;}
 );
 const pending=h.add();assert.equal(requests[0].args.settingId,'b');
 if(moveAt==='save')h.move();
 finishSave();await new Promise(resolve=>setImmediate(resolve));
 if(moveAt==='apply')h.move();
 finishApply();await pending;
 assert.deepEqual(applied,['b']);
 assert.equal(h.state().avatarSettings[0].dynamics_match_overrides[0].source_id,'physbone:hair');
 assert.equal(h.state().selectedSettingId,moveAt==='none'?'b':'c');
 assert.equal(h.state().activeTab,moveAt==='none'?'settings':'renderers');
});

for (const scenario of ['renamed','missing','legacy']) test(`renderer restart resolves original profile before stop: ${scenario}`,async()=>{
 const calls=[];
 const code=`let busy=false,message='',selectedRendererId=null;const hasTauriRuntime=()=>true,$_=(s)=>s;const refreshAll=async()=>{};
 ${body('restartRenderer')}
 return {restartRenderer,state:()=>({busy,message,selectedRendererId})};`;
 const h=new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(async(cmd,args)=>{
  calls.push({cmd,args});
  if(cmd==='get_avatar_setting_details') {if(scenario==='missing') throw Error('profile not found'); return {id:'stable-a',manifest_path:'renamed.toml'};}
  if(cmd==='launch_renderer') return {id:8,name:'A'};
 });
 const ok=await h.restartRenderer({id:7,manifest_path:'old.toml',...(scenario==='legacy'?{}:{profile_id:'stable-a'})});
 assert.equal(calls[0].cmd,'get_avatar_setting_details');
 assert.equal(calls[0].args.settingId,scenario==='legacy'?'old.toml':'stable-a');
 if(scenario==='missing') {assert.equal(ok,false);assert.equal(calls.length,1);assert.match(h.state().message,/profile not found/);}
 else {assert.equal(ok,true);assert.deepEqual(calls.slice(1),[{cmd:'stop_renderer',args:{id:7}},{cmd:'launch_renderer',args:{settingId:'stable-a'}}]);}
 assert.equal(h.state().busy,false);
});

for (const move of [false,true]) test('renderer restart retains newer selection: '+move,async()=>{
 let finish;const gate=new Promise(resolve=>finish=resolve);
 const code=`let busy=false,message='',errorMessage='',selectedRendererId=7;const hasTauriRuntime=()=>true,$_=(s)=>s;const refreshAll=async()=>{};
 ${body('restartRenderer')}
 return {run:()=>restartRenderer({id:7,profile_id:'a',manifest_path:'a.toml'}),move:()=>{selectedRendererId=99},selected:()=>selectedRendererId};`;
 const h=new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(async(cmd)=>{
  if(cmd==='get_avatar_setting_details')return {id:'a'};
  if(cmd==='launch_renderer'){await gate;return {id:8,name:'A'};}
 });
 const task=h.run();await new Promise(resolve=>setImmediate(resolve));if(move)h.move();finish();await task;
 assert.equal(h.selected(),move?99:8);
});

for (const failure of ['none','launch','refresh','both']) test(`renderer group launch preserves outcomes: ${failure}`,async()=>{
 const calls=[];let refreshes=0;let h;
 const code=`let busy=false,message='',selectedRendererId=7,launchGroupName='Group A',launchGroupSettings=[{id:'a',name:'A'},{id:'b',name:'B'}],activeTab='settings',editorNavigationGeneration=0;
 const hasTauriRuntime=()=>true,$_=(key,options)=>key+JSON.stringify(options??{}),appSettings={jump_to_renderers_on_quick_run:true};const setActiveTab=tab=>{activeTab=tab};
 ${body('launchSetting')}
 return {run:()=>launchSetting(null),move:()=>{launchGroupSettings=[{id:'c'}];selectedRendererId=99;activeTab='logs';editorNavigationGeneration++;},state:()=>({busy,message,selectedRendererId,activeTab})};`;
 h=new Function('invoke','refreshAll',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(async(cmd,args)=>{
  calls.push(args.settingId);if(args.settingId==='a'){h.move();return {id:10,name:'A'};}
  if(failure==='launch'||failure==='both')throw Error('launch-original-error');return {id:11,name:'B'};
 },async()=>{refreshes++;if(failure==='refresh'||failure==='both')throw Error('refresh-secondary-error');});
 await h.run();assert.deepEqual(calls,['a','b']);assert.equal(refreshes,1);assert.equal(h.state().busy,false);
 assert.equal(h.state().selectedRendererId,99);assert.equal(h.state().activeTab,'logs');
 if(failure==='launch'||failure==='both'){assert.match(h.state().message,/launch-original-error/);assert.match(h.state().message,/partial_launch/);assert.match(h.state().message,/10/);}
 else {assert.match(h.state().message,/opened_renderers/);assert.match(h.state().message,/"count":2/);}
 if(failure==='refresh'||failure==='both')assert.match(h.state().message,/refresh-secondary-error/);
});

test('startup does not consume its attempt while another operation is busy',async()=>{
 const code=`let busy=true,backendAppSettingsReady=true,startupAutoLaunchAttempted=false,activeTab='settings',launches=0;
 const appSettings={auto_launch_selected_on_startup:true},launchTargetId='a',avatarSettings=[],isValidLaunchTarget=()=>true,launchTargetSetting={id:'a'};
 const visibleRenderers=[],liveCapturers=()=>[],setActiveTab=tab=>{activeTab=tab},launchSetting=async()=>{if(!busy)launches++;};
 ${body('maybeAutoLaunchSelectedOnStartup')}
 return {run:maybeAutoLaunchSelectedOnStartup,idle:()=>{busy=false},state:()=>({launches,startupAutoLaunchAttempted,activeTab})};`;
 const h=new Function(ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)();
 await h.run();assert.deepEqual(h.state(),{launches:0,startupAutoLaunchAttempted:false,activeTab:'settings'});
 h.idle();await h.run();await h.run();assert.deepEqual(h.state(),{launches:1,startupAutoLaunchAttempted:true,activeTab:'renderers'});
});

for (const inFlight of [false,true]) {
 test(`duplicate refuses source with retained or in-flight edits (${inFlight})`,async()=>{
  const h=creationHarness();h.pendingEdit('a',inFlight);const run=h.duplicateSetting('a');if(h.pending.length)h.pending[0].resolve({id:'unexpected',name:'Unexpected'});await run;
  assert.equal(h.pending.length,0);assert.equal(h.state().selectedSettingId,'a');
  assert.equal(h.state().avatarSettings.length,2);assert.match(h.state().message,/duplicate_pending_edits/);
 });
}
test('duplicate permits saved source when another profile has retained edits',async()=>{
 const h=creationHarness();h.pendingEdit('b');const run=h.duplicateSetting('a');
 assert.equal(h.pending.length,1);h.pending[0].resolve({id:'copy',name:'Copy'});await run;
 assert.equal(h.state().avatarSettings.find(s=>s.id==='b').note,'unsaved');
});

for(const state of ['saving','failed'])for(const action of ['duplicateSetting','deleteSetting']){
 test(`${action} retains unresolved batch (${state})`,async()=>{
  const h=action==='duplicateSetting'?creationHarness():deletionHarness();h.batchEdit('a',state);
  const task=h[action]('a');if(h.pending.length)h.pending[0].reject(Error('unexpected mutation'));await task;
  assert.equal(h.pending.length,0);assert.equal(h.state().selectedSettingId,'a');assert.match(h.state().message,/pending_edits/);
 });
}
for(const action of ['duplicateSetting','deleteSetting']){
 test(`${action} does not block unrelated batch`,async()=>{
  const h=action==='duplicateSetting'?creationHarness():deletionHarness();h.batchEdit('b','failed');
  const task=h[action]('a');assert.equal(h.pending.length,1);h.pending[0].resolve({id:'copy',name:'Copy'});await task;
 });
}

function matchDraftHarness() {
 const calls=[];
 const invoke=(cmd,args)=>new Promise((resolve,reject)=>calls.push({cmd,args,resolve,reject}));
 const code=`
 let selectedSettingId='a',message='';
 const avatarSettings=[{id:'a',revision:'a1',dynamics_match_overrides:[{source_id_contains:['hair'],solver:'xpbd',constraint_iterations:9}]},{id:'b',revision:'b1',dynamics_match_overrides:[{source_id:'cloth'}]}];
 const profileMatchDrafts={},profileColliderDrafts={},pendingProfileSettingUpdates=new Map(); let failedProfileSettingUpdates=[];
 const DYNAMICS_MATCH_OVERRIDE_FIELD='physics.dynamics.solver.match_overrides';
 const window={setTimeout:()=>1,clearTimeout:()=>{}},profileSettingUpdateCoalesceMs=100;
 const avatarSettingBySelectedId=id=>avatarSettings.find(s=>s.id===id),canApplyWithoutRestart=()=>true;
 const $_=s=>s,applyRuntimeProfileUpdate=async()=>{},queueRendererRestart=()=>{};
 const replaceAvatarSetting=s=>{avatarSettings[avatarSettings.findIndex(v=>v.id===s.id)]=s;};
 ${body('physicsSettingForEditor')} ${body('profileSettingUpdateKey')}
 ${body('scheduleCoalescedProfileSettingUpdate')} ${body('flushCoalescedProfileSettingUpdate')}
 return {edit:(id,value)=>scheduleCoalescedProfileSettingUpdate(avatarSettingBySelectedId(id),DYNAMICS_MATCH_OVERRIDE_FIELD,value),
 flush:id=>flushCoalescedProfileSettingUpdate(profileSettingUpdateKey(id,DYNAMICS_MATCH_OVERRIDE_FIELD)),
 view:id=>physicsSettingForEditor(avatarSettingBySelectedId(id)),saved:id=>avatarSettingBySelectedId(id),
 select:id=>selectedSettingId=id,drafts:()=>profileMatchDrafts,failures:()=>failedProfileSettingUpdates};`;
 return {...new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke),calls};
}
test('incomplete match edits retain coefficients and profile target until corrected save',async()=>{
 const h=matchDraftHarness();
 const draft=[{source_id_contains:[],solver:'xpbd',constraint_iterations:9}];
 h.edit('a',draft);draft[0].constraint_iterations=1;
 const run=h.flush('a');h.select('b');h.calls[0].reject(Error('requires a source_id'));await run;
 assert.equal(h.view('a').dynamics_match_overrides.length,1);
 assert.equal(h.view('a').dynamics_match_overrides[0].constraint_iterations,9);
 assert.deepEqual(h.saved('a').dynamics_match_overrides[0].source_id_contains,['hair']);
 assert.deepEqual(h.view('b'),h.saved('b'));
 const completed=h.view('a').dynamics_match_overrides.map(r=>({...r,source_id_regex:['Hair/Tip$']}));
 h.select('a');h.edit('a',completed);const retry=h.flush('a');
 assert.equal(h.calls[1].args.settingId,'a');
 h.calls[1].resolve({...h.saved('a'),revision:'a2',dynamics_match_overrides:h.calls[1].args.value});await retry;
 assert.equal(h.drafts().a,undefined);assert.deepEqual(h.view('a').dynamics_match_overrides,completed);
 assert.equal(h.failures().length,0);
});
test('older match save cannot clear newer draft; explicit removal still saves empty array',async()=>{
 const h=matchDraftHarness();h.edit('a',[{source_id:'first'}]);const run=h.flush('a');
 h.edit('a',[{source_id:'second'}]);h.calls[0].resolve({...h.saved('a'),revision:'a2',dynamics_match_overrides:[{source_id:'first'}]});await run;
 assert.equal(h.view('a').dynamics_match_overrides[0].source_id,'second');
 h.edit('a',[]);const remove=h.flush('a');assert.deepEqual(h.calls[1].args.value,[]);
 h.calls[1].resolve({...h.saved('a'),revision:'a3',dynamics_match_overrides:[]});await remove;
 assert.equal(h.drafts().a,undefined);assert.deepEqual(h.view('a').dynamics_match_overrides,[]);
});
test('match component sends incomplete row to draft owner rather than filtering it out',async()=>{
 const component=await readFile(new URL('../src/lib/ProfileDynamicsMatchOverrides.svelte',import.meta.url),'utf8');
 const start=component.indexOf('\tfunction cleanOverride('),end=component.indexOf('\n\tfunction addOverride(',start);
 const calls=[];
 const code=`let overrides=[{source_id_contains:['hair'],solver:'xpbd',constraint_iterations:9}]; const DYNAMICS_MATCH_OVERRIDE_FIELD='match'; ${component.slice(start,end)}; return updateOverride;`;
 const update=new Function('onUpdateSettingValue',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)((field,value)=>calls.push(value));
 update(0,{source_id_contains:[]});assert.equal(calls[0].length,1);assert.equal(calls[0][0].constraint_iterations,9);
});

for(const field of ['avatar_path','icon_path'])for(const outcome of ['cancel','error'])test(`${field} picker ${outcome} preserves both profiles after selection switch`,async()=>{
 const h=harness();const task=h.browseSettingPath(field,field==='avatar_path'?'avatar':'icon');
 assert.equal(h.pending[0].cmd,'pick_file_path');
 h.selectB();const before=structuredClone(h.state());
 if(outcome==='cancel')h.pending[0].resolve(null);else h.pending[0].reject(Error('dialog failed'));
 await task;
 assert.equal(h.pending.length,1,'No save or metadata request after picker cancel/error');
 assert.deepEqual(h.state(),before);
 assert.deepEqual(h.failures(),[]);
});
