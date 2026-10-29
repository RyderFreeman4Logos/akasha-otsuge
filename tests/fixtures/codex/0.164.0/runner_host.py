#!/usr/bin/python3
"""Fault-injectable native protocol fixture; not real-host acceptance."""
import json, os, pathlib, subprocess, sys, time

if '--version' in sys.argv:
    print('codex-cli 0.164.0')
    raise SystemExit(0)
root = pathlib.Path(__file__).parent
scenario = (root / 'scenario').read_text()
detached = scenario == 'cancel_detached'
if detached:
    scenario = 'cancel'

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
assert {t['name'] for t in request['params']['dynamicTools']} == {'code_read', 'code_search', 'context_describe', 'context_list', 'context_read', 'context_search', 'delegate', 'memory_get', 'memory_search'}
assert 'token' not in json.dumps(request)
catalog = next(sys.argv[i+1] for i, arg in enumerate(sys.argv[:-1]) if arg == '-c' and sys.argv[i+1].startswith('model_catalog_json='))
model = json.loads(pathlib.Path(json.loads(catalog.split('=', 1)[1])).read_text())['models'][0]
assert model['apply_patch_tool_type'] is None and model['shell_type'] == 'disabled'
send({'id': 3, 'result': {'thread': {'id': 'thread'}, 'model': 'fixture', 'approvalPolicy': 'never', 'sandbox': {'type': 'readOnly', 'networkAccess': False}}})
turn_request = read()
assert turn_request['method'] == 'turn/start'
hint = json.loads(turn_request['params']['input'][0]['text']).get('hint')
if scenario in ('timeout', 'cancel'):
    child = subprocess.Popen(['/bin/sleep', '30'], start_new_session=detached)
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
if scenario.startswith('grill'):
    payload = json.loads(request['params']['baseInstructions'].split('\nInput: ', 1)[1])
    stage = payload['stage']
    if scenario == 'grill_denied':
        send({'id':10,'method':'item/tool/call','params':{'threadId':'thread','turnId':'turn',
            'callId':'forbidden-grill-read','tool':'code_read','arguments':{'path':'private.txt','start':0,'max_bytes':1024}}})
        time.sleep(30)
    if scenario == 'grill_wait' and stage['role'] == 'expert':
        (root / 'grill-wait.pid').write_text(str(os.getpid()))
        time.sleep(30)
    with (root / 'grill-runs.jsonl').open('a') as log:
        log.write(json.dumps(dict(payload, pid=os.getpid())) + '\n')
    if stage['role'] == 'interviewer':
        report = dict(schema_version=1, questions=[dict(id='q1', text='Is keyword-only sufficient?', depends_on=[]), dict(id='q2', text='Which deployment follows?', depends_on=['q1'])], answers=[])
    else:
        expert = stage['role'] == 'expert'
        report = dict(schema_version=1, questions=[], answers=[dict(
            question=q['id'], kind='decision', claim=('Use keyword-only.' if expert else 'Keep hybrid search.') + q['id'],
            rationale='A conditional automated proposal.', strongest_opposition='The alternative may handle unseen queries better.',
            conditions=['offline'], assumptions=[], unresolved=['No representative evaluation.'],
            sources=[dict(source=0, start=0, end=len(payload['evidence'][0]['text'].encode()))] if q['id']=='q1' else [], resolved=False)
            for q in payload['questions']])
    output = json.dumps(report)
    send({'method':'turn/completed','params':{'threadId':'thread','turn':{
        'id':'turn','status':'completed','error':None,'items':[{
        'type':'agentMessage','id':'answer','phase':'final_answer','text':output}]}}})
    assert sys.stdin.read() == ''
    raise SystemExit(0)
