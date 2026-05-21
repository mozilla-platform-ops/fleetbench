"""fleetbench-run CLI.

This module owns argument parsing only. The actual orchestration (throttle,
activity check, subprocess, envelope write) lands in follow-up tasks.
"""

from __future__ import annotations

import argparse
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Sequence

from fleetbench_run import __version__
from fleetbench_run.activity import check_activity
from fleetbench_run.duration import DurationParseError, parse_duration
from fleetbench_run.orchestrate import perform_run
from fleetbench_run.throttle import decide


def _duration_arg(s: str):
    try:
        return parse_duration(s)
    except DurationParseError as e:
        raise argparse.ArgumentTypeError(str(e)) from e


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
        type=_duration_arg,
        default=parse_duration("24h"),
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
        type=_duration_arg,
        default=parse_duration("10m"),
        help="hard timeout for the collector subprocess (default: 10m)",
    )
    return p


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    results_dir = Path(args.results_dir)
    decision = decide(datetime.now(timezone.utc), results_dir, args.min_interval)
    if not decision.should_run:
        print(f"throttled: {decision.reason}")
        return 0
    print(f"running: {decision.reason}")

    if not args.skip_activity_check:
        activity = check_activity()
        if not activity.should_proceed:
            print(f"activity check: {activity.reason}")
            return 0
        print(f"activity check: {activity.reason}")

    envelope, final_path = perform_run(
        results_dir=results_dir,
        mode=args.mode,
        collector_binary=args.collector_binary,
        trigger=args.trigger,
        timeout=args.timeout,
    )

    if envelope.collector_output is None:
        print(
            f"wrote failure envelope: {final_path} "
            f"(exit_code={envelope.collector_exit_code}, "
            f"killed_by_runner={envelope.collector_killed_by_runner})",
            file=sys.stderr,
        )
    else:
        print(f"wrote envelope: {final_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
