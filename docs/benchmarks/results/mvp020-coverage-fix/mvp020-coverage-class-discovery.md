# R3 coverage class inventory before the next production edit

Frozen source: d9d1e4a7c8984dcb076022c40e6c31a243d80490, tree
83452bc9db7101bdde4bbfbe29326ddb386291b3. R2 and R3 are both FAIL;
the next repair covers the shared Research response boundary.

| Existing representation | Current collection | Required control |
| --- | --- | --- |
| Captured Description coverage | Final invocation coverage union | Keep incomplete binding conservative |
| Successful List/Search Page.gaps | LOST even when binding is CompleteToFence | Real missing-parent capture, both tools, valid patch, model omits gap |
| Successful ContextText.truncated | LOST | Real bounded context read; ordinary Consult and Editor |
| Memory search receipt coverage.gaps | Collected from authoritative receipt | Keep existing retrieval tests |
| Memory originals truncated | Collected as original_budget | Retain R2 omission regression |
| Code search truncated | Collected as code_search_budget | Keep existing bounded search tests |
| Code search/read changed | Collected as workspace_changed | Retain R2 omission regression |
| Dispatch errors returned as ContextResponse::Error | Collected with original source code | Retain INDEX_NOT_READY control |
| Errors before/after dispatch, including output budget | Not collected | Preserve Err and source code; prove boundary budget error remains a gap |
| Successful delegate conclusion gaps/conflicts | LOST if parent omits them | Valid child incomplete result and parent found/empty gaps/conflicts |
| Root conclusion gaps/conflicts | Shared coverage_gaps union | Keep ordinary conflict and uncertain Editor controls |
| Editor ready checkpoint | Stores collected union | SIGKILL response/applied retain newly collected gap labels; legacy remains conservative |

Codex, Pi and Hermes propagate ContextSource errors. Claude's private bridge
serializes an Err for the native model, so outer Research errors must be observed
without converting them to success or weakening authority/budget failures.
Do not parse arbitrary Research JSON for guessed gap fields: collect child
conclusions while typed and retain existing typed per-operation observations.
No gap is inferred merely from Page.next or an intentional code byte range.
Description coverage is the same immutable invocation already checked at final
publication. The 32-entry bounded, deduplicated union remains conservative.

Scope: Research, its existing controlled Runner fixture, Editor/Consult tests,
recovery tests, docs and immutable receipts. No schema/config/timeout changes,
new dependencies, native authentication claims or unrelated outcome work.
