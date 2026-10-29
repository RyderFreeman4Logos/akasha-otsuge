#!/usr/bin/python3
"""Fault-injectable native protocol fixture; not real-host acceptance."""
import json, os, pathlib, subprocess, sys, time

if '--version' in sys.argv:
    print('codex-cli 0.164.0')
    raise SystemExit(0)
root = pathlib.Path(__file__).parent
scenario = (root / 'scenario').read_text()

def read():
    return json.loads(sys.stdin.readline())

def send(value):
    print(json.dumps(value), flush=True)

assert read()['method'] == 'initialize'
send({'id': 1, 'result': {'userAgent': 'akasha/0.164.0'}})
send({'method': 'configWarning', 'params': {'summary': 'Ignored legacy config field', 'details': None}})
send({'method': 'account/updated', 'params': {'authMode': 'chatgpt', 'planType': 'test'}})
assert read()['method'] == 'initialized'
assert read()['method'] == 'config/read'
send({'id': 2, 'result': {'config': {'model': 'fixture', 'mcp_servers': {'other.server': {'enabled': True}}}}})
request = read()
assert request['method'] == 'thread/start'
assert request['params']['config']['mcp_servers']['other.server']['enabled'] is False
assert request['params']['sandbox'] == 'read-only'
assert request['params']['approvalPolicy'] == 'never'
assert {t['name'] for t in request['params']['dynamicTools']} == {'context_describe', 'context_list', 'context_read', 'context_search'}
assert 'token' not in json.dumps(request)
catalog = next(sys.argv[i+1] for i, arg in enumerate(sys.argv[:-1]) if arg == '-c' and sys.argv[i+1].startswith('model_catalog_json='))
model = json.loads(pathlib.Path(json.loads(catalog.split('=', 1)[1])).read_text())['models'][0]
assert model['apply_patch_tool_type'] is None and model['shell_type'] == 'disabled'
send({'id': 3, 'result': {'thread': {'id': 'thread'}, 'model': 'fixture', 'approvalPolicy': 'never', 'sandbox': {'type': 'readOnly', 'networkAccess': False}}})
assert read()['method'] == 'turn/start'
if scenario in ('timeout', 'cancel'):
    child = subprocess.Popen(['/bin/sleep', '30'])
    (root / 'child.pid').write_text(str(child.pid))
send({'id': 4, 'result': {'turn': {'id': 'turn'}}})
send({'method': 'item/reasoning/summaryPartAdded', 'params': {'threadId': 'thread', 'turnId': 'turn', 'itemId': 'reason', 'summaryIndex': 0}})
if scenario in ('auth', 'auth_http'):
    info = 'unauthorized' if scenario == 'auth' else {'httpConnectionFailed': {'httpStatusCode': 401}}
    send({'method': 'error', 'params': {'threadId': 'thread', 'turnId': 'turn', 'error': {'message': 'private provider error', 'codexErrorInfo': info}, 'willRetry': False}})
    time.sleep(30)
if scenario == 'request_as_notification':
    send({'id': 11, 'method': 'warning', 'params': {}}); time.sleep(30)
if scenario in ('timeout', 'cancel'):
    time.sleep(30)
if scenario == 'eof_live':
    os.close(1); os.close(2); time.sleep(30)
if scenario == 'no_final': raise SystemExit(0)
if scenario == 'partial':
    sys.stdout.write('{'); sys.stdout.flush(); raise SystemExit(0)
if scenario == 'oversized':
    print('x' * 131073, flush=True); time.sleep(30)
if scenario == 'stderr':
    print('x' * 8193, file=sys.stderr, flush=True); time.sleep(30)

def usage(incoming, outgoing):
    counts = dict(inputTokens=incoming, outputTokens=outgoing, cachedInputTokens=0,
                  reasoningOutputTokens=0, totalTokens=incoming + outgoing)
    send({'method':'thread/tokenUsage/updated', 'params':{'threadId':'thread',
          'turnId':'turn', 'tokenUsage':{'total':counts,'last':counts}}})

usage(200000 if scenario == 'token_budget' else 10, 2)
usage(9 if scenario == 'usage_regression' else 10, 2)
call = {'id': 10, 'method': 'item/tool/call', 'params': {'threadId': 'thread', 'turnId': 'turn', 'callId': 'call', 'tool': 'context_describe', 'arguments': {}}}
if scenario == 'wrong_thread': call['params']['threadId'] = 'other'
if scenario == 'wrong_turn': call['params']['turnId'] = 'other'
if scenario == 'forged_args': call['params']['arguments']['operation'] = 'read'
if scenario == 'denied_tool': call['params']['tool'] = 'shell_command'
if scenario == 'missing_id': del call['id']
if scenario == 'invalid_id': call['id'] = {'forged': 1}
send(call)
result = read()
assert result['id'] == 10 and result['result']['success'] is True
assert 'invocation_id' in result['result']['contentItems'][0]['text']
send({'method': 'turn/moderationMetadata', 'params': {'threadId': 'thread', 'turnId': 'turn', 'metadata': {}}})
if scenario == 'duplicate_call':
    send(call); time.sleep(30)
usage(20, 4)
final = {'method': 'turn/completed', 'params': {'threadId': 'thread', 'turn': {'id': 'turn', 'status': 'completed', 'error': None, 'items': [{'type': 'agentMessage', 'id': 'answer', 'phase': 'final_answer', 'text': '{"answer":"bounded-context"}'}]}}}
if scenario == 'malformed_final': final['params']['turn']['items'][0]['text'] = '{"answer":"ok","authority":"forged"}'
send(final)
if scenario == 'double_final': send(final)
assert sys.stdin.read() == ''

if scenario == 'nonzero': raise SystemExit(9)
