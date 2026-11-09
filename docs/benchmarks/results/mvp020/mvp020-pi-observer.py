#!/usr/bin/python3
import json, os, pathlib, re, subprocess, sys

pi = '/mnt/ssd/mirror-rootfs/home/obj/.local/share/mise-rebuilt/installs/npm-earendil-works-pi-coding-agent/1.4.0/node_modules/.bin/pi'
if sys.argv[1:] == ['--version']:
    os.execv(pi, [pi, '--version'])
output = pathlib.Path(os.environ['AKASHA_PI_DIAGNOSTIC'])
assert not output.exists()
child = subprocess.Popen([pi, *sys.argv[1:]], stdin=sys.stdin.buffer, stdout=subprocess.PIPE, stderr=sys.stderr.buffer)
with output.open('x') as log:
    for line in child.stdout:
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
        try:
            value = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            continue
        if not isinstance(value, dict):
            continue
        message = value.get('message', {})
        if not isinstance(message, dict):
            message = {}
        error = value.get('error') or message.get('errorMessage')
        if error or value.get('type') == 'extension_error' or message.get('stopReason') == 'error':
            text = str(error or value.get('message') or value.get('stack') or '')
            text = re.sub(r'https?://\S+|Bearer\s+\S+|\bsk-[\w-]+|[A-Za-z0-9_=-]{64,}', '[redacted]', text)
            if log.tell() < 64 * 1024:
                log.write(json.dumps({'type': value.get('type'), 'stop_reason': message.get('stopReason'), 'error': text[:1200], 'keys': sorted(value)}) + '\n')
                log.flush()
raise SystemExit(child.wait())
