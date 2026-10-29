"""Hermes native Caller integration; runtime identity never comes from model arguments."""

import json
import logging
import os
from pathlib import Path
import re
import socket
import struct
import threading
import time
import uuid

from hermes_constants import get_hermes_home

LOG = logging.getLogger('akasha_otsuge.hermes')
DEFAULTS = {}


class BridgeError(Exception):
    pass


def _exchange(ctx, event):
    path = ctx.get_config('socket', DEFAULTS.get('socket', ''))
    if not isinstance(path, str) or not Path(path).is_absolute():
        raise BridgeError('CONFIG_INVALID')
    deadline = time.monotonic() + 10
    request_id = uuid.uuid4().hex
    request = {'protocol_version': 1, 'request_id': request_id,
               'command': {'op': 'hermes', 'event': event}}
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


def register(ctx):
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
