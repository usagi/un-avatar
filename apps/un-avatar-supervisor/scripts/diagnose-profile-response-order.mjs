// Offline fault-injection probe. This is not a real IPC/runtime reproduction.
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import ts from 'typescript';
const source = await readFile(new URL('../src/App.svelte', import.meta.url), 'utf8');
function body(name) {
  let start = source.indexOf(`\tasync function ${name}(`);
  if (start < 0) start = source.indexOf(`\tfunction ${name}(`);
  assert.ok(start >= 0, name);
  return source.slice(start, source.indexOf('\n\t}', start) + 3);
}
const requests = [];
const nativeInvoke = (command, args) => new Promise((resolve) => requests.push({command, args, resolve}));
const ordered = process.argv.includes('--ordered');
const orderingSource = await readFile(new URL('../src/lib/orderedProfileInvoke.ts', import.meta.url), 'utf8');
const orderingCode = ts.transpileModule(orderingSource, {compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText.replace('export function', 'function');
const wrap = new Function(orderingCode+'\nreturn orderedProfileInvoke;')();
const invoke = ordered ? wrap(nativeInvoke) : nativeInvoke;
const code = `
let selectedSettingId='a', launchTargetId='a', message='';
let avatarSettings=[{id:'a',manifest_path:'a.toml',camera:{longitude_deg:0,latitude_deg:0}}, {id:'b',manifest_path:'b.toml'}];
const pendingProfileSettingUpdates=new Map();let failedProfileSettingUpdates=[];
const profileSettingUpdateCoalesceMs=100,window={setTimeout:()=>1,clearTimeout:()=>{}};
const avatarSettingBySelectedId=id=>avatarSettings.find(s=>s.id===id);
const canApplyWithoutRestart=()=>true, $_=s=>s;
const compareAvatarSettings=(a,b)=>a.id.localeCompare(b.id);
const pickInitialLaunchTargetId=current=>current;
const applyRuntimeProfileUpdate=async()=>{},queueRendererRestart=()=>{};
${['replaceAvatarSetting','profileSettingUpdateKey','scheduleCoalescedProfileSettingUpdate','flushCoalescedProfileSettingUpdate'].map(body).join('\n')}
return {
 schedule:(field,value)=>scheduleCoalescedProfileSettingUpdate(avatarSettings[0],field,value),
 flush:field=>flushCoalescedProfileSettingUpdate('a\\n'+field),
 state:()=>structuredClone(avatarSettings)
};`;
const h = new Function('invoke', ts.transpileModule(code, {compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText)(invoke);
h.schedule('camera.longitude_deg', 30);
const first = h.flush('camera.longitude_deg');
h.schedule('camera.latitude_deg', 20);
const second = h.flush('camera.latitude_deg');
await new Promise(resolve=>setImmediate(resolve));
assert.equal(requests.length, ordered ? 1 : 2);
// Model sequential successful disk writes whose IPC responses arrive reversed.
const latest = {id:'a',manifest_path:'a.toml',camera:{longitude_deg:30,latitude_deg:20}};
if (ordered) {
 requests[0].resolve({...latest,camera:{longitude_deg:30,latitude_deg:0}});await first;
 await new Promise(resolve=>setImmediate(resolve));
 assert.equal(requests.length,2);
 requests[1].resolve(latest);await second;
 assert.deepEqual(h.state()[0].camera,latest.camera);
} else {
 requests[1].resolve(latest);await second;
 assert.deepEqual(h.state()[0].camera, latest.camera);
 requests[0].resolve({...latest,camera:{longitude_deg:30,latitude_deg:0}});await first;
}
const visible=h.state()[0];
assert.equal(h.state()[1].id,'b');
console.log(JSON.stringify({
 source_sha256:createHash('sha256').update(source).digest('hex'),
 ordered,
 scope:'Actual frontend functions with injected response order; no native IPC, disk, renderer or network',
 expected:latest.camera,observed:visible.camera,
 stale_response_overwrites_newer_field:visible.camera.latitude_deg!==latest.camera.latitude_deg,
 next:ordered ? 'Verify native IPC and renderer behavior separately; this probe covers frontend snapshots only.' : 'Run with --ordered to verify the production ordering wrapper prevents this response order.'
},null,2));
