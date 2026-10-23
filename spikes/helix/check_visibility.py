#!/usr/bin/env python3
"""Check latest-head filtering and live permissions with real stale search entries."""

import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import uuid

from run_case import digest, process_identity, publish


def main():
    binary, fixture, source = [pathlib.Path(value).resolve(strict=True) for value in sys.argv[1:4]]
    output = pathlib.Path(sys.argv[4]).resolve()
    manifest = source.with_suffix(".manifest.json")
    if manifest.stat().st_size > 65536 or fixture.stat().st_size > 4 * 1024**2:
        raise ValueError("fixture metadata exceeds bounds")
    expected = json.loads(manifest.read_text())
    if digest(source) != expected["prepared_sha256"] or source.stat().st_size != expected["prepared_bytes"]:
        raise ValueError("prepared public input identity changed")
    queries = json.loads(fixture.read_text())["queries"]
    if len(queries) != 8:
        raise ValueError("diagnostic requires eight real partition queries")
    keys = {query["key"] for query in queries}
    selected = {}
    with source.open() as stream:
        while len(selected) < len(keys):
            line = stream.readline(512 * 1024 + 1)
            if not line:
                raise ValueError("not all query documents occur in source")
            if len(line.encode()) > 512 * 1024:
                raise ValueError("source record exceeds input bound")
            row = json.loads(line)
            if row["key"] in keys:
                selected[row["key"]] = row
    for query in queries:
        if selected[query["key"]]["embedding"] != query["embedding"]:
            raise ValueError("query vector differs from its real source")
    output.mkdir(mode=0o700)
    (output / "OWNER").write_text("akasha-otsuge MVP-003 owned visibility diagnostic receipts\n")
    publish(output / "admission.json", {"controller":process_identity(os.getpid()),
        "binary_sha256":digest(binary), "source_sha256":expected["prepared_sha256"],
        "fixture_sha256":digest(fixture), "selected_documents":8, "data_kind":"public source subset; not scale acceptance"})
    prepared=output / "input.jsonl"
    prepared.write_text("".join(json.dumps(selected[query["key"]])+"\n" for query in queries))
    observer = pathlib.Path(__file__).resolve().parents[1] / "measure.py"
    started=time.monotonic()
    results={}
    attempt=uuid.uuid4().hex
    with tempfile.TemporaryDirectory(prefix="akasha-visibility-") as temporary:
        database=pathlib.Path(temporary)/"database"
        for index,(name,args) in enumerate([
            ("import",["--load",str(prepared)]), ("indexes",["--build-indexes","300"]),
            ("seed",["--visibility-seed",str(fixture)]), ("reopen",["--visibility-verify",str(fixture)])]):
            elapsed=time.monotonic()-started
            publish(output/"progress.json",{"phase":name,"completed":index,"total":4,"elapsed_s":elapsed,"rate_per_s":index/elapsed,"eta_s":None})
            unit=f"akasha-visibility-{attempt}-{name}"
            command=["systemd-run","--user","--quiet","--wait","--pipe","--collect","--unit="+unit,
                     "-p","MemoryMax=671088640","-p","MemorySwapMax=0","-p","RuntimeMaxSec=600",
                     "-p","Environment=AKASHA_SPIKE_PROFILE=low",sys.executable,str(observer),
                     str(output/(name+"-memory.jsonl")),str(binary),str(database),*args]
            publish(output/(name+"-command.json"),{"unit":unit,"command":command})
            try:
                with (output/(name+".log")).open("x") as log:
                    result=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=630)
            finally:
                subprocess.run(["systemctl","--user","stop",unit],capture_output=True,timeout=30,check=False)
            if result.returncode:
                raise RuntimeError(f"{name} failed; inspect {output / (name+'.log')}")
            rows=[json.loads(line) for line in (output/(name+".log")).read_text().splitlines() if line.startswith("{")]
            results[name]=rows[-1]
    publish(output/"progress.json",{"phase":"completed","completed":4,"total":4,"elapsed_s":time.monotonic()-started,"rate_per_s":4/(time.monotonic()-started),"eta_s":0})
    summary={"status":"passed","seconds":time.monotonic()-started,"native":results,
             "artifact_sha256":{path.name:digest(path) for path in sorted(output.iterdir()) if path.is_file()}}
    publish(output/"result.json",summary)
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
