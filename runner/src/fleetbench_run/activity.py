"""Activity pre-flight via gwhc (generic worker host check).

Before invoking the collector, ask gwhc whether the host is currently doing
useful work. The operational model already guards against this by invoking
the runner before the TC worker boots, but a manual invocation mid-test
would otherwise produce noisy results. This is defense in depth.

Behavior:
  * On Linux with gwhc available: parse `state` from `gwhc --json`. Anything
    other than ``"IDLE"`` triggers a skip with a human-readable summary.
  * gwhc not installed: silently allow the run to proceed. Non-Linux hosts
    will hit this path.
  * gwhc fails to produce JSON or returns non-zero: warn but allow the run.
    We treat the check as advisory, not authoritative — a broken gwhc must
    not stop benchmarking.
"""

from __future__ import annotations

import json
import shutil
import subprocess
from dataclasses import dataclass
from typing import Callable, Optional, Tuple


GWHC_TIMEOUT_SECONDS = 10


@dataclass(frozen=True)
class ActivityDecision:
    should_proceed: bool
    reason: str
    state: Optional[str] = None  # the raw gwhc "state" field, when known


GwhcInvoker = Callable[[], Tuple[Optional[str], Optional[int]]]


def check_activity(invoker: Optional[GwhcInvoker] = None) -> ActivityDecision:
    """Run the gwhc activity check and decide whether to proceed."""
    invoke = invoker or _default_invoker
    stdout, returncode = invoke()

    if stdout is None:
        return ActivityDecision(
            should_proceed=True,
            reason="gwhc not available on host (skipping activity check)",
        )
    try:
        data = json.loads(stdout)
    except json.JSONDecodeError:
        return ActivityDecision(
            should_proceed=True,
            reason=f"gwhc produced non-JSON output (advisory only, proceeding); exit={returncode}",
        )
    if not isinstance(data, dict):
        return ActivityDecision(
            should_proceed=True,
            reason="gwhc output was not a JSON object (advisory only, proceeding)",
        )

    state = data.get("state")
    desc = data.get("state_description", "")
    if state == "IDLE":
        return ActivityDecision(
            should_proceed=True,
            reason=f"gwhc state=IDLE ({desc})" if desc else "gwhc state=IDLE",
            state=state,
        )
    return ActivityDecision(
        should_proceed=False,
        reason=_format_busy(state, desc, data),
        state=state,
    )


def _format_busy(state, desc, data) -> str:
    pieces = [f"gwhc state={state!r}"]
    if desc:
        pieces.append(f"({desc})")
    non_pass = [c for c in data.get("checks", []) if c.get("status") != "pass"]
    if non_pass:
        names = ", ".join(c.get("name", "?") for c in non_pass)
        pieces.append(f"non-passing checks: {names}")
    return " ".join(pieces)


def _default_invoker() -> Tuple[Optional[str], Optional[int]]:
    """Locate gwhc on PATH and run it with --json."""
    binary = shutil.which("gwhc")
    if not binary:
        return None, None
    try:
        cp = subprocess.run(
            [binary, "--json"],
            capture_output=True,
            text=True,
            timeout=GWHC_TIMEOUT_SECONDS,
        )
        return cp.stdout, cp.returncode
    except (subprocess.TimeoutExpired, OSError):
        return None, None
