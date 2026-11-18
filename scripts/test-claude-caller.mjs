import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const source = await readFile(new URL('../src/adapter/claude/caller.js', import.meta.url), 'utf8');
const fixtureSource = source.replace(
  "import { bridge, binary, args } from './config.js';",
  "const bridge = '/fixture/bridge', binary = '/fixture/claude', args = [];",
);
assert.notEqual(fixtureSource, source);
let instance = 0;

test('runner calls inherit the absolute deadline without restarting its budget', async t => {
  const source = await readFile(new URL('../src/adapter/claude/runner.js', import.meta.url), 'utf8');
  const { register } = await import('data:text/javascript;base64,' + Buffer.from(source).toString('base64'));
  const hooks = new Map(), timeouts = [];
  register((name, callback) => {
    hooks.set(name, callback);
    return {catch: handler => hooks.set(name, async (...args) => {
      try { return await callback(...args); } catch (error) { return handler(error); }
    })};
  });
  const start = Date.now(), deadline = start + 1_700_000;
  const output = JSON.stringify({data:{answer:'done'}});
  const event = {tool:'mcp__akasha-runner__delegate',hint:'child'};
  const argv = ['/owned/bridge', 'adapter', 'claude-read'];
  const request = {operation:'delegate', hint:'child'};
  let chunks = [{stream:'stdout', text:output.slice(0, 3)}, {stream:'stderr', text:'notice'}, {stream:'stdout', text:output.slice(3)}];
  let exit = {code:0, signal:null};
  let closed = 0;
  const $ = {
    env:{get:async key => key === 'AKASHA_RUN_BRIDGE' ? argv[0] : String(deadline)},
    process:{
      run:async (command, options) => {
        assert.deepEqual(command, argv);
        assert.deepEqual(JSON.parse(options.stdin), request);
        timeouts.push(options.timeoutMs);
        return {exitCode:0, stdout:output};
      },
      async *spawn(options) {
        assert.deepEqual(options, {argv, input:JSON.stringify(request)});
        try {
          yield* chunks;
          return exit;
        } finally {
          closed++;
        }
      },
    },
  };
  const call = () => hooks.get('tool.call')($, event);
  const success = {result:JSON.stringify({answer:'done'})};
  const failure = {deny:'AKASHA_READ_FAILED'};
  t.mock.method(Date, 'now', () => start);
  assert.deepEqual(await call(), success);
  t.mock.method(Date, 'now', () => deadline - 600001);
  assert.deepEqual(await call(), success);
  exit = {code:1, signal:null};
  assert.deepEqual(await call(), failure);
  exit = {code:null, signal:'SIGTERM'};
  assert.deepEqual(await call(), failure);
  exit = {code:0, signal:null};
  for (const stream of ['stderr', 'stdout']) {
    chunks = [{stream:'stdout', text:output}, {stream, text:'字'.repeat(1_400_000)}];
    assert.deepEqual(await call(), failure);
  }
  assert.equal(closed, 6);
  for (const remaining of [600000, 30000, 19000, 1]) {
    t.mock.method(Date, 'now', () => deadline - remaining);
    assert.deepEqual(await call(), success);
  }
  t.mock.method(Date, 'now', () => deadline);
  assert.deepEqual(await call(), {deny:'RUNNER_TIMEOUT'});
  assert.deepEqual(timeouts, [600000, 30000, 19000, 1]);
});

