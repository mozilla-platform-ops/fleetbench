"""Tests for the collector subprocess wrapper.

We don't run the real collector here. Instead we use python -c snippets as
fake collectors that print known output and exit with known codes. The
subprocess wrapper doesn't care what the binary is — it just runs it with
"cpu --mode <mode> --json" appended.

The "fake collector" is invoked as: python -c "<script>" cpu --mode normal --json
Argv parsing inside the snippet ignores the fleetbench-style arguments.
"""

from __future__ import annotations

import sys
from datetime import timedelta

import pytest

from fleetbench_run.collector import run_collector


def test_runs_a_script_and_captures_output(tmp_path):
    """End-to-end via a small executable wrapper script."""
    wrapper = tmp_path / "collector"
    wrapper.write_text(
        "#!/usr/bin/env python3\n"
        "import sys\n"
        "print('stdout-line')\n"
        "print('stderr-line', file=sys.stderr)\n"
        "sys.exit(0)\n"
    )
    wrapper.chmod(0o755)

    result = run_collector(
        binary=str(wrapper),
        mode="normal",
        timeout=timedelta(seconds=10),
    )
    assert result.exit_code == 0
    assert "stdout-line" in result.stdout
    assert "stderr-line" in result.stderr
    assert result.killed_by_timeout is False
    assert result.started_utc.endswith("Z")
    assert result.finished_utc.endswith("Z")


def test_propagates_nonzero_exit(tmp_path):
    wrapper = tmp_path / "fail"
    wrapper.write_text(
        "#!/usr/bin/env python3\n"
        "import sys\n"
        "print('partial', file=sys.stderr)\n"
        "sys.exit(7)\n"
    )
    wrapper.chmod(0o755)

    result = run_collector(
        binary=str(wrapper),
        mode="quick",
        timeout=timedelta(seconds=5),
    )
    assert result.exit_code == 7
    assert "partial" in result.stderr
    assert result.killed_by_timeout is False


def test_timeout_kills_and_flags(tmp_path):
    wrapper = tmp_path / "hang"
    wrapper.write_text(
        "#!/usr/bin/env python3\n"
        "import time\n"
        "time.sleep(30)\n"
    )
    wrapper.chmod(0o755)

    result = run_collector(
        binary=str(wrapper),
        mode="quick",
        timeout=timedelta(seconds=1),
    )
    assert result.killed_by_timeout is True
    assert result.exit_code is None


def test_records_invocation_args(tmp_path):
    """The collector must be called with `cpu --mode <mode> --json`."""
    wrapper = tmp_path / "echo"
    wrapper.write_text(
        "#!/usr/bin/env python3\n"
        "import sys, json\n"
        "print(json.dumps(sys.argv[1:]))\n"
    )
    wrapper.chmod(0o755)

    result = run_collector(
        binary=str(wrapper),
        mode="long",
        timeout=timedelta(seconds=5),
    )
    import json
    parsed = json.loads(result.stdout)
    assert parsed == ["cpu", "--mode", "long", "--json"]


def test_missing_binary_raises(tmp_path):
    with pytest.raises(FileNotFoundError):
        run_collector(
            binary=str(tmp_path / "does-not-exist"),
            mode="quick",
            timeout=timedelta(seconds=5),
        )
