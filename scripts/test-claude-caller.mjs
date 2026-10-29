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
  const start = Date.now(), deadline = start + 30000;
  const $ = {env:{get:async key => key === 'AKASHA_RUN_BRIDGE' ? '/owned/bridge' : String(deadline)},
    process:{run:async (_command, options) => {
      timeouts.push(options.timeoutMs);
      return {exitCode:0,stdout:JSON.stringify({data:{answer:'done'}})};
    }}};
  const event = {tool:'mcp__akasha-runner__delegate',hint:'child'};
  t.mock.method(Date, 'now', () => start);
  assert((await hooks.get('tool.call')($, event)).result);
  t.mock.method(Date, 'now', () => start + 11000);
  assert((await hooks.get('tool.call')($, event)).result);
  assert.equal(timeouts[0] - timeouts[1], 11000);
  assert(timeouts[0] > 15000);
  t.mock.method(Date, 'now', () => deadline + 1);
  assert((await hooks.get('tool.call')($, event)).deny);
  assert.equal(timeouts.length, 2);
});

async function fixture(initialCwd = '/workspace/A') {
  const { register } = await import(`data:text/javascript;base64,${Buffer.from(fixtureSource).toString('base64')}#${instance++}`);
  const hooks = new Map(), registrations = new Map(), binds = [];
  let cwd = initialCwd;
  const session = 'native-session';
  const $ = {
    session: { id: async () => session, cwd: async () => cwd },
    agent: { list: async () => [{ id: 'child' }, { id: 'grandchild', parentId: 'child' }] },
    process: { run: async (_argv, { stdin }) => {
      const event = JSON.parse(stdin);
      let data;
      if (event.op === 'register' || event.op === 'register_child') {
        const parent = registrations.get(event.parent_registration);
        if (event.op === 'register_child') assert.ok(parent);
        const registration = `registration-${registrations.size}`;
        registrations.set(registration, { cwd: parent ? parent.cwd : event.cwd });
        data = { registration };
      } else {
        assert.equal(event.op, 'bind');
        const registered = registrations.get(event.registration);
        assert.ok(registered);
        data = { workspace: registered.cwd };
        binds.push({ ...event, ...data });
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
    async invoke(agentId) {
      const before = binds.length;
      let executed = false;
      const result = await hooks.get('tool.call')($, { agentId, tool_use_id: `call-${before}` }, async () => {
        executed = true;
        return { workspace: binds.at(-1).workspace };
      });
      if (executed) {
        assert.equal(binds.length, before + 1);
        assert.equal(result.workspace, cwd, 'tool body received stale workspace');
        assert.equal(binds.at(-1).tip, `tip-${agentId || 'root'}`);
      } else {
        assert.deepEqual(result, { deny: 'BINDING_UNAVAILABLE' });
        assert.equal(binds.length, before, 'refusal must precede binding');
      }
      return executed;
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
