import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import test from 'node:test';
import ts from 'typescript';
const source=await readFile(new URL('../src/lib/orderedProfileInvoke.ts',import.meta.url),'utf8');
const code=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace('export function','function');
const wrap=new Function(code+'\nreturn orderedProfileInvoke;')();
const settle=()=>new Promise(resolve=>setImmediate(resolve));
test('failed save does not poison queued edits; arguments and target are captured',async()=>{
 const calls=[];const invoke=wrap((cmd,args)=>new Promise((resolve,reject)=>calls.push({cmd,args,resolve,reject})));
 const first=invoke('update_avatar_setting_values',{settingId:'a',updates:[]});
 const failure=assert.rejects(first,/disk failure/);
 const args={settingId:'b',field:'camera.target',value:[1,2,3]};
 const second=invoke('update_avatar_setting_value',args);args.settingId='a';args.value[0]=99;
 await settle();assert.equal(calls.length,1);
 calls[0].reject(Error('disk failure'));await failure;await settle();
 assert.deepEqual(calls[1].args,{settingId:'b',field:'camera.target',value:[1,2,3]});
 calls[1].resolve('saved');assert.equal(await second,'saved');
});
test('detail/list snapshots and icon updates cannot overtake writes; renderer controls stay available',async()=>{
 const calls=[];const invoke=wrap((cmd)=>new Promise(resolve=>calls.push({cmd,resolve})));
 const jobs=['update_avatar_setting_value','get_avatar_setting_details','save_avatar_thumbnail_icon','list_avatar_settings'].map(cmd=>invoke(cmd));
 await settle();assert.equal(calls.length,1);
 const live=invoke('list_renderers');assert.equal(calls[1].cmd,'list_renderers');calls[1].resolve([]);await live;
 calls[0].resolve({});await jobs[0];await settle();
 for(let i=1;i<jobs.length;i++){assert.equal(calls.length,i+2);calls[i+1].resolve({});await jobs[i];await settle();}
});

test('only own successful writes advance a queued edit revision',async()=>{
 const calls=[];let revision='r0';
 const invoke=wrap(async(cmd,args)=>{
  calls.push({cmd,args:structuredClone(args)});
  if(cmd==='get_avatar_setting_details')return {id:'a',revision:'external'};
  if(args.expectedRevision!==revision)throw Error('profile_conflict');
  revision=revision==='r0'?'r1':'r2';return {id:'a',revision};
 });
 const first=invoke('update_avatar_setting_value',{settingId:'a',expectedRevision:'r0',field:'a',value:1});
 const second=invoke('update_avatar_setting_values',{settingId:'a',expectedRevision:'r0',updates:[]});
 await first;await second;
 assert.equal(calls[0].args.expectedRevision,'r0');assert.equal(calls[1].args.expectedRevision,'r1');
 revision='external';
 await assert.rejects(invoke('update_avatar_setting_value',{settingId:'a',expectedRevision:'r2',field:'a',value:2}),/profile_conflict/);
 await invoke('get_avatar_setting_details',{settingId:'a'});
 await assert.rejects(invoke('update_avatar_setting_value',{settingId:'a',expectedRevision:'r2',field:'a',value:2}),/profile_conflict/);
 assert.equal(calls.at(-1).args.expectedRevision,'r2');
 await invoke('update_avatar_setting_value',{settingId:'a',expectedRevision:'external',field:'a',value:3});
 assert.equal(calls.at(-1).args.expectedRevision,'external');
});

test('legacy paths and generated icons participate in the same successful revision chain',async()=>{
 let revision='r0';const requests=[];
 const invoke=wrap(async(cmd,args)=>{requests.push(structuredClone(args));if(args.expectedRevision!==revision)throw Error('profile_conflict');revision=`r${requests.length}`;return {id:'a',revision};});
 const commands=['update_avatar_setting_path','save_avatar_thumbnail_icon','save_profile_icon_from_data_url','update_avatar_setting_values'];
 await Promise.all(commands.map(cmd=>invoke(cmd,{settingId:'a',expectedRevision:'r0'})));
 assert.deepEqual(requests.map(r=>r.expectedRevision),['r0','r1','r2','r3']);
 revision='external';
 for(const cmd of commands)await assert.rejects(invoke(cmd,{settingId:'a',expectedRevision:'r0'}),/profile_conflict/);
 assert.deepEqual(requests.slice(4).map(r=>r.expectedRevision),['r4','r4','r4','r4']);
});
