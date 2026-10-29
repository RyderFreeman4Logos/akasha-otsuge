'use strict';
const { spawn } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

function code(error) {
  return /^[A-Z][A-Z0-9_]{0,63}$/.test(error?.message) ? error.message : 'SOURCE_UNAVAILABLE';
}
function identifier(value) {
  return typeof value === 'string' && /^[A-Za-z0-9_./:+-]{1,512}$/.test(value);
}

// The installed Rust CLI owns framing, peer-UID verification and request correlation.
// Wait for close even on cancellation; never leave an unowned bridge process behind.
async function exchange(defaults, event, signal) {
  if (!path.isAbsolute(defaults.akasha_binary ?? '') || !path.isAbsolute(defaults.socket ?? '')) {
    throw new Error('CONFIG_INVALID');
  }
  const input = Buffer.from(JSON.stringify(event));
  if (input.length > 16384) throw new Error('OUTPUT_LIMIT');
  if (signal?.aborted) throw new Error('CANCELLED');
  return new Promise((resolve, reject) => {
    const child = spawn(defaults.akasha_binary, ['--socket', defaults.socket, 'adapter', 'pi-call'],
      {stdio:['pipe', 'pipe', 'pipe'], detached:true});
    let failure, output = [], errors = [], outSize = 0, errSize = 0;
    const stop = reason => {
      failure ??= reason;
      if (child.pid) {
        try { process.kill(-child.pid, 'SIGKILL'); } catch (error) { if (error.code !== 'ESRCH') child.kill('SIGKILL'); }
      }
    };
    const abort = () => stop('CANCELLED');
    const timer = setTimeout(() => stop('IPC_TIMEOUT'), 10000);
    signal?.addEventListener('abort', abort, {once:true});
    if (signal?.aborted) abort();
    child.on('error', () => { failure ??= 'EXECUTOR_UNAVAILABLE'; });
    child.stdin.on('error', () => stop('PROTOCOL_INVALID'));
    child.stdout.on('data', bytes => {
      outSize += bytes.length;
      if (outSize > 2097152) stop('OUTPUT_LIMIT'); else output.push(bytes);
    });
    child.stderr.on('data', bytes => {
      errSize += bytes.length;
      if (errSize > 8192) stop('OUTPUT_LIMIT'); else errors.push(bytes);
    });
    child.on('close', status => {
      clearTimeout(timer); signal?.removeEventListener('abort', abort);
      try {
        if (failure) throw new Error(failure);
        let response;
        try { response = JSON.parse(Buffer.concat(status === 0 ? output : errors).toString('utf8')); }
        catch { throw new Error('PROTOCOL_INVALID'); }
        if (status !== 0 || response.error) throw new Error(code({message:response.error?.code}));
        if (response.schema_version !== 1 || response.sequence !== 1 || !identifier(response.request_id)
            || !response.data || typeof response.data !== 'object' || Array.isArray(response.data)) {
          throw new Error('PROTOCOL_INVALID');
        }
        resolve(response.data);
      } catch (error) { reject(error); }
    });
    child.stdin.end(input);
  });
}

function registerCaller(pi, getAgentDir, defaults, parameters) {
  let registration, dirty = false;
  function identity(ctx) {
    const manager = ctx.sessionManager;
    const transcript = manager.getSessionFile(), leaf = manager.getLeafId(), session = manager.getSessionId();
    if (!identifier(session) || !identifier(leaf) || !transcript) throw new Error('BINDING_UNAVAILABLE');
    return {session_id:session, leaf_id:leaf, transcript:fs.realpathSync(transcript),
      agent_dir:fs.realpathSync(getAgentDir()), cwd:fs.realpathSync(ctx.cwd)};
  }
  async function registered(native, signal) {
    const event = {op:'register', binary:defaults.pi_binary, agent_dir:native.agent_dir,
      session_id:native.session_id, transcript_path:native.transcript, cwd:native.cwd};
    const key = JSON.stringify(event);
    if (registration?.key !== key) {
      const pending = {key};
      pending.promise = exchange(defaults, event, signal).then(reply => {
        if (!/^[a-f0-9]{64}$/.test(reply.registration)) throw new Error('PROTOCOL_INVALID');
        return reply.registration;
      }).catch(error => { if (registration === pending) registration = undefined; throw error; });
      registration = pending;
    }
    return registration.promise;
  }
  async function sync(_event, ctx) {
    try {
      // A newly created native session may not have a file or any persisted entry yet.
      if (!ctx.sessionManager.getLeafId() || !ctx.sessionManager.getSessionFile()) return;
      const native = identity(ctx), token = await registered(native);
      const job = await exchange(defaults, {op:'sync', registration:token, until_entry:native.leaf_id});
      if (!identifier(job.id) || typeof job.state !== 'string') throw new Error('PROTOCOL_INVALID');
      dirty = false;
      ctx.ui?.setStatus?.('akasha-capture', `Akasha capture: ${job.state}`);
    } catch (error) {
      dirty = true;
      ctx.ui?.setStatus?.('akasha-capture', `Akasha capture: ${code(error)}`);
      ctx.ui?.notify?.(`Akasha capture: ${code(error)}`, 'warning');
    }
  }
  pi.registerTool({name:'akasha_consult', label:'Akasha consult',
    description:'Consult saved user context.', parameters,
    async execute(toolCallId, args, signal, _onUpdate, ctx) {
      let result;
      try {
        if (!args || typeof args !== 'object' || Array.isArray(args) || Object.keys(args).some(k => k !== 'hint')
            || (args.hint != null && (typeof args.hint !== 'string' || args.hint.length > 512))) {
          throw new Error('INVALID_ARGUMENT');
        }
        const native = identity(ctx), branch = ctx.sessionManager.getBranch();
        if (!identifier(toolCallId) || !Array.isArray(branch) || branch.length > 16384) throw new Error('CALLER_AMBIGUOUS');
        const matches = branch.flatMap(entry => entry.type === 'message' && entry.message?.role === 'assistant'
          && Array.isArray(entry.message.content) ? entry.message.content.filter(block =>
            block.type === 'toolCall' && block.id === toolCallId && block.name === 'akasha_consult').map(() => entry.id) : []);
        if (matches.length !== 1 || !identifier(matches[0])) throw new Error('CALLER_AMBIGUOUS');
        const token = await registered(native, signal);
        const data = await exchange(defaults, {op:'bind', registration:token,
          call:{...native, turn_id:matches[0], tool_call_id:toolCallId}}, signal);
        result = {schema_version:1, data};
      } catch (error) { result = {schema_version:1, error:{code:code(error)}}; }
      return {content:[{type:'text', text:JSON.stringify(result)}], details:{}};
    }});
  // These callbacks precede other extensions' transformations and native persistence.
  // Mark pending only; reconciliation uses the final native file at lifecycle boundaries.
  for (const event of ['input', 'message_end']) pi.on(event, (_event, ctx) => {
    if (!dirty) ctx.ui?.setStatus?.('akasha-capture', 'Akasha capture: pending');
    dirty = true;
  });
  for (const event of ['session_start', 'turn_end', 'session_compact', 'session_tree', 'agent_settled']) pi.on(event, sync);
  pi.on('session_shutdown', async (event, ctx) => { await sync(event, ctx); registration = undefined; });
}

module.exports = { exchange, registerCaller };
