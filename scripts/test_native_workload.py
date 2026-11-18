import asyncio
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import AsyncMock

from scripts.native_workload import __main__ as runner
from scripts.native_workload.journal import Journal, records
from scripts.native_workload.operations import NativeOperations, OperationError
from scripts.native_workload.schedule import run_schedule, summarize


class SearchRules(unittest.IsolatedAsyncioTestCase):
    def setUp(self):
        self.scope = dict(user="owner", project="alpha", branch_or_version="main", session="one")
        self.parent = dict(self.scope, branch_or_version=None, session=None)
        self.global_scope = dict(self.parent, project=None)
        self.expected = dict(
            mode="keyword",
            query="offline",
            scope=self.scope,
            top_k=20,
            required_refs=[["fact", 2]],
            excluded_ids=["blocked"],
            expected_rules=[
                dict(id="global", revision=4, scope=self.global_scope),
                dict(id="project", revision=6, scope=self.parent),
                dict(id="local", revision=8, scope=self.scope),
            ],
        )

    def item(self, key, seq, scope, kind="preference", authority="human_confirmed"):
        return dict(
            memory=dict(
                revision=dict(key=dict(id=key), commit_seq=seq),
                record=dict(draft=dict(scope=scope, kind=kind), authority=authority),
            )
        )

    async def search(self, *, oracle=None, rules=None, hits=None):
        expected = self.expected if oracle is None else oracle
        if rules is None:
            rules = [
                self.item(r["id"], r["revision"], r["scope"])
                for r in self.expected["expected_rules"]
            ]
        if hits is None:
            hits = [self.item("fact", 2, self.scope, "fact")]
        output = dict(
            mode=expected["mode"],
            query="offline",
            scope=self.scope,
            items=hits,
            rules=rules,
            coverage=dict(complete=True, gaps=[]),
        )
        ops = NativeOperations("unused")
        ops.job = AsyncMock(
            return_value=dict(result=dict(receipt_id="receipt", output=json.dumps(output)))
        )
        return await ops.search(expected, "job")

    async def test_admitted_rules_and_exact_scored_scope(self):
        for mode in ["keyword", "hybrid"]:
            result = await self.search(oracle=dict(self.expected, mode=mode))
            self.assertEqual(
                result["refs"], [("fact", 2), ("global", 4), ("local", 8), ("project", 6)]
            )
        with self.assertRaisesRegex(OperationError, "SEARCH_REQUIRED_HIT_MISSING"):
            await self.search(oracle=dict(self.expected, required_refs=[["local", 8]]))
        with self.assertRaisesRegex(OperationError, "SEARCH_REQUIRED_RULE_MISSING"):
            await self.search(rules=[])
        with self.assertRaisesRegex(OperationError, "SEARCH_AUTHORITY_OR_SCOPE_MISMATCH"):
            await self.search(hits=[self.item("fact", 2, self.parent, "fact")])

    async def test_rule_witnesses_reject_extra_wrong_or_duplicate_results(self):
        real = [
            self.item(r["id"], r["revision"], r["scope"]) for r in self.expected["expected_rules"]
        ]
        for bad in [
            self.item("unknown", 9, self.global_scope),
            self.item("unknown", 9, self.scope),
            self.item("blocked", 4, self.global_scope),
            self.item("fact", 2, self.scope),
            self.item("global", 4, self.global_scope),
            self.item("global", 5, self.global_scope),
        ]:
            with (
                self.subTest(bad=bad),
                self.assertRaisesRegex(OperationError, "SEARCH_AUTHORITY_OR_SCOPE_MISMATCH"),
            ):
                await self.search(rules=[*real, bad])
        for bad in [
            self.item("global", 5, self.global_scope),
            self.item("global", 4, self.parent),
            self.item("global", 4, dict(self.parent, project="beta")),
            self.item("global", 4, self.global_scope, "fact"),
            self.item("global", 4, self.global_scope, authority="candidate"),
        ]:
            with (
                self.subTest(bad=bad),
                self.assertRaisesRegex(OperationError, "SEARCH_AUTHORITY_OR_SCOPE_MISMATCH"),
            ):
                await self.search(rules=[bad, *real[1:]])
        for bad_hit in [self.item("fact", 3, self.scope), self.item("blocked", 1, self.scope)]:
            with (
                self.subTest(bad_hit=bad_hit),
                self.assertRaisesRegex(OperationError, "SEARCH_AUTHORITY_OR_SCOPE_MISMATCH"),
            ):
                await self.search(hits=[self.item("fact", 2, self.scope), bad_hit])

    async def test_rule_oracle_cannot_admit_a_sibling_or_excluded_id(self):
        rule = self.expected["expected_rules"][0]
        for scope in [
            dict(self.parent, user="other"),
            dict(self.parent, project="beta"),
            dict(self.scope, session="two"),
            dict(self.scope, branch_or_version="next"),
            dict(self.scope, project=None),
        ]:
            with (
                self.subTest(scope=scope),
                self.assertRaisesRegex(ValueError, "invalid search rule oracle"),
            ):
                await self.search(
                    oracle=dict(self.expected, expected_rules=[dict(rule, scope=scope)])
                )
        for invalid in [[dict(rule, id="blocked")], [rule, rule]]:
            with (
                self.subTest(invalid=invalid),
                self.assertRaisesRegex(ValueError, "invalid search rule oracle"),
            ):
                await self.search(oracle=dict(self.expected, expected_rules=invalid))

    async def test_legacy_query_keeps_pinned_same_scope_rule(self):
        legacy = self.expected.copy()
        del legacy["expected_rules"]
        self.assertEqual((await self.search(oracle=legacy, rules=[]))["refs"], [("fact", 2)])
        local = self.item("local", 8, self.scope)
        legacy["required_refs"] = [["fact", 2], ["local", 8]]
        self.assertEqual(
            (await self.search(oracle=legacy, rules=[local]))["refs"], [("fact", 2), ("local", 8)]
        )
        for unexpected in [
            self.item("unknown", 4, self.scope),
            self.item("local", 8, self.global_scope),
        ]:
            with (
                self.subTest(unexpected=unexpected),
                self.assertRaisesRegex(OperationError, "SEARCH_AUTHORITY_OR_SCOPE_MISMATCH"),
            ):
                await self.search(oracle=legacy, rules=[unexpected])


