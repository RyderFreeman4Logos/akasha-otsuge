#!/usr/bin/env python3
"""Small deterministic content and resumed-selection contract checks."""
import hashlib
import unittest

from prepare_text_population import reconcile, sample


class TextPopulationTests(unittest.TestCase):
    def test_exact_content_identity_and_global_ordinal(self):
        body = 'Independent verification preserves immutable documents.'
        digest = hashlib.sha256(body.encode()).hexdigest()
        record = dict(key='fineweb:'+digest, body=body, partition='p3')
        query = sample(record, 1_234_567)
        self.assertEqual(query['memory'], 2_234_567)
        self.assertEqual(query['body_sha256'], digest)
        self.assertTrue(query['text_only'])
        self.assertNotIn('embedding', query)
        self.assertIsNone(sample({**record, 'embedding':[1.0]}, 0))
        with self.assertRaisesRegex(ValueError, 'source content'):
            sample({**record, 'body':body+' changed'}, 0)

    def test_completed_samples_are_reverified_without_duplicate_effects(self):
        first, second = dict(memory=2_000_000), dict(memory=2_010_000)
        selected = []
        self.assertTrue(reconcile(selected, 0, first))
        self.assertFalse(reconcile(selected, 0, first.copy()))
        with self.assertRaisesRegex(ValueError, 'changed'):
            reconcile(selected, 0, second)
        with self.assertRaisesRegex(ValueError, 'gap'):
            reconcile(selected, 2, second)
        self.assertEqual(selected, [first])
        self.assertTrue(reconcile(selected, 1, second))
        self.assertEqual(selected, [first, second])


if __name__ == '__main__':
    unittest.main()
