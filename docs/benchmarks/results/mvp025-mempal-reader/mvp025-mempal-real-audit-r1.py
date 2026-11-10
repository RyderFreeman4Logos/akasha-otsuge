import datetime, hashlib, json, os, sqlite3, struct, sys, time, tomllib
from pathlib import Path

E = Path(__file__).parent
OUT = E / 'mvp025-mempal-real-audit-r1'
PLAN = E / 'mvp025-mempal-audit-plan-r1.json'
CONFIG = E / 'mvp025-mempal-real-audit-r1.toml'


def atomic(path, value):
    temporary = path.with_name(path.name + '.new')
    with temporary.open('w') as f:
        os.chmod(temporary, 0o600)
        json.dump(value, f, indent=2); f.write('\n'); f.flush(); os.fsync(f.fileno())
    os.replace(temporary, path)
    fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try: os.fsync(fd)
    finally: os.close(fd)


def row_digest(row):
    digest = hashlib.sha256(b'mempal-typed-row-v1\0')
    for value in row:
        if value is None: tag, data = b'n', b''
        elif isinstance(value, int): tag, data = b'i', value.to_bytes(8, 'big', signed=True)
        elif isinstance(value, float): tag, data = b'r', struct.pack('>d', value)
        elif isinstance(value, str): tag, data = b't', value.encode('utf-8')
        elif isinstance(value, bytes): tag, data = b'b', value
        else: raise TypeError(type(value).__name__)
        digest.update(tag); digest.update(len(data).to_bytes(8, 'big')); digest.update(data)
    return digest.digest()


def self_check():
    values = [(None,), ('',), (b'',), (0,), (0.0,), ('0',), ('中\0x',)]
    assert len({row_digest(v) for v in values}) == len(values)
    expected = hashlib.sha256(b'mempal-typed-row-v1\0' + b't' + (5).to_bytes(8, 'big') + '中\0x'.encode()).digest()
    assert row_digest(('中\0x',)) == expected
    assert row_digest(('a', 'b')) != row_digest(('b', 'a'))
    print('typed row oracle self-check PASS')


