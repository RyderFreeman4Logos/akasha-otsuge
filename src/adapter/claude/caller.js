import { bridge, binary, args } from './config.js';

let root;
const children = new Map();

function stateFor(agent) {
  if (!root || !agent) return root;
  if (!children.has(agent)) children.set(agent, {session: root.session, cwd: root.cwd, agent, cursor: 0});
  return children.get(agent);
}

async function send($, event) {
  const result = await $.process.run([bridge, ...args, 'adapter', 'claude-call'], {
    stdin: JSON.stringify(event), timeoutMs: 15000,
  });
  if (result.exitCode !== 0 || result.isStdoutTruncated || result.isStderrTruncated || result.stdout.length > 32768)
    throw new Error('BINDING_UNAVAILABLE');
  const response = JSON.parse(result.stdout);
  if (response.error || !response.data) throw new Error('BINDING_UNAVAILABLE');
  return response.data;
}

async function sync($, selected) {
  const state = selected && {...selected};
  if (state && state.tip && await $.session.id() === state.session) {
    await ensureRegistration($, state);
    await send($, {op: 'sync', registration: state.registration, tip: state.tip});
  }
}

async function ensureRegistration($, state, seen = new Set()) {
  if (!state.registration) {
    if (seen.has(state.agent) || seen.size >= 32) throw new Error('CALLER_AMBIGUOUS');
    seen.add(state.agent);
    let event;
    if (state.agent) {
      const native = (await $.agent.list()).find(agent => agent.id === state.agent);
      if (!native) throw new Error('CALLER_AMBIGUOUS');
      const parent = native.parentId ? children.get(native.parentId) : root;
      if (!parent || parent.session !== state.session) throw new Error('CALLER_AMBIGUOUS');
      await ensureRegistration($, parent, seen);
      event = {op: 'register_child', parent_registration: parent.registration, agent_id: state.agent};
    } else {
      event = {op: 'register', binary, session_id: state.session,
        transcript_path: state.transcript, cwd: state.cwd};
    }
    const registered = await send($, event);
    state.registration = registered.registration;
  }
}

export function register(on) {
  on('classic.SessionStart', async ($, e, next) => {
    const state = {session: e.session_id, transcript: e.transcript_path, cwd: e.cwd, cursor: 0};
    root = state;
    children.clear();
    if (e.session_id !== await $.session.id()) throw new Error('CALLER_AMBIGUOUS');
    return next(e);
  });
  on('turn.start', async ($, e, next) => {
    const state = stateFor(e.agentId);
    const generation = root;
    if (state && await $.session.id() === state.session && root === generation) state.turn = e.turnId;
    return next(e);
  });
  on('session.append', async ($, e, next) => {
    const state = stateFor(e.agentId);
    const generation = root;
    const result = await next(e);
    if (state && await $.session.id() === state.session && root === generation) {
      state.tip = e.uuid; state.cursor += 1;
      await ensureRegistration($, state);
    }
    return result;
  });
  on('turn.complete', async ($, e, next) => {
    const result = await next(e);
    await sync($, stateFor(e.agentId));
    return result;
  });
  on('classic.SubagentStop', async ($, e, next) => {await sync($, children.get(e.agent_id)); return next(e);});
  on('classic.SessionEnd', async ($, e, next) => {await sync($, root); return next(e);});
  on('tool.call', {tool: 'mcp__akasha-otsuge__consult'}, async ($, e, next) => {
    // Freeze the event fence before any await can allow another turn to advance it.
    const selected = e.agentId ? children.get(e.agentId) : root;
    const generation = root;
    const caller = selected && {...selected};
    if (!caller || !caller.turn || !caller.tip || await $.session.id() !== caller.session)
      return {deny: 'BINDING_UNAVAILABLE'};
    const cwd = await $.session.cwd();
    if (cwd !== caller.cwd) {
      caller.cwd = cwd; caller.registration = undefined;
    }
    await ensureRegistration($, caller);
    if (root !== generation) return {deny: 'BINDING_UNAVAILABLE'};
    await send($, {op: 'bind', registration: caller.registration, session_id: caller.session,
      agent_id: caller.agent || null, turn_id: caller.turn, tool_call_id: e.tool_use_id, cursor: caller.cursor, tip: caller.tip});
    return next(e);
  }).catch(() => ({deny: 'BINDING_UNAVAILABLE'}));
}
