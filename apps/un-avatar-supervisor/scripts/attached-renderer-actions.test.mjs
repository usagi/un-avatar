import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import ts from 'typescript';
const source=fs.readFileSync(new URL('../src/lib/RendererStageActions.svelte',import.meta.url),'utf8');
const ast=ts.createSourceFile('actions.ts',source.match(/<script lang="ts">([\s\S]*?)<\/script>/)[1],ts.ScriptTarget.Latest,true);
const fn=ast.statements.find(n=>ts.isFunctionDeclaration(n)&&n.name?.text==='stageActionDisabled');
const code=ts.transpileModule(fn.getText(ast),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const disabled=new Function(code+';return stageActionDisabled')();
test('attached live Renderer permits protocol controls without an owned PID',()=>{
 const r={id:1,pid:null,manifest_path:'isolated.toml',state:'Running'};
 for(const action of ['stop','resetCamera','screenshot','restart'])assert.equal(disabled(action,r,false),false,action);
 assert.equal(disabled('activate',r,false),true);
});
test('timed-out stop remains retryable; completed or busy instances cannot be stopped',()=>{
 const r={id:1,pid:null,manifest_path:'isolated.toml',state:'Stopping'};
 assert.equal(disabled('stop',r,false),false);
 for(const action of ['resetCamera','screenshot'])assert.equal(disabled(action,r,false),true);
 for(const state of ['Exited','Crashed'])assert.equal(disabled('stop',{...r,state},false),true);
 for(const action of ['stop','resetCamera','screenshot','restart','activate'])assert.equal(disabled(action,{...r,pid:123,state:'Running'},true),true);
});

const labelSource=fs.readFileSync(new URL('../src/lib/runtimeLabels.ts',import.meta.url),'utf8');
const labelAst=ts.createSourceFile('labels.ts',labelSource,ts.ScriptTarget.Latest,true);
const functions=labelAst.statements.filter(n=>ts.isFunctionDeclaration(n)&&['rendererHealthKind','rendererHealthLabel'].includes(n.name?.text));
const labelCode=ts.transpileModule(functions.map(n=>n.getText(labelAst).replace(/^export /,'')).join('\n'),{compilerOptions:{target:ts.ScriptTarget.ESNext}}).outputText;
const healthLabel=new Function(labelCode+';return rendererHealthLabel')();
test('stopping takes precedence over stale startup telemetry in summary and table labels',()=>{
 const labels={pending:'起動中',stopping:'停止中',connected:'接続済み',idle:'終了',crashed:'異常終了'};
 const stale={connected:false,startup_message:'Loading model'};
 assert.equal(healthLabel({state:'Stopping'},stale,labels),'停止中');
 assert.equal(healthLabel({state:'Stopping'},null,labels),'停止中');
 assert.equal(healthLabel({state:'Starting'},stale,labels),'Loading model');
 assert.equal(healthLabel({state:'Exited'},stale,labels),'終了');
 for(const name of ['RendererStageChips.svelte','RendererProcessStatusCell.svelte']){
  assert.match(fs.readFileSync(new URL('../src/lib/'+name,import.meta.url),'utf8'),/stopping: \$_\("renderers\.state\.stopping"\)/);
 }
});
