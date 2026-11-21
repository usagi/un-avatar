import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import ts from 'typescript';

const source = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
function body(name) {
  const start = source.indexOf(`\tasync function ${name}(`);
  assert.ok(start >= 0);
  return source.slice(start, source.indexOf('\n\t}', start) + 3);
}
function harness() {
  const calls = [], renderer = { id: 2, name: 'A', transparent: false };
  let fail = true;
  const invoke = async (command, payload) => {
    calls.push({ command, payload });
    if (fail) throw Error('window unavailable');
  };
  const code = `
    let message = '', runtimeCommandFailures = [{rendererId:9,command:'other',error:'keep'}];
    const runtimeCommandGenerations = new Map(), profileRuntimeApplyTails = new Map();
    const $_ = key => key;
    const rendererWindowPayload = setting => ({width:setting.width});
    const refreshRendererRuntimeViewAfterApply = async () => calls.push('refresh');
    const queueTransparentEnableRestart = r => calls.push({restart:r.id});
    const fieldSetHas = (fields, test) => fields.some(test);
    const isRuntimeWindowField = field => field.startsWith('window.');
    const rendererForSetting = () => renderer;
    const renderersForSetting = () => [renderer];
    const withDeferredRuntimeRefresh = callback => callback();
    ${['BackgroundColor','MotionUpdate','PhysicsUpdate','AvatarEffectsUpdate','PostEffectsUpdate','EnvironmentUpdate','CameraUpdate'].map(name => `const applyRuntime${name} = async () => calls.push('${name}');`).join('\n')}
    ${body('applyRendererCommand')}
    ${body('applyRuntimeWindowUpdate')}
    ${body('applyRuntimeProfileUpdates')}
    return {apply:applyRuntimeProfileUpdates, failures:()=>runtimeCommandFailures};
  `;
  const h = new Function('invoke', 'calls', 'renderer', 'console', ts.transpileModule(code, {compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke, calls, renderer, {warn(){}});
  return {...h, calls, recover(){fail=false;}};
}

test('window failure retains recipient evidence and does not abort remaining saved settings', async () => {
  const h = harness(), setting = {id:'a',width:800};
  await h.apply([['window.transparent',true],['camera.radius',3]], setting, setting);
  assert.deepEqual(h.calls[0], {command:'set_renderer_window',payload:{id:2,width:800}});
  assert.ok(h.calls.includes('PhysicsUpdate'));
  assert.ok(h.calls.includes('CameraUpdate'));
  assert.equal(h.calls.some(call => call?.restart), false);
  assert.equal(h.failures().find(f => f.rendererId === 2)?.error, 'Error: window unavailable');
  h.recover();
  await h.apply([['window.transparent',true]], setting, setting);
  assert.deepEqual(h.failures(), [{rendererId:9,command:'other',error:'keep'}]);
  assert.ok(h.calls.some(call => call?.restart === 2));
});

test('queued profile application does not follow replacement runtimes or editor selection', async () => {
  const sent = [];
  const old = {id:2,profile_id:'a',transparent:true};
  let targets = [old], release;
  const gate = new Promise(resolve => {release=resolve;});
  const code = `
    const profileRuntimeApplyTails = new Map([['a',gate]]);
    const fieldSetHas = fields => fields.some(f=>f.startsWith('window.'));
    const isRuntimeWindowField = () => true;
    const rendererForSetting = () => getTargets()[0] ?? null;
    const renderersForSetting = () => [...getTargets()];
    const withDeferredRuntimeRefresh = callback => callback();
    const applyRuntimeWindowUpdate = async (setting,r) => {if(r)sent.push(['window',r.id]);};
    const applyRuntimeCameraUpdate = async (fields,setting,rs) => sent.push(['camera',...rs.map(r=>r.id)]);
    ${['BackgroundColor','MotionUpdate','PhysicsUpdate','AvatarEffectsUpdate','PostEffectsUpdate','EnvironmentUpdate'].map(name => `const applyRuntime${name}=async()=>{};`).join('\n')}
    ${body('applyRuntimeProfileUpdates')}
    return applyRuntimeProfileUpdates;
  `;
  const apply = new Function('gate','getTargets','sent',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(gate,()=>targets,sent);
  const pending = apply([['window.width',800],['camera.radius',3]],{id:'a'},{id:'a'});
  // Renderer 2 has exited; the same profile now runs as 3, while another
  // selected profile runs as 4. Neither is a recipient of the queued edit.
  targets = [{id:3,profile_id:'a'},{id:4,profile_id:'b'}];
  release();
  await pending;
  assert.deepEqual(sent,[['window',2],['camera',2]]);
});


test('profile apply selects every live matching identity without following colliding manifest paths', async () => {
  const start = source.indexOf('\tfunction renderersForSetting(');
  assert.ok(start >= 0);
  const select = source.slice(start,source.indexOf('\n\t}',start)+3);
  const sent=[];
  const code = `
    const renderers = [
      {id:1,profile_id:'a',manifest_path:'same',state:'running'},
      {id:2,profile_id:'a',manifest_path:'other',state:'running'},
      {id:3,profile_id:'b',manifest_path:'same',state:'running'},
      {id:4,profile_id:'a',manifest_path:'same',state:'stopped'},
      {id:5,profile_id:null,manifest_path:'same',state:'running'}
    ];
    const isLiveRenderer = r=>r.state==='running';
    const sameNormalizedPath = (a,b)=>a===b;
    ${select}
    const rendererForSetting = setting=>renderersForSetting(setting)[0]??null;
    const profileRuntimeApplyTails = new Map();
    const fieldSetHas = (fields,predicate)=>fields.some(predicate);
    const isRuntimeWindowField = f=>f.startsWith('window.');
    const withDeferredRuntimeRefresh = callback=>callback();
    const applyRuntimeWindowUpdate = async (_setting,r)=>{if(r)sent.push(['window',r.id]);};
    ${['BackgroundColor','MotionUpdate','PhysicsUpdate','AvatarEffectsUpdate','PostEffectsUpdate','EnvironmentUpdate','CameraUpdate'].map(name=>`const applyRuntime${name}=async (_fields,_setting,targets)=>sent.push(['${name}',...targets.map(r=>r.id)]);`).join('\n')}
    ${body('applyRuntimeProfileUpdates')}
    return applyRuntimeProfileUpdates;
  `;
  const apply = new Function('sent',ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(sent);
  await apply([['window.width',640],['camera.radius',3]],{id:'a',manifest_path:'same'},{id:'a',manifest_path:'same'});
  assert.deepEqual(sent[0],['window',1]); // Existing policy; changing this awaits the scope decision.
  for(const command of sent.slice(1)) assert.deepEqual(command.slice(1),[1,2,5]);
});