async function fixture(initialCwd = '/workspace/A') {
  const { register } = await import(`data:text/javascript;base64,${Buffer.from(fixtureSource).toString('base64')}#${instance++}`);
  const hooks = new Map(), registrations = new Map(), binds = [];
  let cwd = initialCwd;
  const session = 'native-session';
  const $ = {
    session: { id: async () => session, cwd: async () => cwd },
    env: {get:async key => key === 'HOME' ? '/fixture/home' : undefined},
    agent: { list: async () => [{ id: 'child' }, { id: 'grandchild', parentId: 'child' }] },
    process: { run: async (_argv, { stdin }) => {
      const event = JSON.parse(stdin);
      let data;
      if (event.op === 'register' || event.op === 'register_child') {
        const parent = registrations.get(event.parent_registration);
        if (event.op === 'register_child') assert.ok(parent);
        const registration = `registration-${registrations.size}`;
        if (!parent) assert.equal(event.native_home, '/fixture/home/.claude');
        registrations.set(registration, { cwd: parent ? parent.cwd : event.cwd });
        data = { registration };
      } else {
        assert.equal(event.op, 'consult');
        const registered = registrations.get(event.call.registration);
        assert.ok(registered);
        data = {status:'pending',job_id:'consult-fixture',receipt_id:null};
        binds.push({...event.call, args:event.args, workspace:registered.cwd});
      }
      return { exitCode: 0, stdout: JSON.stringify({ data }), stderr: '' };
    } },
  };
  register((event, ...args) => {
    const callback = args.at(-1);
    hooks.set(event, callback);
    return { catch: handler => hooks.set(event, async (...values) => {
      try { return await callback(...values); } catch (error) { return handler(error); }
    }) };
  });
  await hooks.get('classic.SessionStart')($, { session_id: session, cwd, transcript_path: '/fixture/root.jsonl', source: 'startup' }, async () => {});
  for (const agentId of [undefined, 'child', 'grandchild']) {
    await hooks.get('turn.start')($, { agentId, turnId: `turn-${agentId || 'root'}` }, async () => {});
    await hooks.get('session.append')($, { agentId, door: 'assistant' }, async () => ({ uuid: `tip-${agentId || 'root'}` }));
  }
  return {
    move: path => { cwd = path; },
    async invoke(agentId, hint) {
      const before = binds.length;
      let executed = false;
      const result = await hooks.get('tool.call')($, { agentId, tool_use_id: `call-${before}`, hint }, async () => {
        executed = true;
        return { workspace: binds.at(-1).workspace };
      });
      assert.equal(executed, false, 'the registered tool must handle its own result');
      if (result.result) {
        assert.equal(binds.length, before + 1);
        assert.equal(binds.at(-1).workspace, cwd, 'tool received stale workspace');
        assert.equal(binds.at(-1).tip, `tip-${agentId || 'root'}`);
        assert.deepEqual(binds.at(-1).args, {hint:hint ?? null});
        assert.equal(JSON.parse(result.result).status, 'pending');
      } else {
        assert(['BINDING_UNAVAILABLE','INVALID_ARGUMENT'].includes(result.deny));
        assert.equal(binds.length, before, 'refusal must precede submission');
      }
      return Boolean(result.result);
    },
  };
}

test('a new session registers the current workspace for root and descendants', async () => {
  const host = await fixture('/workspace/B');
  for (const agent of [undefined, 'child', 'grandchild']) assert.equal(await host.invoke(agent), true);
});

for (const agentId of [undefined, 'child', 'grandchild']) {
  test(`${agentId || 'root'} cannot bind an old workspace after cwd changes`, async () => {
    for (const refreshRootFirst of [false, true]) {
      const host = await fixture();
      for (const agent of [undefined, 'child', 'grandchild']) assert.equal(await host.invoke(agent), true);
      host.move('/workspace/B');
      if (refreshRootFirst) await host.invoke(undefined);
      await host.invoke(agentId);
      host.move('/workspace/A');
      assert.equal(await host.invoke(agentId), true);
    }
  });
}


test('caller registers the real consult tool with only an optional hint', async () => {
  const { register } = await import(`data:text/javascript;base64,${Buffer.from(fixtureSource).toString('base64')}#${instance++}`);
  const hooks = new Map(), definitions = [];
  register((name, ...args) => { hooks.set(name, args.at(-1)); return {catch: () => {}}; });
  assert(hooks.has('session.start'), 'the installed caller must register its model tool');
  const $ = {tool:{register:async definition => definitions.push(definition)}};
  await hooks.get('session.start')($, {}, async () => {});
  assert.equal(definitions.length, 1);
  assert.equal(definitions[0].name, 'consult');
  assert.deepEqual(Object.keys(definitions[0].inputSchema.properties), ['hint']);
  assert.equal(definitions[0].inputSchema.additionalProperties, false);
  assert.deepEqual(await hooks.get('tool.check')($, {tool:'mcp__akasha-otsuge__consult'}, async () => 'untouched'), {decision:'allow'});
});


test('caller accepts empty and Unicode hints and rejects oversized hints before submission', async () => {
  const host = await fixture();
  assert.equal(await host.invoke(undefined, ''), true);
  assert.equal(await host.invoke(undefined, '😀'.repeat(512)), true);
  assert.equal(await host.invoke(undefined, '😀'.repeat(513)), false);
});
