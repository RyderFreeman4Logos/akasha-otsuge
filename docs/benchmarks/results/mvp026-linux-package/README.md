# Linux release candidate verification

The [hosted download receipt](hosted-4e7b3a2.json) records a clean Linux x86_64 consumer installation from the archive built on main at `4e7b3a2`. The downloaded ZIP matched the digest published by GitHub Actions. The inner archive was 100,403,500 bytes with SHA-256 `4cc09f4c5d0cb51f7d778d181bdf74747bf4876402c2ee136e8b5652b07509a8`. The artifact expires on 2026-11-25. The receipt preserves its provenance and checksums. [Build and download](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/44311528619)

Pi 1.4.0 captured an event, obtained a consult result, entered review and delivered a notification from this archive. The upgrade and rollback preserved 129 originals, 30 earlier commits and the queued job. Restoring the neutral archive in a fresh store reproduced all three exported files byte for byte. The Pi fixture used its local test provider.


The [x86_64 receipt](candidate-3961ac0.json) records two byte-identical packaging runs on 2026-11-18 from source `3961ac03819c23b179beb0ac4512ad67c51f3be9`. Both used the same toolchain, checkout and Cargo cache. A fresh Linux namespace with a read-only host userland, private network, empty HOME/XDG directories and the real Pi 1.4.0 installation ran the archive's binary. The manifest contained 487 compiled third-party packages and 293 distinct notice texts. The upgrade test verified the digest of every referenced text.

The real Pi Caller captured a new user message. The real Pi Runner read that message and returned a cited conclusion. The installed CLI read the same original bytes and verified the persisted consult output. Extraction produced a candidate. The operator changed its open review to investigating, read its notification and acknowledged the delivered generation twice with the same idempotency key. Its authority remained candidate. After removing the Pi adapter and installed CLI, the existing Pi configuration, native session and all seven application data files had their original digests. The provider supplied deterministic loopback responses, with zero authenticated model calls or inference charges.

An independent upgrade started from the executable built at `63acf39d2138bc582ee746b6eefede970fd3f8cc`. It completed one capture, submitted another and stopped the daemon. A stopped-store export showed the second job still pending. The new binary resumed the same job and preserved its admission timestamps, budgets and idempotency key. The export retained all prior logical revisions, commit clocks and receipts. The verifier reconstructed and matched all 129 originals, totaling 528,384 raw bytes and 528,384 normalized bytes.

## Repeat for another candidate

On Linux, provide Python 3.11 or later, a built baseline executable and its trusted checksum, the candidate archive and checksum, and a short writable scratch path. For the Pi test use a clean user space with Pi 1.4.0, its supported Node interpreter and no other agent CLI on PATH. Both tests use temporary homes, run the installed executable and print one JSON receipt. Save the receipts alongside the selected candidate.

```sh
python3 -B scripts/check_linux_upgrade.py \
  --baseline "$baseline" --baseline-sha256 "$baseline_sha256" \
  --archive "$archive" --sha256 "$archive_sha256" --commit "$commit" --scratch "$scratch"

python3 -B tests/fixtures/pi/1.4.0/check_linux_package.py \
  --pi "$pi" --archive "$archive" --sha256 "$archive_sha256" \
  --commit "$commit" --scratch "$scratch"
```

## Earlier attempts

- The first upgrade probe received `STORAGE_ERROR` opening the stopped-store export. Another probe received `IO_ERROR` reading the pending job. The successful run waited for process termination as well as the draining acknowledgement before replacing the binary.
- The preceding package from `10b09a8` preserved the workload but had 29 third-party inventory entries with no bundled license text. The later candidate includes a verified upstream fallback for missing texts.
- An early assertion compared physical export prefixes. Export traverses request keys, so a later request may change blob offsets. The maintained verifier compares each prior commit's metadata and all original payload bytes by immutable logical revision.

The receipt covers a local Linux x86_64 build. A GitHub Actions artifact download and checksum readback are separate publication checks. [Candidate builds and CI downloads](../../../development.md).
