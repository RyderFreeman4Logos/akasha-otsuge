"""Hermes native Caller integration; runtime identity never comes from model arguments."""

from contextlib import redirect_stderr, redirect_stdout
import json
import logging
import math
import os
from pathlib import Path
import re
import socket
import struct
import stat
import sys
import threading
import time
import uuid

from hermes_constants import get_hermes_home

LOG = logging.getLogger('akasha_otsuge.hermes')
DEFAULTS = {}


class BridgeError(Exception):
    pass


def _exchange(ctx, event, *, runner=False):
    path = ctx.get_config('socket', DEFAULTS.get('socket', ''))
    token = os.environ.get('AKASHA_HERMES_RUN_TOKEN', '')
    if runner:
        name = os.environ.get('AKASHA_HERMES_RUN_SOCKET', '')
        if not re.fullmatch(r'akasha-[a-f0-9]{64}', name) or not re.fullmatch(r'[a-f0-9]{64}', token):
            raise BridgeError('CONFIG_INVALID')
        path = '\0' + name
    if not runner and (not isinstance(path, str) or not Path(path).is_absolute()):
        raise BridgeError('CONFIG_INVALID')
    deadline = time.monotonic() + 10
    request_id = uuid.uuid4().hex
    request = {'protocol_version': 1, 'request_id': request_id,
               'command': {'op': 'hermes', 'event': event}}
    if runner:
        request['command'] = {'op': 'hermes_context', 'token': token, 'event': event}
    with socket.socket(socket.AF_UNIX) as client:
        def remaining():
            seconds = deadline - time.monotonic()
            if seconds <= 0:
                raise BridgeError('IPC_TIMEOUT')
            client.settimeout(seconds)

        def exact(size):
            data = bytearray()
            while len(data) < size:
                remaining()
                part = client.recv(size - len(data))
                if not part:
                    raise BridgeError('PROTOCOL_ERROR')
                data.extend(part)
            return data

        def read():
            size = struct.unpack('!I', exact(4))[0]
            if not 0 < size <= 2097152:
                raise BridgeError('MESSAGE_TOO_LARGE')
            value = json.loads(exact(size))
            if not isinstance(value, dict):
                raise BridgeError('PROTOCOL_ERROR')
            return value

        try:
            remaining()
            client.connect(path)
            peer = struct.unpack('3i', client.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
            if peer[1] != os.geteuid():
                raise BridgeError('PEER_DENIED')
            greeting = read()
            if greeting.get('protocol_version') != 1 or greeting.get('max_message_bytes') != 2097152:
                raise BridgeError('PROTOCOL_VERSION')
            if 'error' in greeting:
                raise BridgeError('RESOURCE_BUSY')
            encoded = json.dumps(request).encode()
            if len(encoded) > 16384:
                raise BridgeError('MESSAGE_TOO_LARGE')
            remaining()
            client.sendall(struct.pack('!I', len(encoded)) + encoded)
            reply = read()
            if (reply.get('schema_version') != 1 or reply.get('sequence') != 1
                    or reply.get('request_id') != request_id):
                raise BridgeError('PROTOCOL_ERROR')
            if 'error' in reply:
                error = reply['error']
                code = error.get('code') if isinstance(error, dict) else None
                raise BridgeError(code if isinstance(code, str) and re.fullmatch(r'[A-Z0-9_]{1,64}', code)
                                  else 'PROTOCOL_ERROR')
            if not isinstance(reply.get('data'), dict):
                raise BridgeError('PROTOCOL_ERROR')
            return reply['data']
        except TimeoutError:
            raise BridgeError('IPC_TIMEOUT') from None
        except OSError:
            raise BridgeError('DAEMON_UNAVAILABLE') from None
        except (ValueError, UnicodeError):
            raise BridgeError('PROTOCOL_ERROR') from None


def _key(args, session_id, task_id):
    if (not isinstance(args, dict) or set(args) - {'hint'}
            or (args.get('hint') is not None
                and (not isinstance(args['hint'], str) or len(args['hint']) > 512))):
        raise BridgeError('INVALID_ARGUMENT')
    if not all(_identifier(s) for s in (session_id, task_id)):
        raise BridgeError('CALLER_AMBIGUOUS')
    return (str(get_hermes_home().resolve()), session_id, task_id,
            json.dumps(args, sort_keys=True, separators=(',', ':')))


def _identifier(value):
    return isinstance(value, str) and re.fullmatch(r'[A-Za-z0-9_./:+-]{1,512}', value)


def _event(ctx, op):
    return {'op': op, 'binary': ctx.get_config('hermes_binary', DEFAULTS.get('hermes_binary', 'hermes')),
            'home': str(get_hermes_home().absolute())}


def _run_native(ctx, names, args):
    """Bridge a public plugin CLI command to the native executor in this owned process."""
    from hermes_cli.auth import AuthError
    output, agent = sys.stdout, None
    result = {'type': 'result', 'session_id': '', 'exit_code': 1, 'error': 'RUNNER_FAILED'}
    started = time.monotonic()
    print(json.dumps({'type': 'system', 'subtype': 'init'}), file=output, flush=True)

    def report_usage(*_args, **_kwargs):
        tokens = {'input': getattr(agent, 'session_prompt_tokens', None),
                  'output': getattr(agent, 'session_output_tokens', None)}
        if any(type(n) is not int or not 0 <= n <= 2**64 - 1 for n in tokens.values()):
            raise BridgeError('PROTOCOL_INVALID')
        if _exchange(ctx, {'op':'usage', 'input_tokens':tokens['input'],
                          'output_tokens':tokens['output']}, runner=True) != {'accepted':True}:
            raise BridgeError('PROTOCOL_INVALID')
        return tokens

    try:
        with open(os.devnull, 'w') as quiet, redirect_stdout(quiet), redirect_stderr(quiet):
            from hermes_cli.config import load_config_readonly, split_model_config_default
            from hermes_cli.fallback_config import get_fallback_chain
            from hermes_cli.runtime_provider import resolve_runtime_with_fallback
            from hermes_constants import resolve_reasoning_config
            from run_agent import AIAgent

            if not math.isfinite(args.budget) or args.budget <= 0:
                raise BridgeError('INVALID_ARGUMENT')
            fd = os.open(args.request, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
            with os.fdopen(fd, 'r') as request:
                meta = os.fstat(request.fileno())
                if (not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.geteuid()
                        or stat.S_IMODE(meta.st_mode) != 0o600 or meta.st_nlink != 1
                        or meta.st_size > 131072):
                    raise BridgeError('INVALID_ARGUMENT')
                prompt = request.read(131073)
            if not prompt.strip() or len(prompt.encode()) > 131072:
                raise BridgeError('INVALID_ARGUMENT')
            cfg = load_config_readonly()
            choice = cfg.get('model') or {}
            raw = choice.get('default') or choice.get('model') if isinstance(choice, dict) else choice
            model, provider = split_model_config_default(raw)
            if not model:
                raise BridgeError('CONFIG_INVALID')
            provider = provider or (choice.get('provider') if isinstance(choice, dict) else None)
            runtime, fallback = resolve_runtime_with_fallback(cfg, requested=provider, target_model=model)
            if fallback is not None:
                model = fallback['model']
            remaining = args.budget - (time.monotonic() - started)
            if remaining <= 0:
                raise BridgeError('RUNNER_TIMEOUT')
            agent = AIAgent(
                **{key: runtime.get(key) for key in ('api_key', 'base_url', 'provider',
                    'requested_provider', 'api_mode', 'credential_pool', 'request_overrides')},
                model=model, fallback_model=get_fallback_chain(cfg) or None,
                reasoning_config=resolve_reasoning_config(cfg, model),
                enabled_toolsets=['akasha_context'], quiet_mode=True, platform='cli',
                skip_context_files=True, skip_memory=True, skip_background_review=True,
                load_soul_identity=False, prefill_messages=[], checkpoints_enabled=False,
                cwd=str(Path.cwd()), run_budget_seconds=remaining,
                step_callback=report_usage, tool_start_callback=report_usage)
            if agent.valid_tool_names != names:
                raise BridgeError('ADAPTER_UNSUPPORTED')
            if _exchange(ctx, {'op': 'ready', 'home': str(get_hermes_home().resolve()),
                               'tools': sorted(names)}, runner=True) != {'ready': True}:
                raise BridgeError('PROTOCOL_INVALID')
            answer = agent.run_conversation(prompt)
            if (answer.get('failed') or answer.get('interrupted') or answer.get('partial')
                    or answer.get('completed') is False
                    or not isinstance(answer.get('final_response'), str)
                    or not answer['final_response'].strip()):
                raise BridgeError('RUNNER_FAILED')
            result = {'type': 'result', 'session_id': agent.session_id, 'exit_code': 0,
                      'text': answer['final_response'], 'tokens': report_usage()}
    except BridgeError as error:
        result['error'] = str(error)
    except AuthError as error:
        result['error'] = 'AUTH_REQUIRED' if error.relogin_required else 'RUNNER_FAILED'
        if isinstance(error.code, str) and re.fullmatch(r'[A-Za-z0-9_]{1,64}', error.code):
            result['source_code'] = error.code
    except Exception:
        pass  # Native exception text can contain credentials or provider payloads.
    finally:
        if agent is not None:
            try:
                with open(os.devnull, 'w') as quiet, redirect_stdout(quiet), redirect_stderr(quiet):
                    agent.close()
            except Exception:
                result = {'type': 'result', 'session_id': '', 'exit_code': 1, 'error': 'RUNNER_FAILED'}
    print(json.dumps(result, ensure_ascii=True), file=output, flush=True)
    raise SystemExit(result['exit_code'])


def _register_runner(ctx):
    schemas = {
        'context_describe': ({}, []),
        'context_list': ({'cursor': {'type': ['string', 'null']},
                  'limit': {'type': 'integer', 'minimum': 1, 'maximum': 16}}, ['limit']),
        'context_read': ({'handle': {'type': 'string'}, 'start': {'type': 'integer', 'minimum': 0},
                  'max_bytes': {'type': 'integer', 'minimum': 1, 'maximum': 16384}},
                 ['handle', 'start', 'max_bytes']),
        'context_search': ({'cursor': {'type': ['string', 'null']}, 'query': {'type': 'string'},
                    'limit': {'type': 'integer', 'minimum': 1, 'maximum': 16}}, ['query', 'limit']),
        'memory_search': ({'query': {'type': 'string', 'maxLength': 512}}, ['query']),
        'memory_get': ({'memory_id': {'type': 'string'}, 'revision': {'type': 'integer', 'minimum': 1}}, ['memory_id', 'revision']),
        'code_search': ({'paths': {'type': 'array', 'items': {'type': 'string'}, 'minItems': 1, 'maxItems': 16}, 'query': {'type': 'string', 'maxLength': 512}}, ['paths', 'query']),
        'code_read': ({'path': {'type': 'string'}, 'start': {'type': 'integer', 'minimum': 0}, 'max_bytes': {'type': 'integer', 'minimum': 1, 'maximum': 16384}}, ['path', 'start', 'max_bytes']),
        'delegate': ({'hint': {'type': 'string', 'maxLength': 512}}, ['hint']),
    }
    names = set(schemas)

    def before(tool_name='', **_):
        if tool_name not in names:
            return {'action': 'block', 'message': 'TOOL_DENIED'}
        return None

    def handler(op):
        def call(args, **_):
            try:
                if not isinstance(args, dict) or 'operation' in args:
                    raise BridgeError('INVALID_ARGUMENT')
                return json.dumps(_exchange(ctx, {'op': 'context',
                    'request': dict(args, operation=op)}, runner=True))
            except BridgeError as error:
                return json.dumps({'error': {'code': str(error)}})
        return call

    for name, (properties, required) in schemas.items():
        op = name.removeprefix('context_')
        if ctx.register_tool(name=name, toolset='akasha_context', handler=handler(op),
                schema={'name': name, 'description': op + ' the authorized caller context',
                        'parameters': {'type': 'object', 'properties': properties,
                                       'required': required, 'additionalProperties': False}}) is None:
            raise RuntimeError('TOOL_REGISTRATION_CONFLICT')
    ctx.register_hook('pre_tool_call', before)
    def setup(parser):
        parser.add_argument('--request', required=True)
        parser.add_argument('--budget', required=True, type=float)
    ctx.register_cli_command(name='akasha-runner', help='Run a scoped Akasha worker.',
                             setup_fn=setup, handler_fn=lambda args: _run_native(ctx, names, args))


def register(ctx):
    if 'AKASHA_HERMES_RUN_SOCKET' in os.environ or 'AKASHA_HERMES_RUN_TOKEN' in os.environ:
        return _register_runner(ctx)
    pending = {}
    lock = threading.Lock()

    def before(tool_name='', args=None, session_id='', task_id='', turn_id='', tool_call_id='', **_):
        if tool_name != 'akasha_consult':
            return None
        key, entry = None, None
        try:
            key = _key(args, session_id, task_id)
            if not all(_identifier(s) for s in (turn_id, tool_call_id)):
                raise BridgeError('CALLER_AMBIGUOUS')
            with lock:
                # Native tool handlers receive session/task but no turn/call ID.
                if key in pending:
                    pending[key] = None
                    raise BridgeError('CALLER_AMBIGUOUS')
                if len(pending) >= 128:
                    raise BridgeError('RESOURCE_BUSY')
                entry = {'bound': None}
                pending[key] = entry
            event = _event(ctx, 'bind')
            event['call'] = {'session_id': session_id, 'task_id': task_id, 'turn_id': turn_id,
                             'tool_call_id': tool_call_id, 'profile_home': key[0],
                             'cwd': ctx.session_cwd()}
            bound = _exchange(ctx, event)
            with lock:
                if pending.get(key) is not entry:
                    raise BridgeError('CALLER_AMBIGUOUS')
                entry['bound'] = bound
        except BridgeError as error:
            with lock:
                if entry is not None and pending.get(key) is entry:
                    del pending[key]
            return {'action': 'block', 'message': str(error)}
        return None

    def consult(args, session_id='', task_id='', **_):
        try:
            key = _key(args, session_id, task_id)
            with lock:
                if key not in pending:
                    raise BridgeError('BINDING_UNAVAILABLE')
                if pending[key] is None:
                    raise BridgeError('CALLER_AMBIGUOUS')
                entry = pending.pop(key)
            if entry['bound'] is None:
                raise BridgeError('BINDING_UNAVAILABLE')
            result = {'schema_version': 1, 'data': entry['bound']}
        except BridgeError as error:
            result = {'schema_version': 1, 'error': {'code': str(error)}}
        return json.dumps(result)

    def sync(session_id='', **_):
        status = {'session_id': session_id}
        try:
            if not _identifier(session_id):
                raise BridgeError('CALLER_AMBIGUOUS')
            event = _event(ctx, 'sync')
            event['session_id'] = session_id
            job = _exchange(ctx, event)
            if not isinstance(job.get('id'), str) or not isinstance(job.get('state'), str):
                raise BridgeError('PROTOCOL_ERROR')
            status.update(job_id=job['id'], state=job['state'])
        except BridgeError as error:
            status['error'] = str(error)
            LOG.warning('Background capture: %s', error)
        try:
            ctx.state.set('capture', status)
        except (OSError, RuntimeError, ValueError):
            LOG.warning('Background capture status: STATE_UNAVAILABLE')

    def end(session_id='', **kwargs):
        home = str(get_hermes_home().resolve())
        with lock:
            for key in list(pending):
                if key[:2] == (home, session_id):
                    del pending[key]
        sync(session_id=session_id, **kwargs)

    registered = ctx.register_tool(name='akasha_consult', toolset='akasha', handler=consult,
                      schema={'name': 'akasha_consult', 'description': 'Consult saved user context.',
                              'parameters': {'type': 'object', 'properties': {
                                  'hint': {'type': ['string', 'null'], 'maxLength': 512,
                                           'description': 'Short optional intent.'}},
                                  'additionalProperties': False}})
    if registered is None:
        raise RuntimeError('TOOL_REGISTRATION_CONFLICT')
    ctx.register_hook('pre_tool_call', before)
    ctx.register_hook('on_session_start', sync)
    ctx.register_hook('on_session_end', end)
    ctx.register_hook('post_compaction', sync)
