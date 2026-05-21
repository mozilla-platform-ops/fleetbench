import json

from fleetbench_run.activity import ActivityDecision, check_activity


def _stub(stdout, returncode=0):
    return lambda: (stdout, returncode)


def test_idle_state_allows_proceed():
    payload = json.dumps({
        "state": "IDLE",
        "state_description": "g-w up",
        "checks": [{"name": "puppet", "status": "pass"}],
    })
    d = check_activity(invoker=_stub(payload))
    assert d.should_proceed is True
    assert d.state == "IDLE"
    assert "IDLE" in d.reason


def test_non_idle_state_blocks():
    payload = json.dumps({
        "state": "BUSY",
        "state_description": "task running",
        "checks": [],
    })
    d = check_activity(invoker=_stub(payload))
    assert d.should_proceed is False
    assert d.state == "BUSY"
    assert "BUSY" in d.reason


def test_non_idle_includes_non_passing_checks():
    payload = json.dumps({
        "state": "ERROR",
        "state_description": "",
        "checks": [
            {"name": "puppet", "status": "pass"},
            {"name": "disk_free", "status": "fail", "detail": "/: 1GB"},
            {"name": "clock_skew", "status": "warn", "detail": "skew 5s"},
        ],
    })
    d = check_activity(invoker=_stub(payload))
    assert d.should_proceed is False
    assert "disk_free" in d.reason
    assert "clock_skew" in d.reason


def test_gwhc_missing_proceeds_silently():
    d = check_activity(invoker=lambda: (None, None))
    assert d.should_proceed is True
    assert "not available" in d.reason
    assert d.state is None


def test_non_json_output_proceeds_advisory():
    d = check_activity(invoker=_stub("hello world", returncode=0))
    assert d.should_proceed is True
    assert "non-JSON" in d.reason


def test_non_object_json_proceeds_advisory():
    d = check_activity(invoker=_stub("[1, 2, 3]"))
    assert d.should_proceed is True
    assert "not a JSON object" in d.reason


def test_missing_state_field_treated_as_non_idle():
    # If gwhc returns valid JSON but no "state" field, we'd rather block than
    # proceed — better to skip a run than to record noisy data.
    d = check_activity(invoker=_stub('{"checks": []}'))
    assert d.should_proceed is False
    assert d.state is None


def test_returns_activity_decision_dataclass():
    d = check_activity(invoker=_stub('{"state": "IDLE"}'))
    assert isinstance(d, ActivityDecision)
    assert hasattr(d, "should_proceed") and hasattr(d, "reason") and hasattr(d, "state")
