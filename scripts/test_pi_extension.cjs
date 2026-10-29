'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { registerCaller, registerRunner, exchange } = require('../plugins/pi/bridge.cjs');

function fixture(t, behavior = '') {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'akasha-pi-extension-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const log = path.join(root, 'requests.jsonl');
  const binary = path.join(root, 'bridge');
  fs.writeFileSync(binary, `#!${process.execPath}\n'use strict';
const fs = require('node:fs');
let text = '';
process.stdin.on('data', b => text += b);
process.stdin.on('end', () => {
  const event = JSON.parse(text);
  fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify(event) + '\\n');
  ${behavior}
  const data = event.op === 'register' ? {registration:'a'.repeat(64)} :
    event.op === 'sync' ? {id:'pi-capture/test',state:'queued'} : {invocation_id:event.call.session_id,tip:event.call.turn_id};
  console.log(JSON.stringify({schema_version:1,sequence:1,request_id:'native-request',data}));
});\n`, { mode: 0o700 });
  const defaults = { akasha_binary: binary, pi_binary: '/native/pi', socket: path.join(root, 'daemon.sock') };
  const agent = path.join(root, 'agent'); fs.mkdirSync(agent);
  const transcript = path.join(agent, 'session.jsonl');
  const notices = [], hooks = {}, tools = {};
  let leaf = 'assistant', session = 'session-a';
  const branch = [{ type: 'message', id: 'assistant', message: { role: 'assistant', content: [
    {type:'toolCall', id:'call-a', name:'akasha_consult', arguments:{hint:'preferences'}},
  ]}}];
  const ctx = { cwd:root, sessionManager: {
    getSessionId: () => session, getSessionFile: () => transcript,
    getLeafId: () => leaf, getBranch: () => branch,
  }, ui: { notify: (...args) => notices.push(args), setStatus: (...args) => notices.push(args) } };
  const pi = { on: (name, fn) => { hooks[name] = fn; }, registerTool: tool => { tools[tool.name] = tool; } };
  return {root, agent, transcript, defaults, ctx, pi, hooks, tools, branch, notices,
    leaf: value => { leaf = value; }, session: value => { session = value; },
    requests: () => fs.existsSync(log) ? fs.readFileSync(log, 'utf8').trim().split('\n').map(JSON.parse) : []};
}

test('lifecycle reads only persisted identity and resumes with a fresh extension runtime', async t => {
  const f = fixture(t);
  registerCaller(f.pi, () => f.agent, f.defaults, {type:'object'});
  assert.deepEqual(Object.keys(f.tools), ['akasha_consult']);
  await f.hooks.session_start({reason:'startup'}, f.ctx);
  await f.hooks.input({text:'PRIVATE_INPUT', source:'interactive'}, f.ctx);
  await f.hooks.message_end({message:{content:'BEFORE_OTHER_EXTENSION_TRANSFORM'}}, f.ctx);
  assert.deepEqual(f.requests(), []);
  fs.writeFileSync(f.transcript, 'CANONICAL_OTHER_EXTENSION_TRANSFORM\n');
  await f.hooks.turn_end({}, f.ctx);
  const requests = f.requests();
  assert.deepEqual(requests.map(r => r.op), ['register', 'sync']);
  assert.equal(requests[0].session_id, 'session-a');
  assert.equal(requests[0].transcript_path, f.transcript);
  assert.equal(requests[1].until_entry, 'assistant');
  assert(!JSON.stringify(requests).includes('PRIVATE_INPUT'));
  assert(!JSON.stringify(requests).includes('EXTENSION_TRANSFORM'));
  assert.equal(fs.readFileSync(f.transcript, 'utf8'), 'CANONICAL_OTHER_EXTENSION_TRANSFORM\n');
  f.leaf('compaction');
  await f.hooks.session_compact({}, f.ctx);
  f.leaf('selected-branch');
  await f.hooks.session_tree({}, f.ctx);
  await f.hooks.agent_settled({}, f.ctx);
  await f.hooks.session_shutdown({reason:'reload'}, f.ctx);
  registerCaller(f.pi, () => f.agent, f.defaults, {type:'object'});
  await f.hooks.session_start({reason:'reload'}, f.ctx);
  f.session('fork-session'); f.leaf('fork-leaf');
  await f.hooks.session_start({reason:'fork'}, f.ctx);
  assert.equal(f.requests().filter(r => r.op === 'register').length, 3);
  assert.equal(f.requests().at(-1).until_entry, 'fork-leaf');
  assert(!Object.hasOwn(f.hooks, 'agent_end'));
});

