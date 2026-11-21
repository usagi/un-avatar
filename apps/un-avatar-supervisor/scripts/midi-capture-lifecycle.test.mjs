import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/lib/ProfileAvatarSection.svelte',import.meta.url),'utf8');
function extract(name,async=true){const start=source.indexOf(`\t${async?'async ':''}function ${name}(`);assert.ok(start>=0);return source.slice(start,source.indexOf('\n\t}',start)+3);}
function harness(initial={}){
 const calls=[],writes=[],pending=[],cancelled=[];
 const code=ts.transpileModule(`let midiCaptureRequestId=0,midiCaptureToken=null,midiCaptureSetId=initial.set??null,midiCaptureActionId=initial.action??null,midiCaptureError='',busy=initial.busy??false,animatorRequestId=0;
 ${extract('captureWardrobeMidi')} ${extract('captureAnimatorMidi')}
 ${extract('cancelWardrobeMidiCapture',false)} ${extract('releaseMidiCapture',false)} ${extract('invalidatePendingAvatarReads',false)}
 return {wardrobe:captureWardrobeMidi,animator:captureAnimatorMidi,cancel:cancelWardrobeMidiCapture,destroy:invalidatePendingAvatarReads,state:()=>({set:midiCaptureSetId,action:midiCaptureActionId,error:midiCaptureError})};`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
 const methods=new Function('initial','hasTauriRuntime','invoke','updateWardrobeMidi','updateAnimatorMidi',code)(initial,()=>true,(command,args)=>{
  calls.push({command,args});if(command==='cancel_midi_note_capture'){cancelled.push(args.captureId);return Promise.resolve();}
  return new Promise((resolve,reject)=>pending.push({command,resolve,reject}));
 },(id,value)=>writes.push({kind:'wardrobe',id,value}),(id,value)=>writes.push({kind:'animator',id,value}));
 return {...methods,calls,writes,pending,cancelled};
}
async function started(h,kind){const done=h[kind]('target');h.pending[0].resolve('reservation-1');await Promise.resolve();assert.equal(h.pending[1].command,'capture_midi_note_binding');return {done};}
for(const kind of ['wardrobe','animator']){
 test(`${kind}: active capture or save prevents a second native request`,async()=>{
  for(const initial of [{set:'old'},{action:'old'},{busy:true}]){const h=harness(initial);const request=h[kind]('new');assert.equal(h.calls.length,0);await request;}
 });
 test(`${kind}: cancellation before reservation returns releases it without opening input`,async()=>{
  const h=harness();const done=h[kind]('target');h.cancel();h.pending[0].resolve('reservation-1');await done;
  assert.deepEqual(h.writes,[]);assert.deepEqual(h.cancelled,['reservation-1']);assert.equal(h.pending.length,1);
 });
 test(`${kind}: cancellation and destruction ignore late notes and release native input`,async()=>{
  for(const cancel of ['cancel','destroy']){const h=harness();const {done}=await started(h,kind);h[cancel]();h.pending[1].resolve({device:'fixture',channel:1,note:0});await done;
   assert.deepEqual(h.writes,[]);assert.ok(h.cancelled.every(id=>id==='reservation-1'));assert.ok(h.cancelled.length>0);assert.deepEqual(h.state(),{set:null,action:null,error:''});}
 });
 test(`${kind}: completion writes target including note zero and releases reservation`,async()=>{
  const h=harness();const {done}=await started(h,kind);h.pending[1].resolve({device:'fixture',channel:16,note:0});await done;
  assert.deepEqual(h.writes,[{kind,id:'target',value:{device:'fixture',channel:16,note:0}}]);assert.deepEqual(h.cancelled,['reservation-1']);assert.deepEqual(h.state(),{set:null,action:null,error:''});
 });
 test(`${kind}: native failure releases reservation and permits retry`,async()=>{
  const h=harness();const {done}=await started(h,kind);h.pending[1].reject('no device');await done;assert.equal(h.state().error,'no device');assert.deepEqual(h.cancelled,['reservation-1']);
  const retry=h[kind]('target');h.pending[2].reject('reservation failed');await retry;assert.equal(h.state().error,'reservation failed');
 });
}

test('late completion of cancelled wardrobe capture cannot clear or write the new animator target',async()=>{
 const h=harness();const {done:old}=await started(h,'wardrobe');h.cancel();const next=h.animator('new-target');
 h.pending[2].resolve('reservation-2');await Promise.resolve();h.pending[1].resolve({device:'old',channel:1,note:60});await old;
 assert.equal(h.state().action,'new-target');assert.deepEqual(h.writes,[]);
 assert.ok(!h.cancelled.includes('reservation-2'));
 h.pending[3].resolve({device:'new',channel:2,note:61});await next;
 assert.deepEqual(h.writes,[{kind:'animator',id:'new-target',value:{device:'new',channel:2,note:61}}]);
});
