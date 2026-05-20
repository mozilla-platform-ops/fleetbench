from datetime import datetime, timedelta, timezone

import pytest

from fleetbench_run.filenames import (
    generate_run_id,
    is_final_envelope,
    is_partial,
    make_envelope_filename,
    parse_started_utc,
    partial_path_for,
)


def _utc(year, month, day, h=0, m=0, s=0) -> datetime:
    return datetime(year, month, day, h, m, s, tzinfo=timezone.utc)


def test_filename_shape_matches_design():
    name = make_envelope_filename(
        _utc(2026, 5, 20, 19, 15, 2),
        "linux-perf-123",
        "cpu-v0",
        "0123456789abcdef0123456789abcdef",
    )
    assert name == "2026-05-20T19-15-02Z_linux-perf-123_cpu-v0_0123456789abcdef0123456789abcdef.json"


def test_filename_has_no_colons_for_windows():
    name = make_envelope_filename(
        _utc(2026, 5, 20, 19, 15, 2), "h", "cpu-v0", "id"
    )
    assert ":" not in name


def test_filename_normalizes_non_utc_to_utc():
    # 10:00 in UTC+2 is 08:00 UTC; filename must reflect UTC.
    tz = timezone(timedelta(hours=2))
    dt = datetime(2026, 5, 20, 10, 0, 0, tzinfo=tz)
    name = make_envelope_filename(dt, "h", "cpu-v0", "id")
    assert name.startswith("2026-05-20T08-00-00Z_")


def test_filename_rejects_naive_datetime():
    with pytest.raises(ValueError):
        make_envelope_filename(datetime(2026, 5, 20), "h", "cpu-v0", "id")


def test_sanitizes_hostname_and_suite():
    name = make_envelope_filename(
        _utc(2026, 5, 20, 0, 0, 0),
        "host.example.com",
        "cpu_v0!",
        "id",
    )
    # dots in FQDN and unsafe chars in suite become hyphens
    assert "host-example-com" in name
    assert "cpu-v0" in name
    assert "." not in name.rsplit(".", 1)[0]  # no dots before the .json extension


def test_empty_hostname_becomes_unknown():
    name = make_envelope_filename(_utc(2026, 5, 20), "", "cpu-v0", "id")
    assert "_unknown_" in name


def test_partial_path_appends_suffix():
    final = "2026-05-20T00-00-00Z_h_cpu-v0_id.json"
    assert partial_path_for(final) == final + ".partial"


def test_partial_path_rejects_non_json():
    with pytest.raises(ValueError):
        partial_path_for("/tmp/something.txt")


def test_is_partial_and_is_final_envelope_classify_correctly():
    assert is_partial("foo.json.partial") is True
    assert is_partial("foo.json") is False
    assert is_final_envelope("foo.json") is True
    assert is_final_envelope("foo.json.partial") is False
    assert is_final_envelope("foo.txt") is False


def test_parse_started_utc_roundtrips():
    dt = _utc(2026, 5, 20, 19, 15, 2)
    name = make_envelope_filename(dt, "h", "cpu-v0", "id")
    parsed = parse_started_utc(name)
    assert parsed == dt
    assert parsed.tzinfo == timezone.utc


def test_parse_started_utc_accepts_full_paths():
    dt = _utc(2026, 5, 20)
    name = make_envelope_filename(dt, "h", "cpu-v0", "id")
    assert parse_started_utc(f"/var/lib/fleetbench/{name}") == dt


def test_parse_started_utc_rejects_partial_and_garbage():
    assert parse_started_utc("not-an-envelope.txt") is None
    assert parse_started_utc("2026-05-20T00-00-00Z_h_cpu-v0_id.json.partial") is None
    assert parse_started_utc("nodate_h_cpu-v0_id.json") is None
    assert parse_started_utc("2026-05-20T99-99-99Z_h_cpu-v0_id.json") is None


def test_generate_run_id_is_32_char_hex():
    rid = generate_run_id()
    assert len(rid) == 32
    int(rid, 16)  # raises ValueError if not hex


def test_filenames_sort_chronologically():
    a = make_envelope_filename(_utc(2026, 5, 20, 10, 0, 0), "h", "cpu-v0", "a" * 32)
    b = make_envelope_filename(_utc(2026, 5, 20, 11, 0, 0), "h", "cpu-v0", "b" * 32)
    c = make_envelope_filename(_utc(2026, 5, 21, 0, 0, 0), "h", "cpu-v0", "c" * 32)
    assert sorted([c, a, b]) == [a, b, c]
