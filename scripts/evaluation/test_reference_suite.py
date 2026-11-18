import copy
import gzip
import hashlib
import json
import unittest
from collections import Counter
from pathlib import Path

from reference_judgment import score, task_score, worksheet
from reference_suite import COST_FIELDS, canonical, input_only, plan, read_reference

ROOT = Path(__file__).resolve().parents[2]
_, TASKS = read_reference()
BY_ID = {t["id"]: t for t in TASKS}


def blank_assessment(task, answer):
    return {
        "anonymous_answer_id": "answer-a",
        "public_answer": answer,
        "public_answer_sha256": hashlib.sha256(answer.encode()).hexdigest(),
        "subject_sha256": plan(task)["subject_sha256"],
        "typed_status": task["oracle"]["status"],
        "gaps": task["oracle"]["gaps"],
        "terminal_received": True,
        "episode_import_verified": True,
        "all_returned_items_checked": True,
        "invariant_violations": [],
        "factual_memory_uses": [],
        "credited_items": [],
        "propositions": [],
        "reason": "Synthetic format test.",
    }


class ReferenceSuiteTests(unittest.TestCase):
    def test_oracles_never_enter_native_subjects(self):
        for task in TASKS:
            source = input_only(task)
            self.assertTrue(
                set(source).isdisjoint(
                    {"critical", "oracle", "category", "intent_pair", "variant"}
                )
            )
            edited = copy.deepcopy(task)
            edited["oracle"] = {"status": "changed"}
            edited["critical"] = not task["critical"]
            self.assertEqual(canonical(source), canonical(input_only(edited)))

    def test_intent_pairs_share_request_scope_and_history(self):
        groups = {}
        for task in TASKS:
            if key := task["intent_pair"]:
                groups.setdefault(key, []).append(plan(task))
        self.assertEqual(len(groups), 10)
        for left, right in groups.values():
            self.assertEqual(left["main_request_sha256"], right["main_request_sha256"])
            self.assertEqual(
                left["intent_shared_input_sha256"], right["intent_shared_input_sha256"]
            )
            self.assertNotEqual(left["hint_sha256"], right["hint_sha256"])

    def test_absent_observations_never_shrink_denominators(self):
        result = score(TASKS, {})
        self.assertEqual(result["acceptance"], "incomplete")
        self.assertEqual(len(result["missing_or_incomplete"]), 60)
        self.assertEqual(len(result["critical_not_satisfied"]), 31)
        self.assertEqual(
            result["noncritical_items"], {"earned": 0, "frozen_denominator": 36}
        )
        self.assertEqual(result["irrelevance"]["zero_denominator_fraction"], 0)
        self.assertIn("billed_amount", COST_FIELDS)
        self.assertIn("main_added_output_tokens", COST_FIELDS)

    def test_blank_worksheet_rejects_unobserved_import_or_terminal(self):
        w = worksheet(TASKS)
        self.assertEqual(score(TASKS, w)["acceptance"], "incomplete")
        record = blank_assessment(BY_ID["H01"], "No data.")
        record["episode_import_verified"] = False
        self.assertEqual(task_score(BY_ID["H01"], record)["state"], "incomplete")

    def test_actual_h01_typed_mismatch_survives_anonymous_input(self):
        public = json.loads(
            (
                ROOT / "docs/benchmarks/results/mvp027/h01-subscription-r1.json"
            ).read_bytes()
        )
        consult = public["consult"]
        record = blank_assessment(BY_ID["H01"], consult["main_messages"][-1])
        record.update(
            typed_status=consult["publication"]["status"],
            gaps=consult["publication"]["gaps"],
        )
        self.assertIn(
            "status_or_gap_mismatch", task_score(BY_ID["H01"], record)["reasons"]
        )
        record["strategy"] = "consult"
        with self.assertRaisesRegex(ValueError, "blind"):
            task_score(BY_ID["H01"], record)

    def test_no_partial_credit_for_omitted_condition(self):
        t = BY_ID["D01"]
        memory = t["memories"][0]
        item = t["oracle"]["required_items"][0]
        record = blank_assessment(t, item["claim"] + " " + " ".join(item["conditions"]))
        match = {"memory_id": item["memory_id"], "revision": item["revision"]}
        record["factual_memory_uses"] = [
            {
                **match,
                "source_ids": item["source_ids"],
                "scope": memory["scope"],
                "authority": item["authority"],
            }
        ]
        record["credited_items"] = [
            {
                **item,
                "excerpt": record["public_answer"],
                "reason": "Literal claim and condition.",
            }
        ]
        record["propositions"] = [
            {
                "text": record["public_answer"],
                "excerpt": record["public_answer"],
                "reason": "One complete item.",
                "relevant": True,
                "satisfies": [match],
            }
        ]
        self.assertEqual(task_score(t, record)["credited"], 1)
        record["credited_items"][0]["conditions"] = []
        record["propositions"][0].update(relevant=False, satisfies=[])
        self.assertEqual(task_score(t, record)["credited"], 0)
        record["factual_memory_uses"][0]["authority"] = "candidate"
        self.assertIn(
            "factual_memory_provenance_mismatch", task_score(t, record)["reasons"]
        )

    def test_accepted_proposition_needs_a_public_excerpt_and_supported_item(self):
        t = BY_ID["H01"]
        r = blank_assessment(t, "No decision found.")
        r["propositions"] = [
            {
                "text": "other",
                "excerpt": "missing text",
                "reason": "test",
                "relevant": False,
            }
        ]
        with self.assertRaisesRegex(ValueError, "excerpt"):
            task_score(t, r)
        r["propositions"][0].update(excerpt="No decision", relevant=True)
        with self.assertRaisesRegex(ValueError, "relevance"):
            task_score(t, r)

    def test_s04_erratum_requires_error_and_no_success_receipt(self):
        t = BY_ID["S04"]
        r = blank_assessment(t, "Permission denied.")
        self.assertIn("denial_or_receipt_mismatch", task_score(t, r)["reasons"])
        r.update(error_code="READ_DENIED", successful_receipt=False)
        self.assertEqual(task_score(t, r)["reasons"], [])

    def test_native_artifact_records_every_scope_and_hash(self):
        path = ROOT / "docs/benchmarks/results/mvp027/offline-import-r1.json.gz"
        data = json.loads(gzip.decompress(path.read_bytes()))
        self.assertEqual(data["summary"]["native_failures"], 0)
        self.assertEqual(data["summary"]["native_cases"], 60)
        self.assertEqual(data["summary"]["model_turns_in_this_command"], 0)
        counts = Counter()
        for row in data["rows"]:
            t = BY_ID[row["id"]]
            n = row["native"]
            self.assertEqual(row["subject_sha256"], plan(t)["subject_sha256"])
            self.assertEqual(n["scope"], t["caller"]["scope"])
            self.assertEqual(
                set(n["native_visible_ids"]), set(t["caller"]["allowed_event_ids"])
            )
            self.assertEqual(n["index"]["watermark"], n["index"]["target"])
            self.assertGreater(n["index"]["target"], 0)
            self.assertEqual(
                {(s["id"], s["sha256"]) for s in n["sources"]},
                {(s["id"], s["sha256"]) for s in t["sources"]},
            )
            for s in n["sources"]:
                self.assertTrue(s["request_id"].startswith("capture-publish/"))
            counts.update(m.get("authority", m.get("error")) for m in n["memories"])
        self.assertEqual(
            counts,
            {
                "candidate": 3,
                "delegated_approved": 1,
                "human_confirmed": 44,
                "AUTHORITY_DENIED": 33,
                "PREDECESSOR_NOT_IMPORTED": 1,
            },
        )

    def test_archived_transition_inputs_and_outputs(self):
        archive = ROOT / "docs/benchmarks/results/mvp027/offline-transitions-r1.json.gz"
        report = json.loads(gzip.decompress(archive.read_bytes()))
        self.assertEqual([r["id"] for r in report["rows"]], list(BY_ID))
        self.assertEqual(report["summary"]["native_failures"], 0)
        self.assertEqual(report["summary"]["model_turns_in_this_command"], 0)
        self.assertEqual(
            report["native_build"]["source_sha256"],
            hashlib.sha256(
                (ROOT / "scripts/evaluation/reference_import.rs").read_bytes()
            ).hexdigest(),
        )
        refused = Counter()

        def verify_stdout(raw):
            n = copy.deepcopy(raw)
            expected = n.pop("observer_stdout_sha256")
            n.pop("outside_lineage_keyword_candidates", None)
            for m in n["memories"]:
                m.pop("declared_input", None)
            self.assertEqual(hashlib.sha256(canonical(n) + b"\n").hexdigest(), expected)

        for r in report["rows"]:
            t = BY_ID[r["id"]]
            n = r["native"]
            self.assertEqual(
                r["subject_sha256"],
                hashlib.sha256(canonical(input_only(t))).hexdigest(),
            )
            verify_stdout(n)
            originals = {s["id"]: s for s in t["sources"]}
            for m in n["memories"]:
                if "error" not in m:
                    continue
                d = m["declared_input"]
                self.assertEqual(d["authority"], "human_confirmed")
                self.assertTrue(d["conditions_count"] > 0)
                self.assertEqual(d["channels"], ["verified_human"])
                self.assertEqual(
                    d["source_sha256"],
                    [originals[k]["sha256"] for k in d["source_ids"]],
                )
                refused[
                    m["error"],
                    d["conditions_in_original_text"],
                    d["claim_is_whole_original"],
                ] += 1
        self.assertEqual(
            refused,
            {
                ("AUTHORITY_DENIED", True, False): 33,
                ("PREDECESSOR_NOT_IMPORTED", False, True): 1,
            },
        )
        for c in report["ungraded_verbatim_controls"]:
            t = BY_ID[c["registered_task"]]
            v = copy.deepcopy(input_only(t))
            self.assertEqual(
                hashlib.sha256(canonical(v)).hexdigest(), c["registered_input_sha256"]
            )
            v["id"] = "control-" + v["id"]
            for m in v["memories"]:
                s = next(s for s in t["sources"] if s["id"] == m["source_ids"][0])
                m["claim"] = s["text"]
                m["conditions"] = []
            self.assertEqual(
                hashlib.sha256(canonical(v)).hexdigest(), c["control_input_sha256"]
            )
            self.assertEqual(c["registered_result_credit"], 0)
            verify_stdout(c["observation"])

    def test_archived_granted_historical_reads_and_terminal_transitions(self):
        raw = (
            ROOT / "docs/benchmarks/results/mvp027/offline-transitions-r1.json.gz"
        ).read_bytes()
        report = json.loads(gzip.decompress(raw))
        native = {r["id"]: r["native"] for r in report["rows"]}
        controls = {
            c["registered_task"]: c["observation"]
            for c in report["ungraded_verbatim_controls"]
        }
        self.assertEqual(set(controls), {"V02", "V03", "A04"})
        for n in (native["V02"], controls["V02"]):
            h = n["historical_read"]
            self.assertEqual(h["as_of_ordinal"], 1)
            self.assertLessEqual(h["source_commit"], h["physical_cutoff"])
            self.assertEqual(
                (h["historical"]["read_seq"], h["historical"]["mode"]),
                (h["physical_cutoff"], "historical_state"),
            )
            self.assertGreater(h["current"]["read_seq"], h["physical_cutoff"])
            self.assertEqual(
                h["historical"]["validated_at_seq"], h["current"]["validated_at_seq"]
            )
        self.assertEqual(
            native["V02"]["historical_read"]["historical"]["state"], "absent"
        )
        h = controls["V02"]["historical_read"]
        self.assertEqual(
            [m["commit"] for m in controls["V02"]["memories"]],
            [h["historical"]["commit"], h["current"]["commit"]],
        )
        self.assertNotEqual(
            h["historical"]["semantic_sha256"], h["current"]["semantic_sha256"]
        )
        self.assertEqual(
            native["V03"]["transitions"][0]["outcome"], "predecessor_not_imported"
        )
        for n, old, new in [
            (native["C03"], "candidate", "rejected"),
            (controls["V03"], "active", "retired"),
        ]:
            (x,) = n["transitions"]
            self.assertEqual(
                (x["previous_lifecycle"], x["action"], x["revision_after"]),
                (old, "reject", new),
            )
            self.assertLess(x["previous_commit"], x["commit"])
        for n, state in ((native["A04"], "absent"), (controls["A04"], "active")):
            x = n["rescope_attempt"]
            self.assertLess(x["grant_commit"], x["first_read_commit"])
            self.assertGreaterEqual(x["receipt_validated_at"], x["first_read_commit"])
            self.assertEqual(x["before"]["state"], state)
            self.assertEqual(x["after"], x["before"])
            self.assertEqual(
                (x["mutation_error"], x["private_read_error"]),
                ("SCOPE_DENIED", "READ_DENIED"),
            )
            self.assertIsNone(x["mutation_commit"])
            self.assertIsNone(x["model_publication"])


if __name__ == "__main__":
    unittest.main()
