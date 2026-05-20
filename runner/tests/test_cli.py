import pytest

from fleetbench_run.cli import build_parser


def test_help_renders():
    parser = build_parser()
    # parse_args would call sys.exit on --help; format_help just returns the string
    help_text = parser.format_help()
    assert "fleetbench-run" in help_text
    assert "--results-dir" in help_text
    assert "--min-interval" in help_text


def test_results_dir_required():
    parser = build_parser()
    with pytest.raises(SystemExit):
        parser.parse_args([])


def test_minimal_invocation_parses():
    parser = build_parser()
    args = parser.parse_args(["--results-dir", "/tmp/x"])
    assert args.results_dir == "/tmp/x"
    assert args.mode == "normal"
    assert args.collector_binary == "fleetbench"
    assert args.min_interval == "24h"
    assert args.skip_activity_check is False
    assert args.trigger == "boot"
    assert args.timeout == "10m"


def test_all_flags_parse():
    parser = build_parser()
    args = parser.parse_args([
        "--results-dir", "/var/lib/fb",
        "--mode", "quick",
        "--collector-binary", "/usr/local/bin/fleetbench",
        "--min-interval", "1h",
        "--skip-activity-check",
        "--trigger", "manual",
        "--timeout", "5m",
    ])
    assert args.mode == "quick"
    assert args.collector_binary == "/usr/local/bin/fleetbench"
    assert args.min_interval == "1h"
    assert args.skip_activity_check is True
    assert args.trigger == "manual"
    assert args.timeout == "5m"


def test_mode_rejects_invalid_value():
    parser = build_parser()
    with pytest.raises(SystemExit):
        parser.parse_args(["--results-dir", "/tmp/x", "--mode", "bogus"])


def test_trigger_rejects_invalid_value():
    parser = build_parser()
    with pytest.raises(SystemExit):
        parser.parse_args(["--results-dir", "/tmp/x", "--trigger", "cron"])
