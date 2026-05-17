#!/usr/bin/env python3
"""Compare EGB protocol transcripts for ordering, fields, and timing."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


@dataclass
class Entry:
    timestamp_ms: int | None
    device: str
    direction: str
    payload: str

    @property
    def key_values(self) -> dict[str, str]:
        if "|" in self.payload:
            body = self.payload.split("|", 2)[-1]
        else:
            body = self.payload
        values: dict[str, str] = {}
        for part in body.split():
            if "=" in part:
                key, value = part.split("=", 1)
                values[key] = value
        return values


def parse_line(line: str) -> Entry | None:
    line = line.strip()
    if not line:
        return None

    parts = line.split(maxsplit=4)
    if len(parts) >= 5 and parts[0].isdigit() and parts[2] in {"RX", "TX"}:
        return Entry(int(parts[0]), parts[1], f"{parts[2]} {parts[3]}", parts[4])

    return Entry(None, "", "", line)


def load_entries(path: Path) -> list[Entry]:
    return [entry for entry in (parse_line(line) for line in path.read_text().splitlines()) if entry]


def timing_delta(previous: Entry | None, current: Entry) -> int | None:
    if previous is None or previous.timestamp_ms is None or current.timestamp_ms is None:
        return None
    return current.timestamp_ms - previous.timestamp_ms


def compare(expected: list[Entry], actual: list[Entry], timing_tolerance_ms: int) -> Iterable[str]:
    max_len = max(len(expected), len(actual))
    previous_expected: Entry | None = None
    previous_actual: Entry | None = None
    for index in range(max_len):
        exp = expected[index] if index < len(expected) else None
        act = actual[index] if index < len(actual) else None

        if exp is None:
            yield f"+ actual extra line {index + 1}: {act.payload}"
            previous_actual = act
            continue
        if act is None:
            yield f"- missing actual line {index + 1}: {exp.payload}"
            previous_expected = exp
            continue

        if exp.payload != act.payload:
            yield f"! line {index + 1} payload differs"
            yield f"  expected: {exp.payload}"
            yield f"  actual:   {act.payload}"

        exp_fields = exp.key_values
        act_fields = act.key_values
        missing_fields = sorted(set(exp_fields) - set(act_fields))
        extra_fields = sorted(set(act_fields) - set(exp_fields))
        changed_fields = sorted(
            key for key in set(exp_fields) & set(act_fields) if exp_fields[key] != act_fields[key]
        )
        if missing_fields:
            yield f"  missing fields: {', '.join(missing_fields)}"
        if extra_fields:
            yield f"  extra fields: {', '.join(extra_fields)}"
        for key in changed_fields:
            yield f"  changed field {key}: expected {exp_fields[key]!r}, actual {act_fields[key]!r}"

        exp_gap = timing_delta(previous_expected, exp)
        act_gap = timing_delta(previous_actual, act)
        if exp_gap is not None and act_gap is not None:
            diff = abs(exp_gap - act_gap)
            if diff > timing_tolerance_ms:
                yield (
                    f"  timing delta differs by {diff} ms: "
                    f"expected gap {exp_gap} ms, actual gap {act_gap} ms"
                )

        previous_expected = exp
        previous_actual = act


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--expected", required=True, type=Path)
    parser.add_argument("--actual", required=True, type=Path)
    parser.add_argument("--timing-tolerance-ms", default=50, type=int)
    args = parser.parse_args()

    differences = list(
        compare(load_entries(args.expected), load_entries(args.actual), args.timing_tolerance_ms)
    )
    if differences:
        print("\n".join(differences))
        return 1
    print("transcripts match")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
