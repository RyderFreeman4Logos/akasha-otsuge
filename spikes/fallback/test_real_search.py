"""A full reference population cannot silently collapse to one query per scope."""
import copy
import json
import pathlib
import sys
import tempfile
import unittest

from check_real_search import selected_queries, run_native


class ExactPopulation(unittest.TestCase):
    def test_all_reference_queries_and_diagnostic_scope_sampling(self):
        population = dict(queries=[dict(memory=1_000_000+i, partition=f'p{i%8}',
            vector_offset=i*1536*4, exact_neighbors=[f'neighbor:{j}' for j in range(20)])
            for i in range(32)])
        self.assertEqual(len(selected_queries(population, 32)), 32)
        for invalid in ['missing_reference', 'duplicate_reference', 'outside_corpus']:
            value = copy.deepcopy(population)
            if invalid == 'missing_reference':del value['queries'][0]['exact_neighbors']
            if invalid == 'duplicate_reference':value['queries'][0]['exact_neighbors'][1] = 'neighbor:0'
            if invalid == 'outside_corpus':value['queries'][0]['memory'] = 1_000_032
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                selected_queries(value, 32)
        for query in population['queries']:del query['exact_neighbors']
        self.assertEqual(len(selected_queries(population, 32)), 8)


class NativeFailureReceipt(unittest.TestCase):
    def test_timeout_preserves_identity_partial_output_and_exit(self):
        with tempfile.TemporaryDirectory() as directory:
            output, records = pathlib.Path(directory), []
            command = [sys.executable, '-c', 'import time;print("partial",flush=True);time.sleep(30)']
            stdout, stderr = run_native(command, output, records, timeout=0.3)
            record = json.loads((output / 'commands.json').read_text())[0]
            self.assertEqual(stdout, b'partial\n')
            self.assertEqual((output / '000.stdout').read_bytes(), stdout)
            self.assertTrue(record['timed_out'])
            self.assertLess(record['exit'], 0)
            self.assertIsInstance(record['child']['pid'], int)
            self.assertTrue(record['child']['start_ticks'])
            self.assertFalse(pathlib.Path(f"/proc/{record['child']['pid']}").exists())
            self.assertEqual(record['phase'], 'completed')


if __name__ == '__main__':unittest.main()
