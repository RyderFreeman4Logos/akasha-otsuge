"""Loopback protocol fixture. This does not attest authenticated model quality."""
import json
import os
import threading
import re
import shlex
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Provider:
    def __init__(self):
        self.api = os.environ.get('AKASHA_NATIVE_API', 'openai-completions')
        assert self.api in ['openai-completions', 'openai-responses']
        self.case = 'roundtrip'
        self.requests = []
        self.errors = []
        self.bindings = []
        self.overlap = threading.Barrier(2)
        self.interrupt_seen = threading.Event()
        self.interrupt_release = threading.Event()
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_args):
                pass

            def responses_reply(self, name, args):
                response_id = 'resp_' + uuid.uuid4().hex
                def emit(kind, **fields):
                    value = {'type': kind, **fields}
                    self.wfile.write(('event: ' + kind + '\ndata: ' + json.dumps(value) + '\n\n').encode())
                    self.wfile.flush()
                emit('response.created', response={'id': response_id, 'status': 'in_progress', 'output': []})
                if name:
                    item = {'type': 'function_call', 'id': 'fc_' + uuid.uuid4().hex,
                            'call_id': 'call_' + uuid.uuid4().hex, 'name': name, 'arguments': '', 'status': 'in_progress'}
                    emit('response.output_item.added', output_index=0, item=item)
                    arguments = json.dumps(args)
                    emit('response.function_call_arguments.delta', output_index=0, item_id=item['id'], delta=arguments)
                    item = {**item, 'arguments': arguments, 'status': 'completed'}
                    emit('response.function_call_arguments.done', output_index=0, item_id=item['id'], arguments=arguments)
                else:
                    text = 'BEFORE_TRANSFORM' if owner.case == 'caller' else '{"answer":"bounded-context"}'
                    item = {'type': 'message', 'id': 'msg_' + uuid.uuid4().hex, 'role': 'assistant', 'content': [], 'status': 'in_progress'}
                    emit('response.output_item.added', output_index=0, item=item)
                    emit('response.content_part.added', output_index=0, content_index=0, item_id=item['id'], part={'type': 'output_text', 'text': '', 'annotations': []})
                    emit('response.output_text.delta', output_index=0, content_index=0, item_id=item['id'], delta=text)
                    item = {**item, 'content': [{'type': 'output_text', 'text': text, 'annotations': []}], 'status': 'completed'}
                emit('response.output_item.done', output_index=0, item=item)
                emit('response.completed', response={'id': response_id, 'status': 'completed', 'output': [item],
                     'usage': {'input_tokens': 10, 'output_tokens': 10, 'total_tokens': 20}})
                self.close_connection = True

            def do_POST(self):
                try:
                    self.connection.settimeout(5)
                    responses = owner.api == 'openai-responses'
                    assert self.path == ('/v1/responses' if responses else '/v1/chat/completions')
                    size = int(self.headers['Content-Length'])
                    assert 0 < size < 1048576
                    request = json.loads(self.rfile.read(size))
                    assert request['stream'] is True
                    names = sorted(tool['name'] if responses else tool['function']['name'] for tool in request['tools'])
                    expected = ['akasha_consult'] if owner.case == 'caller' else ['code_read', 'code_search', 'context_describe', 'context_list', 'context_read', 'context_search', 'delegate', 'memory_get', 'memory_search']
                    assert names == expected, names
                    messages = request['input'] if responses else request['messages']
                    last_user = max(i for i,m in enumerate(messages) if m.get('role')=='user')
                    prompt = messages[last_user]['content']
                    if not isinstance(prompt,str):prompt = json.dumps(prompt)
                    results = ([{'content': m['output']} for m in messages[last_user+1:] if m.get('type') == 'function_call_output']
                               if responses else [m for m in messages[last_user+1:] if m['role'] == 'tool'])
                    owner.requests.append({'api': owner.api, 'tool_names': names, 'previous_tool_results': len(results)})
                    assert len(owner.requests) <= 64
                    if owner.case == 'retry' and len(owner.requests) == 1:
                        body=json.dumps({'error':{'message':'controlled transient server failure','type':'server_error'}}).encode()
                        self.send_response(503);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);return
                    args = {}
                    name = None
                    if owner.case == 'caller':
                        if 'INTERRUPT_PENDING' in prompt:
                            owner.interrupt_seen.set();owner.interrupt_release.wait(30)
                        if not results:
                            if 'SIMULTANEOUS_' in prompt:owner.overlap.wait(8)
                            name, args = 'akasha_consult', {'hint': 'controlled native caller'}
                        else:
                            binding=json.loads(results[-1]['content']);assert not binding.get('error'), binding
                            owner.bindings.append(binding['data'])
                    elif owner.case == 'forbidden':
                        marker=re.search(r'OUTSIDE_MARKER=(\S+)',prompt).group(1).rstrip('.')
                        name,args='bash',{'command':'touch '+shlex.quote(marker)}
                    elif owner.case == 'denied':
                        name, args = 'context_read', {'handle': '/outside/unissued', 'start': 0, 'max_bytes': 32}
                    elif len(results) == 0:
                        name = 'context_describe'
                    elif len(results) == 1:
                        name, args = 'context_list', {'limit': 16}
                    elif len(results) == 2:
                        page = json.loads(results[-1]['content'])
                        assert page['kind'] == 'page' and page['value']['events']
                        name, args = 'context_read', {'handle': page['value']['events'][0]['handle'], 'start': 0, 'max_bytes': 1024}
                    elif len(results) == 3:
                        name, args = 'context_search', {'query': 'Original', 'limit': 16}
                    else:
                        assert len(results) == 4
                    self.send_response(200)
                    self.send_header('Content-Type', 'text/event-stream')
                    self.send_header('Connection', 'close')
                    self.end_headers()
                    if responses:
                        self.responses_reply(name, args)
                        return
                    def chunk(delta, finish=None):
                        value = {'id': 'fixture-stream', 'object': 'chat.completion.chunk', 'created': 1,
                                 'model': 'fixture', 'choices': [{'index': 0, 'delta': delta, 'finish_reason': finish}]}
                        self.wfile.write(('data: ' + json.dumps(value) + '\n\n').encode())
                        self.wfile.flush()
                    chunk({'role': 'assistant'})
                    if name:
                        chunk({'tool_calls': [{'index': 0, 'id': 'call_' + uuid.uuid4().hex, 'type': 'function',
                                               'function': {'name': name, 'arguments': json.dumps(args)}}]})
                        chunk({}, 'tool_calls')
                    else:
                        chunk({'content': 'BEFORE_TRANSFORM' if owner.case=='caller' else '{"answer":"bounded-context"}'})
                        chunk({}, 'stop')
                    self.wfile.write(b'data: [DONE]\n\n')
                    self.wfile.flush()
                    self.close_connection = True
                except (BrokenPipeError, ConnectionResetError):
                    pass  # Cancellation intentionally closes the native transport.
                except Exception as error:
                    owner.errors.append(type(error).__name__ + ': ' + str(error))
                    self.close_connection = True

        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.server.daemon_threads = True
        self.server.block_on_close = False
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def configure(self, profile):
        (profile / 'settings.json').write_text(json.dumps({'defaultProvider': 'akasha-fixture',
            'defaultModel': 'fixture', 'defaultThinkingLevel': 'off', 'compaction': {'enabled': False, 'keepRecentTokens': 32, 'reserveTokens': 512},
            'retry': {'enabled': True, 'maxRetries': 1, 'baseDelayMs': 1, 'maxAgentDelayMs': 1, 'provider': {'maxRetries': 0}}}))
        (profile / 'models.json').write_text(json.dumps({'providers': {'akasha-fixture': {
            'baseUrl': f'http://127.0.0.1:{self.server.server_port}/v1', 'api': self.api,
            'apiKey': 'fixture-only-not-a-secret', 'models': [{'id': 'fixture', 'name': 'Controlled fixture',
            'reasoning': False, 'input': ['text'], 'contextWindow': 32768, 'maxTokens': 2048,
            'cost': {'input': 0, 'output': 0, 'cacheRead': 0, 'cacheWrite': 0}}]}}}))

    def close(self):
        self.interrupt_release.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)
        assert not self.thread.is_alive(), 'provider thread did not stop'
