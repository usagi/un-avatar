import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const root = new URL('../src/',import.meta.url);
const compile = code => ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ES2022}}).outputText;
const url = code => 'data:text/javascript;base64,'+Buffer.from(compile(code)).toString('base64');
const presets = await readFile(new URL('lib/dynamicsPresets.ts',root),'utf8');
const rules = await import(url((await readFile(new URL('lib/profileFieldRules.ts',root),'utf8')).replace('"./dynamicsPresets"',JSON.stringify(url(presets)))));
const app = await readFile(new URL('App.svelte',root),'utf8');
const start = app.indexOf('\tasync function applyRuntimePhysicsUpdate(');
assert.ok(start>=0);
const body = app.slice(start,app.indexOf('\n\t}',start)+3);
const component=await readFile(new URL('lib/ProfileAdvancedPhysics.svelte',root),'utf8');
test('nested reactive collider arrays are converted before the save queue clones them',()=>{
 const begin=component.indexOf(' function persistRules('),end=component.indexOf('\n }',begin)+3;
 assert.ok(begin>=0);
 const writes=[];
 const persist=new Function('onUpdateSettingValue',compile(`const collider='physics.dynamics.solver.collider_augment_overrides';${component.slice(begin,end)};return persistRules;`))((field,value)=>writes.push({field,value:structuredClone(value)}));
 const rules=[{name:'cloth',source_id_contains:new Proxy(['skirt'],{}),collider_path_contains:new Proxy(['leg'],{})}];
 assert.throws(()=>structuredClone(rules));
 persist(rules);assert.deepEqual(writes[0].value,[{name:'cloth',source_id_contains:['skirt'],collider_path_contains:['leg']}]);
 rules[0].source_id_contains.push('tail');assert.deepEqual(writes[0].value[0].source_id_contains,['skirt']);
});
test('mesh assistance requires restart; other physics still applies live',async()=>{
 const fields=['physics.dynamics.solver.mesh_cloth_assist','physics.dynamics.solver.mesh_cloth_assist.enabled','physics.dynamics.solver.mesh_cloth_assist.max_assist_weight'];
 for(const field of fields){assert.equal(rules.canApplyWithoutRestart(field),false);assert.equal(rules.isLaunchTimeRendererField(field),true);}
 const calls=[];
 const apply = new Function('canApplyWithoutRestart','calls',compile(`
 const DYNAMICS_ENABLED_FIELD='dynamics_enabled',DYNAMICS_OVERRIDE_FIELD_PREFIX='physics.dynamics.solver.',DYNAMICS_BONE_COLLIDER_FIELD_PREFIX='physics.dynamics.bone_colliders.';
 const fieldSetIncludes=(fields,key)=>fields.includes(key),fieldSetStartsWith=(fields,prefix)=>fields.some(f=>f.startsWith(prefix));
 const applyRendererCommand=async(targets,command,payload)=>{calls.push({targets,command,payload:payload()});return 1;};
 const setAppliedMessage=async()=>{},$_=key=>key;
 ${body}
 return applyRuntimePhysicsUpdate;
 `))(rules.canApplyWithoutRestart,calls);
 await apply(fields,{id:'a'},[{id:7}]);assert.equal(calls.length,0,'must not claim mesh changes applied live');
 await apply(['physics.dynamics.solver.surface_constraints.enabled'],{id:'a'},[{id:7}]);
 assert.equal(calls.length,1);assert.deepEqual(calls[0].targets,[{id:7}]);
 assert.equal(calls[0].command,'set_renderer_dynamics');
});
