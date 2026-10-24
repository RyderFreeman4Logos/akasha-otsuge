"""A writer may be between writes when the observer snapshots its log."""
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch

from run_generation import observe


class GenerationObserver(unittest.TestCase):
    def test_prune_observes_its_own_table_checkpoint(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=pathlib.Path(temporary)
            stale=dict(completed=999,total=1000,pid=42,start_ticks='123')
            current={**stale,'completed':2,'total':4,'deadline_ms':10000}
            (root/'generation-2-backfill.json').write_text(json.dumps(stale))
            (root/'generation-2-prune.json').write_text(json.dumps(current))
            log=root/'engine.log';log.write_text('')
            read_text=pathlib.Path.read_text
            def read(path,*args,**kwargs):
                return '0::/test-unit.service\n' if str(path)=='/proc/42/cgroup' else read_text(path,*args,**kwargs)
            with patch('run_generation.process_identity',return_value=dict(pid=42,start_ticks='123',state='R')):
                with patch.object(pathlib.Path,'read_text',read):
                    actual=observe(root,dict(op='prune_versions',generation=2,deadline_ms=10000),
                                   dict(unit='test-unit',log=str(log)))
            self.assertEqual(actual,current)

    def test_named_verifier_observes_only_its_checkpoint(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            legacy = dict(phase='oracle_verify', completed=999, pid=42, start_ticks='123')
            named = {**legacy, 'completed': 7}
            (root/'oracle-verify-text-2048.json').write_text(json.dumps(legacy))
            (root/'oracle-verify-text-post-index-2048.json').write_text(json.dumps(named))
            log = root/'engine.log';log.write_text('')
            read_text = pathlib.Path.read_text
            def read(path, *args, **kwargs):
                return '0::/test-unit.service\n' if str(path)=='/proc/42/cgroup' else read_text(path,*args,**kwargs)
            with patch('run_generation.process_identity', return_value=dict(pid=42,start_ticks='123',state='R')):
                with patch.object(pathlib.Path, 'read_text', read):
                    actual = observe(root, dict(op='verify_text',generation=2,count=2048,verification_id='post-index'),
                                     dict(unit='test-unit',log=str(log)))
            self.assertEqual(actual, named)

    def test_partial_last_progress_keeps_last_complete_checkpoint(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            log = root / 'engine.log'
            progress = dict(phase='tokenize_docs', progress=dict(completed=100))
            complete = 'INDEX_PROGRESS:' + json.dumps(progress) + '\n'
            partial = 'INDEX_PROGRESS:{"phase":"tokenize_docs","progress":'
            for content, expected in [(partial, None), (complete + partial, progress), (complete, progress)]:
                log.write_text(content)
                actual = observe(root, {'op': 'build', 'generation': 2}, {'log': str(log)})
                self.assertEqual(None if actual is None else actual['native_index'], expected)
            log.write_text(partial + '\n')
            with self.assertRaises(json.JSONDecodeError):
                observe(root, {'op': 'build', 'generation': 2}, {'log': str(log)})


if __name__ == '__main__':
    unittest.main()
