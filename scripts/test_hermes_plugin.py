import contextvars
import importlib.util
import io
import json
import os
import pathlib
import socket
import struct
import tempfile
import threading
import time
import types
import unittest
from unittest.mock import patch


HOME = contextvars.ContextVar('hermes_plugin_test_home')
SOURCE = pathlib.Path(__file__).resolve().parents[1] / 'plugins/hermes/__init__.py'


def receive(sock):
    def exact(size):
        chunks = bytearray()
        while len(chunks) < size:
            chunk = sock.recv(size - len(chunks))
            if not chunk:
                raise EOFError()
            chunks.extend(chunk)
        return chunks
    size = struct.unpack('!I', exact(4))[0]
    assert 0 < size <= 2 * 1024 * 1024
    return json.loads(exact(size))


def send(sock, value):
    data = json.dumps(value).encode()
    sock.sendall(struct.pack('!I', len(data)) + data)


class Server:
    def __init__(self, path):
        self.path = path
        self.requests = []
        self.errors = []
        self.stop = threading.Event()
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.bind(str(path))
        self.socket.listen()
        self.socket.settimeout(0.1)
        self.reply = self.normal_reply
        self.thread = threading.Thread(target=self.serve)
        self.thread.start()

    @staticmethod
    def normal_reply(request):
        event = request['command']['event']
        data = {'invocation_id': event['home'], 'caller': event.get('call')}
        if event['op'] == 'sync':
            data = {'id': 'capture/fixture', 'state': 'queued'}
        return {'schema_version': 1, 'request_id': request['request_id'],
                'sequence': 1, 'data': data}

    def serve(self):
        while not self.stop.is_set():
            try:
                client, _ = self.socket.accept()
            except TimeoutError:
                continue
            with client:
                client.settimeout(2)
                try:
                    send(client, {'protocol_version': 1, 'max_message_bytes': 2097152})
                    request = receive(client)
                    self.requests.append(request)
                    send(client, self.reply(request))
                except (OSError, EOFError):
                    pass
                except Exception as error:
                    self.errors.append(error)

    def close(self):
        self.stop.set()
        self.thread.join(3)
        self.socket.close()
        assert not self.thread.is_alive()
        assert not self.errors, self.errors


class Context:
    def __init__(self, root, socket_path):
        self.root = root
        self.settings = {'socket': str(socket_path), 'hermes_binary': '/native/hermes'}
        self.hooks = {}
        self.tools = {}
        self.commands = {}
        self.records = {}
        self.state = types.SimpleNamespace(set=self.set_state)

    def set_state(self, key, value):
        self.records[(str(HOME.get()), key)] = value

    def get_config(self, key, default=None):
        return self.settings.get(key, default)

    def session_cwd(self):
        return str(self.root)

    def register_hook(self, name, callback):
        self.hooks[name] = callback

    def register_tool(self, **kwargs):
        self.tools[kwargs['name']] = kwargs
        return object()

    def register_cli_command(self, **kwargs):
        self.commands[kwargs['name']] = kwargs
        return object()


