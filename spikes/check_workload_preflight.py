#!/usr/bin/env python3
"""Invalid full-scale admission cannot initialize or mutate authority storage."""
import json
import pathlib
import subprocess
import sys
import tempfile


def main():
    binary, fixture, output = [pathlib.Path(v).resolve() for v in sys.argv[1:]]
    output.mkdir(mode=0o700)
    with tempfile.TemporaryDirectory(prefix='akasha-workload-preflight-') as temporary:
        root = pathlib.Path(temporary)
        (root/'OWNER').write_text('akasha-otsuge MVP-003 owned corpus import and database\n')
        database = root/'database'
        database.mkdir()
        (database/'OWNER').write_text('akasha-otsuge MVP-003 fallback database\n')
        manifest = root/'manifest.json'
        manifest.write_text(json.dumps(dict(status='diagnostic_only',corpus=dict(unique_content_utf8_bytes=1,unique_vectors=1))))
        request = root/'request.json'
        request.write_text(json.dumps(dict(run_id='invalid-full-admission',phase='steady',seconds=1800,
            fixture=str(fixture),output=str(root/'runtime'),corpus_manifest=str(manifest),diagnostic=False,generation=1)))
        child = subprocess.run([str(binary),str(database),'--workload',str(request)],capture_output=True,timeout=180)
        (output/'stdout').write_bytes(child.stdout)
        (output/'stderr').write_bytes(child.stderr)
        result = dict(exit=child.returncode,authority_entries=sorted(p.name for p in database.iterdir()))
        (output/'result.json').write_text(json.dumps(result,indent=2)+'\n')
        assert child.returncode!=0 and b'corpus does not meet frozen scale' in child.stderr,result
        assert result['authority_entries']==['OWNER'],result
    print(json.dumps(dict(status='passed',temporary_root_removed=not pathlib.Path(temporary).exists())))


if __name__=='__main__':main()
