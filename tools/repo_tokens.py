#!/usr/bin/env python3
"""Compute token counts for tracked files using tiktoken."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path, PurePosixPath
from typing import Any


def run_git(args: list[str]) -> bytes:
    return subprocess.check_output(["git", *args], stderr=subprocess.DEVNULL)


def tracked_paths(rev: str | None) -> list[str]:
    if rev:
        raw = run_git(["ls-tree", "-r", "--name-only", rev])
    else:
        raw = run_git(["ls-files"])
    return [line for line in raw.decode("utf-8").splitlines() if line]


def read_file_bytes(path: str, rev: str | None) -> bytes | None:
    if rev:
        try:
            return run_git(["show", f"{rev}:{path}"])
        except subprocess.CalledProcessError:
            return None
    try:
        return Path(path).read_bytes()
    except OSError:
        return None


def count_lines(text: str) -> int:
    if not text:
        return 0
    return text.count("\n") + (0 if text.endswith("\n") else 1)


def matches(path: str, includes: list[str], excludes: list[str]) -> bool:
    pure = PurePosixPath(path)
    included = any(pure.match(pattern) for pattern in includes)
    excluded = any(pure.match(pattern) for pattern in excludes)
    return included and not excluded


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Count tokens in tracked files using tiktoken."
    )
    parser.add_argument(
        "--rev",
        help="Git revision to read from (e.g. origin/main). Default: current worktree.",
    )
    parser.add_argument(
        "--encoding",
        default="cl100k_base",
        help="tiktoken encoding name (default: cl100k_base).",
    )
    parser.add_argument(
        "--include",
        action="append",
        default=[],
        help="Include glob pattern. Repeatable. Default: **/*",
    )
    parser.add_argument(
        "--exclude",
        action="append",
        default=[],
        help="Exclude glob pattern. Repeatable.",
    )
    parser.add_argument(
        "--top",
        type=int,
        default=0,
        help="Show top N files by token count.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Print JSON instead of text.",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    includes = args.include or ["*", "**/*"]
    excludes = args.exclude or []

    try:
        import tiktoken  # type: ignore
    except ImportError:
        print(
            "error: missing dependency 'tiktoken'. Install with: python3 -m pip install --user tiktoken",
            file=sys.stderr,
        )
        return 2

    try:
        encoding = tiktoken.get_encoding(args.encoding)
    except Exception as exc:  # pragma: no cover - defensive
        print(f"error: invalid encoding '{args.encoding}': {exc}", file=sys.stderr)
        return 2

    paths = tracked_paths(args.rev)

    matched_files = 0
    utf8_files = 0
    skipped_read = 0
    skipped_non_utf8 = 0
    total_tokens = 0
    total_lines = 0
    total_bytes = 0
    top_rows: list[dict[str, Any]] = []

    for path in paths:
        if not matches(path, includes, excludes):
            continue
        matched_files += 1

        payload = read_file_bytes(path, args.rev)
        if payload is None:
            skipped_read += 1
            continue

        try:
            text = payload.decode("utf-8")
        except UnicodeDecodeError:
            skipped_non_utf8 += 1
            continue

        utf8_files += 1
        token_count = len(encoding.encode(text))
        line_count = count_lines(text)
        byte_count = len(payload)

        total_tokens += token_count
        total_lines += line_count
        total_bytes += byte_count

        if args.top > 0:
            top_rows.append(
                {
                    "path": path,
                    "tokens": token_count,
                    "lines": line_count,
                    "bytes": byte_count,
                }
            )

    top_rows.sort(key=lambda row: row["tokens"], reverse=True)
    if args.top > 0:
        top_rows = top_rows[: args.top]

    report = {
        "revision": args.rev or "WORKTREE",
        "encoding": args.encoding,
        "include": includes,
        "exclude": excludes,
        "tracked_files": len(paths),
        "matched_files": matched_files,
        "utf8_files": utf8_files,
        "skipped_read": skipped_read,
        "skipped_non_utf8": skipped_non_utf8,
        "total_tokens": total_tokens,
        "total_lines": total_lines,
        "total_bytes": total_bytes,
        "top_files": top_rows,
    }

    if args.json:
        print(json.dumps(report, indent=2))
        return 0

    print(f"revision: {report['revision']}")
    print(f"encoding: {report['encoding']}")
    print(f"include: {', '.join(includes)}")
    if excludes:
        print(f"exclude: {', '.join(excludes)}")
    print(f"tracked_files: {report['tracked_files']}")
    print(f"matched_files: {report['matched_files']}")
    print(f"utf8_files: {report['utf8_files']}")
    print(f"skipped_read: {report['skipped_read']}")
    print(f"skipped_non_utf8: {report['skipped_non_utf8']}")
    print(f"total_tokens: {report['total_tokens']}")
    print(f"total_lines: {report['total_lines']}")
    print(f"total_bytes: {report['total_bytes']}")

    if top_rows:
        print("\nTop files by tokens:")
        print("tokens\tlines\tbytes\tpath")
        for row in top_rows:
            print(f"{row['tokens']}\t{row['lines']}\t{row['bytes']}\t{row['path']}")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
