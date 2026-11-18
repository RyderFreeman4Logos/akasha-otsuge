# H01 frozen-profile fixture

The H01 fixture runs the current request in Codex 0.164.0 with the model and reasoning effort from `reference-v1/profile.json`. The historical comparison and its costs are in the [first pair receipt](../../docs/benchmarks/results/mvp027/h01-subscription-r1.json).

Use a dedicated Linux SSD directory for the observers and reuse a Cargo target directory with the locked dependencies. The builder reads the frozen product source from the local Git object database and adds the evaluation module in its private source export. Pass `--prepare-only` to check the Cargo targets without compiling.

```sh
python3 -B scripts/evaluation/build_reference.py \
    --output "$OBSERVERS" --target-dir "$CARGO_TARGET_DIR"
```

With the physical Codex executable, run the local preflight in a new result path. It uses a fresh private store, one synthetic invocation and no network or credentials. The diagnostic runs the production scoped search before and after an index sync. It also checks capacity for two native runner reservations.

```sh
python3 -B scripts/evaluation/reference_direct.py --strategy consult --preflight \
    --codex "$PINNED_CODEX" --scratch "$PRIVATE_SCRATCH" \
    --observer "$OBSERVERS/reference-service" --observer-build "$OBSERVERS/reference-service.json" \
    --output "$PRIVATE_SCRATCH/preflight.json"
```

For a registered model run, choose `--strategy direct` with `reference-read` or `--strategy consult` with `reference-service`. Replace `--preflight` with `--auth` and `--catalog` paths from the existing subscription profile. Each model arm consumes subscription usage. The driver creates an ephemeral Linux mount namespace, pins the native executable hash, selects one model descriptor, and mounts the credential read-only for the native client. The tools use the caller's fixture scope.

The direct H01 tool reads the empty scope with `Domain::state`. The consult fixture binds the native main thread, accepted turn and tool-call identity to the frozen principal using `Invocation::bind`. It calls `Service::submit_consult` and reads the published result and receipt. The current request is a live event alongside zero retained historical inputs. The public registration path uses the OS user and has separate integration coverage.

The revised fixture syncs its empty index and permits two native reservations before opening the scheduled consult. The first pair's original response and failing status remain in the receipt. Episodes with retained history, persisted memories or changing grants require their own import and fence mappings.

## Offline inspection of the 60 episodes

Create the plan without compiling or opening a store. It verifies the frozen manifests, applies the S04 erratum and emits one subject hash per episode. The native importer receives an explicit allowlist of fields. Oracle contents, expected status and scoring labels stay in the coordinator.

```sh
python3 -B scripts/evaluation/reference_suite.py --output "$PRIVATE_SCRATCH/plan.json"
python3 -B scripts/evaluation/build_reference.py --observer reference-import \
    --output "$IMPORT_OBSERVER" --target-dir "$CARGO_TARGET_DIR"
python3 -B scripts/evaluation/reference_suite.py --observer "$IMPORT_OBSERVER/reference-import" \
    --build "$IMPORT_OBSERVER/reference-import.json" --scratch "$PRIVATE_SCRATCH" \
    --full-native --output "$PRIVATE_SCRATCH/native.json"
```

Each native episode gets a fresh store. The observer reads the original bytes and normalized source spans, walks the declared ancestry using product Capture, checks original reads under the caller's grant, imports Memory revisions through the production Domain, synchronizes Lance and performs a scoped keyword lookup. The report records the logical ordinals, physical commit receipts and each refused import. The `pending` field identifies the remaining setup by task. Use `--task B03` or another exact ID for a targeted replay.

The [archived native observations](../../docs/benchmarks/results/mvp027/offline-import-r1.json.gz) contain all 60 episodes. The 87 original payloads matched their hashes, all 60 lineages matched the allowed original IDs, and 82 scoped original reads succeeded while five were denied. All 60 indexes reached their target watermark. This measured index readiness for the rows that the product accepted.

| Frozen Memory revision outcome | Count |
| --- | ---: |
| Stored with human confirmation | 44 |
| Stored as a candidate | 3 |
| Stored with delegated approval | 1 |
| Explicit-statement adoption returned `AUTHORITY_DENIED` | 33 |
| A required predecessor revision was unavailable | 1 |

The frozen inputs omit the production Memory kind. Both import runs use `Fact` as a fixture convention. A frozen `human_confirmed` field declares a synthetic historical state. Only production adoption or a separately authenticated targeted review can produce that authority in the fresh store. The table above records the first run.

### Historical reads, withdrawals and scope changes

