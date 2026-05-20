"""Envelope shape and construction.

The envelope wraps the collector's JSON output bit-for-bit so the original
collector emission is recoverable from any stored file. On hard collector
failures (non-JSON stdout, panic, signal kill, timeout), the envelope still
gets written with collector_output=null and the raw stdout/stderr captured
for diagnostics.

Design contract per docs/fleetbench_design_v2.md:

  envelope_version: schema version of the envelope itself (1)
  runner_version:   from fleetbench_run.__version__
  run_id:           uuid4 hex, unique per run
  trigger:          "boot" | "manual"
  started_utc:      ISO-8601 UTC, when the runner started this run
  finished_utc:     ISO-8601 UTC, when the runner finished this run
  collector_exit_code:         raw integer exit code, or null on timeout/signal kill
  collector_killed_by_runner:  true iff the runner timed out and killed the child
  collector_output:            parsed collector JSON (the entire object) or null

On hard failure (collector_output is null) the following fields are also set:

  collector_stdout_raw:           captured stdout, truncated to 16 KB
  collector_stderr:               captured stderr, truncated to 16 KB
  collector_output_parse_error:   reason stdout could not be parsed as JSON

On success, the *_raw / stderr / parse_error fields are omitted from the
serialized JSON to keep success envelopes clean.
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field
from typing import Optional

from fleetbench_run import __version__
from fleetbench_run.collector import CollectorResult

ENVELOPE_VERSION = 1
STDOUT_BYTE_CAP = 16 * 1024
STDERR_BYTE_CAP = 16 * 1024


@dataclass(frozen=True)
class Envelope:
    envelope_version: int
    runner_version: str
    run_id: str
    trigger: str
    started_utc: str
    finished_utc: str
    collector_exit_code: Optional[int]
    collector_killed_by_runner: bool
    collector_output: Optional[dict]
    collector_stdout_raw: Optional[str] = None
    collector_stderr: Optional[str] = None
    collector_output_parse_error: Optional[str] = None


def build_envelope(
    *,
    run_id: str,
    trigger: str,
    collector_result: CollectorResult,
    parsed_output: Optional[dict] = None,
    parse_error: Optional[str] = None,
) -> Envelope:
    """Assemble an Envelope from runner metadata and a CollectorResult.

    If ``parsed_output`` is provided, the envelope describes a clean run:
    collector_output is populated and the diagnostic fields stay None so
    they'll be omitted from the JSON.

    If ``parsed_output`` is None, the envelope describes a hard failure:
    collector_stdout_raw and collector_stderr are populated from the
    CollectorResult (truncated to the byte caps), and parse_error is
    recorded if supplied.
    """
    if trigger not in ("boot", "manual"):
        raise ValueError(f"unsupported trigger: {trigger!r}")

    if parsed_output is not None:
        return Envelope(
            envelope_version=ENVELOPE_VERSION,
            runner_version=__version__,
            run_id=run_id,
            trigger=trigger,
            started_utc=collector_result.started_utc,
            finished_utc=collector_result.finished_utc,
            collector_exit_code=collector_result.exit_code,
            collector_killed_by_runner=collector_result.killed_by_timeout,
            collector_output=parsed_output,
        )

    return Envelope(
        envelope_version=ENVELOPE_VERSION,
        runner_version=__version__,
        run_id=run_id,
        trigger=trigger,
        started_utc=collector_result.started_utc,
        finished_utc=collector_result.finished_utc,
        collector_exit_code=collector_result.exit_code,
        collector_killed_by_runner=collector_result.killed_by_timeout,
        collector_output=None,
        collector_stdout_raw=_truncate(collector_result.stdout, STDOUT_BYTE_CAP),
        collector_stderr=_truncate(collector_result.stderr, STDERR_BYTE_CAP),
        collector_output_parse_error=parse_error,
    )


def envelope_to_json(env: Envelope, *, indent: Optional[int] = 2) -> str:
    """Serialize an Envelope to JSON, dropping default-None diagnostic fields.

    On success (collector_output set, no failure), stdout_raw/stderr/parse_error
    are omitted from output. On failure they are emitted even if null, so a
    downstream consumer can distinguish "parsed but empty stderr" from "this
    field was never relevant".
    """
    d = asdict(env)
    is_failure = env.collector_output is None
    if not is_failure:
        for field_name in ("collector_stdout_raw", "collector_stderr",
                           "collector_output_parse_error"):
            d.pop(field_name, None)
    return json.dumps(d, indent=indent, separators=(",", ": ") if indent else (",", ":"))


def _truncate(s: str, cap_bytes: int) -> str:
    """Truncate a string to at most cap_bytes when UTF-8 encoded.

    Avoids splitting in the middle of a multi-byte character.
    """
    if s is None:
        return ""
    encoded = s.encode("utf-8", errors="replace")
    if len(encoded) <= cap_bytes:
        return s
    truncated = encoded[:cap_bytes]
    # Walk back to a valid UTF-8 boundary.
    while truncated and (truncated[-1] & 0xC0) == 0x80:
        truncated = truncated[:-1]
    return truncated.decode("utf-8", errors="replace")
