import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

const source = fs.readFileSync(new URL('../src/dev-ipc-mock.ts', import.meta.url), 'utf8');
const ast = ts.createSourceFile('mock.ts', source, ts.ScriptTarget.Latest, true);
let launch;
function visit(node) {
  if (ts.isCaseClause(node) && node.expression.getText(ast) === '"launch_renderer"') launch = node.statements.map(s => s.getText(ast)).join('\n');
  ts.forEachChild(node, visit);
}
visit(ast);
const archive = ast.statements.find(n => ts.isFunctionDeclaration(n) && n.name.text === 'archivePreviewSetting').getText(ast);
assert.ok(launch);
function harness() {
  const code = `let avatarSettings=[{id:'a',name:'A',allow_multiple_renderers:false},{id:'b',name:'B',allow_multiple_renderers:true}];
  const previewRenderers=[], previewProfileArchives=new Map();let previewRendererSequence=0,previewArchiveSequence=0;
  ${archive}
  function launchRenderer(args){${launch}}
  return {launchRenderer,archivePreviewSetting,previewRenderers,previewProfileArchives,profiles:()=>avatarSettings};`;
  return new Function(ts.transpileModule(code,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)();
}
test('single-instance setting reuses active identity and returns an independent snapshot',()=>{
  const h=harness(), first=h.launchRenderer({settingId:'a'}), second=h.launchRenderer({settingId:'a'});
  assert.equal(first.id,second.id);assert.equal(h.previewRenderers.length,1);
  second.name='changed';assert.equal(h.previewRenderers[0].name,'A');
  h.previewRenderers[0].state='Exited';assert.notEqual(h.launchRenderer({settingId:'a'}).id,first.id);
});
test('multiple-instance setting creates separate identities without touching another profile',()=>{
  const h=harness();const a=h.launchRenderer({settingId:'a'});
  const b=h.launchRenderer({settingId:'b'}), b2=h.launchRenderer({settingId:'b'});
  assert.notEqual(b.id,b2.id);assert.equal(h.previewRenderers[0].id,a.id);
});
test('live profile deletion preserves profile and archive; stopped profile can be archived',()=>{
  const h=harness();h.launchRenderer({settingId:'a'});
  assert.throws(()=>h.archivePreviewSetting('a'),/in use/);
  assert.equal(h.profiles().length,2);assert.equal(h.previewProfileArchives.size,0);
  h.previewRenderers[0].state='Exited';h.archivePreviewSetting('a');
  assert.deepEqual(h.profiles().map(p=>p.id),['b']);assert.equal(h.previewProfileArchives.size,1);
});

const rendererOnlyCommands = [
  'set_renderer_apply_vmc_root_translation',
  'set_renderer_avatar_outline',
  'set_renderer_bloom',
  'set_renderer_contact_shadow',
  'set_renderer_environment_color',
  'set_renderer_lighting',
  'set_renderer_look_at',
  'set_renderer_ssao',

  'activate_renderer_window', 'reset_renderer_camera',
  'set_renderer_expression_override', 'clear_renderer_expression_overrides',
  'activate_renderer_runtime_action', 'set_renderer_runtime_parameter',
  'set_renderer_clear_color', 'set_renderer_camera_lock', 'set_renderer_camera_state',
  'set_renderer_show_axes', 'set_renderer_show_bone_colliders', 'set_renderer_spout_output',
  'set_renderer_motion_receivers', 'set_renderer_dynamics', 'set_renderer_dynamics_enabled',
  'set_renderer_window',
];
function actualCaseBody(command) {
  let result;
  function find(node) {
    if (ts.isCaseBlock(node)) {
      const index = node.clauses.findIndex(c => ts.isCaseClause(c) && c.expression.text === command);
      if (index >= 0) {
        const statements=[];
        for(let i=index;i<node.clauses.length;i++) {
          statements.push(...node.clauses[i].statements);
          if (statements.length) break;
        }
        result=statements.map(s=>s.getText(ast)).join('\n');
      }
    }
    ts.forEachChild(node,find);
  }
  find(ast); assert.ok(result,command);
  return new Function('args',ts.transpileModule(result,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText);
}
for (const command of rendererOnlyCommands) {
  test(`preview does not claim real Renderer effect: ${command}`,()=>{
    assert.throws(()=>actualCaseBody(command)({rendererId:'preview-a'}),/実Renderer.*実行していません/);
  });
}
