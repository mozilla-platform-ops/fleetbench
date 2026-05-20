import json
import stat
from pathlib import Path

import pytest

from fleetbench_run.orchestrate import parse_collector_output, perform_run


def test_parse_valid_object():
    parsed, err = parse_collector_output('{"status": "ok"}')
    assert parsed == {"status": "ok"}
    assert err is None


def test_parse_empty_stdout_reports_error():
    parsed, err = parse_collector_output("")
    assert parsed is None
    assert "no stdout" in err


def test_parse_whitespace_only_reports_error():
    parsed, err = parse_collector_output("   \n\t  ")
    assert parsed is None
    assert "no stdout" in err


def test_parse_invalid_json_reports_error():
    parsed, err = parse_collector_output("not json {")
    assert parsed is None
    assert "not valid JSON" in err


def test_parse_json_array_rejected():
    parsed, err = parse_collector_output("[1, 2, 3]")
    assert parsed is None
    assert "not a JSON object" in err


def test_parse_json_scalar_rejected():
    parsed, err = parse_collector_output("42")
    assert parsed is None
    assert "not a JSON object" in err


def _make_collector_script(tmp_path: Path, content: str) -> Path:
    p = tmp_path / "fake_collector"
    p.write_text(content)
    p.chmod(p.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    return p


def test_perform_run_writes_success_envelope(tmp_path):
    collector = _make_collector_script(tmp_path, (
        "#!/usr/bin/env python3\n"
        "import json\n"
        'print(json.dumps({"schema_version": 3, "cpu_suite_version": "cpu-v0",'
        ' "status": "ok", "host": {"hostname": "h"}, "cpu": {}}))\n'
    ))
    results_dir = tmp_path / "results"
    from datetime import timedelta

    env, path = perform_run(
        results_dir=results_dir,
        mode="quick",
        collector_binary=str(collector),
        trigger="boot",
        timeout=timedelta(seconds=10),
        hostname="testhost",
    )
    assert env.collector_output is not None
    assert env.collector_output["status"] == "ok"
    assert path.exists()
    on_disk = json.loads(path.read_text())
    assert on_disk["collector_output"]["status"] == "ok"
    # Filename has the parsed suite version baked in.
    assert "_cpu-v0_" in path.name
    assert "_testhost_" in path.name


def test_perform_run_writes_failure_envelope_on_garbage_stdout(tmp_path):
    collector = _make_collector_script(tmp_path, (
        "#!/usr/bin/env python3\n"
        "print('not json')\n"
    ))
    results_dir = tmp_path / "results"
    from datetime import timedelta

    env, path = perform_run(
        results_dir=results_dir, mode="quick",
        collector_binary=str(collector),
        trigger="boot", timeout=timedelta(seconds=10),
        hostname="testhost",
    )
    assert env.collector_output is None
    assert env.collector_stdout_raw == "not json\n"
    assert env.collector_output_parse_error
    assert "not valid JSON" in env.collector_output_parse_error
    # Filename falls back to unknown-suite since we couldn't parse.
    assert "_unknown-suite_" in path.name
    assert path.exists()


def test_perform_run_handles_missing_binary(tmp_path):
    from datetime import timedelta

    env, path = perform_run(
        results_dir=tmp_path / "results",
        mode="quick",
        collector_binary=str(tmp_path / "does-not-exist"),
        trigger="boot",
        timeout=timedelta(seconds=5),
        hostname="testhost",
    )
    assert env.collector_output is None
    assert env.collector_exit_code is None
    assert "not found" in env.collector_stderr
    assert "_unknown-suite_" in path.name
    assert path.exists()


def test_perform_run_propagates_killed_by_timeout(tmp_path):
    collector = _make_collector_script(tmp_path, (
        "#!/usr/bin/env python3\n"
        "import time\n"
        "time.sleep(30)\n"
    ))
    from datetime import timedelta

    env, path = perform_run(
        results_dir=tmp_path / "results",
        mode="quick",
        collector_binary=str(collector),
        trigger="boot",
        timeout=timedelta(seconds=1),
        hostname="testhost",
    )
    assert env.collector_killed_by_runner is True
    assert env.collector_output is None
    assert env.collector_exit_code is None
    assert path.exists()


def test_perform_run_filename_uses_started_timestamp(tmp_path):
    collector = _make_collector_script(tmp_path, (
        "#!/usr/bin/env python3\n"
        "import json\n"
        'print(json.dumps({"cpu_suite_version": "cpu-v0", "status": "ok"}))\n'
    ))
    from datetime import timedelta

    env, path = perform_run(
        results_dir=tmp_path / "results",
        mode="quick",
        collector_binary=str(collector),
        trigger="boot",
        timeout=timedelta(seconds=10),
        hostname="h",
    )
    # The filename's leading timestamp must equal the envelope started_utc
    # (modulo colon-to-hyphen substitution).
    ts_in_name = path.name.split("_")[0]
    expected = env.started_utc.replace(":", "-")
    assert ts_in_name == expected