if scenario.startswith('extraction'):
    instructions = request['params']['baseInstructions']
    assert 'extraction-v1' in instructions
    assert 'candidates' in turn_request['params']['outputSchema']['properties']
    scope = json.loads(instructions.split('Requested scope: ', 1)[1].split('\nEvidence:', 1)[0])
    evidence = json.loads(instructions.split('\nEvidence: ', 1)[1])
    assert evidence[0]['text'] == 'Only if offline, use keyword search.'
    if scenario == 'extraction_denied':
        send({'id':10,'method':'item/tool/call','params':{'threadId':'thread','turnId':'turn',
            'callId':'forbidden-expansion','tool':'code_read',
            'arguments':{'path':'private.txt','start':0,'max_bytes':1024}}})
        time.sleep(30)
    candidate = dict(kind='preference', scope=scope, claim='Use keyword search when offline.',
        rationale='A conditional preference.', conditions=['offline'], assumptions=[],
        sources=[dict(source=0, start=0, end=36)], valid_from_ms=None, valid_until_ms=None)
    output = json.dumps(dict(schema_version=1, candidates=[candidate]))
    send({'method':'turn/completed','params':{'threadId':'thread','turn':{
        'id':'turn','status':'completed','error':None,'items':[{
        'type':'agentMessage','id':'answer','phase':'final_answer','text':output}]}}})
    assert sys.stdin.read() == ''
    raise SystemExit(0)
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
seen_sources, seen_memories = [], []
if scenario in ('research', 'code_policy') or scenario.startswith(('delegate_', 'conclusion_')):
    call_number = 100
    def tool(name, arguments):
        global call_number
        call_number += 1
        send({'id':call_number,'method':'item/tool/call','params':{
            'threadId':'thread','turnId':'turn','callId':f'research-{call_number}',
            'tool':name,'arguments':arguments}})
        reply = read()
        assert reply['id'] == call_number and reply['result']['success'] is True
        response = json.loads(reply['result']['contentItems'][0]['text'])
        value = response.get('value', {})
        for span in value.get('sources', []) + ([value['source']] if value.get('source') else []) + [v['reference'] for v in value.get('originals', [])]:
            if span not in seen_sources: seen_sources.append(span)
        if name == 'memory_get' and response.get('kind') != 'error':
            seen_memories.append({'memory_id':arguments['memory_id'],'revision':arguments['revision']})
        (root / 'research-checkpoint.json').write_text(json.dumps({
            'tool':name,'kind':response.get('kind'),'code':value.get('code'),
            'source_code':value.get('source_code'),'keys':list(value),
            'items':[item['memory']['revision']['key']['id'] for item in value.get('items', [])],
            'changed':value.get('changed'),'originals':len(value.get('originals', []))}))
        return response
    page = tool('context_list', {'cursor':None,'limit':4})['value']
    assert tool('context_read', {'handle':page['events'][0]['handle'],'start':0,'max_bytes':1024})['kind'] == 'text'
if scenario.startswith('delegate_'):
    branch = hint or 'root'
    if scenario == 'delegate_parents':
        for phase in ('ready', 'returned'):
            (root / f'{phase}-{os.getpid()}').touch()
            end = time.monotonic() + 8
            while len(list(root.glob(f'{phase}-*'))) != 2:
                assert time.monotonic() < end, 'parent barrier timed out'
                time.sleep(0.01)
            if phase == 'ready':
                result = tool('delegate', {'hint':'child'})
                assert result['kind'] == 'error' and result['value']['code'] == 'DELEGATION_UNAVAILABLE'
    elif scenario == 'delegate_cancel':
        if branch == 'root':
            (root / 'parent.pid').write_text(str(os.getpid()))
            tool('delegate', {'hint':'child'})
            raise AssertionError('cancelled delegate returned')
        child = subprocess.Popen(['/bin/sleep', '30'], start_new_session=True)
        (root / 'child.pid').write_text(str(child.pid))
        time.sleep(30)
    elif scenario == 'delegate_denied':
        result = tool('delegate', {'hint':'child'})
        assert result['kind'] == 'error' and result['value']['code'] == 'DELEGATION_UNAVAILABLE'
    elif scenario == 'delegate_tree':
        result = tool('delegate', {'hint':'child' if branch == 'root' else 'grandchild'})
        if branch == 'grandchild':
            assert result['kind'] == 'error' and result['value']['code'] == 'BUDGET_EXHAUSTED'
        else:
            assert result['kind'] == 'research' and result['value']['answer'] == 'bounded-context'
    elif scenario == 'delegate_failure':
        if branch == 'fail':
            send({'method':'error','params':{'threadId':'thread','turnId':'turn','error':{'message':'controlled child failure','codexErrorInfo':'unauthorized'},'willRetry':False}})
            time.sleep(30)
        elif branch == 'root':
            result = tool('delegate', {'hint':'fail'})
            assert result['kind'] == 'error' and result['value']['code'] == 'AUTH_REQUIRED'
            result = tool('delegate', {'hint':'succeed'})
            assert result['kind'] == 'research' and result['value']['answer'] == 'bounded-context'
