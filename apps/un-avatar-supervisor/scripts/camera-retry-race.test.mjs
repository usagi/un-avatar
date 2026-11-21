import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/App.svelte',import.meta.url),'utf8');
function body(name){const start=source.indexOf(`\tasync function ${name}(`);assert.ok(start>=0);return source.slice(start,source.indexOf('\n\t}',start)+3);}
function gate(){let release;const promise=new Promise(r=>release=r);return {promise,release};}
function harness(){
 const renderer={id:2,pid:202,state:'Running',profile_id:'a'};const requests=[],calls=[];
 const code=`let busy=false,message='';const profile={id:'a'},pendingProfileSettingUpdates=new Map(),profileRuntimeApplyTails=new Map();
 const selectedRendererById=id=>renderer.id===id?renderer:null;
 const settingForRenderer=r=>r?.profile_id==='a'?profile:null;
 const isLiveRenderer=r=>r.pid!=null&&r.state==='Running';
 const flushCoalescedProfileSettingUpdate=async()=>{};
 const applyRuntimeCameraUpdate=async(fields,saved,targets)=>{calls.push({value:saved.value,ids:targets.map(t=>t.id)});};
 ${body('retrySavedCameraForRenderer')}
 return {retry:retrySavedCameraForRenderer,tails:profileRuntimeApplyTails,state:()=>({busy,message})};`;
 const invoke=(cmd,args)=>new Promise((resolve,reject)=>requests.push({cmd,args,resolve,reject}));
 return {...new Function('renderer','calls','invoke',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(renderer,calls,invoke),renderer,requests,calls};
}
async function queued(h){for(let i=0;i<10&&!h.tails.has('a');i++)await Promise.resolve();assert.ok(h.tails.has('a'));}
test('retry skips an exited renderer while waiting for current profile apply',async()=>{
 const h=harness(),g=gate();h.tails.set('a',g.promise);const retry=h.retry(2);h.requests[0].resolve({id:'a',value:42});await Promise.resolve();h.renderer.state='Exited';g.release();await retry;assert.deepEqual(h.calls,[]);
});
test('retry must freeze process identity before awaiting saved profile',async()=>{
 const h=harness();const retry=h.retry(2);h.renderer.pid=303;h.requests[0].resolve({id:'a',value:42});await retry;assert.deepEqual(h.calls,[]);
});
test('later profile application follows retry and keeps its queue ownership',async()=>{
 const h=harness();const retry=h.retry(2);h.requests[0].resolve({id:'a',value:42});await queued(h);
 const later=h.tails.get('a').then(()=>h.calls.push({value:44,ids:[2]}));h.tails.set('a',later);
 await retry;await later;assert.deepEqual(h.calls,[{value:42,ids:[2]},{value:44,ids:[2]}]);assert.equal(h.tails.get('a'),later);
});
test('failed saved-profile read retains failure and emits no command',async()=>{
 const h=harness();const retry=h.retry(2);h.requests[0].reject(Error('read failed'));await retry;assert.deepEqual(h.calls,[]);assert.match(h.state().message,/read failed/);assert.equal(h.state().busy,false);
});
