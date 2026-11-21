import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source = await readFile(new URL('../src/App.svelte',import.meta.url),'utf8');
const start = source.indexOf('\tasync function refreshWardrobeOptionsForSetting(');
assert.ok(start >= 0);
const body = source.slice(start,source.indexOf('\n\t}',start)+3);
function harness() {
 const pending=[];
 const invoke=()=>new Promise((resolve,reject)=>pending.push({resolve,reject}));
 const js=ts.transpileModule(`let wardrobeOptions=null,wardrobeOptionsKey='',wardrobeOptionsRequestId=0;
 ${body}
 return {refresh:refreshWardrobeOptionsForSetting,get:()=>wardrobeOptions};`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
 return {...new Function('invoke','hasTauriRuntime',js)(invoke,()=>true),pending};
}
const setting=path=>({avatar_path:path,manifest_path:'profiles.toml'});
const options=id=>({available:true,base_label:'Base',sets:[{id,name:id}]});
test('changing model immediately removes previous model clothing choices',async()=>{
 const h=harness();const a=h.refresh(setting('a.unavatar'));h.pending[0].resolve(options('a-dress'));await a;
 assert.equal(h.get().sets[0].id,'a-dress');
 const b=h.refresh(setting('b.unavatar'));assert.equal(h.get(),null);
 h.pending[1].resolve(options('b-dress'));await b;assert.equal(h.get().sets[0].id,'b-dress');
});
test('A to B to A rejects stale success and stale failure even when paths match again',async()=>{
 for(const fail of [false,true]){
  const h=harness();const a=h.refresh(setting('a.unavatar')),b=h.refresh(setting('b.unavatar')),latest=h.refresh(setting('a.unavatar'));
  h.pending[2].resolve(options('current'));await latest;
  if(fail)h.pending[0].reject(new Error('old failure'));else h.pending[0].resolve(options('old'));
  await a;h.pending[1].resolve(options('b'));await b;assert.equal(h.get().sets[0].id,'current');
 }
});
test('clearing the model invalidates pending options; current errors remain visible',async()=>{
 const h=harness();const a=h.refresh(setting('a.unavatar'));await h.refresh(null);
 h.pending[0].resolve(options('old'));await a;assert.equal(h.get(),null);
 const b=h.refresh(setting('b.unavatar'));h.pending[1].reject(new Error('current failure'));await b;
 assert.equal(h.get().available,false);assert.match(h.get().error,/current failure/);
});