if scenario == 'code_policy':
    assert tool('code_read', {'path':'notes.rs','start':0,'max_bytes':1024})['value']['text'] == 'memory needle\n'
    (root / 'code-read-ready').touch()
    end = time.monotonic() + 15
    while not (root / 'code-read-release').exists():
        assert time.monotonic() < end, 'code policy barrier timed out'
        time.sleep(0.01)
if scenario == 'research':
    found = tool('memory_search', {'query':'offline'})['value']
    item = next(item for item in found['items'] if item['memory']['revision']['key']['id'] == 'offline')
    revision = item['memory']['revision']['commit_seq']
    denied = tool('memory_get', {'memory_id':'private','revision':revision})
    assert denied['kind'] == 'error' and denied['value']['code'] == 'READ_DENIED'
    expanded = tool('memory_get', {'memory_id':'offline','revision':revision})['value']
    assert 'offline' in expanded['originals'][0]['output']
    other = tool('memory_search', {'query':'weather'})['value']
    assert any(item['memory']['revision']['key']['id'] == 'weather' for item in other['items'])
    assert tool('code_search', {'paths':['notes.rs'],'query':'needle'})['value']['matches']
    first = tool('code_read', {'path':'notes.rs','start':0,'max_bytes':1024})['value']
    assert first['text'] == 'memory needle\n' and first['changed'] is False
    (root / 'workspace' / 'notes.rs').write_text('memory needle changed\n')
    second = tool('code_read', {'path':'notes.rs','start':0,'max_bytes':1024})['value']
    assert second['changed'] is True and second['sha256'] != first['sha256']
    denied = tool('code_read', {'path':'../scenario','start':0,'max_bytes':1024})
    assert denied['kind'] == 'error' and denied['value']['code'] == 'READ_DENIED'
send({'method': 'turn/moderationMetadata', 'params': {'threadId': 'thread', 'turnId': 'turn', 'metadata': {}}})
if scenario == 'duplicate_call':
    send(call); time.sleep(30)
usage(20, 4)
final = {'method': 'turn/completed', 'params': {'threadId': 'thread', 'turn': {'id': 'turn', 'status': 'completed', 'error': None, 'items': [{'type': 'agentMessage', 'id': 'answer', 'phase': 'final_answer', 'text': '{"answer":"bounded-context"}'}]}}}
if hint == 'long-output':
    final['params']['turn']['items'][0]['text'] = json.dumps({'answer':'说明' * 2500 + '；仅在全部测试通过时适用。'}, ensure_ascii=False)
if scenario.startswith('conclusion_') or 'status' in turn_request['params']['outputSchema']['properties']:
    conclusion = dict(answer=json.loads(final['params']['turn']['items'][0]['text'])['answer'],
        status='found' if seen_sources or seen_memories else 'incomplete', sources=seen_sources,
        memories=seen_memories, conditions=[], conflicts=[], gaps=[] if seen_sources or seen_memories else ['no_evidence'])
    if scenario == 'conclusion_no_hit':
        tool('context_search', {'cursor':None,'query':'absent-conclusion-query','limit':4})
        conclusion.update(status='no_hit', answer='No reliable relevant result in the searched scope.', sources=[], memories=[], gaps=[])
    elif scenario == 'conclusion_forged':
        conclusion['sources'] = [dict(seen_sources[0], event_id='private/unissued')]
    elif scenario == 'conclusion_conflict':
        conclusion['conditions'] = ['Only for the original local environment.']
        conclusion['conflicts'] = ['The two observations disagree.']
    final['params']['turn']['items'][0]['text'] = json.dumps(conclusion, ensure_ascii=False)
if scenario == 'malformed_final': final['params']['turn']['items'][0]['text'] = '{"answer":"ok","authority":"forged"}'
send(final)
if scenario == 'double_final': send(final)
assert sys.stdin.read() == ''

if scenario == 'nonzero': raise SystemExit(9)
