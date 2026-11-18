"""Fsynced append-only records for one owned benchmark interval."""

import asyncio
import hashlib
import json
import os


def encode(value):
    return json.dumps(
        value, sort_keys=True, ensure_ascii=False, separators=(",", ":"), allow_nan=False
    ).encode()


class Journal:
    def __init__(self, path):
        self.file = path.open("xb")
        self.sequence = 0
        self.previous = "0" * 64
        self.lock = asyncio.Lock()
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)

    async def append(self, value):
        async with self.lock:
            record = dict(sequence=self.sequence + 1, previous_sha256=self.previous, value=value)
            line = encode(record) + b"\n"
            if len(line) > 4 * 1024**2:
                raise ValueError("journal record exceeds bound")

            def write():
                self.file.write(line)
                self.file.flush()
                os.fsync(self.file.fileno())

            pending = asyncio.create_task(asyncio.to_thread(write))
            try:
                await asyncio.shield(pending)
            except asyncio.CancelledError:
                # Finish the admitted disk write before a cancelled caller can close the file.
                await pending
                self.sequence += 1
                self.previous = hashlib.sha256(line).hexdigest()
                raise
            self.sequence += 1
            self.previous = hashlib.sha256(line).hexdigest()

    def close(self):
        self.file.close()


def records(path):
    previous = "0" * 64
    number = 0
    with path.open("rb") as source:
        while True:
            line = source.readline(4 * 1024**2 + 1)
            if not line:
                break
            if len(line) > 4 * 1024**2 or not line.endswith(b"\n"):
                raise ValueError("partial or oversized journal record")
            row = json.loads(line)
            number += 1
            if row["sequence"] != number or row["previous_sha256"] != previous:
                raise ValueError("journal chain mismatch")
            previous = hashlib.sha256(line).hexdigest()
            yield row["value"]
