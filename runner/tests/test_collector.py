"""Tests for the collector subprocess wrapper.

We don't run the real collector here. Instead we use python -c snippets as
fake collectors that print known output and exit with known codes. The
subprocess wrapper doesn't care what the binary is — it just runs it with
"cpu --mode <mode> --json" appended.

The "fake collector" is invoked as: python -c "<script>" cpu --mode normal --json
Argv parsing inside the snippet ignores the fleetbench-style arguments.
"""

from __future__ import annotations

import os
import sys
import time
from datetime import timedelta

import pytest

from fleetbench_run.collector import run_collector


def _pid_alive(pid: int) -> bool:
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    return True


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


@pytest.mark.skipif(not hasattr(os, "setsid"), reason="POSIX-only")
def test_timeout_kills_grandchildren_too(tmp_path):
    """A collector that spawns a long-running child must have its descendants
    killed when the runner times out, not orphan them.

    The fake collector spawns a sleep, writes the sleep's pid to a file we
    pass on argv, then sleeps itself. After the timeout fires, both the
    direct child and the sleep grandchild must be dead.
    """
    pid_file = tmp_path / "child.pid"
    wrapper = tmp_path / "spawn"
    wrapper.write_text(
        "#!/usr/bin/env python3\n"
        "import subprocess, sys, time\n"
        "child = subprocess.Popen(['sleep', '60'])\n"
        "with open(sys.argv[-1], 'w') as f:\n"
        "    f.write(str(child.pid))\n"
        "time.sleep(60)\n"
    )
    wrapper.chmod(0o755)

    result = run_collector(
        binary=str(wrapper),
        mode="quick",
        timeout=timedelta(seconds=1),
        extra_args=[str(pid_file)],
    )
    assert result.killed_by_timeout is True

    # The wrapper had time to write the pid before the timeout fired.
    grandchild_pid = int(pid_file.read_text().strip())

    # Allow a moment for the SIGKILL to be delivered and the kernel to reap.
    for _ in range(20):
        if not _pid_alive(grandchild_pid):
            break
        time.sleep(0.05)
    assert not _pid_alive(grandchild_pid), (
        f"grandchild pid {grandchild_pid} survived the timeout kill"
    )
