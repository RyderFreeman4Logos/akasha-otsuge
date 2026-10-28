const prefix = 'mcp__akasha-runner__context_';
const operations = ['describe', 'list', 'read', 'search'];

export function register(on) {
  on('session.start', async ($, e, next) => {
    const fields = {
      describe: {},
      list: { cursor: { type: ['string', 'null'] }, limit: { type: 'integer', minimum: 1, maximum: 16 } },
      read: { handle: { type: 'string' }, start: { type: 'integer', minimum: 0 }, max_bytes: { type: 'integer', minimum: 1, maximum: 65536 } },
      search: { cursor: { type: ['string', 'null'] }, query: { type: 'string' }, limit: { type: 'integer', minimum: 1, maximum: 16 } },
    };
    for (const operation of operations) {
      await $.tool.register({
        name: 'context_' + operation,
        description: 'Read the bound caller context: ' + operation + '. Handles come from list/search; no paths or caller identity are accepted.',
        inputSchema: { type: 'object', properties: fields[operation], required: Object.keys(fields[operation]).filter(k => k !== 'cursor'), additionalProperties: false },
        isDeferred: false,
      });
    }
    return next(e);
  });
  on('tool.check', async ($, e) => ({
    decision: operations.some(operation => e.tool === prefix + operation) ? 'allow' : 'deny',
    reason: 'This worker only reads its bound context.',
  })).catch(() => ({ decision: 'deny', reason: 'AKASHA_READ_FAILED' }));
  on('tool.call', async ($, e) => {
    const operation = operations.find(operation => e.tool === prefix + operation);
    if (!operation || e.agentId) return { deny: 'TOOL_DENIED' };
    const request = { operation };
    if (operation === 'list' || operation === 'search') {
      request.cursor = e.cursor ?? null;
      request.limit = e.limit;
    }
    if (operation === 'search') request.query = e.query;
    if (operation === 'read') {
      request.handle = e.handle;
      request.start = e.start;
      request.max_bytes = e.max_bytes;
    }
    const binary = await $.env.get('AKASHA_RUN_BRIDGE');
    if (!binary) return { deny: 'BINDING_UNAVAILABLE' };
    const response = await $.process.run([binary, 'adapter', 'claude-read'], { stdin: JSON.stringify(request), timeoutMs: 15000 });
    if (response.exitCode !== 0 || response.isStdoutTruncated || response.isStderrTruncated) return { deny: 'AKASHA_READ_FAILED' };
    const value = JSON.parse(response.stdout);
    if (value.error) return { deny: JSON.stringify(value.error) };
    return { result: JSON.stringify(value.data) };
  }).catch(() => ({ deny: 'AKASHA_READ_FAILED' }));
}
