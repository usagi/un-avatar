import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
const src=readFileSync(new URL('../src/dev-ipc-mock.ts',import.meta.url),'utf8');
const ast=ts.createSourceFile('mock.ts',src,ts.ScriptTarget.Latest,true);
const fn=ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='withPreviewProfileRevision');
const code=ts.transpileModule(fn.getText(ast),{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;
for(const cmd of ['update_avatar_setting_value','update_avatar_setting_values','save_avatar_thumbnail_icon','save_profile_icon_from_data_url','delete_avatar_setting'])test(`${cmd} preview rejects stale and unknown target before mutation`,()=>{
 const settings=[{id:'a',revision:'r0',value:0},{id:'b',revision:'b0',value:3}];let calls=0;
 const run=new Function('avatarSettings',code+';return withPreviewProfileRevision;')(settings);
 const apply=()=>{calls++;settings[0].value++;return structuredClone(settings[0])};
 assert.throws(()=>run(cmd,{settingId:'a',expectedRevision:'stale'},apply),/profile_conflict/);
 assert.throws(()=>run(cmd,{settingId:'missing'},apply),/not found/);assert.equal(calls,0);
 const result=run(cmd,{settingId:'a',expectedRevision:'r0'},apply);assert.equal(calls,1);
 if(cmd!=='delete_avatar_setting'){assert.notEqual(result.revision,'r0');assert.equal(result.revision,settings[0].revision);}
 assert.deepEqual(settings[1],{id:'b',revision:'b0',value:3});
});
