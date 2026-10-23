# Storage experiment environment

Captured 2026-10-23 01:08 UTC (2026-10-22 Pacific), before candidate measurements.
Raw local inventory: `.git/akasha-evidence/mvp-003-environment.json`.
No benchmark result is implied by this inventory.

| Item | Observed |
| --- | --- |
| OS/kernel | Debian 12; Linux 6.1.0-52-amd64, x86_64 |
| CPU | Intel Core i9-12900H; 20 logical CPUs |
| RAM | 32,555,648 KiB total; 20,026,820 KiB available at capture |
| Repository filesystem | Btrfs on Samsung MZVLW256HEHP-00000 NVMe SSD |
| Repository filesystem available | 129,716,551,680 bytes at capture |
| Temporary filesystem | Btrfs on Samsung MZVL21T0HCLR-00B00 NVMe SSD |
| Temporary filesystem available | 214,736,838,656 bytes at follow-up inventory |
| Swap | 8,334,241,792-byte zram plus 8,589,930,496-byte disk swap |
| Rust | 1.99.1 (58cf8f9c3, 2026-10-13), LLVM 23.1.1 |
| Competing load | Shared workstation with existing services and other agents; no services stopped |
| cgroups | v2; login scope not delegated, systemd user service limits available |

Successful dedicated-service capability probe (exited 0):

```sh
systemd-run --user --wait --pipe --collect \
  --unit=akasha-storage-cgroup-probe \
  -p MemoryHigh=536870912 -p MemoryMax=671088640 -p MemorySwapMax=0 \
  python3 -c 'import pathlib; p=pathlib.Path("/sys/fs/cgroup"+pathlib.Path("/proc/self/cgroup").read_text().strip().split("::")[1]); print({n:(p/n).read_text().strip() for n in ["memory.high","memory.max","memory.swap.max","memory.current","memory.events"]})'
```

Read back `memory.high=536870912`, `memory.max=671088640`,
`memory.swap.max=0`; all memory event counters zero. Subsequent experiment units
must verify their own limits rather than borrowing this receipt. Cgroup accounting
also charges page cache, so report it separately from RSS. No global cache drop,
swap change, disk cleanup, private data access or existing service restart occurred.

Space is shared and changes over time. Recheck free space and reserve before
ingestion/reindex; the inventory does not guarantee a full corpus plus all derived
generations will fit. No full corpus has been downloaded or generated yet.
