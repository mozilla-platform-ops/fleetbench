import json
from datetime import datetime, timezone

import pytest

from fleetbench_run.collector import CollectorResult
from fleetbench_run.disk import write_envelope, write_text_atomically
from fleetbench_run.envelope import build_envelope


def _envelope():
    cr = CollectorResult(
        stdout="{}", stderr="", exit_code=0, killed_by_timeout=False,
        started_utc="2026-05-20T19:15:00Z",
        finished_utc="2026-05-20T19:15:02Z",
    )
    return build_envelope(
        run_id="r1", trigger="boot",
        collector_result=cr, parsed_output={"status": "ok"},
    )


def test_creates_missing_results_dir(tmp_path):
    target = tmp_path / "fresh" / "subdir"
    final = write_envelope(_envelope(), target, "out.json")
    assert final == target / "out.json"
    assert final.exists()


def test_file_contents_match_envelope_json(tmp_path):
    env = _envelope()
    final = write_envelope(env, tmp_path, "out.json")
    d = json.loads(final.read_text())
    assert d["run_id"] == "r1"
    assert d["trigger"] == "boot"
    assert d["collector_output"]["status"] == "ok"


def test_partial_file_does_not_remain_after_success(tmp_path):
    write_envelope(_envelope(), tmp_path, "out.json")
    leftovers = [p.name for p in tmp_path.iterdir() if p.name.endswith(".partial")]
    assert leftovers == []


def test_replaces_existing_file_atomically(tmp_path):
    final = tmp_path / "out.json"
    final.write_text("old contents")
    write_envelope(_envelope(), tmp_path, "out.json")
    assert "old contents" not in final.read_text()
    assert json.loads(final.read_text())["run_id"] == "r1"


def test_pre_existing_partial_is_overwritten(tmp_path):
    final = tmp_path / "out.json"
    stale_partial = tmp_path / "out.json.partial"
    stale_partial.write_text("crashed run leftover")
    write_envelope(_envelope(), tmp_path, "out.json")
    assert not stale_partial.exists()
    assert final.exists()


def test_write_text_atomically_writes_via_partial(tmp_path, monkeypatch):
    final = tmp_path / "thing.json"
    seen_partials = []
    real_replace = __import__("os").replace

    def tracking_replace(src, dst):
        # When replace is invoked, the partial must exist with the new content
        # and the final must NOT yet exist (first-write case).
        seen_partials.append(str(src))
        return real_replace(src, dst)

    monkeypatch.setattr("fleetbench_run.disk.os.replace", tracking_replace)
    write_text_atomically(final, "hello")
    assert seen_partials == [str(final) + ".partial"]
    assert final.read_text() == "hello"


def test_partial_cleaned_up_if_write_raises(tmp_path, monkeypatch):
    final = tmp_path / "boom.json"

    class _ExplodingStr(str):
        def __new__(cls):
            return str.__new__(cls, "x")

    # Force the inner write to raise by patching write() on the underlying file.
    real_fdopen = __import__("os").fdopen

    def exploding_fdopen(fd, *args, **kwargs):
        f = real_fdopen(fd, *args, **kwargs)
        f.write = lambda *a, **k: (_ for _ in ()).throw(IOError("disk full"))
        return f

    monkeypatch.setattr("fleetbench_run.disk.os.fdopen", exploding_fdopen)
    with pytest.raises(IOError):
        write_text_atomically(final, "anything")

    # No final file produced; no stale .partial left behind.
    assert not final.exists()
    assert not (tmp_path / "boom.json.partial").exists()


def test_filename_does_not_have_partial_suffix_in_final(tmp_path):
    final = write_envelope(_envelope(), tmp_path, "out.json")
    assert not final.name.endswith(".partial")
