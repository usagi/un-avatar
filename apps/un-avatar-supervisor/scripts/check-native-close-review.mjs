import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import vm from 'node:vm';
import test from 'node:test';
const script=readFileSync(new URL('../src-tauri/src/close_review.js',import.meta.url),'utf8').replace('__REQUEST_ID__','42');
function run({dirty=false,inert=false,missingApi=false,ready=true}={}) {
 const root={inert,dataset:{unCloseGuardReady:ready ? "true" : undefined}},events=[],calls=[];
 let resolve,reject;
 const result=new Promise((ok,no)=>{resolve=ok;reject=no});
 const window={dispatchEvent(event){events.push(event.type);if(dirty&&event.type==='beforeunload')event.preventDefault();},
  __TAURI_INTERNALS__:missingApi?undefined:{invoke(command,args){calls.push({command,args});return result;}}};
 vm.runInNewContext(script,{window,document:{documentElement:root},Event,Promise,console:{error(){}}});
 return {root,events,calls,resolve,reject};
}
const flush=()=>new Promise(resolve=>setImmediate(resolve));
test('retained input refuses native shutdown without freezing editing',async()=>{
 const x=run({dirty:true});await flush();
 assert.equal(x.root.inert,false);assert.deepEqual(x.events,['beforeunload','un-avatar-close-blocked']);
 assert.equal(x.calls[0].command,'resolve_supervisor_close');assert.equal(x.calls[0].args.allowed,false);
 assert.equal(x.calls[0].args.requestId,42);x.resolve(false);await flush();assert.equal(x.root.inert,false);
});
test('clean approval freezes editing synchronously through native completion',async()=>{
 const x=run();assert.equal(x.root.inert,true);await flush();
 assert.equal(x.calls[0].args.allowed,true);x.resolve(true);await flush();assert.equal(x.root.inert,true);
});
test('failed command restores prior interaction state and explains blocked exit',async()=>{
 for(const inert of [false,true]){const x=run({inert});await flush();x.reject(Error('stale response'));await flush();
 assert.equal(x.root.inert,inert);assert.equal(x.events.at(-1),'un-avatar-close-failed');}
});
test('unavailable IPC does not leave the UI inert',async()=>{
 const x=run({missingApi:true});await flush();assert.equal(x.root.inert,false);
 assert.equal(x.events.at(-1),'un-avatar-close-failed');
});
test('late failure cannot unlock a newer close review; refusal restores original state',async()=>{
 const root={inert:false,dataset:{unCloseGuardReady:"true"}},events=[],requests=[];
 let dirty=false;
 const window={dispatchEvent(event){events.push(event.type);if(dirty&&event.type==='beforeunload')event.preventDefault();},
  __TAURI_INTERNALS__:{invoke(command,args){return new Promise((resolve,reject)=>requests.push({args,resolve,reject}));}}};
 const context={window,document:{documentElement:root},Event,Promise,console:{error(){}}};
 vm.runInNewContext(script,context);await flush();
 vm.runInNewContext(script.replace('const requestId = 42','const requestId = 43'),context);await flush();
 requests[0].reject(Error('stale'));await flush();assert.equal(root.inert,true);
 assert.equal(events.includes('un-avatar-close-failed'),false);
 dirty=true;
 vm.runInNewContext(script.replace('const requestId = 42','const requestId = 44'),context);await flush();
 assert.equal(root.inert,false);assert.equal(requests[2].args.allowed,false);
 requests[1].reject(Error('stale'));requests[2].resolve(false);await flush();assert.equal(root.inert,false);
});

test('unmounted or not-yet-mounted draft guard cannot approve native shutdown',async()=>{
 const x=run({ready:false});await flush();
 assert.equal(x.calls[0].args.allowed,false);
 assert.equal(x.root.inert,false);
 x.resolve(false);await flush();
});
