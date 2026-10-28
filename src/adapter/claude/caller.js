import { bridge, binary, args } from './config.js';

let root;

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

async function sync($) {
  const state = root && {...root};
  if (state && state.tip && await $.session.id() === state.session) {
    await ensureRegistration($, state);
    await send($, {op: 'sync', registration: state.registration, tip: state.tip});
  }
}

async function ensureRegistration($, state) {
  if (!state.registration) {
    const registered = await send($, {op: 'register', binary, session_id: state.session,
      transcript_path: state.transcript, cwd: state.cwd});
    state.registration = registered.registration;
  }
}

export function register(on) {
  on('classic.SessionStart', async ($, e, next) => {
    const state = {session: e.session_id, transcript: e.transcript_path, cwd: e.cwd, cursor: 0};
    root = state;
    if (e.session_id !== await $.session.id()) throw new Error('CALLER_AMBIGUOUS');
    return next(e);
  });
  on('turn.start', async ($, e, next) => {
    const state = root;
    if (state && !e.agentId && await $.session.id() === state.session && root === state) state.turn = e.turnId;
    return next(e);
  });
  on('session.append', async ($, e, next) => {
    const state = root;
    const result = await next(e);
    if (state && !e.agentId && await $.session.id() === state.session && root === state) {
      state.tip = e.uuid; state.cursor += 1;
      await ensureRegistration($, state);
    }
    return result;
  });
  on('turn.complete', async ($, e, next) => {
    const result = await next(e);
    if (!e.agentId) await sync($);
    return result;
  });
  on('classic.SessionEnd', async ($, e, next) => {await sync($); return next(e);});
  on('tool.call', {tool: 'mcp__akasha-otsuge__consult'}, async ($, e, next) => {
    // Freeze the event fence before any await can allow another turn to advance it.
    const caller = root && {...root};
    if (!caller || e.agentId || !caller.turn || !caller.tip || await $.session.id() !== caller.session)
      return {deny: 'BINDING_UNAVAILABLE'};
    const cwd = await $.session.cwd();
    if (cwd !== caller.cwd) {
      caller.cwd = cwd; caller.registration = undefined;
    }
    await ensureRegistration($, caller);
    await send($, {op: 'bind', registration: caller.registration, session_id: caller.session,
      agent_id: null, turn_id: caller.turn, tool_call_id: e.tool_use_id, cursor: caller.cursor, tip: caller.tip});
    return next(e);
  }).catch(() => ({deny: 'BINDING_UNAVAILABLE'}));
}
