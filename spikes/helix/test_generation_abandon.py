"""Historical receipt compatibility rejects unknown or changed inputs."""
import json
import pathlib
import subprocess
import sys
import tempfile
import unittest

from run_case import digest, publish


class HistoricalAbandonment(unittest.TestCase):
    def test_frozen_initial_controller_requires_original_request_and_manifest(self):
        with tempfile.TemporaryDirectory(prefix='akasha-legacy-abandon-') as temporary:
            root = pathlib.Path(temporary)
            (root/'OWNER').write_text('akasha-otsuge MVP-003 legacy fixture\n')
            database, output = root/'database', root/'run'
            database.mkdir();output.mkdir()
            (output/'OWNER').write_text('akasha-otsuge MVP-003 owned generation operation\n')
            (output/'probe').write_bytes(b'fixture binary identity only')
            manifest = root/'manifest.json';manifest.write_text('{}\n')
            publish(output/'request.json', dict(op='begin_corpus', manifest=str(manifest)))
            config = dict(run=dict(id='legacy'), paths=dict(output=str(output),database=str(database)))
            (output/'config.toml').write_text(f'[run]\nid="legacy"\n[paths]\noutput={json.dumps(str(output))}\ndatabase={json.dumps(str(database))}\n')
            publish(database/'.akasha-generation-owner.json', dict(output=str(output),run_id='legacy'))
            identity = dict(config=config,binary_sha256=digest(output/'probe'),request_sha256=digest(output/'request.json'),manifest_sha256=digest(manifest),controller_sha256='f485cdc29afd8d60963888dceebab4d27ba0859b134965a97bece7b88dc5a0f3')
            state = dict(identity=identity,phase='interrupted',controller=dict(pid=2**30,start_ticks='0'),attempts=[])
            path = output/'state.json';publish(path,state)
            command = [sys.executable,str(pathlib.Path(__file__).with_name('reconcile_generation.py')),str(output),'--abandon','legacy']
            def run(success):
                result = subprocess.run(command,capture_output=True,text=True,timeout=10)
                self.assertEqual(result.returncode == 0,success,result.stderr)
            identity['controller_sha256']='unknown';publish(path,state);run(False)
            identity['controller_sha256']='f485cdc29afd8d60963888dceebab4d27ba0859b134965a97bece7b88dc5a0f3';publish(path,state)
            manifest.write_text('{"changed":true}\n');run(False)
            manifest.write_text('{}\n')
            original=(output/'request.json').read_bytes();(output/'request.json').write_bytes(original+b' ');run(False)
            (output/'request.json').write_bytes(original);run(True)
            saved=digest(path);run(True);self.assertEqual(digest(path),saved)
            receipt=json.loads(path.read_text())['abandonment']
            self.assertEqual(receipt['external_artifact_sha256'],{str(manifest):digest(manifest)})


if __name__ == '__main__':
    unittest.main()
