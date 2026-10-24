"""Evict only an exclusively owned experiment's pages and measure kernel residency."""
import ctypes
import fcntl
import mmap
import os
import pathlib
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent / 'helix'))
from run_case import process_identity, publish


def resident_pages(fd, size):
    if not size:
        return 0
    libc = ctypes.CDLL(None, use_errno=True)
    libc.mincore.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.POINTER(ctypes.c_ubyte)]
    libc.mincore.restype = ctypes.c_int
    bits = (ctypes.c_ubyte * ((size + mmap.PAGESIZE - 1) // mmap.PAGESIZE))()
    with mmap.mmap(fd, size, access=mmap.ACCESS_COPY) as mapping:
        pointer = ctypes.addressof(ctypes.c_char.from_buffer(mapping))
        if libc.mincore(pointer, size, bits) != 0:
            raise OSError(ctypes.get_errno(), 'mincore failed')
    return sum(bit & 1 for bit in bits)


def prepare(root, output, owner_lock):
    root, output = root.resolve(strict=True), output.resolve()
    marker = root.parent / 'OWNER'
    if not marker.is_file() or not marker.read_text().startswith('akasha-otsuge MVP-003 '):
        raise ValueError('cache preparation requires an owned experiment')
    if output == root or root in output.parents:
        raise ValueError('cache receipts must be outside the database')
    if (root / '.akasha-case-owner.lock').is_symlink():
        raise ValueError('owner lock is a symlink')
    expected, actual = (root / '.akasha-case-owner.lock').stat(), os.fstat(owner_lock.fileno())
    if (expected.st_dev, expected.st_ino) != (actual.st_dev, actual.st_ino):
        raise ValueError('owner lock does not belong to this database')
    fcntl.flock(owner_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    files = []
    for path in root.rglob('*'):
        if path.is_symlink():
            raise ValueError('cache preparation refuses symlink: ' + str(path))
        if path.is_file() and not path.name.startswith('.akasha-'):
            files.append(path)
    started, before, after, rows = time.monotonic(), 0, 0, []
    for i, path in enumerate(sorted(files), 1):
        with path.open('rb') as source:
            fd = source.fileno()
            stat = os.fstat(fd)
            prior = resident_pages(fd, stat.st_size)
            os.fsync(fd)
            os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
            remaining = resident_pages(fd, stat.st_size)
            final = os.fstat(fd)
            if any(getattr(stat, key) != getattr(final, key) for key in
                   ['st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_ctime_ns']):
                raise ValueError('file changed during cold preparation')
        before += prior
        after += remaining
        rows.append(dict(file=str(path.relative_to(root)), bytes=stat.st_size,
            allocated_bytes=stat.st_blocks*512, resident_before=prior, resident_after=remaining))
        if i % 64 == 0 or i == len(files):
            elapsed = time.monotonic()-started
            publish(output / 'cold-cache-progress.json', dict(completed=i, total=len(files),
                elapsed_s=elapsed, rate_per_s=i/elapsed, eta_s=(len(files)-i)*elapsed/i,
                **process_identity(os.getpid())))
    result = dict(status='passed' if rows and after == 0 else 'failed', files=len(rows),
        seconds=time.monotonic()-started, resident_pages_before=before, resident_pages_after=after,
        finished_unix_s=time.time(), database=str(root),
        method='fsync and POSIX_FADV_DONTNEED on owned files; mincore without reading mapped pages',
        scope='per-file residency immediately after eviction under exclusive database lock',
        rows=rows)
    publish(output / 'cold-cache.json', result)
    if result['status'] != 'passed':
        raise ValueError('owned database pages remain resident or no data files exist')
    return result