class HermesPlugin(unittest.TestCase):
    def test_runner_research_uses_original_deadline(self):
        name = 'akasha-' + 'a' * 64
        server = Server('\0' + name)
        self.addCleanup(server.close)
        def delayed(request):
            time.sleep(11)
            return {'schema_version': 1, 'sequence': 1,
                    'request_id': request['request_id'], 'data': {'answer': 'slow research'}}
        server.reply = delayed
        with patch.dict(os.environ, {
            'AKASHA_HERMES_RUN_SOCKET': name,
            'AKASHA_HERMES_RUN_TOKEN': 'b' * 64,
            'AKASHA_RUN_DEADLINE_MS': str(int(time.time() * 1000) + 30000),
        }):
            result = self.plugin._exchange(self.ctx, {'op': 'context', 'request': {'operation': 'delegate', 'hint': 'child'}}, runner=True)
            self.assertEqual(result['answer'], 'slow research')
            os.environ['AKASHA_RUN_DEADLINE_MS'] = '1'
            with self.assertRaisesRegex(self.plugin.BridgeError, 'RUNNER_TIMEOUT'):
                self.plugin._exchange(self.ctx, {'op': 'context'}, runner=True)
            del os.environ['AKASHA_RUN_DEADLINE_MS']
            with self.assertRaisesRegex(self.plugin.BridgeError, 'CONFIG_INVALID'):
                self.plugin._exchange(self.ctx, {'op': 'context'}, runner=True)

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='akasha-hermes-plugin-')
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        self.home_a = self.root / 'profile-a'
        self.home_b = self.root / 'profile-b'
        self.home_a.mkdir()
        self.home_b.mkdir()
        HOME.set(self.home_a)
        self.server = Server(self.root / 'daemon.sock')
        self.addCleanup(self.server.close)
        native = types.SimpleNamespace(get_hermes_home=lambda: HOME.get())
        self.modules = patch.dict('sys.modules', {'hermes_constants': native})
        self.modules.start()
        self.addCleanup(self.modules.stop)
        spec = importlib.util.spec_from_file_location('akasha_hermes_plugin_test', SOURCE)
        self.plugin = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.plugin)
        self.ctx = Context(self.root, self.server.path)
        self.plugin.register(self.ctx)

    def bind(self, **changes):
        args = dict(tool_name='akasha_consult', args={'hint': 'preferences'},
                    session_id='session-a', task_id='task-a', turn_id='turn-a',
                    tool_call_id='call-a')
        args.update(changes)
        return self.ctx.hooks['pre_tool_call'](**args)

    def call(self, args=None, **changes):
        kwargs = dict(session_id='session-a', task_id='task-a')
        kwargs.update(changes)
        return json.loads(self.ctx.tools['akasha_consult']['handler'](
            {'hint': 'preferences'} if args is None else args, **kwargs))

    def test_private_hook_identity_and_two_profile_calls_remain_separate(self):
        self.assertIn('akasha_consult', self.ctx.tools)
        schema = self.ctx.tools['akasha_consult']['schema']['parameters']
        self.assertEqual(set(schema['properties']), {'hint'})
        self.assertIs(schema['additionalProperties'], False)
        self.assertIsNone(self.bind())
        HOME.set(self.home_b)
        self.assertIsNone(self.bind())
        b = self.call()
        HOME.set(self.home_a)
        a = self.call()
        self.assertEqual(a['data']['invocation_id'], str(self.home_a))
        self.assertEqual(b['data']['invocation_id'], str(self.home_b))
        event = self.server.requests[0]['command']['event']
        self.assertEqual(self.server.requests[0]['command']['op'], 'hermes')
        self.assertEqual(event['call'], {'session_id': 'session-a', 'task_id': 'task-a',
                         'turn_id': 'turn-a', 'tool_call_id': 'call-a',
                         'profile_home': str(self.home_a), 'cwd': str(self.root)})
        self.assertEqual(event['binary'], '/native/hermes')
        self.assertEqual(len(self.server.requests), 2)

    def test_duplicate_pending_calls_fail_closed_and_session_end_recovers(self):
        self.assertIsNone(self.bind())
        denied = self.bind(tool_call_id='parallel-call')
        self.assertEqual(denied, {'action': 'block', 'message': 'CALLER_AMBIGUOUS'})
        self.assertEqual(self.call()['error']['code'], 'CALLER_AMBIGUOUS')
        self.ctx.hooks['on_session_end'](session_id='session-a', interrupted=True)
        self.assertIsNone(self.bind(turn_id='next-turn', tool_call_id='next-call'))
        self.assertIn('data', self.call())
        self.assertEqual(self.call()['error']['code'], 'BINDING_UNAVAILABLE')

    def test_missing_ids_and_model_supplied_context_are_rejected_before_io(self):
        for changes in ({'turn_id': ''}, {'tool_call_id': ''}, {'task_id': ''},
                        {'session_id': ''}, {'args': {'session_id': 'forged'}},
                        {'args': {'hint': 'x' * 513}}):
            with self.subTest(changes=changes):
                denied = self.bind(**changes)
                self.assertEqual(denied['action'], 'block')
        self.assertEqual(self.server.requests, [])
        self.assertEqual(self.call()['error']['code'], 'BINDING_UNAVAILABLE')

    def test_failed_native_export_is_visible_and_does_not_create_a_binding(self):
        def denied(request):
            reply = self.server.normal_reply(request)
            reply.pop('data')
            reply['error'] = {'code': 'SOURCE_UNAVAILABLE', 'message': 'PRIVATE_SENTINEL'}
            return reply
        self.server.reply = denied
        self.assertEqual(self.bind(), {'action': 'block', 'message': 'SOURCE_UNAVAILABLE'})
        self.assertEqual(self.call()['error']['code'], 'BINDING_UNAVAILABLE')

    def test_lifecycle_queues_interrupted_and_compacted_history_with_profile_status(self):
        for hook in ('on_session_start', 'on_session_end', 'post_compaction'):
            self.assertIn(hook, self.ctx.hooks)
            self.ctx.hooks[hook](session_id='session-a', interrupted=True)
        self.assertEqual([r['command']['event']['op'] for r in self.server.requests],
                         ['sync', 'sync', 'sync'])
        self.assertEqual(self.ctx.records[(str(self.home_a), 'capture')]['state'], 'queued')
        self.server.reply = lambda r: dict(self.server.normal_reply(r), sequence=2)
        with self.assertLogs('akasha_otsuge.hermes', level='WARNING'):
            self.ctx.hooks['on_session_end'](session_id='session-a', interrupted=True)
        status = self.ctx.records[(str(self.home_a), 'capture')]
        self.assertEqual(status['error'], 'PROTOCOL_ERROR')
        self.assertEqual(status['session_id'], 'session-a')

    def test_existing_tool_registration_cannot_silently_receive_our_hook(self):
        conflicting = Context(self.root, self.server.path)
        conflicting.register_tool = lambda **kwargs: None
        with self.assertRaisesRegex(RuntimeError, 'TOOL_REGISTRATION_CONFLICT'):
            self.plugin.register(conflicting)
        self.assertEqual(conflicting.hooks, {})

    def test_runner_registers_only_scoped_context_tools_and_blocks_other_tools(self):
        runner = Context(self.root, self.server.path)
        events = []
        def exchange(ctx, event, *, runner=False):
            self.assertTrue(runner)
            events.append(event)
            return {'ready': True} if event['op'] == 'ready' else {'kind': 'description'}
        with patch.dict('os.environ', {'AKASHA_HERMES_RUN_SOCKET': 'akasha-fixture',
                                      'AKASHA_HERMES_RUN_TOKEN': 'a' * 64}), \
                patch.object(self.plugin, '_exchange', side_effect=exchange):
            self.plugin.register(runner)
            names = {'code_read', 'code_search', 'context_describe', 'context_list', 'context_read', 'context_search', 'delegate', 'memory_get', 'memory_search'}
            self.assertEqual(set(runner.tools), names)
            self.assertEqual(set(runner.hooks), {'pre_tool_call'})
            self.assertEqual(events, [])
            self.assertIn('akasha-runner', runner.commands)
            for name in ('terminal', 'execute_code', 'delegate_task', 'tool_call',
                         'write_file', 'akasha_consult'):
                self.assertEqual(runner.hooks['pre_tool_call'](tool_name=name),
                                 {'action': 'block', 'message': 'TOOL_DENIED'})
            result = json.loads(runner.tools['context_describe']['handler']({}))
            self.assertEqual(result, {'kind': 'description'})
            self.assertEqual(events[-1], {'op': 'context', 'request': {'operation': 'describe'}})
            result = json.loads(runner.tools['context_read']['handler']({'operation': 'describe'}))
            self.assertEqual(result['error']['code'], 'INVALID_ARGUMENT')

    def test_runner_native_entry_checks_actual_tools_and_closes_before_final(self):
        request = self.root / 'request.txt'
        request.write_text('Read context.')
        request.chmod(0o600)
        args = types.SimpleNamespace(request=str(request), budget=30.0)
        names = {'code_read', 'code_search', 'context_describe', 'context_list', 'context_read', 'context_search', 'delegate', 'memory_get', 'memory_search'}
        calls = []
        completion = {}
        class Agent:
            valid_tool_names = names
            session_id = 'native-session'
            session_prompt_tokens = 12
            session_output_tokens = 7
            def __init__(self, **kwargs):
                calls.append(kwargs)
            def run_conversation(self, prompt):
                calls.append(prompt)
                return {'final_response': '{"answer":"done"}', **completion}
            def close(self):
                calls.append('closed')
        modules = {
            'hermes_cli.auth': types.SimpleNamespace(AuthError=type('AuthError', (Exception,), {})),
            'hermes_cli.config': types.SimpleNamespace(load_config_readonly=lambda: {'model': {'default': 'native-model', 'provider': 'native-provider'}}, split_model_config_default=lambda s: (s, '')),
            'hermes_cli.runtime_provider': types.SimpleNamespace(resolve_runtime_with_fallback=lambda *a, **k: ({'provider': 'native-provider'}, None)),
            'hermes_cli.fallback_config': types.SimpleNamespace(get_fallback_chain=lambda cfg: []),
            'run_agent': types.SimpleNamespace(AIAgent=Agent),
        }
        native = __import__('sys').modules['hermes_constants']
        native.resolve_reasoning_config = lambda *a: None
        for widened, completion in ((False, {}), (True, {}), (False, {'partial': True}),
                                    (False, {'completed': False}), (False, {'interrupted': True})):
            failed = widened or bool(completion)
            calls.clear()
            Agent.valid_tool_names = names | ({'terminal'} if widened else set())
            output = io.StringIO()
            with patch.dict('sys.modules', modules), patch('sys.stdout', output), \
                    patch.object(self.plugin, '_exchange', side_effect=lambda ctx, event, **kw: {'ready': True} if event['op'] == 'ready' else {'accepted': True}), \
                    self.assertRaises(SystemExit) as exited:
                self.plugin._run_native(self.ctx, names, args)
            self.assertEqual(exited.exception.code, 1 if failed else 0)
            frames = [json.loads(s) for s in output.getvalue().splitlines()]
            self.assertEqual(frames[-1]['exit_code'], 1 if failed else 0)
            if not failed:
                self.assertEqual(frames[-1]['tokens'], {'input':12, 'output':7})
            self.assertEqual(calls[-1], 'closed')
            if widened:
                self.assertNotIn('Read context.', calls)
            else:
                for key in ('skip_memory', 'skip_context_files', 'skip_background_review'):
                    self.assertTrue(calls[0][key])
                self.assertEqual(calls[0]['enabled_toolsets'], ['akasha_context'])
                self.assertGreater(calls[0]['run_budget_seconds'], 0)
                self.assertLessEqual(calls[0]['run_budget_seconds'], 30)

    def test_omitted_null_and_blank_hints_follow_the_shared_consult_contract(self):
        for args in ({}, {'hint': None}, {'hint': '   '}):
            with self.subTest(args=args):
                self.assertIsNone(self.bind(args=args))
                self.assertIn('data', self.call(args=args))


if __name__ == '__main__':
    unittest.main()