test('tool execution binds the unique native call, rejecting nested and model authority fields', async t => {
  const f = fixture(t); fs.writeFileSync(f.transcript, 'persisted');
  registerCaller(f.pi, () => f.agent, f.defaults, {type:'object'});
  const execute = (id, args) => f.tools.akasha_consult.execute(id, args, undefined, undefined, f.ctx);
  const result = await execute('call-a', {hint:'preferences'});
  const body = JSON.parse(result.content[0].text);
  assert.equal(body.data.invocation_id, 'session-a');
  assert.equal(body.data.tip, 'assistant');
  const call = f.requests().at(-1).call;
  assert.deepEqual(call, {session_id:'session-a', leaf_id:'assistant', turn_id:'assistant', tool_call_id:'call-a',
    agent_dir:f.agent, transcript:f.transcript, cwd:f.root});
  assert(!JSON.stringify(result).includes('a'.repeat(64)));
  const count = f.requests().length;
  for (const [id, args] of [['call-a/1', {}], ['call-a', {session_id:'forged'}], ['call-a', {hint:'x'.repeat(513)}]]) {
    const denied = await execute(id, args);
    assert(JSON.parse(denied.content[0].text).error);
  }
  f.branch.push(f.branch[0]);
  assert(JSON.parse((await execute('call-a', {})).content[0].text).error);
  assert.equal(f.requests().length, count);
});

test('Caller preserves opaque native Responses IDs and shared ID boundaries', async t => {
  const f = fixture(t); fs.writeFileSync(f.transcript, 'persisted');
  registerCaller(f.pi, () => f.agent, f.defaults, {type:'object'});
  const execute = id => f.tools.akasha_consult.execute(id, {}, undefined, undefined, f.ctx);
  for (const id of ['call_abc|fc_def', '😀'.repeat(64)]) {
    f.branch[0].message.content[0].id = id;
    assert.equal(JSON.parse((await execute(id)).content[0].text).error, undefined);
    assert.equal(f.requests().at(-1).call.tool_call_id, id);
  }
  const count = f.requests().length;
  for (const id of ['', 'call\u0000id', 'a'.repeat(257), '😀'.repeat(65)]) {
    f.branch[0].message.content[0].id = id;
    assert.equal(JSON.parse((await execute(id)).content[0].text).error.code, 'CALLER_AMBIGUOUS');
  }
  f.branch[0].message.content[0].id = 'call_abc|fc_def';
  for (const id of ['call_abc', 'call_abc|fc_def/child']) {
    assert.equal(JSON.parse((await execute(id)).content[0].text).error.code, 'CALLER_AMBIGUOUS');
  }
  f.branch.push(f.branch[0]);
  assert.equal(JSON.parse((await execute('call_abc|fc_def')).content[0].text).error.code, 'CALLER_AMBIGUOUS');
  assert.equal(f.requests().length, count);
});

test('Caller accepts 512 Unicode characters and rejects 513 before binding', async t => {
  const f = fixture(t); fs.writeFileSync(f.transcript, 'persisted');
  registerCaller(f.pi, () => f.agent, f.defaults, {type:'object'});
  const hint = '😀'.repeat(512);
  f.branch[0].message.content[0].arguments = {hint};
  const execute = args => f.tools.akasha_consult.execute('call-a', args, undefined, undefined, f.ctx);
  const accepted = JSON.parse((await execute({hint})).content[0].text);
  assert.equal(accepted.error, undefined);
  assert.equal(accepted.data.invocation_id, 'session-a');
  assert.deepEqual(f.requests().map(r => r.op), ['register', 'consult']);
  assert.equal(f.requests().at(-1).args.hint, hint);
  const refused = JSON.parse((await execute({hint: hint + '😀'})).content[0].text);
  assert.equal(refused.error.code, 'INVALID_ARGUMENT');
  assert.equal(f.requests().length, 2);
});

test('lifecycle errors expose only codes and are retried against persisted history', async t => {
  const f = fixture(t, `if (event.op === 'sync') { console.error(JSON.stringify({error:{code:'READ_DENIED',message:'PRIVATE_SENTINEL'}})); process.exit(1); }`);
  fs.writeFileSync(f.transcript, 'persisted');
  registerCaller(f.pi, () => f.agent, f.defaults, {type:'object'});
  await f.hooks.session_start({reason:'resume'}, f.ctx);
  await f.hooks.session_shutdown({reason:'quit'}, f.ctx);
  assert.equal(f.requests().filter(r => r.op === 'sync').length, 2);
  assert(JSON.stringify(f.notices).includes('READ_DENIED'));
  assert(!JSON.stringify(f.notices).includes('PRIVATE_SENTINEL'));
});

test('bridge cancellation reaps its child and rejects bounded malformed replies', async t => {
  for (const behavior of ["console.log('not json'); process.exit(0);", "process.stdout.write('x'.repeat(2097153)); return;"]) {
    const f = fixture(t, behavior);
    await assert.rejects(exchange(f.defaults, {op:'register'}), /PROTOCOL_INVALID|OUTPUT_LIMIT/);
  }
  const f = fixture(t);
  // Replace only our owned executable before launching it, then await its readiness marker.
  const pidFile = path.join(f.root, 'child.pid');
  fs.writeFileSync(f.defaults.akasha_binary, `#!${process.execPath}\nrequire('node:fs').writeFileSync(${JSON.stringify(pidFile)}, String(process.pid)); process.stdin.resume(); setInterval(() => {}, 1000);\n`);
  const abort = new AbortController();
  const result = exchange(f.defaults, {op:'register'}, abort.signal);
  const rejected = assert.rejects(result, /CANCELLED/);
  const end = Date.now() + 5000;
  while (!fs.existsSync(pidFile)) { assert(Date.now() < end); await new Promise(r => setTimeout(r, 10)); }
  const pid = Number(fs.readFileSync(pidFile, 'utf8'));
  abort.abort(); await rejected;
  assert.throws(() => process.kill(pid, 0), {code:'ESRCH'});
});

