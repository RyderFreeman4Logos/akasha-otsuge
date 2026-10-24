"""Independent current-read receipts must match the pinned public corpus."""
import copy
import hashlib
import json
import pathlib
import tempfile
import unittest

from run_case import digest, verify_requests


class CorpusCurrentOracle(unittest.TestCase):
    def test_pinned_content_and_identity_cannot_be_replaced_by_self_report(self):
        with tempfile.TemporaryDirectory(prefix="akasha-current-oracle-") as temporary:
            root = pathlib.Path(temporary)
            (root / "runtime").mkdir()
            run, base = "corpus-oracle-fixture", 1_000_000_000
            heads = []
            for i in range(50):
                prefix = f"Synthetic workload; excluded from corpus. Run {run}; client {i}; request 0. "
                heads.append(dict(memory=base+i, seq=i+1, scope=i % 8, active=True,
                                  body_sha256=hashlib.sha256((prefix + "x" * (4096-len(prefix))).encode()).hexdigest()))
            (root / "runtime/baseline.json").write_text(json.dumps(dict(base=base, run_id=run, heads=heads)))
            query = dict(memory=1_000_000, key="public:point", partition="p3", body_sha256=hashlib.sha256(b"public original").hexdigest())
            fixture = root / "queries.json"
            fixture.write_text(json.dumps(dict(queries=[query])))
            (root / "request.json").write_text(json.dumps(dict(fixture=str(fixture))))
            (root / "runtime/admission.json").write_text(json.dumps(dict(fixture_sha256=digest(fixture))))
            corpus = dict(at=50, query_index=0, key=query["key"], permission_revision=1,
                          entry=dict(memory=query["memory"], seq=0, scope=3, active=True, body_sha256=query["body_sha256"]))
            row = dict(client=60, ordinal=1, operation="current", status="success",
                       receipt=dict(at=50, memory=base, entry=heads[0], corpus=corpus))
            requests = root / "runtime/requests.jsonl"
            for case in ["missing", "body", "memory", "scope", "key", "future", "source"]:
                changed = copy.deepcopy(row)
                if case == "missing":del changed["receipt"]["corpus"]
                if case == "body":changed["receipt"]["corpus"]["entry"]["body_sha256"] = "0" * 64
                if case == "memory":changed["receipt"]["corpus"]["entry"]["memory"] += 1
                if case == "scope":changed["receipt"]["corpus"]["entry"]["scope"] = 4
                if case == "key":changed["receipt"]["corpus"]["key"] = "forged"
                if case == "future":changed["receipt"]["corpus"]["at"] = 51
                if case == "source":fixture.write_text(fixture.read_text() + "\n")
                requests.write_text(json.dumps(changed) + "\n")
                with self.subTest(case=case), self.assertRaises(ValueError):
                    verify_requests(root)
                fixture.write_text(json.dumps(dict(queries=[query])))
            requests.write_text(json.dumps(row) + "\n")
            result = verify_requests(root)
            self.assertEqual(result["corpus_current_checks"], 1)
            self.assertEqual(result["current_checks"], 1)
            (root / "request.json").write_text(json.dumps(dict(fixture=str(fixture), generation=1)))
            derived = dict(progress=dict(generation=1, committed=50, text=50, vector=50, remaining=0), indexed_jobs=100, text=heads, vectors=[])
            derived_path = root / "runtime/derived-oracle.json"
            for case in ["body", "jobs", "generation", "watermark", "commit", "vectors", "missing", "duplicate"]:
                changed = copy.deepcopy(derived)
                if case == "body":changed["text"][0]["body_sha256"] = "0" * 64
                if case == "jobs":changed["indexed_jobs"] -= 1
                if case == "generation":changed["progress"]["generation"] = 2
                if case == "watermark":changed["progress"]["text"] -= 1
                if case == "commit":changed["progress"]["committed"] += 1
                if case == "vectors":changed["vectors"] = [dict(memory=base)]
                if case == "missing":changed["text"].pop()
                if case == "duplicate":changed["text"].append(changed["text"][0])
                derived_path.write_text(json.dumps(changed))
                with self.subTest(derived=case), self.assertRaises(ValueError):
                    verify_requests(root)
            derived_path.write_text(json.dumps(derived))
            self.assertEqual(verify_requests(root)["derived_current_checks"], 50)
            query['text_only'] = True
            fixture.write_text(json.dumps(dict(queries=[query])))
            (root / 'runtime/admission.json').write_text(json.dumps(dict(fixture_sha256=digest(fixture))))
            hits = [dict(memory=query['memory'], seq=0, partition='p3', key=query['key'])]
            search = dict(client=80, ordinal=0, operation='bm25', status='success', receipt=dict(
                query_index=0, query_key=query['key'], partition='p3', hits=hits,
                validation=dict(generation=1, read_seq=50, validated_at_seq=50, hits=hits, permission_revision=1)))
            for case in ['ann_text', 'index', 'key', 'scope', 'empty', 'unvalidated', 'future', 'generation', 'missing_generation']:
                changed = copy.deepcopy(search)
                if case == 'ann_text':changed['operation'] = 'ann'
                if case == 'index':changed['receipt']['query_index'] = 1
                if case == 'key':changed['receipt']['query_key'] = 'wrong'
                if case == 'scope':changed['receipt']['partition'] = 'p4'
                if case == 'empty':changed['receipt']['hits'] = []
                if case == 'unvalidated':changed['receipt']['validation']['hits'] = []
                if case == 'future':changed['receipt']['validation']['validated_at_seq'] = 51
                if case == 'generation':changed['receipt']['validation']['generation'] = 2
                if case == 'missing_generation':del changed['receipt']['validation']['generation']
                requests.write_text(json.dumps(row)+'\n'+json.dumps(changed)+'\n')
                with self.subTest(search=case), self.assertRaises(ValueError):
                    verify_requests(root)
            requests.write_text(json.dumps(row)+'\n'+json.dumps(search)+'\n')
            checked = verify_requests(root)
            self.assertEqual(checked['text_only_current_checks'], 1)
            self.assertEqual(checked['text_only_bm25_checks'], 1)
            self.assertEqual(checked['search_checks'], dict(bm25=1, ann=0))


