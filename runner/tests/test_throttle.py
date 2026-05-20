from datetime import datetime, timedelta, timezone
from pathlib import Path

import pytest

from fleetbench_run.filenames import make_envelope_filename
from fleetbench_run.throttle import (
    ThrottleDecision,
    decide,
    find_latest_envelope_time,
)


def _utc(year, month, day, h=0, m=0, s=0) -> datetime:
    return datetime(year, month, day, h, m, s, tzinfo=timezone.utc)


def _write_envelope(dirpath: Path, dt: datetime, suffix: str = "") -> Path:
    name = make_envelope_filename(dt, "h", "cpu-v0", "0" * 32)
    if suffix:
        name = name + suffix
    p = dirpath / name
    p.write_text("{}")
    return p


def test_no_dir_returns_none(tmp_path):
    assert find_latest_envelope_time(tmp_path / "nope") is None


def test_empty_dir_returns_none(tmp_path):
    assert find_latest_envelope_time(tmp_path) is None


def test_single_envelope_returns_its_timestamp(tmp_path):
    dt = _utc(2026, 5, 20, 12, 0, 0)
    _write_envelope(tmp_path, dt)
    assert find_latest_envelope_time(tmp_path) == dt


def test_multiple_envelopes_returns_latest(tmp_path):
    _write_envelope(tmp_path, _utc(2026, 5, 20, 10))
    _write_envelope(tmp_path, _utc(2026, 5, 21, 9))
    latest = _utc(2026, 5, 21, 11)
    _write_envelope(tmp_path, latest)
    _write_envelope(tmp_path, _utc(2026, 5, 21, 10))
    assert find_latest_envelope_time(tmp_path) == latest


def test_partial_files_ignored(tmp_path):
    final_dt = _utc(2026, 5, 20, 10)
    _write_envelope(tmp_path, final_dt)
    # A partial with a NEWER timestamp must not count — it's mid-write.
    _write_envelope(tmp_path, _utc(2026, 5, 21, 12), suffix=".partial")
    assert find_latest_envelope_time(tmp_path) == final_dt


def test_unrelated_files_ignored(tmp_path):
    _write_envelope(tmp_path, _utc(2026, 5, 20))
    (tmp_path / "README.md").write_text("hi")
    (tmp_path / ".DS_Store").write_text("garbage")
    (tmp_path / "junk.json").write_text("{}")  # right ext, wrong shape
    assert find_latest_envelope_time(tmp_path) == _utc(2026, 5, 20)


def test_decide_runs_when_dir_empty(tmp_path):
    d = decide(_utc(2026, 5, 20, 12), tmp_path, timedelta(hours=24))
    assert d.should_run is True
    assert d.last_run_utc is None
    assert "no prior envelope" in d.reason


def test_decide_runs_when_interval_satisfied(tmp_path):
    _write_envelope(tmp_path, _utc(2026, 5, 20, 0))
    now = _utc(2026, 5, 21, 1)  # 25h later
    d = decide(now, tmp_path, timedelta(hours=24))
    assert d.should_run is True
    assert d.last_run_utc == _utc(2026, 5, 20, 0)
    assert "exceeds min_interval 1d" in d.reason
    assert "1d1h ago" in d.reason


def test_decide_skips_when_interval_not_satisfied(tmp_path):
    _write_envelope(tmp_path, _utc(2026, 5, 20, 0))
    now = _utc(2026, 5, 20, 12)  # 12h later
    d = decide(now, tmp_path, timedelta(hours=24))
    assert d.should_run is False
    assert "less than min_interval 1d" in d.reason
    assert "next run in 12h" in d.reason
    assert "last run 12h ago" in d.reason


def test_decide_zero_interval_always_runs(tmp_path):
    _write_envelope(tmp_path, _utc(2026, 5, 20, 12))
    d = decide(_utc(2026, 5, 20, 12, 0, 1), tmp_path, timedelta(0))
    assert d.should_run is True


def test_decide_at_exact_boundary_runs(tmp_path):
    _write_envelope(tmp_path, _utc(2026, 5, 20, 0))
    now = _utc(2026, 5, 21, 0)  # exactly 24h later
    d = decide(now, tmp_path, timedelta(hours=24))
    assert d.should_run is True


def test_decide_rejects_naive_now(tmp_path):
    with pytest.raises(ValueError):
        decide(datetime(2026, 5, 20), tmp_path, timedelta(hours=1))


def test_decide_returns_dataclass_with_all_fields(tmp_path):
    d = decide(_utc(2026, 5, 20), tmp_path, timedelta(hours=1))
    assert isinstance(d, ThrottleDecision)
    assert hasattr(d, "should_run") and hasattr(d, "reason") and hasattr(d, "last_run_utc")
