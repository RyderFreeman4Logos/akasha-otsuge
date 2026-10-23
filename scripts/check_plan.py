"""Validate the immutable v0.1 specification and actual GitHub issue mapping."""

import argparse
import hashlib
import json
from pathlib import Path
import re


def require(condition, message):
    if not condition:
        raise ValueError(message)


def validate_snapshot(root):
    manifest = json.loads((root / "docs/spec/manifest.json").read_text())
    expected = {f"docs/spec/{i:02}.md" for i in range(13)} | {
        f"docs/spec/issues/MVP-{i:03}.md" for i in range(1, 29)
    }
    pages = manifest["pages"]
    require(len(pages) == 41, "expected all 41 source pages")
    require({p["markdown_path"] for p in pages} == expected, "source page set changed")
    require(len({p["url"] for p in pages}) == 41, "duplicate source URLs")
    for page in pages:
        for kind in ("raw", "markdown"):
            path = root / page[f"{kind}_path"]
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            require(digest == page[f"{kind}_sha256"], f"snapshot hash mismatch: {path}")
        raw = json.loads((root / page["raw_path"]).read_text())
        require(raw["url"] == page["url"], "snapshot URL mismatch")
        require(not raw.get("truncated") and not raw.get("unknown_block_count"),
                "incomplete source page")
        require("<mention" not in (root / page["markdown_path"]).read_text(),
                "unconverted Notion link")


def validate_index(root, data, remote=None):
    issues = data["issues"]
    expected = {f"MVP-{i:03}" for i in range(1, 29)}
    require(len(issues) == 28 and {i["plan_id"] for i in issues} == expected,
            "expected 28 unique plan IDs")
    require(len({i["number"] for i in issues}) == 28, "duplicate GitHub issue number")
    require(data["repository"] == "RyderFreeman4Logos/akasha-otsuge", "wrong repository")
    by_id = {i["plan_id"]: i for i in issues}
    source_index = (root / "docs/spec/11.md").read_text()
    covered = set()
    for issue in issues:
        plan = issue["plan_id"]
        number = issue["number"]
        require(type(number) is int and number > 0, f"invalid issue number: {plan}")
        require(issue["url"] == f"https://github.com/{data['repository']}/issues/{number}",
                f"wrong issue URL: {plan}")
        require(issue["title"].startswith(f"[{plan}] "), f"wrong title: {plan}")
        require(issue["spec_path"] == f"docs/spec/issues/{plan}.md", "wrong spec path")
        source = (root / issue["spec_path"]).read_text()
        require(issue["source_url"] in source, f"missing source link: {plan}")
        row = next(line for line in source_index.splitlines() if line.startswith(f"| {plan} |"))
        deps = re.findall(r"MVP-\d{3}", row.split("|")[3])
        req_line = next(line for line in source.splitlines() if "覆盖需求" in line)
        reqs = re.findall(r"REQ-\d{2}", req_line)
        if plan == "MVP-028":
            deps = [f"MVP-{i:03}" for i in range(1, 28)]
            reqs = [f"REQ-{i:02}" for i in range(1, 25)]
        require(issue["dependencies"] == deps, f"source dependency mismatch: {plan}")
        require(issue["requirements"] == reqs, f"source requirement mismatch: {plan}")
        require(all(d in expected and d < plan for d in deps), f"missing or cyclic dependency: {plan}")
        if plan != "MVP-028":
            covered.update(reqs)
    require(covered == {f"REQ-{i:02}" for i in range(1, 25)}, "incomplete substantive coverage")
    if remote is not None:
        by_number = {i["number"]: i for i in remote if "pull_request" not in i}
        for issue in issues:
            actual = by_number[issue["number"]]
            require(actual["html_url"] == issue["url"] and actual["title"] == issue["title"],
                    f"remote issue mismatch: {issue['plan_id']}")
            require(actual["milestone"]["number"] == data["milestone"], "wrong milestone")
            require("## Blocked by" in actual["body"], "missing dependency section")
            require(all(by_id[d]["url"] in actual["body"] for d in issue["dependencies"]),
                    "missing remote dependency link")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--github-json", type=Path, help="gh api issues response to verify")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    validate_snapshot(root)
    data = json.loads((root / "docs/plan/issues-index.json").read_text())
    remote = json.loads(args.github_json.read_text()) if args.github_json else None
    validate_index(root, data, remote)
    print("PASS: 41 source pages, 28 unique issues, complete dependencies, 24 requirements")


if __name__ == "__main__":
    main()
