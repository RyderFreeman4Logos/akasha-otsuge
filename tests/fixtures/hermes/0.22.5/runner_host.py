#!/usr/bin/env python3
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import sys
import time
import uuid

if sys.argv[1:] == ['--version']:
    print('Hermes Agent v0.22.5+1915.g4960e41')
    sys.exit(0)
home = Path(os.environ['HERMES_HOME'])
scenario = (home / 'scenario').read_text()
assert sys.argv[1:3] == ['akasha-runner', '--request']
assert sys.argv[4] == '--budget' and 0 < float(sys.argv[5]) <= 1700
assert 'Read context.' in Path(sys.argv[3]).read_text()
assert not os.environ.get('HERMES_KANBAN_TASK')

def emit(value):
    print(json.dumps(value), flush=True)

def exchange(event):
    with socket.socket(socket.AF_UNIX) as client:
        client.settimeout(3)
        client.connect('\0' + os.environ['AKASHA_HERMES_RUN_SOCKET'])
        def exact(size):
            value = bytearray()
            while len(value) < size:
                chunk = client.recv(size - len(value))
                if not chunk:
                    raise EOFError()
                value.extend(chunk)
            return value
        def read():
            return json.loads(exact(struct.unpack('!I', exact(4))[0]))
        assert read()['protocol_version'] == 1
        token = os.environ['AKASHA_HERMES_RUN_TOKEN']
        if scenario == 'wrong_token':
            token = '0' * 64
        request = {'protocol_version': 1, 'request_id': uuid.uuid4().hex,
                   'command': {'op': 'hermes_context', 'token': token, 'event': event}}
        data = json.dumps(request).encode()
        client.sendall(struct.pack('!I', len(data)) + data)
        return read()['data']

emit({'type': 'system', 'subtype': 'init', 'session_id': ''})
if scenario == 'cancel':
    child = subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(30)'])
    (home / 'child.pid').write_text(str(child.pid))
ready = {'op': 'ready', 'home': str(home if scenario != 'wrong_home' else home / 'other'),
         'tools': ['context_describe', 'context_list', 'context_read', 'context_search']}
if scenario != 'no_ready':
    assert exchange(ready) == {'ready': True}
if scenario == 'double_ready':
    exchange(ready)
if scenario in ('cancel', 'timeout', 'eof_live'):
    if scenario == 'eof_live':
        os.close(1)
        os.close(2)
    time.sleep(30)
if scenario != 'no_ready':
    assert exchange({'op':'usage','input_tokens':200000 if scenario == 'token_budget' else 12,'output_tokens':7}) == {'accepted':True}
if scenario == 'usage_regression': exchange({'op':'usage','input_tokens':11,'output_tokens':7})
if scenario == 'denied_tool':
    emit({'type': 'tool_use', 'name': 'terminal'})
elif scenario == 'forged_args':
    exchange({'op': 'context', 'request': {'operation': 'describe', 'session_id': 'forged'}})
elif scenario == 'unissued_handle':
    exchange({'op': 'context', 'request': {'operation': 'read', 'handle': 'forged', 'start': 0, 'max_bytes': 100}})
elif scenario == 'success':
    result = exchange({'op': 'context', 'request': {'operation': 'describe'}})
    assert result['kind'] == 'description'
    emit({'type': 'tool_use', 'name': 'context_describe'})
    emit({'type': 'tool_result', 'name': 'context_describe', 'output': 'bounded', 'is_error': False})
if scenario == 'oversized':
    print('x' * 131073, flush=True)
elif scenario == 'stderr':
    print('x' * 8193, file=sys.stderr, flush=True)
elif scenario == 'partial':
    print('{', end='', flush=True)
elif scenario != 'no_final':
    result = {'type': 'result', 'session_id': 'native-runner-session', 'exit_code': 0,
              'text': 'bad' if scenario == 'malformed_final' else '{"answer":"bounded-context"}',
              'tokens': {'input': 12, 'output': 7}}
    emit(result)
    if scenario == 'double_final':
        emit(result)
sys.exit(7 if scenario == 'nonzero' else 0)
