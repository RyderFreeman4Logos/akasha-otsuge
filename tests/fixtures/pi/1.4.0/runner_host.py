#!/usr/bin/env python3
"""Controlled native-shaped RPC peer; never an authenticated Pi acceptance receipt."""
import json
import os
from pathlib import Path
import subprocess
import sys
import time

if sys.argv[1:] == ['--version']:
    print('1.4.0')
    sys.exit(0)
home = Path(os.environ['PI_CODING_AGENT_DIR'])
scenario = (home / 'scenario').read_text()
for flag in ['--no-session', '--no-builtin-tools', '--no-extensions', '--no-mcp', '--no-skills',
             '--no-prompt-templates', '--no-themes', '--no-context-files', '--no-approve']:
    assert flag in sys.argv
assert sys.argv[sys.argv.index('--mode') + 1] == 'rpc'
tools = ['context_describe', 'context_list', 'context_read', 'context_search']
assert sys.argv[sys.argv.index('--tools') + 1].split(',') == tools
assert Path(sys.argv[sys.argv.index('-e') + 1]) == home / 'extensions/akasha-otsuge/index.ts'

def emit(value):
    print(json.dumps(value, ensure_ascii=False), flush=True)

def exchange(event):
    env = os.environ.copy()
    if scenario == 'wrong_token':
        env['AKASHA_PI_RUN_TOKEN'] = '0' * 64
    result = subprocess.run([(home / 'bridge-executable').read_text(), 'adapter', 'pi-context'],
                            input=json.dumps(event), text=True, capture_output=True,
                            env=env, timeout=5, check=True)
    return json.loads(result.stdout)['data']

if scenario == 'cancel':
    child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(30)'])
    (home / 'child.pid').write_text(str(child.pid))
ready = {'op':'ready', 'agent_dir':str(home if scenario != 'wrong_home' else home / 'other'),
         'version':'1.4.0', 'tools':tools, 'active_tools':tools}
if scenario == 'inventory':
    ready['tools'] = tools + ['bash']
if scenario == 'no_ready':
    emit({'type':'agent_start'})
    sys.exit(0)
assert exchange(ready) == {'ready':True}
if scenario == 'double_ready':
    exchange(ready)
prompt = json.loads(sys.stdin.readline())
assert prompt['type'] == 'prompt' and 'Read context.' in prompt['message']
response = {'type':'response', 'command':'prompt', 'id':prompt['id'], 'success':True,
            'data':{'disposition':'handled' if scenario == 'handled' else 'started'}}
if scenario == 'wrong_id': response['id'] = 'wrong'
if scenario == 'preflight_auth':
    response.update(success=False, error='No API key found for "fixture"')
emit(response)
emit({'type':'agent_start'})
if scenario in ['timeout', 'cancel', 'eof_live']:
    if scenario == 'eof_live': os.close(1); os.close(2)
    time.sleep(30)
if scenario == 'denied_tool':
    emit({'type':'tool_execution_start', 'toolName':'bash', 'toolCallId':'forbidden'})
if scenario == 'forged_args': exchange({'op':'context', 'request':{'operation':'describe', 'session_id':'forged'}})
if scenario == 'unissued_handle': exchange({'op':'context', 'request':{'operation':'read', 'handle':'forged', 'start':0, 'max_bytes':16}})
if scenario in ['success', 'retry', 'auth_retry']:
    assert exchange({'op':'context', 'request':{'operation':'describe'}})['kind'] == 'description'
    emit({'type':'tool_execution_start', 'toolName':'context_describe', 'toolCallId':'description'})
    emit({'type':'tool_execution_end', 'toolName':'context_describe', 'toolCallId':'description', 'isError':False})
if scenario in ['retry', 'auth_retry']:
    error = 'OAuth refresh failed for fixture: token refresh failed (401): invalid_refresh_token' if scenario == 'auth_retry' else 'TRANSIENT_PRIVATE_SENTINEL'
    emit({'type':'message_end', 'message':{'role':'assistant','content':[], 'stopReason':'error', 'errorMessage':error}})
    emit({'type':'agent_end','messages':[]})
    emit({'type':'auto_retry_start','attempt':1})
    emit({'type':'auto_retry_end','success':True})
if scenario == 'oversized': print('x' * 131073, flush=True)
elif scenario == 'stderr': print('x' * 8193, file=sys.stderr, flush=True)
elif scenario == 'partial': print('{', end='', flush=True); sys.exit(0)
if scenario != 'no_final':
    text = 'bad' if scenario == 'malformed_final' else '{"answer":"bounded-context\u2028retained"}'
    message = {'role':'assistant','content':[{'type':'text','text':text}], 'stopReason':'stop'}
    if scenario == 'expired_auth':
        message.update(content=[],stopReason='error',errorMessage='OAuth refresh failed for fixture: token refresh failed (401): invalid_refresh_token')
    emit({'type':'message_end','message':message})
emit({'type':'agent_end','messages':[]})
if scenario != 'no_settled': emit({'type':'agent_settled','aborted':scenario == 'aborted'})
if scenario == 'double_settled': emit({'type':'agent_settled','aborted':False})
if scenario == 'no_settled': sys.exit(0)
sys.stdin.read()  # The parent must close stdin only after authoritative settlement.
sys.exit(7 if scenario == 'nonzero' else 0)
