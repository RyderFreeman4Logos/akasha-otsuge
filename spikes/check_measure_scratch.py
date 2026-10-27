#!/usr/bin/env python3
"""Native scratch stays beside its receipts and is removed after success or failure."""
import json
import pathlib
import shutil
import subprocess
import sys
import uuid


def main():
    output = pathlib.Path(sys.argv[1]).resolve()
    output.mkdir(mode=0o700)
    (output / 'OWNER').write_text('akasha-otsuge MVP-003 observer scratch regression\n')
    child = '''import json,pathlib,subprocess,sys,tempfile
scratch=pathlib.Path(tempfile.gettempdir())
assert scratch.parent==pathlib.Path(sys.argv[1]),str(scratch)
(scratch/'payload').write_bytes(b'x'*1048576)
reply=dict(scratch=str(scratch))
if sys.argv[2]=='descendant':
 process=subprocess.Popen([sys.executable,'-c','import time;time.sleep(30)'])
 reply['descendant']=process.pid
 reply['start_ticks']=pathlib.Path(f'/proc/{process.pid}/stat').read_text().rsplit(')',1)[1].split()[19]
 print(json.dumps(reply),flush=True)
 raise SystemExit(0)
print(json.dumps(reply),flush=True)
raise SystemExit(int(sys.argv[2]))
'''
    records = []
    for mode, expected in [('0', 0), ('7', 7), ('descendant', 1)]:
        unit = 'akasha-scratch-test-' + uuid.uuid4().hex
        samples = output / f'{mode}.jsonl'
        command = ['systemd-run', '--user', '--quiet', '--wait', '--pipe', '--collect',
                   '--unit=' + unit, '-p', 'MemoryMax=134217728', '-p', 'MemorySwapMax=0',
                   '-p', 'RuntimeMaxSec=30', '-p', 'Environment=TMPDIR=/tmp',
                   sys.executable, str(pathlib.Path(__file__).with_name('measure.py')),
                   str(samples), sys.executable, '-c', child, str(output), mode]
        result = subprocess.run(command, capture_output=True, timeout=60)
        (output / f'{mode}.stdout').write_bytes(result.stdout)
        (output / f'{mode}.stderr').write_bytes(result.stderr)
        events = [json.loads(line) for line in samples.read_text().splitlines()]
        records.append(dict(unit=unit, command=command, exit=result.returncode, expected=expected))
        (output / 'commands.json').write_text(json.dumps(records, indent=2) + '\n')
        assert events[-1]['event'] == 'exit' and events[-1]['code'] == expected, events[-1]
        reply = json.loads(result.stdout)
        scratch = pathlib.Path(reply['scratch'])
        assert scratch.parent == output, scratch
        assert events[0]['native_environment']['TMPDIR'] == str(scratch)
        state = subprocess.run(['systemctl', '--user', 'is-active', unit], capture_output=True, text=True)
        assert state.stdout.strip() in {'inactive', 'unknown', 'failed'}, state.stdout
        if mode == 'descendant':
            assert not events[-1]['scratch_cleanup']['removed'] and scratch.exists()
            assert reply['descendant'] in events[-1]['scratch_cleanup']['live_pids']
            stat = pathlib.Path(f"/proc/{reply['descendant']}/stat")
            if stat.exists():
                fields = stat.read_text().rsplit(')', 1)[1].split()
                assert fields[19] != reply['start_ticks'] or fields[0] in {'Z', 'X'}
            shutil.rmtree(scratch)
        else:
            assert events[-1]['scratch_cleanup']['removed'] and not scratch.exists()
    (output / 'result.json').write_text(json.dumps(dict(status='passed', cases=records), indent=2) + '\n')
    print(json.dumps(dict(status='passed', cases=len(records))))


if __name__ == '__main__':
    main()
