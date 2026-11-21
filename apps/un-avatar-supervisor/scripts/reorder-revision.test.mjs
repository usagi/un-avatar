import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/App.svelte',import.meta.url),'utf8');
function extract(name){let start=source.indexOf(`\tasync function ${name}(`);if(start<0)start=source.indexOf(`\tfunction ${name}(`);assert.ok(start>=0);return source.slice(start,source.indexOf('\n\t}',start)+3);}
const body=extract('saveSettingsOrder')+extract('discardSettingsOrder')+extract('beginSettingPointerDrag')+extract('finishSettingPointerDrag');
function harness(){
 const pending=[];
 const code=`let avatarSettings=[{id:'b',revision:'b1'},{id:'a',revision:'a1'}],message='',orderDraft=null,orderSaving=false,settingPointerDrag=null,draggedSettingId=null,busy=false;const pendingProfileSettingUpdates=new Map();const window={addEventListener:()=>{}};const document={documentElement:{classList:{add:()=>{}}}};const updateSettingPointerDrag=()=>{},cancelSettingPointerDrag=()=>{},removeSettingDragListeners=()=>{};const $_=s=>s;const replaceAvatarSetting=(s)=>{avatarSettings=avatarSettings.map(p=>p.id===s.id?s:p)};
 ${body}
 return {save:saveSettingsOrder,discard:discardSettingsOrder,draft:()=>orderDraft,state:()=>avatarSettings,change:()=>{avatarSettings[0]={id:'b',revision:'b3'}},begin:beginSettingPointerDrag,finish:finishSettingPointerDrag,activate:()=>{settingPointerDrag.active=true;settingPointerDrag.desiredIds=['a','b'];}};`;
 return {...new Function('invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)((cmd,args)=>new Promise((resolve,reject)=>pending.push({cmd,args,resolve,reject}))),pending};
}
test('reorder captures revisions and accepts returned versions without changing selection',async()=>{
 const h=harness(),save=h.save();assert.deepEqual(h.pending[0].args.expectedRevisions,{b:'b1',a:'a1'});
 h.pending[0].resolve([{id:'b',revision:'b2'},{id:'a',revision:'a2'}]);await save;
 assert.deepEqual(h.state().map(p=>p.revision),['b2','a2']);
});
test('failed reorder retains original versions and desired order across background refresh and retry',async()=>{
 const h=harness(),save=h.save();h.change();h.pending[0].reject(Error('conflict'));await save;
 assert.deepEqual(h.draft().ids,['b','a']);const retry=h.save();assert.deepEqual(h.pending[1].args.expectedRevisions,{b:'b1',a:'a1'});
 h.pending[1].resolve([{id:'a',revision:'a2'}]);await retry;assert.equal(h.draft(),null);
});
test('drag starts capture before a background snapshot changes',async()=>{
 const h=harness(),event={button:0,clientX:0,clientY:0,currentTarget:{closest:()=>({getBoundingClientRect:()=>({left:0,top:0,width:10,height:10})})},preventDefault(){},stopPropagation(){}};
 h.begin(event,'b');h.change();h.activate();h.finish(event);
 assert.deepEqual(h.pending[0].args,{settingIds:['a','b'],expectedRevisions:{b:'b1',a:'a1'}});
 h.pending[0].reject(Error('conflict'));await new Promise(resolve=>setImmediate(resolve));assert.deepEqual(h.draft().ids,['a','b']);
});
test('discard reload failure retains draft; successful explicit reload clears it',async()=>{
 const h=harness(),save=h.save();h.pending[0].reject(Error('conflict'));await save;
 const failed=h.discard();h.pending[1].reject(Error('offline'));await failed;assert.ok(h.draft());
 const success=h.discard();h.pending[2].resolve([{id:'a',revision:'a4'}]);await success;assert.equal(h.draft(),null);assert.deepEqual(h.state(),[{id:'a',revision:'a4'}]);
});
test('late reorder response does not overwrite a newer profile snapshot',async()=>{
 const h=harness(),save=h.save();h.change();h.pending[0].resolve([{id:'b',revision:'b2'},{id:'a',revision:'a2'}]);await save;
 assert.deepEqual(h.state().map(p=>p.revision),['b3','a2']);
});
test('committed save with failed readback blocks resubmission until explicit successful reload',async()=>{
 const h=harness(),save=h.save();h.pending[0].reject('profile_order_committed_refresh_failed: disk read unavailable');await save;
 assert.equal(h.draft().committed,true);await h.save();assert.equal(h.pending.length,1);
 const failed=h.discard();h.pending[1].reject('still unavailable');await failed;
 assert.equal(h.draft().committed,true);await h.save();assert.equal(h.pending.length,2);
 const reload=h.discard();h.pending[2].resolve([{id:'b',revision:'saved-version'}]);await reload;
 assert.equal(h.draft(),null);assert.equal(h.state()[0].revision,'saved-version');
});

test('late discard read preserves changed profile and retained order',async()=>{
 const h=harness(),save=h.save();h.pending[0].reject(Error('conflict'));await save;
 const reload=h.discard();h.change();h.pending[1].resolve([{id:'b',revision:'old'},{id:'a',revision:'a1'}]);await reload;
 assert.ok(h.draft());assert.equal(h.state()[0].revision,'b3');
 const retry=h.discard();h.pending[2].resolve([{id:'b',revision:'b4'}]);await retry;
 assert.equal(h.draft(),null);assert.equal(h.state()[0].revision,'b4');
});
