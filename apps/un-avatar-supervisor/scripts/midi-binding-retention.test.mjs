import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const source=await readFile(new URL('../src/lib/ProfileAvatarSection.svelte',import.meta.url),'utf8');
function fn(text,name){const start=text.indexOf(`\tasync function ${name}(`);assert.ok(start>=0);return text.slice(start,text.indexOf('\n\t}',start)+3);}
function harness(kind,body){
 const field=kind==='Wardrobe'?'set_id':'action_id',key=kind==='Wardrobe'?'wardrobe_bindings':'animator_bindings';
 const current={[field]:'target',kind:'midi_note',channel:2,note:60,device:'Keyboard'};
 const other={[field]:'peer',kind:'midi_note',channel:3,note:61};
 const writes=[];
 const code=ts.transpileModule(`let midiDraftVersion=0;${body};return update${kind}Midi;`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
 const update=new Function('setting',`${kind.toLowerCase()}MidiBindingFor`,'onUpdateSettingValue','midiDraftKey',`${kind.toLowerCase()}MidiDrafts`,code)({[key]:[current,other]},()=>current,(field,value)=>writes.push({field,value}),id=>id,new Map());
 return {update,writes,current,other};
}
for(const kind of ['Wardrobe','Animator']){
 const body=fn(source,`update${kind}Midi`);
 test(`${kind}: invalid numeric edits never remove a valid binding`,async()=>{
  for(const patch of [{channel:17},{channel:0},{channel:1.5},{note:-1},{note:128},{note:60.5},{note:NaN}]){
   const h=harness(kind,body);await h.update('target',patch);assert.equal(h.writes.length,0);assert.equal(h.current.note,60);
  }
 });
 test(`${kind}: valid replacement preserves the peer binding and zero note`,async()=>{
  const h=harness(kind,body);await h.update('target',{channel:16,note:0});assert.equal(h.writes.length,1);
  assert.deepEqual(h.writes[0].value[0],h.other);assert.equal(h.writes[0].value[1].note,0);assert.equal(h.current.note,60);
 });
}

for(const kind of ['Wardrobe','Animator']){
 test(`${kind}: incomplete input follows its own profile/model/target until saved`,async()=>{
  const lower=kind.toLowerCase(), field=kind==='Wardrobe'?'set_id':'action_id';
  const drafts=new Map(),writes=[];
  const setting={manifest_path:'profileA',avatar_path:'modelA',[`${lower}_bindings`]:[]};
  const body=fn(source,`update${kind}Midi`);
  const start=source.indexOf(`\tfunction ${lower}MidiBindingFor(`);
  const lookup=source.slice(start,source.indexOf('\n\t}',start)+3);
  const code=ts.transpileModule(`let midiDraftVersion=0;${lookup};${body};return update${kind}Midi;`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
  const key=id=>JSON.stringify([setting.manifest_path,setting.avatar_path,id]);
  const update=new Function('setting','onUpdateSettingValue','midiDraftKey',`${lower}MidiDrafts`,code)(setting,(field,value)=>writes.push(value),key,drafts);
  await update('target',{device:'Keyboard'});await update('target',{channel:7});
  assert.equal(writes.length,0);
  setting.manifest_path='profileB';await update('target',{note:61});
  assert.equal(writes.at(-1)[0].channel,1);
  setting.manifest_path='profileA';setting.avatar_path='modelB';await update('target',{note:62});
  assert.equal(writes.at(-1)[0].channel,1);
  setting.avatar_path='modelA';await update('peer',{note:63});
  assert.equal(writes.at(-1)[0].channel,1);
  await update('target',{note:0});
  assert.equal(writes.at(-1)[0].channel,7);assert.equal(writes.at(-1)[0].device,'Keyboard');
  assert.equal(drafts.size,0);
 });
}

for(const kind of ['Wardrobe','Animator']){
 test(`${kind}: clear removes only target MIDI and its incomplete draft`,async()=>{
  const lower=kind.toLowerCase(),field=kind==='Wardrobe'?'set_id':'action_id';
  const target={[field]:'target',kind:'midi_note',channel:2,note:60};
  const keyboard={[field]:'target',kind:'keyboard',binding:'F8'};
  const peer={[field]:'peer',kind:'midi_note',channel:3,note:61};
  const drafts=new Map([['target',{channel:7}],['peer',{channel:8}]]),writes=[];
  const code=ts.transpileModule(`let midiDraftVersion=0;${fn(source,`clear${kind}Midi`)};return clear${kind}Midi;`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
  const clear=new Function('setting','onUpdateSettingValue','midiDraftKey',`${lower}MidiDrafts`,code)({[`${lower}_bindings`]:[target,keyboard,peer]},(field,value)=>writes.push(value),id=>id,drafts);
  await clear('target');assert.deepEqual(writes[0],[keyboard,peer]);assert.equal(drafts.has('target'),false);assert.equal(drafts.has('peer'),true);
 });
}
