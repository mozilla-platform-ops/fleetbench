"""fleetbench-run CLI.

This module owns argument parsing only. The actual orchestration (throttle,
activity check, subprocess, envelope write) lands in follow-up tasks.
"""

from __future__ import annotations

import argparse
import sys
from typing import Sequence

from fleetbench_run import __version__


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="fleetbench-run",
        description=(
            "Invoke the fleetbench collector and persist results. Designed to be "
            "called from the worker-startup wrapper before the TC worker boots; "
            "self-throttles based on the newest envelope timestamp in the results "
            "directory."
        ),
    )
    p.add_argument("--version", action="version", version=f"%(prog)s {__version__}")
    p.add_argument(
        "--results-dir",
        required=True,
        help="directory to write envelope files into (created if absent)",
    )
    p.add_argument(
        "--mode",
        choices=["quick", "normal", "long"],
        default="normal",
        help="collector cpu mode (default: normal)",
    )
    p.add_argument(
        "--collector-binary",
        default="fleetbench",
        help="path to the collector binary (default: fleetbench on PATH)",
    )
    p.add_argument(
        "--min-interval",
        default="24h",
        help=(
            "minimum elapsed time since the last envelope before running again. "
            "Soft lower bound; actual cadence is gated by invocation frequency. "
            "Examples: 30m, 1h, 24h, 7d (default: 24h)"
        ),
    )
    p.add_argument(
        "--skip-activity-check",
        action="store_true",
        help="skip the gwhc activity pre-flight (Linux only; no-op elsewhere)",
    )
    p.add_argument(
        "--trigger",
        choices=["boot", "manual"],
        default="boot",
        help="value recorded in the envelope's trigger field (default: boot)",
    )
    p.add_argument(
        "--timeout",
        default="10m",
        help="hard timeout for the collector subprocess (default: 10m)",
    )
    return p


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    print(f"fleetbench-run: not yet implemented", file=sys.stderr)
    print(f"  parsed args: {vars(args)}", file=sys.stderr)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
