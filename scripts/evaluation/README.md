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

For a registered model run, choose `--strategy direct` with `reference-read` or `--strategy consult` with `reference-service`. Replace `--preflight` with `--auth` and `--catalog` paths from the existing subscription profile. Each model arm consumes subscription usage. The driver creates an ephemeral Linux mount namespace, pins the native executable hash, selects one model descriptor, and mounts the credential read-only in the main app-server namespace. Consult also mounts it read-only in the separate namespace shared by the trusted service observer and its native client. Model tools read the caller's fixture scope and bound workspace.

The direct H01 tool reads the empty scope with `Domain::state`. The consult fixture binds the native main thread, accepted turn and tool-call identity to the frozen principal using `Invocation::bind`. It calls `Service::submit_consult` and reads the published result and receipt. The current request is a live event alongside zero retained historical inputs. The public registration path uses the OS user and has separate integration coverage.

The revised fixture syncs its empty index and permits two native reservations before opening the scheduled consult. The first pair's original response and failing status remain in the receipt. Episodes with retained history, persisted memories or changing grants require their own import and fence mappings.

Each attempt needs a new output filename in an existing writable private directory. Before account access the driver exclusively claims that filename with an `IN_PROGRESS` receipt. The terminal receipt atomically replaces the claim. After an interruption, inspect the existing receipt and check whether the attempt still runs before starting another paid turn. A failed or interrupted main-model turn retains its native terminal status and exits unsuccessfully. A completed turn may still fail the quality oracle recorded in its result.

New private receipts record `attempt_kind` for setup, offline preflight and model runs. In `timing`, `invocation_wall_s` starts at driver entry and ends after private scratch cleanup, immediately before final serialization. For attempts that reach the native service, `preparation_s` covers input checks, executable probes and namespace configuration inside the run function. The historical `total_s` clock starts after this preparation and stops after subprocess teardown. `setup_s` covers native service setup through main-thread configuration on that inner clock. The first pair receipt lists separately observed supervisor wall times and the original setup-only attempt.
