"""A writer may be between writes when the observer snapshots its log."""
import json
import pathlib
import tempfile
import unittest

from run_generation import observe


class GenerationObserver(unittest.TestCase):
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
