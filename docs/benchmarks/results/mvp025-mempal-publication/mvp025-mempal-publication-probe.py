import hashlib, json, os, pathlib, sqlite3, subprocess, tempfile

E = pathlib.Path(__file__).parent
W = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025')
cache = pathlib.Path('/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9')
baseline = W / 'target/release/akasha-otsuge'
candidate = W / 'target/debug/akasha-otsuge'
expected_baseline = 'a5a37f3a8cdc05dea1f7e4d751544a48593e61b8bf9912ce4eb389a2d65c88fe'
assert hashlib.sha256(baseline.read_bytes()).hexdigest() == expected_baseline
observations = []
with tempfile.TemporaryDirectory(prefix='mempal-publish-probe-', dir=cache) as tmp:
    root = pathlib.Path(tmp)
    library = root / 'probe.so'
    subprocess.run(['cc', '-shared', '-fPIC', '-Wall', '-Wextra', '-Werror', '-o', str(library), str(E/'mvp025-mempal-publication-probe.c'), '-ldl'], check=True)
    for label, binary in [('baseline', baseline), ('candidate', candidate)]:
        for phase in ['control', 'identity-final', 'manifest-final', 'identity-temporary', 'manifest-temporary']:
            case = root / (label + '-' + phase)
            case.mkdir(mode=0o700)
            source, destination = case/'source.db', case/'archive'
            db = sqlite3.connect(source)
            db.executescript((W/'tests/fixtures/mempal21-28.sql').read_text())
            db.close()
            source.chmod(0o600)
            original = source.read_bytes()
            metadata = source.stat()
            config = case/'config.toml'
            config.write_text('[paths]\ndata_dir='+json.dumps(str(case/'data'))+'\nsocket_path='+json.dumps(str(case/'socket'))+'\n')
            env = os.environ.copy()
            moved = source
            if phase != 'control':
                leaf = 'conversion.json' if phase.startswith('identity') else 'manifest.json'
                if phase.endswith('temporary'):
                    leaf += '.tmp'
                moved = destination/leaf
                env.update(LD_PRELOAD=str(library), AKASHA_PROBE_SOURCE=str(source), AKASHA_PROBE_TARGET=str(moved), AKASHA_PROBE_PREFIX='{"mapping_version":' if phase.startswith('identity') else '{"archive_version":')
            command = [str(binary), '--config', str(config), 'migrate', 'mempal', str(source), str(destination)]
            result = subprocess.run(command, env=env, stdin=subprocess.DEVNULL, capture_output=True, timeout=60)
            if phase != 'control':
                assert b'AKASHA_PROBE_SOURCE_MOVED_BEFORE_METADATA_FSYNC' in result.stderr, (label, phase, result.stderr)
            preserved = moved.exists() and moved.read_bytes() == original and (moved.stat().st_dev, moved.stat().st_ino) == (metadata.st_dev, metadata.st_ino)
            row = {'binary':label, 'phase':phase, 'exit':result.returncode, 'moved_source_preserved':preserved, 'fixture_move_observed':phase!='control'}
            observations.append(row)
            print(json.dumps(row), flush=True)
            if phase == 'control':
                assert result.returncode == 0 and preserved
            elif label == 'baseline':
                assert not preserved, ('baseline did not expose the original replacement', row)
            else:
                assert preserved, row
                if phase.endswith('final'):
                    assert result.returncode != 0, row
receipt = {'status':'PASS', 'observations':observations, 'baseline_binary_sha256':expected_baseline,
           'candidate_binary_sha256':hashlib.sha256(candidate.read_bytes()).hexdigest(),
           'interposer_sha256':hashlib.sha256((E/'mvp025-mempal-publication-probe.c').read_bytes()).hexdigest(),
           'owned_fixture_removed':not root.exists(), 'limits':['Controlled tiny structural SQLite fixtures only.', 'The fixture performs the source move; production must preserve the moved inode and bytes.']}
with (E/'mvp025-mempal-publication-probe-r1.json').open('x') as file:
    json.dump(receipt, file, indent=2)
print('Both controls pass; four baseline replacement failures and four candidate source-preservation cases verified; scratch removed')
