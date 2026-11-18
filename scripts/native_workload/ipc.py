"""Bounded asynchronous client for the existing one-request-per-connection IPC."""

import asyncio
import json
import os
import socket
import struct

MAX_MESSAGE = 2 * 1024**2


class ProtocolError(Exception):
    pass


class TransportError(Exception):
    def __init__(self, phase, cause):
        self.phase = phase
        self.cause = type(cause).__name__
        self.effect_may_have_started = phase in ("send", "response")
        super().__init__(f"{phase}: {self.cause}")


async def read_frame(reader):
    size = struct.unpack("!I", await reader.readexactly(4))[0]
    if not 0 < size <= MAX_MESSAGE:
        raise ProtocolError("invalid frame size")
    try:
        value = json.loads(await reader.readexactly(size))
    except (ValueError, UnicodeDecodeError) as error:
        raise ProtocolError("invalid JSON frame") from error
    if not isinstance(value, dict):
        raise ProtocolError("frame must be an object")
    return value


def frame(value):
    encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":"), allow_nan=False).encode()
    if not 0 < len(encoded) <= MAX_MESSAGE:
        raise ProtocolError("outgoing frame size")
    return struct.pack("!I", len(encoded)) + encoded


async def call(path, command, request_id, deadline_unix_ms, timeout_s=10):
    """One absolute client timeout; never automatically retry a possible effect.

    Server errors are returned intact for the owning workload's classification.
    They are not converted into successful replies or silently retried.
    """
    phase, writer = "connect", None
    if (
        not isinstance(request_id, str)
        or not request_id
        or type(deadline_unix_ms) is not int
        or timeout_s <= 0
    ):
        raise ValueError("invalid request identity or deadline")
    raw = frame(
        dict(
            protocol_version=1, request_id=request_id, deadline_ms=deadline_unix_ms, command=command
        )
    )
    try:
        async with asyncio.timeout(timeout_s):
            reader, writer = await asyncio.open_unix_connection(str(path), limit=MAX_MESSAGE + 4)
            peer = writer.get_extra_info("socket")
            _, uid, _ = struct.unpack(
                "3i", peer.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
            )
            if uid != os.geteuid():
                raise ProtocolError("peer uid mismatch")
            phase = "greeting"
            hello = await read_frame(reader)
            if hello.get("protocol_version") != 1 or hello.get("max_message_bytes") != MAX_MESSAGE:
                raise ProtocolError("greeting version or limit mismatch")
            if "error" in hello:
                return dict(
                    schema_version=1, request_id=request_id, sequence=1, error=hello["error"]
                )
            phase = "send"
            writer.write(raw)
            await writer.drain()
            phase = "response"
            reply = await read_frame(reader)
            if (
                reply.get("schema_version") != 1
                or reply.get("request_id") != request_id
                or reply.get("sequence") != 1
                or ("data" in reply) == ("error" in reply)
            ):
                raise ProtocolError("response identity or payload mismatch")
            return reply
    except (OSError, EOFError, asyncio.IncompleteReadError, TimeoutError, ProtocolError) as error:
        raise TransportError(phase, error) from error
    finally:
        if writer is not None:
            writer.close()
            # No daemon effect is retried if local connection shutdown fails.
            try:
                async with asyncio.timeout(1):
                    await writer.wait_closed()
            except (OSError, TimeoutError):
                pass