class CorpusCorrectionOracleTest(unittest.TestCase):
    def test_real_candidate_changes_restore_and_reject_stale_hits(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            (root / 'runtime').mkdir()
            run, base = 'corpus-corrections', 1_000_000_000
            def body(client, ordinal):
                prefix = f'Synthetic workload; excluded from corpus. Run {run}; client {client}; request {ordinal}. '
                return prefix + 'x' * (4096-len(prefix))
            def hashed(value):
                return hashlib.sha256(value.encode()).hexdigest()
            heads = [dict(memory=base+i, seq=i+1, scope=i%8, active=True, body_sha256=hashed(body(i, 0))) for i in range(50)]
            queries = [dict(memory=1_000_000+i, key=f'protected:{i}', partition=f'p{i}', body_sha256=hashed(f'protected {i}'), text_only=True) for i in range(8)]
            corrections = [dict(memory=2_000_000+i, scope=i, key=f'original:{i}', body=f'original public body {i}', query_index=i) for i in range(8)]
            fixture, correction_file = root/'queries.json', root/'corrections.json'
            fixture.write_text(json.dumps(dict(queries=queries)))
            correction_file.write_text(json.dumps(corrections))
            original = [dict(memory=s['memory'], seq=0, scope=s['scope'], active=True, body_sha256=hashed(s['body']),
                key=s['key'], query_index=i, candidates=[dict(memory=s['memory'], seq=0, partition=f'p{i}', key=s['key'])]) for i,s in enumerate(corrections)]
            (root/'request.json').write_text(json.dumps(dict(fixture=str(fixture), corrections=str(correction_file), generation=1, diagnostic=True)))
            (root/'runtime/admission.json').write_text(json.dumps(dict(fixture_sha256=digest(fixture), corrections_sha256=digest(correction_file))))
            (root/'runtime/baseline.json').write_text(json.dumps(dict(base=base, run_id=run, heads=heads, corrections=original)))
            rows, restored = [], []
            for stage in range(4):
                ordinal = stage + 1
                for i, s in enumerate(corrections):
                    seq = 51 + stage*8 + i
                    entry = dict(memory=s['memory'], seq=seq, scope=i if stage in (0,3) else (i+1)%8,
                        active=stage != 2, body_sha256=hashed(s['body'] if stage==3 else body(92+i, ordinal)))
                    rows.append(dict(client=92+i, ordinal=ordinal, operation='correction', status='success', receipt=dict(commit_seq=seq, oracle=entry)))
                    if stage == 3: restored.append(entry)
            derived = dict(progress=dict(generation=1, committed=82, text=82, vector=82, remaining=0), indexed_jobs=164,
                           text=sorted(heads+restored, key=lambda x:x['memory']), vectors=[])
            (root/'runtime/derived-oracle.json').write_text(json.dumps(derived))
            requests = root/'runtime/requests.jsonl'
            def write(values): requests.write_text(''.join(json.dumps(v)+'\n' for v in values))
            write(rows)
            result = verify_requests(root)
            self.assertEqual(result['corpus_correction_checks'], 32)
            self.assertEqual(result['corpus_correction_scopes'], list(range(8)))
            hit = dict(memory=2_000_000, seq=0, partition='p0', key='original:0')
            stale = dict(client=72, ordinal=0, operation='bm25', status='success', receipt=dict(query_index=0,
                query_key=queries[0]['key'], partition='p0', hits=[hit], validation=dict(generation=1, read_seq=51,
                validated_at_seq=51, hits=[hit], permission_revision=1)))
            write(rows+[stale])
            with self.assertRaisesRegex(ValueError, 'stale|acknowledged'):
                verify_requests(root)
            for case in ['body', 'scope', 'missing_restore', 'protected', 'identity']:
                changed = copy.deepcopy(rows)
                if case == 'body': changed[0]['receipt']['oracle']['body_sha256'] = '0'*64
                if case == 'scope': changed[8]['receipt']['oracle']['scope'] = 0
                if case == 'missing_restore': changed[-1]['status'] = 'error'
                if case == 'protected':
                    changed_source=copy.deepcopy(corrections);changed_source[0]['memory']=queries[0]['memory']
                    correction_file.write_text(json.dumps(changed_source))
                    (root/'runtime/admission.json').write_text(json.dumps(dict(fixture_sha256=digest(fixture),corrections_sha256=digest(correction_file))))
                if case == 'identity': correction_file.write_text(correction_file.read_text()+'\n')
                write(changed)
                with self.subTest(case=case), self.assertRaises(ValueError): verify_requests(root)
                correction_file.write_text(json.dumps(corrections))
                (root/'runtime/admission.json').write_text(json.dumps(dict(fixture_sha256=digest(fixture),corrections_sha256=digest(correction_file))))


if __name__ == "__main__":
    unittest.main()
