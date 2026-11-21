import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
import * as presets from '../src/lib/profilePresets.ts';
import { defaultTextureCompressionAdvanced } from '../src/lib/qualityOptions.ts';
import { defaultDynamicsCategoryOverrides, DYNAMICS_BONE_COLLIDER_FIELD_PREFIX } from '../src/lib/dynamicsPresets.ts';
import { BONE_COLLIDER_RADIUS_FIELD_PREFIX } from '../src/lib/boneColliderFields.ts';
const source=fs.readFileSync(new URL('../src/dev-ipc-mock.ts',import.meta.url),'utf8');
const ast=ts.createSourceFile('mock.ts',source,ts.ScriptTarget.Latest,true);
const functions=ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&['applyPreviewSettingValue','updatePreviewSettings'].includes(n.name.text));
assert.equal(functions.length,2);
const code=ts.transpileModule(functions.map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
function harness(settings=[{id:'a',name:'Alpha',color_exposure:0,window_width:800},{id:'b',name:'Beta',color_exposure:0,window_width:800}]){
 return new Function('avatarSettings','BONE_COLLIDER_RADIUS_FIELD_PREFIX','DYNAMICS_BONE_COLLIDER_FIELD_PREFIX',`${code}; return {
  get settings(){return avatarSettings;},
  update:args=>updatePreviewSettings(args.settingId,[{field:args.field,value:args.value}]),
  batch:updatePreviewSettings
 };`)(settings,BONE_COLLIDER_RADIUS_FIELD_PREFIX,DYNAMICS_BONE_COLLIDER_FIELD_PREFIX);
}

const fixture=ast.statements.filter(ts.isVariableStatement).flatMap(n=>n.declarationList.declarations).find(n=>n.name.getText(ast)==='avatarSettings');
assert(fixture);
const fixtureCode=ts.transpileModule(`const fixture=${fixture.initializer.getText(ast)};`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
function actualFixtures(){return new Function('defaultTextureCompressionAdvanced','defaultDynamicsCategoryOverrides','newAvatarDefaults',`${fixtureCode};return fixture;`)(defaultTextureCompressionAdvanced,defaultDynamicsCategoryOverrides,JSON.parse(fs.readFileSync(new URL('../src/fixtures/new-avatar-setting.json',import.meta.url),'utf8')));}
for(const [family,entries] of Object.entries(presets)) {
 for(const [name,updates] of Object.entries(entries)) {
  test(`existing preset ${family}/${name} can update the actual preview profile`,()=>{
   const h=harness(actualFixtures());
   const other=structuredClone(h.settings[1]);
   const reply=h.batch(h.settings[0].id,updates.map(([field,value])=>({field,value})));
   assert.deepEqual(reply,h.settings[0]);
   assert.deepEqual(h.settings[1],other);
   const lastValues=new Map(updates);
   const outputFields={
    'render_quality.aa':'aa', 'render_quality.texture_resolution_limit':'texture_resolution_limit',
    'output.spout2.width':'spout_width','output.spout2.height':'spout_height','output.spout2.enabled':'spout_enabled',
    'window.minimized':'minimized','window.width':'window_width','window.height':'window_height',
    'effects.avatar.outline.policy':'outline_policy','effects.avatar.outline.width':'outline_width',
    'environment.color.look':'color_look','effects.post.bloom.enabled':'bloom_enabled'
   };
   for(const [field,property] of Object.entries(outputFields)) if(lastValues.has(field)) assert.deepEqual(reply[property],lastValues.get(field));
   for(const [field,value] of lastValues){
    if(field==='camera.target_y') assert.equal(reply.camera_target[1],value);
    else if(field.startsWith('render_quality.texture_compression_advanced.')) assert.equal(reply.texture_compression_advanced[field.split('.').at(-1)],value);
   }
  });
 }
}
test('unsupported field and unknown target reject without changing any fixture',()=>{
 const h=harness(),before=structuredClone(h.settings);
	assert.throws(()=>h.batch('a',[]),/must not be empty/);
 assert.throws(()=>h.update({settingId:'a',field:'not_supported',value:1}),/unsupported field/);
 assert.throws(()=>h.update({settingId:'missing',field:'profile.display_name',value:'wrong'}),/not found/);
 assert.deepEqual(h.settings,before);
});

test('a preset batch commits together and rejection preserves all profiles',()=>{
 const h=harness(),before=structuredClone(h.settings);
 assert.throws(()=>h.batch('a',[
  {field:'environment.color.exposure',value:3},
  {field:'not_supported',value:1}
 ]),/unsupported field/);
 assert.deepEqual(h.settings,before);
 h.batch('a',[
  {field:'environment.color.exposure',value:3},
  {field:'window.width',value:1280}
 ]);
 assert.equal(h.settings[0].color_exposure,3);
 assert.equal(h.settings[0].window_width,1280);
 assert.deepEqual(h.settings[1],before[1]);
});
test('supported identity, explicit and direct fields update only the target',()=>{
 const h=harness(),other=structuredClone(h.settings[1]);
 h.update({settingId:'a',field:'profile.display_name',value:'Changed'});
 h.update({settingId:'a',field:'environment.color.exposure',value:2});
 const reply=h.update({settingId:'a',field:'window.width',value:1024});
 assert.equal(h.settings[0].name,'Changed');
 assert.equal(h.settings[0].color_exposure,2);
 assert.equal(h.settings[0].window_width,1024);
 assert.deepEqual(h.settings[1],other);
 reply.name='unsaved browser edit';
 assert.equal(h.settings[0].name,'Changed','Mock IPC replies must not expose mutable fixture references');
});

const archiveFunctions=ast.statements.filter(n=>ts.isFunctionDeclaration(n)&&['archivePreviewSetting','listPreviewArchives','restorePreviewSetting'].includes(n.name.text));
assert.equal(archiveFunctions.length,3);
const archiveCode=ts.transpileModule(archiveFunctions.map(n=>n.getText(ast)).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
function archiveHarness(){
 return new Function('avatarSettings',`const previewRenderers=[];const previewProfileArchives=new Map();let previewArchiveSequence=0;${archiveCode};return {archive:archivePreviewSetting,list:listPreviewArchives,restore:restorePreviewSetting,get settings(){return avatarSettings;}};`)(actualFixtures());
}
test('preview delete/restore retains complete settings, other profile and archive',()=>{
 const h=archiveHarness(),before=structuredClone(h.settings),id=before[0].id;
 h.archive(id);assert.deepEqual(h.settings,[before[1]]);const entry=h.list()[0];assert.equal(entry.profile_id,id);
 h.restore(entry.archive_id);assert.deepEqual(h.settings.find(p=>p.id===id),before[0]);assert.deepEqual(h.settings.find(p=>p.id===before[1].id),before[1]);
 assert.equal(h.list().length,1);
});
test('preview restore rejects duplicate ID, occupied destination and unknown archive without mutations',()=>{
 const h=archiveHarness(),first=structuredClone(h.settings[0]);h.archive(first.id);const entry=h.list()[0];
 const before=structuredClone(h.settings);assert.throws(()=>h.restore('unknown'),/archive not found/);assert.deepEqual(h.settings,before);
 h.settings.push({...first,id:'new-id'});assert.throws(()=>h.restore(entry.archive_id),/destination already exists/);h.settings.pop();
 h.restore(entry.archive_id);const restored=structuredClone(h.settings);assert.throws(()=>h.restore(entry.archive_id),/already exists/);assert.deepEqual(h.settings,restored);
});
test('preview archives retain snapshots across repeat deletion and isolate returned metadata',()=>{
 const h=archiveHarness(),id=h.settings[0].id,original=h.settings[0].name;
 assert.throws(()=>h.archive('missing'),/profile not found/);assert.equal(h.list().length,0);
 h.archive(id);const first=h.list()[0];first.name='metadata edit';h.restore(first.archive_id);assert.equal(h.settings.find(p=>p.id===id).name,original);
 h.settings.find(p=>p.id===id).name='later version';h.archive(id);assert.equal(h.list().length,2);
 h.restore(first.archive_id);assert.equal(h.settings.find(p=>p.id===id).name,original);
});