Run the same native suite with `--full-native --verbatim-controls` to reproduce the [second archive of 60 registered episodes and three isolated controls](../../docs/benchmarks/results/mvp027/offline-transitions-r1.json.gz). Each Memory becomes eligible after its last contributing original has been captured. The observer records the latest native commit reached at each logical source ordinal. Each registered input and each native stdout has its own SHA-256. The decompressed archived report has SHA-256 `821b85b6327a9613dfd1a5c1b89d5b902ceda55c6129e4f42b2d7b380a4af44d`.

The per-revision audit found 34 unimported human-confirmed revisions with structured conditions. In 33 cases the frozen claim and condition appear in a single verified original. Direct adoption requires the complete original as the claim and declines separate conditions. V02 revision 2 has a missing predecessor and its original omits the frozen condition. The frozen sources contain no targeted approvals for these 34 revisions. The observer retains each refusal and the original hashes. It reconstructs explicit withdrawals from their distinct verified originals when the affected Memory exists. None of the 60 episodes supplies an operation that restores a withdrawn Memory.

| Episode | Observed on the registered input |
| --- | --- |
| V02 | Ordinal 1 maps to commit 6 after original capture 5. The historical read at 6 and current read at 16 both returned absent under the live grant. The expected east Memory could not be imported. |
| V03 / C01 | V03 retained the verified withdrawal original but lacked its predecessor. C01 adopted its first revision at 7 and refused the conditioned correction. Both require more input to reproduce the expected state. |
| C03 | The unique verified withdrawal targeted the existing candidate at 7. Production review committed its rejection at 13. The current adopted state was absent before and after rejection. The fixture's prior active-to-retired sequence remains unverified for this candidate. |
| A04 | The caller read the exact Atlas original with a sealed receipt at 12. The attempted move to Private returned `SCOPE_DENIED`, and an immediate Private state read returned `READ_DENIED`. No rescope was committed. The input supplies the destination and no new authorization or source. The probe retained the Atlas ingress, grant and policy. The registered run lacks the initial confirmed Memory and a permitted same-ID move. |
| S04 | A real grant revocation denied the next original read. A final model publication under the revised grant still needs separate verification. |

Each ungraded control runs in its own store. It retains the same original bytes, scopes and requested change, uses a distinct episode ID, and exercises direct adoption with the complete unabridged statement and empty structured conditions. V02 read the earlier adoption at 7 and the successor at 13 through the same current grant. V03 retired the active adoption through the original withdrawal and retained the previous version for a historical read. In A04, the Atlas adoption stayed active after the denied rescope, and the Private read remained denied. The original source is scoped to Atlas, and the production Memory write API requires a new ID for a scope change. The controls establish those API behaviors. Their registered-result credit is zero. The run made zero model calls or public model publications.

B01 and B03 returned raw keyword candidates backed by an original after the allowed ancestral cutoff. Their captured lineage excluded that original. The final caller-visible publication requires separate verification. H02 reproduced the missing original in lineage. Interactive handling of that gap also requires a real run. H01 has one observed model pair. The other 59 episodes still require paired model entry points and their applicable import mappings.

## Blind quality and costs

The full denominator includes 31 critical tasks, with 20 required items and 11 critical cases whose denial or no-hit outcome must pass. The remaining tasks contain 36 required items. At least 33 of those items must earn full credit. There are ten paired intents. Irrelevance uses the number of distinct substantive propositions in the actual published answers.

```sh
python3 -B scripts/evaluation/reference_judgment.py --worksheet --output "$PRIVATE_SCRATCH/judge.json"
python3 -B scripts/evaluation/reference_judgment.py "$PRIVATE_SCRATCH/judge.json" \
    --output "$PRIVATE_SCRATCH/scored.json"
```

A coordinator supplies each public answer and its SHA-256 under an opaque ID, confirms the terminal and import evidence, and withholds the arm label. For one anonymous cohort, a human reviews the original evidence, every returned item and every atomic proposition. In `factual_memory_uses` record the Memory ID, revision, scope, authority and source IDs for every factual use. In `credited_items` include those required-item identifiers, the entailed claim, all conditions, an exact public-answer excerpt and the reason for the mapping. In `propositions` include the distinct proposition text, its excerpt, relevance and reason. A relevant proposition lists its credited Memory IDs and revisions in `satisfies`. Record canonical gap IDs and any invariant violations. The scorer reports incomplete cohorts, preserves the frozen denominator and shows both variants of each pair. Run it independently for the other anonymous cohort.

The plan contains an attempt template for the coordinator, including main, external and incremental token counts, reads, tool and model call counts, setup, failed attempts, latency, cost and currency. Use null with the reason when a counter or bill is unavailable. Keep cost labels away from blind adjudicators. The measured H01 amounts and historical serialization hashes remain in the first-pair receipt.

```sh
python3 -B -m unittest discover -s scripts/evaluation -p 'test_reference*.py'
```
