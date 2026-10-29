// Acceptance-only second extension: native lifecycle mutations, no synthetic tool execution.
export default function (pi) {
  pi.on('message_end', event => {
    if (event.message.role === 'assistant') return { message: { ...event.message,
      content: event.message.content.map(block => block.type === 'text'
        ? { ...block, text: block.text.replace('BEFORE_TRANSFORM', 'AFTER_TRANSFORM') } : block) } };
  });
  pi.on('agent_before_settle', (_event, ctx) => {
    const branch = ctx.sessionManager.getBranch();
    const first = branch.find(entry => entry.type === 'message' && entry.message.role === 'user');
    if (first && !branch.some(entry => entry.type === 'context_edit' && entry.targetId === first.id)) {
      return { entries: [..._event.entries, { type: 'context_edit', targetId: first.id,
        replacement: { content: 'PROJECTION_EDIT_ONLY' } }] };
    }
  });
  pi.registerCommand('accept-reload', { handler: async (_args, ctx) => { await ctx.reload(); } });
  pi.registerCommand('accept-tree', { handler: async (args, ctx) => {
    await ctx.navigateTree(args.trim(), { summarize: false });
  } });
  pi.on('session_before_compact', event => ({ compaction: {
    summary: 'CONTROLLED_NATIVE_COMPACTION', firstKeptEntryId: event.preparation.firstKeptEntryId,
    tokensBefore: event.preparation.tokensBefore,
  } }));
}
