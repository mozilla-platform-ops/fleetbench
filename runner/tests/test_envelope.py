import json

import pytest

from fleetbench_run import __version__
from fleetbench_run.collector import CollectorResult
from fleetbench_run.envelope import (
    ENVELOPE_VERSION,
    STDERR_BYTE_CAP,
    STDOUT_BYTE_CAP,
    Envelope,
    build_envelope,
    envelope_to_json,
)


def _ok_collector_result(stdout: str = "{}", stderr: str = "") -> CollectorResult:
    return CollectorResult(
        stdout=stdout,
        stderr=stderr,
        exit_code=0,
        killed_by_timeout=False,
        started_utc="2026-05-20T19:15:00Z",
        finished_utc="2026-05-20T19:15:02Z",
    )


def _failed_collector_result(stdout: str, stderr: str, *, killed: bool = False, exit_code=None) -> CollectorResult:
    return CollectorResult(
        stdout=stdout,
        stderr=stderr,
        exit_code=exit_code,
        killed_by_timeout=killed,
        started_utc="2026-05-20T19:15:00Z",
        finished_utc="2026-05-20T19:25:00Z",
    )


def test_success_envelope_carries_parsed_output():
    parsed = {"schema_version": 3, "status": "ok"}
    env = build_envelope(
        run_id="abc",
        trigger="boot",
        collector_result=_ok_collector_result(),
        parsed_output=parsed,
    )
    assert isinstance(env, Envelope)
    assert env.envelope_version == ENVELOPE_VERSION
    assert env.runner_version == __version__
    assert env.run_id == "abc"
    assert env.trigger == "boot"
    assert env.collector_exit_code == 0
    assert env.collector_killed_by_runner is False
    assert env.collector_output == parsed
    assert env.collector_stdout_raw is None
    assert env.collector_stderr is None
    assert env.collector_output_parse_error is None


def test_failure_envelope_captures_raw_streams():
    r = _failed_collector_result(stdout="garbage", stderr="panicked at line 5", exit_code=-11)
    env = build_envelope(
        run_id="r1",
        trigger="boot",
        collector_result=r,
        parsed_output=None,
        parse_error="expected value at line 1 column 1",
    )
    assert env.collector_output is None
    assert env.collector_exit_code == -11
    assert env.collector_killed_by_runner is False
    assert env.collector_stdout_raw == "garbage"
    assert env.collector_stderr == "panicked at line 5"
    assert env.collector_output_parse_error == "expected value at line 1 column 1"


def test_timeout_kill_propagates():
    r = _failed_collector_result(stdout="", stderr="", killed=True, exit_code=None)
    env = build_envelope(
        run_id="r1",
        trigger="boot",
        collector_result=r,
        parsed_output=None,
    )
    assert env.collector_killed_by_runner is True
    assert env.collector_exit_code is None


def test_stdout_capped_to_byte_limit():
    big = "x" * (STDOUT_BYTE_CAP + 5000)
    r = _failed_collector_result(stdout=big, stderr="")
    env = build_envelope(
        run_id="r1", trigger="boot", collector_result=r, parsed_output=None,
    )
    assert len(env.collector_stdout_raw.encode("utf-8")) <= STDOUT_BYTE_CAP


def test_stderr_capped_to_byte_limit():
    big = "y" * (STDERR_BYTE_CAP + 5000)
    r = _failed_collector_result(stdout="", stderr=big)
    env = build_envelope(
        run_id="r1", trigger="boot", collector_result=r, parsed_output=None,
    )
    assert len(env.collector_stderr.encode("utf-8")) <= STDERR_BYTE_CAP


def test_truncation_does_not_split_utf8():
    # Lots of 3-byte chars. The cap may fall inside a character.
    snowman = "☃"  # 3 bytes in UTF-8
    big = snowman * (STDOUT_BYTE_CAP // 2)
    r = _failed_collector_result(stdout=big, stderr="")
    env = build_envelope(
        run_id="r1", trigger="boot", collector_result=r, parsed_output=None,
    )
    # Must be valid UTF-8 (i.e. round-trips through encode/decode without errors)
    env.collector_stdout_raw.encode("utf-8")


def test_envelope_to_json_omits_failure_fields_on_success():
    env = build_envelope(
        run_id="abc",
        trigger="boot",
        collector_result=_ok_collector_result(),
        parsed_output={"status": "ok"},
    )
    s = envelope_to_json(env)
    d = json.loads(s)
    assert "collector_stdout_raw" not in d
    assert "collector_stderr" not in d
    assert "collector_output_parse_error" not in d
    assert d["collector_output"] == {"status": "ok"}


def test_envelope_to_json_keeps_failure_fields_on_failure():
    r = _failed_collector_result(stdout="", stderr="boom", exit_code=1)
    env = build_envelope(
        run_id="r1", trigger="boot", collector_result=r, parsed_output=None,
        parse_error="empty stdout",
    )
    d = json.loads(envelope_to_json(env))
    assert d["collector_output"] is None
    assert d["collector_stdout_raw"] == ""
    assert d["collector_stderr"] == "boom"
    assert d["collector_output_parse_error"] == "empty stdout"


def test_envelope_to_json_pretty_by_default():
    env = build_envelope(
        run_id="abc",
        trigger="manual",
        collector_result=_ok_collector_result(),
        parsed_output={"status": "ok"},
    )
    assert "\n  " in envelope_to_json(env)
    assert "\n" not in envelope_to_json(env, indent=None)


def test_collector_json_preserved_bit_for_bit():
    parsed = {
        "schema_version": 3,
        "results": {"prime_sieve_1t": {"iterations": [{"seconds": 0.18, "prime_count": 5761455}]}},
        "host": {"hostname": "h"},
    }
    env = build_envelope(
        run_id="abc",
        trigger="boot",
        collector_result=_ok_collector_result(),
        parsed_output=parsed,
    )
    d = json.loads(envelope_to_json(env))
    assert d["collector_output"] == parsed


def test_unsupported_trigger_raises():
    with pytest.raises(ValueError):
        build_envelope(
            run_id="x",
            trigger="cron",
            collector_result=_ok_collector_result(),
            parsed_output={},
        )


def test_collector_timestamps_propagate_into_envelope():
    r = _ok_collector_result()
    env = build_envelope(
        run_id="abc", trigger="boot",
        collector_result=r, parsed_output={"status": "ok"},
    )
    assert env.started_utc == r.started_utc
    assert env.finished_utc == r.finished_utc