test('runner registers only context tools and attests real inventories before work', async t => {
  const previous = process.env.AKASHA_RUN_DEADLINE_MS;
  process.env.AKASHA_RUN_DEADLINE_MS = String(Date.now() + 30000);
  t.after(() => { if (previous === undefined) delete process.env.AKASHA_RUN_DEADLINE_MS; else process.env.AKASHA_RUN_DEADLINE_MS = previous; });
  const f = fixture(t, `if (event.op === 'ready' || event.op === 'context' || event.op === 'usage') {
    if (process.argv.at(-1) !== 'pi-context') process.exit(2);
    console.log(JSON.stringify({schema_version:1,sequence:1,request_id:'native-request',data:event.op === 'ready' ? {ready:true} : event.op === 'usage' ? {accepted:true} : {kind:'description'}})); return;
  }`);
  const names = ['code_read', 'code_search', 'context_describe', 'context_list', 'context_read', 'context_search', 'delegate', 'memory_get', 'memory_search'];
  f.pi.getAllTools = () => names.map(name => ({name}));
  f.pi.getActiveTools = () => names;
  registerRunner(f.pi, () => f.agent, f.defaults, '1.4.0', Object.fromEntries(names.map(name => [name, {type:'object'}])));
  assert.deepEqual(Object.keys(f.tools).sort(), names);
  assert.deepEqual(f.requests(), []);
  await f.hooks.session_start({}, f.ctx);
  const ready = f.requests()[0];
  assert.deepEqual(ready, {op:'ready',agent_dir:f.agent,version:'1.4.0',tools:names,active_tools:names});
  assert.deepEqual(await f.hooks.tool_call({toolName:'bash'},f.ctx), {block:true,reason:'TOOL_DENIED'});
  assert.equal(await f.hooks.tool_call({toolName:'context_read'},f.ctx), undefined);
  const result = await f.tools.context_describe.execute('native-context',{},undefined,undefined,f.ctx);
  assert.equal(JSON.parse(result.content[0].text).kind, 'description');
  const usage = {input:10,output:3,cacheRead:7,cacheWrite:2,totalTokens:22};
  await f.hooks.message_end({message:{role:'assistant',usage}},f.ctx);
  await f.hooks.session_compact({compactionEntry:{usage}},f.ctx);
  assert.deepEqual(f.requests().filter(r => r.op === 'usage'), [
    {op:'usage',input_tokens:19,output_tokens:3},
    {op:'usage',input_tokens:38,output_tokens:6},
  ]);
  let aborted = false; f.ctx.abort = () => { aborted = true; };
  await assert.rejects(f.hooks.message_end({message:{role:'assistant',usage:{...usage,input:-1}}},f.ctx), /PROTOCOL_INVALID/);
  assert(aborted);
  const count = f.requests().length;
  const denied = await f.tools.context_describe.execute('native-context',{operation:'read'},undefined,undefined,f.ctx);
  assert.equal(JSON.parse(denied.content[0].text).error.code, 'INVALID_ARGUMENT');
  assert.equal(f.requests().length, count);
  f.pi.getActiveTools = () => [...names,'bash'];
  await assert.rejects(f.hooks.session_start({},f.ctx), /TOOL_DENIED/);
  assert.equal(f.requests().length, count);
});

test('runner research may exceed transport timeout but keeps the original deadline', async t => {
  const f = fixture(t, `setTimeout(() => console.log(JSON.stringify({schema_version:1,sequence:1,request_id:'native-request',data:{answer:'slow research'}})),11000); return;`);
  const previous = process.env.AKASHA_RUN_DEADLINE_MS;
  t.after(() => { if (previous === undefined) delete process.env.AKASHA_RUN_DEADLINE_MS; else process.env.AKASHA_RUN_DEADLINE_MS = previous; });
  process.env.AKASHA_RUN_DEADLINE_MS = String(Date.now() + 30000);
  assert.equal((await exchange(f.defaults, {op:'context',request:{operation:'delegate',hint:'child'}}, undefined, true)).answer, 'slow research');
  process.env.AKASHA_RUN_DEADLINE_MS = String(Date.now() - 1);
  await assert.rejects(exchange(f.defaults, {op:'context'}, undefined, true), /RUNNER_TIMEOUT/);
  delete process.env.AKASHA_RUN_DEADLINE_MS;
  await assert.rejects(exchange(f.defaults, {op:'context'}, undefined, true), /CONFIG_INVALID/);
});
