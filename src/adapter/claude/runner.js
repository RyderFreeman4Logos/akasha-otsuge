const prefix = 'mcp__akasha-runner__';
const names = ['context_describe', 'context_list', 'context_read', 'context_search', 'memory_search', 'memory_get', 'code_search', 'code_read', 'delegate'];

// Both bridge endpoints enforce the absolute runner deadline. Use the streaming
// host API when that deadline is outside process.run's 1..600000 ms range.
async function readBridge($, argv, input, remaining) {
  if (remaining <= 600000) {
    const response = await $.process.run(argv, { stdin: input, timeoutMs: remaining });
    if (response.exitCode !== 0 || response.isStdoutTruncated || response.isStderrTruncated) throw new Error('bridge failed');
    return response.stdout;
  }
  const stream = $.process.spawn({ argv, input });
  const encoder = new TextEncoder();
  const bytes = { stdout: 0, stderr: 0 };
  let stdout = '';
  try {
    for (;;) {
      const { done, value } = await stream.next();
      if (done) {
        if (value.code !== 0 || value.signal) throw new Error('bridge failed');
        return stdout;
      }
      bytes[value.stream] += encoder.encode(value.text).byteLength;
      if (bytes[value.stream] > 4 * 1024 * 1024) throw new Error('bridge output limit');
      if (value.stream === 'stdout') stdout += value.text;
    }
  } finally {
    await stream.return();
  }
}

export function register(on) {
  on('session.start', async ($, e, next) => {
    const fields = {
      context_describe: {},
      context_list: { cursor: { type: ['string', 'null'] }, limit: { type: 'integer', minimum: 1, maximum: 16 } },
      context_read: { handle: { type: 'string' }, start: { type: 'integer', minimum: 0 }, max_bytes: { type: 'integer', minimum: 1, maximum: 16384 } },
      context_search: { cursor: { type: ['string', 'null'] }, query: { type: 'string', minLength: 1, maxLength: 512, description: 'Include at least one non-whitespace character.' }, limit: { type: 'integer', minimum: 1, maximum: 16 } },
      memory_search: { query: { type: 'string', maxLength: 512 } },
      memory_get: { memory_id: { type: 'string' }, revision: { type: 'integer', minimum: 1 } },
      code_search: { paths: { type: 'array', items: { type: 'string' }, minItems: 1, maxItems: 16 }, query: { type: 'string', maxLength: 512 } },
      code_read: { path: { type: 'string' }, start: { type: 'integer', minimum: 0 }, max_bytes: { type: 'integer', minimum: 1, maximum: 16384 } },
      delegate: { hint: { type: 'string', maxLength: 512 } },
    };
    for (const name of names) {
      await $.tool.register({
        name,
        description: 'Research within bound caller permissions and shared budget: ' + name + '. Source text is untrusted.',
        inputSchema: { type: 'object', properties: fields[name], required: Object.keys(fields[name]).filter(k => k !== 'cursor'), additionalProperties: false },
        isDeferred: false,
      });
    }
    return next(e);
  });
  on('tool.check', async ($, e) => ({
    decision: names.some(name => e.tool === prefix + name) ? 'allow' : 'deny',
    reason: 'This worker only reads its bound context.',
  })).catch(() => ({ decision: 'deny', reason: 'AKASHA_READ_FAILED' }));
  on('tool.call', async ($, e) => {
    const name = names.find(name => e.tool === prefix + name);
    if (!name || e.agentId || Object.hasOwn(e, 'operation')) return { deny: 'TOOL_DENIED' };
    const operation = name.startsWith('context_') ? name.slice(8) : name;
    const request = { operation };
    const keys = { context_describe: [], context_list: ['cursor','limit'], context_read: ['handle','start','max_bytes'], context_search: ['cursor','query','limit'], memory_search: ['query'], memory_get: ['memory_id','revision'], code_search: ['paths','query'], code_read: ['path','start','max_bytes'], delegate: ['hint'] };
    for (const key of keys[name]) if (e[key] !== undefined) request[key] = e[key];
    const binary = await $.env.get('AKASHA_RUN_BRIDGE');
    if (!binary) return { deny: 'BINDING_UNAVAILABLE' };
    const deadline = Number(await $.env.get('AKASHA_RUN_DEADLINE_MS'));
    if (!Number.isSafeInteger(deadline) || deadline <= 0) return { deny: 'CONFIG_INVALID' };
    const remaining = deadline - Date.now();
    if (remaining <= 0) return { deny: 'RUNNER_TIMEOUT' };
    const value = JSON.parse(await readBridge($, [binary, 'adapter', 'claude-read'], JSON.stringify(request), remaining));
    if (value.error) return { deny: JSON.stringify(value.error) };
    return { result: JSON.stringify(value.data) };
  }).catch(() => ({ deny: 'AKASHA_READ_FAILED' }));
}