class WorkloadBounds(unittest.IsolatedAsyncioTestCase):
    async def test_timeout_includes_pending_terminal_poll(self):
        client = NativeOperations("unused", job_timeout_s=0.1)
        calls = []

        async def request(command, _identity):
            calls.append(command["op"])
            if command["op"] == "submit":
                return {"state": "running", "id": "job"}
            await asyncio.Event().wait()

        client.request = request
        with self.assertRaises(OperationError) as raised:
            await asyncio.wait_for(client.job({"op": "submit", "idempotency_key": "job"}, "job"), 1)
        self.assertEqual(raised.exception.code, "JOB_COMPLETION_TIMEOUT")
        self.assertTrue(raised.exception.detail["effect_may_have_started"])
        self.assertEqual(calls.count("submit"), 1)

    async def test_admission_write_cannot_authorize_expired_offer(self):
        events = []

        async def emit(row):
            events.append(row)
            if row["event"] == "admitted":
                await asyncio.sleep(0.2)

        execute = AsyncMock()
        selected = [dict(client=1, kind="original_write", period_s=1, offset_s=0)]
        await run_schedule("qualify", 0.15, execute, emit, selected)
        execute.assert_not_awaited()
        report = summarize(events, 0.15)
        self.assertTrue(report["complete"])
        self.assertEqual(report["kinds"]["original_write"]["counts"]["admission_expired"], 1)

    async def test_partial_journal_refuses_replay(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "requests.jsonl"
            journal = Journal(path)
            await journal.append({"event": "admitted"})
            journal.close()
            path.write_bytes(path.read_bytes()[:-1])
            with self.assertRaisesRegex(ValueError, "partial"):
                list(records(path))

    def test_group_peak_includes_children(self):
        report = {
            "complete": True,
            "progress_windows": [dict(start_s=0, end_s=1, successes={})],
            "kinds": {
                k: {
                    "counts": {"success_within_interval": 100},
                    "success_offered_latency_s": {"95": 0.1},
                }
                for k in runner.MINIMUMS
            },
        }
        sample = {
            "number": 0,
            "observed_monotonic_s": 0.2,
            "daemon": {"VmHWM": 60 * 2**20, "VmRSS": 50 * 2**20},
            "events": {"oom_kill": 0},
            "backlog": sample_tick(0)["backlog"],
            "status_storage_seq": 10,
            "resources": {
                "measurement": {"complete": True, "daemon_group": {"rss_bytes": 641 * 2**20}}
            },
        }
        gaps = runner.assess(report, [sample], 1, 512, 640, {"started_monotonic_s": 0})["gaps"]
        self.assertEqual(set(gaps), {"RSS maximum", "RSS p99"})
        sample["resources"]["measurement"]["complete"] = False
        with self.assertRaisesRegex(ValueError, "incomplete"):
            runner.assess(report, [sample], 1, 512, 640, {"started_monotonic_s": 0})


class AdmissionEvidence(unittest.IsolatedAsyncioTestCase):
    async def test_unobserved_retrieval_blocks_next_offer(self):
        from scripts.native_workload.operations_by_role import NativeWorkload

        for poll in ("timeout", "refused", "transport"):
            with self.subTest(poll=poll), tempfile.TemporaryDirectory() as temp:
                events = []
                submitted = []
                ops = NativeOperations("unused", job_timeout_s=0.09)

                async def request(cmd, _request_id):
                    if cmd["op"] == "retrieval_run":
                        submitted.append(cmd["idempotency_key"])
                        return dict(id=submitted[-1], state="running")
                    if poll == "transport":
                        from scripts.native_workload.ipc import TransportError

                        raise TransportError("connect", OSError())
                    if poll == "refused":
                        raise OperationError("RESOURCE_BUSY")
                    await asyncio.Event().wait()

                ops.request = request
                expected = dict(
                    query="fact", scope={}, top_k=20, mode="keyword", required_refs=[["fact", 1]]
                )
                plan = {
                    k: [expected] for k in ("current", "lexical", "hybrid_vector", "scoped_history")
                }
                plan["corrections"] = [dict(id=str(n)) for n in range(4)]
                workload = NativeWorkload(Path(temp), "pending", plan, ops, AsyncMock())

                async def emit(row):
                    events.append(row)

                await run_schedule(
                    "pending",
                    0.32,
                    workload,
                    emit,
                    [dict(kind="lexical", client=72, period_s=0.12, offset_s=0)],
                )
                self.assertEqual(len(submitted), 1)
                outcome = next(r["outcome"] for r in events if r["event"] == "outcome")
                self.assertTrue(outcome["effect_may_have_started"])
                self.assertEqual(outcome["detail"]["job_id"], submitted[0])

    def test_full_interval_rejects_double_winners_and_stalled_tail(self):
        from scripts.native_workload.schedule import clients, offers

        seconds = 1800

        def events(stall, double_winner):
            selected = clients()
            yield dict(
                event="interval",
                run_id="sim",
                clients=selected,
                duration_s=seconds,
                planned=sum(1 for c in selected for _ in offers(c, seconds)),
            )
            for c in selected:
                kind, client = c["kind"], c["client"]
                late = False
                for ordinal, due in offers(c, seconds):
                    key = f"sim/client/{client}/request/{ordinal}"
                    fields = dict(identity=key, kind=kind, client=client, ordinal=ordinal)
                    yield dict(event="offered", due_s=due, **fields)
                    if late:
                        yield dict(event="not_dispatched", identity=key, reason="client_busy")
                        continue
                    yield dict(event="admitted", **fields)
                    late = stall and kind in ("lexical", "hybrid_vector") and due >= 1560
                    finished = seconds + 0.1 if late else due + 0.01
                    outcome = dict(status="success")
                    if kind == "correction":
                        pair = (client - 92) // 2
                        outcome.update(
                            race=dict(target=str(pair), base_revision=7),
                            expected=dict(id=str(pair), revision=8 + client % 2),
                            receipt=dict(commit_seq=8 + client % 2),
                        )
                        if not double_winner and client % 2:
                            outcome.update(status="conflict", code="REVISION_CONFLICT")
                    yield dict(
                        event="outcome",
                        outcome=outcome,
                        due_s=due,
                        dispatch_s=due,
                        finished_s=finished,
                        latency_s=finished - due,
                        offered_latency_s=finished - due,
                        within_interval=finished < seconds,
                        **fields,
                    )
            yield dict(event="drained", continuous_interval_s=seconds, elapsed_s=seconds + 0.1)

        healthy = summarize(events(stall=False, double_winner=False), seconds)
        samples = [sample_tick(n) for n in range(seconds)]
        passing = runner.assess(healthy, samples, seconds, 512, 640, dict(started_monotonic_s=0))
        self.assertEqual(passing["gaps"], [])
        self.assertEqual(healthy["correction_races"]["counts"]["contested"], 360)
        report = summarize(events(stall=True, double_winner=True), seconds)
        result = runner.assess(report, samples, seconds, 512, 640, dict(started_monotonic_s=0))
        with self.subTest("race"):
            self.assertIn("correction race outcomes", result["gaps"])
        with self.subTest("sustained"):
            self.assertIn("lexical sustained throughput", result["gaps"])
            self.assertIn("hybrid_vector sustained throughput", result["gaps"])
            self.assertNotIn("lexical throughput", result["gaps"])
            self.assertGreater(report["kinds"]["lexical"]["counts"]["late_success"], 0)

    def test_sampler_tags_active_and_building_vector_lag_and_uses_authority_after_indexes(self):
        index = dict(
            generation="lex",
            lexical_watermark=10,
            vector=dict(generation="building", watermark=4),
            active_vector=dict(generation="serving"),
            active_vector_watermark=9,
            queue=dict(storage_seq=12, queued=1, running=2, queued_json_bytes=103),
        )
        backlog = runner.backlog_sample(index)
        self.assertEqual(backlog["vector"], dict(generation="building", watermark=4, lag=8))
        self.assertEqual(backlog["active_vector"], dict(generation="serving", watermark=9, lag=3))
        self.assertEqual(backlog["lexical"], dict(generation="lex", watermark=10, lag=2))
        self.assertTrue(runner.has_backlog_sample(dict(backlog=backlog, status_storage_seq=11)))
        index["active_vector_watermark"] = None
        with self.assertRaises(ValueError):
            runner.backlog_sample(index)

    def test_missing_queue_and_generation_samples_cannot_pass(self):
        report = dict(
            complete=True,
            kinds={
                k: dict(
                    counts=dict(success_within_interval=100), success_offered_latency_s={"95": 0.01}
                )
                for k in runner.MINIMUMS
            },
        )
        sample = sample_tick(0)
        sample.pop("backlog")
        result = runner.assess(report, [sample], 1, 512, 640, dict(started_monotonic_s=0))
        self.assertIn("queue and index sample coverage", result["gaps"])

    def test_correction_round_binds_prepared_target_base_and_each_dispatched_participant(self):
        from scripts.native_workload.schedule import correction_races

        success = dict(
            status="success",
            race=dict(target="one", base_revision=7),
            receipt=dict(commit_seq=8),
            expected=dict(id="one", revision=8),
        )
        conflict = dict(
            status="conflict", code="REVISION_CONFLICT", race=dict(target="one", base_revision=7)
        )

        def prep(key, base):
            return dict(
                kind="correction",
                identity=key,
                pair=0,
                ordinal=0,
                command=dict(
                    idempotency_key=key,
                    patch=dict(changes=[dict(memory_id="one", expected_revision=base)]),
                ),
            )

        entries = {(0, 0): {92: ("a", success), 93: ("b", conflict)}}
        targets = [{"id": "one"}]
        good = correction_races(entries, [prep("a", 7), prep("b", 7)], targets)
        self.assertEqual(good, dict(counts={"contested": 1}, invalid=[], incomplete=[]))
        self.assertTrue(correction_races(entries, [prep("a", 7), prep("b", 6)], targets)["invalid"])
        self.assertTrue(
            correction_races(entries, [prep("a", 7), prep("b", 7)], [{"id": "other"}])["invalid"]
        )
        entries[(0, 0)][93] = ("b", None)
        self.assertEqual(
            correction_races(entries, [prep("a", 7)], targets),
            dict(
                counts={"partial": 1},
                invalid=[],
                incomplete=[dict(pair=0, ordinal=0, dispatched=[92])],
            ),
        )

    async def test_failed_performance_keeps_original_oracle_and_rejects_ambiguous_preparation(self):
        import hashlib
        import json
        import os
        from scripts.native_workload.operations_by_role import body
        from scripts.native_workload.schedule import clients, offers
        from scripts.native_workload.verify import verify

        for ambiguous in (False, True):
            with self.subTest(ambiguous=ambiguous), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                data = root / "data"
                data.mkdir()
                (data / "fixture").touch()
                cfg = root / "daemon.toml"
                cfg.write_text(f"[paths]\ndata_dir={json.dumps(str(data))}\n")

                def sha(path):
                    return hashlib.sha256(path.read_bytes()).hexdigest()

                plan = dict(
                    run_id="one",
                    daemon_config=str(cfg),
                    daemon_config_sha256=sha(cfg),
                    oracles={"corrections": [dict(id=str(n)) for n in range(4)]},
                )
                (root / "admission.json").write_text(
                    json.dumps(dict(plan=plan, config={"execution": {"seconds": 1}}))
                )
                names = ["requests.jsonl", "preparations.jsonl", "metrics.jsonl"]
                req, prep, metrics = [Journal(root / name) for name in names]
                selected = clients()
                await req.append(
                    dict(
                        event="interval",
                        run_id="one",
                        duration_s=1,
                        clients=selected,
                        planned=sum(1 for c in selected for _ in offers(c, 1)),
                    )
                )
                for client in selected:
                    for n, due in offers(client, 1):
                        key = f"one/client/{client['client']}/request/{n}"
                        fields = dict(
                            identity=key, client=client["client"], kind=client["kind"], ordinal=n
                        )
                        await req.append(dict(event="offered", due_s=due, **fields))
                        if client["client"] != 0 and not (ambiguous and client["client"] == 1):
                            await req.append(
                                dict(event="not_dispatched", identity=key, reason="missed_slot")
                            )
                            continue
                        await req.append(dict(event="admitted", **fields))
                        number = client["client"]
                        raw = hashlib.sha256(body(key).encode()).hexdigest()
                        oracle = dict(
                            event_id=f"original-{n}",
                            stream="events",
                            raw_sha256=raw,
                            text_sha256=raw,
                            source=dict(
                                adapter="fixture",
                                profile="native-workload",
                                session="one",
                                thread=f"client-{number}",
                                agent="benchmark",
                            ),
                            scope=dict(
                                user=f"uid:{os.geteuid()}",
                                project=f"workload/one/{number}",
                                session=None,
                                branch_or_version=None,
                            ),
                        )
                        await prep.append(dict(identity=key, kind="original_write", oracle=oracle))
                        outcome = (
                            dict(
                                status="success",
                                duplicate_reused_exact_receipt=False,
                                receipt=dict(id=key, state="succeeded"),
                                original_oracle=oracle,
                                unique_original_bytes=4096,
                            )
                            if number == 0
                            else dict(status="error", effect_may_have_started=True)
                        )
                        await req.append(
                            dict(
                                event="outcome",
                                due_s=due,
                                dispatch_s=due,
                                finished_s=due + 0.01,
                                latency_s=0.01,
                                offered_latency_s=0.01,
                                within_interval=True,
                                outcome=outcome,
                                **fields,
                            )
                        )
                await req.append(dict(event="drained", continuous_interval_s=1, elapsed_s=1))
                for journal in (req, prep, metrics):
                    journal.close()
                (root / "result.json").write_text(
                    json.dumps(
                        dict(
                            requests={"complete": True},
                            assessment=dict(interval_thresholds_pass=False, gaps=["RSS p99"]),
                            journal_sha256={n: sha(root / n) for n in names},
                        )
                    )
                )
                reader = root / "reader"
                reader.write_text(
                    "#!/usr/bin/env python3\nimport json,sys\nfor n,line in enumerate(sys.stdin,1):\n r=json.loads(line); print(json.dumps(dict(verified=n,read_seq=7,event_revision=6,raw_sha256=r['raw_sha256'],text_sha256=r['text_sha256'])))\n"
                )
                reader.chmod(0o700)
                if ambiguous:
                    with self.assertRaisesRegex(ValueError, "unreconciled original"):
                        verify(root, reader)
                else:
                    verified = verify(root, reader)
                    self.assertEqual(verified["verified_originals"], 1)
                    self.assertFalse(verified["interval_thresholds_pass"])
                    self.assertEqual(verified["interval_gaps"], ["RSS p99"])

    def test_performance_failure_reaches_journal_integrity_check(self):
        import json
        from scripts.native_workload.verify import verify

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "admission.json").write_text("{}")
            (root / "requests.jsonl").write_bytes(b"changed")
            (root / "result.json").write_text(
                json.dumps(
                    dict(
                        assessment=dict(interval_thresholds_pass=False, gaps=["RSS p99"]),
                        journal_sha256={"requests.jsonl": "original SHA"},
                    )
                )
            )
            with self.assertRaisesRegex(ValueError, "journal changed"):
                verify(root, root / "unused")


def sample_tick(number):
    return dict(
        number=number,
        observed_monotonic_s=number + 0.05,
        status_storage_seq=10,
        daemon=dict(VmHWM=100),
        resources={"measurement": {"complete": True, "daemon_group": {"rss_bytes": 100}}},
        events={"oom_kill": 0},
        backlog=dict(
            storage_seq=10,
            queued=1,
            running=1,
            queued_json_bytes=128,
            active_vector=None,
            lexical=dict(generation="lex", watermark=8, lag=2),
            vector=dict(generation="vec", watermark=9, lag=1),
        ),
    )