def main():
    if sys.argv[1:] == ['--self-check']:
        self_check(); return
    assert sys.argv[1:] in [[], ['--resume']]
    resume = bool(sys.argv[1:]); start = time.monotonic()
    receipt = json.loads((E / 'mvp025-mempal-online-snapshot-r1.json').read_text())
    source = Path(receipt['snapshot']); expected = receipt['attempts'][-1]
    assert source.is_file() and not source.is_symlink()
    assert os.statvfs(source).f_flag & os.ST_RDONLY, 'source mount must be kernel read-only'
    before = source.stat()
    assert before.st_size == expected['snapshot_bytes']
    assert not any(Path(str(source) + suffix).exists() for suffix in ['-wal', '-shm', '-journal'])
    plan = json.loads(PLAN.read_text())
    identity = {'snapshot_sha256': expected['snapshot_sha256'], 'plan_sha256': hashlib.sha256(PLAN.read_bytes()).hexdigest(),
                'script_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                'source_stat': [before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns],
                'sqlite_version': sqlite3.sqlite_version}
    units = ['quick_check', *plan['authoritative_and_inert_tables'], 'relationships', 'source_sha256']
    if resume:
        state = json.loads((OUT / 'state.json').read_text()); assert state['identity'] == identity
        for unit, digest in state['completed'].items():
            assert unit in units and hashlib.sha256((OUT / (unit + '.json')).read_bytes()).hexdigest() == digest
    else:
        OUT.mkdir(mode=0o700)
        state = {'identity': identity, 'run_id': OUT.name, 'start_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'completed': {}, 'epochs': {}, 'inflight': None}
        atomic(OUT / 'state.json', state)
    db = sqlite3.connect(source.as_uri() + '?mode=ro&immutable=1', uri=True)
    db.setlimit(sqlite3.SQLITE_LIMIT_LENGTH, 64 * 1024 * 1024)
    db.execute('pragma query_only=on'); db.execute('pragma trusted_schema=off')
    db.execute('pragma temp_store=memory'); db.execute('pragma cache_size=-2048'); db.execute('begin')
    assert db.execute('pragma user_version').fetchone()[0] == 21
    assert db.execute("select value from fork_ext_meta where key='fork_ext_version'").fetchone()[0] == '28'
    def progress(unit, epoch, rows=None):
        elapsed = time.monotonic() - start
        atomic(OUT / 'progress.yaml', {'completed': len(state['completed']), 'total': len(units), 'elapsed_s': elapsed,
               'rate_per_s': len(state['completed']) / elapsed if elapsed else None, 'eta_s': None,
               'eta_reason': 'table sizes differ; no comparable unit-duration estimate', 'active_unit': unit, 'rows_scanned_in_unit': rows,
               'pid': os.getpid(), 'start_ticks': Path('/proc/self/stat').read_text().split(') ', 1)[1].split()[19], 'config_epoch': epoch})
    for unit in units:
        if unit in state['completed']: continue
        raw = CONFIG.read_bytes(); config = tomllib.loads(raw.decode()); epoch = hashlib.sha256(raw).hexdigest()
        assert config['reliability']['max_attempts'] == 1
        timeout = config['execution']['unit_timeout_seconds']; assert 60 <= timeout <= 7200
        state['epochs'][epoch] = config; state['inflight'] = unit; atomic(OUT / 'state.json', state)
        began = time.monotonic(); db.set_progress_handler(lambda: int(time.monotonic() - began > timeout), 10000)
        progress(unit, epoch); print(json.dumps({'unit': unit, 'state': 'started'}), flush=True)
        if unit == 'quick_check':
            result = db.execute('pragma quick_check').fetchmany(1001)
            assert result == [('ok',)], 'SQLite quick_check failed'
            result = {'status': 'ok', 'scope': 'SQLite quick_check; no external vec0 extension validation'}
        elif unit == 'source_sha256':
            with source.open('rb') as f: digest = hashlib.file_digest(f, 'sha256').hexdigest()
            after = source.stat()
            assert [after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns] == identity['source_stat']
            assert digest == expected['snapshot_sha256']
            result = {'sha256': digest, 'bytes': after.st_size, 'matches_acquired_snapshot': True}
        elif unit == 'relationships':
            links = [('triples','source_drawer','drawers','id'), ('knowledge_evidence_links','card_id','knowledge_cards','id'),
                     ('knowledge_evidence_links','evidence_drawer_id','drawers','id'), ('knowledge_events','card_id','knowledge_cards','id'),
                     ('foresights','drawer_id','drawers','id'), ('failure_events','drawer_id','drawers','id'),
                     ('pending_message_completions','result_drawer_id','drawers','id')]
            result = {}
            for table, field, target, key in links:
                row = db.execute(f'SELECT count(*), coalesce(sum(NOT EXISTS (SELECT 1 FROM "{target}" t WHERE t."{key}"=s."{field}")),0) FROM "{table}" s WHERE s."{field}" IS NOT NULL AND s."{field}" != ?', ('',)).fetchone()
                result[f'{table}.{field}->{target}.{key}'] = {'present_references': row[0], 'missing_targets': row[1]}
        else:
            columns = plan['authoritative_and_inert_tables'][unit]
            actual = [row[1] for row in db.execute('pragma table_info("' + unit + '")')]
            assert actual == columns and all(name.replace('_','').isalnum() for name in [unit,*columns])
            digest = hashlib.sha256(b'mempal-table-v1\0' + json.dumps(columns,separators=(',',':')).encode())
            rows = text_bytes = blob_bytes = max_text = max_row = 0; samples=[]; last=began
            query = 'SELECT rowid,' + ','.join('"' + name + '"' for name in columns) + ' FROM "' + unit + '" ORDER BY rowid'
            for row in db.execute(query):
                rd = row_digest(row); digest.update(rd); rows += 1
                if len(samples) < 3: samples.append(rd.hex())
                sizes = [len(v.encode('utf-8')) if isinstance(v,str) else len(v) if isinstance(v,bytes) else 0 for v in row]
                text_bytes += sum(size for size,v in zip(sizes,row) if isinstance(v,str)); blob_bytes += sum(size for size,v in zip(sizes,row) if isinstance(v,bytes))
                max_text = max(max_text, *(size for size,v in zip(sizes,row) if isinstance(v,str)), 0); max_row = max(max_row,sum(sizes))
                if time.monotonic() - last >= 5: progress(unit,epoch,rows); last=time.monotonic()
            result = {'rows': rows, 'typed_rows_sha256': digest.hexdigest(), 'utf8_bytes': text_bytes, 'blob_bytes': blob_bytes,
                      'max_text_cell_bytes': max_text, 'max_row_text_blob_bytes': max_row, 'first_three_row_sha256': samples,
                      'digest_order': 'SQLite rowid ascending; typed row includes rowid followed by schema column order'}
        result.update({'unit':unit,'config_epoch':epoch,'seconds':time.monotonic()-began})
        path = OUT / (unit + '.json'); atomic(path, result)
        state['completed'][unit] = hashlib.sha256(path.read_bytes()).hexdigest(); state['inflight'] = None
        atomic(OUT / 'state.json', state); progress(None,epoch)
        print(json.dumps({'unit':unit,'state':'completed','rows':result.get('rows'),'seconds':result['seconds']}),flush=True)
    db.close(); assert len(state['completed']) == len(units)
    print(json.dumps({'status':'PASS','completed':len(units),'seconds':time.monotonic()-start,'migration_acceptance':False}),flush=True)

if __name__ == '__main__': main()
