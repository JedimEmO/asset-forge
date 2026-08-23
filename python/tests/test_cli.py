"""The tree stands with modules absent, and a refusal comes out as its code and its JSON line."""

from __future__ import annotations

import json
import subprocess
import sys

from forge_gen import cli, exit_codes
from tests.conftest import PYTHON_DIR


def _run(*argv: str, env=None):
    return subprocess.run([sys.executable, str(PYTHON_DIR / "forge_gen"), *argv], capture_output=True, text=True, env=env, check=False)


def test_help_lists_every_command():
    done = _run("--help")
    assert done.returncode == 0
    for name in ("doctor", "mesh", "prop", "rig", "export", "rig-build", "motion", "sfx", "music", "speech"):
        assert f"\n    {name} " in done.stdout or f"    {name}\n" in done.stdout, name
    done = _run("motion", "--help")
    assert done.returncode == 0
    for name in ("sweep", "keys", "review"):
        assert name in done.stdout


def test_no_command_is_usage():
    assert _run().returncode == exit_codes.USAGE
    assert _run("motion").returncode == exit_codes.USAGE


def test_an_absent_module_refuses_with_usage_and_json(monkeypatch):
    """Register a command whose module does not exist and see it refuse politely."""
    monkeypatch.setattr(cli, "COMMANDS", (("ghost", "forge_gen.ghost", "A command nobody wrote"),))
    monkeypatch.setattr(cli, "MOTION_COMMANDS", ())
    parser = cli.build_parser()
    args = parser.parse_args(["ghost", "--json"])
    assert args.json is True
    code = cli.main(["ghost", "--json", "--whatever"])
    assert code == exit_codes.USAGE


def test_a_module_that_fails_to_import_does_not_take_the_tree_down(monkeypatch, tmp_path, capsys):
    bad = tmp_path / "forge_gen_bad_mod.py"
    bad.write_text("raise RuntimeError('bug at import')\n")
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.setattr(cli, "COMMANDS", (("bad", "forge_gen_bad_mod", "A broken one"),))
    monkeypatch.setattr(cli, "MOTION_COMMANDS", ())
    parser = cli.build_parser()
    captured = capsys.readouterr()
    assert "bug at import" in captured.err
    assert "bad" in parser.format_help()


def test_json_error_payload_is_the_last_line_and_fake_env_is_read(monkeypatch, tmp_path):
    """A stub command module exercising run vs run_fake, _exit, _text, and the error path."""
    stub = tmp_path / "forge_gen_stub_cmd.py"
    stub.write_text(
        "from forge_gen.exit_codes import InputRejected\n"
        "def add_parser(sub):\n"
        "    p = sub.add_parser('stub', help='x')\n"
        "    p.add_argument('--reject', action='store_true')\n"
        "def run(args):\n"
        "    if args.reject:\n"
        "        raise InputRejected('bad input', image='x.png')\n"
        "    print('progress line')\n"
        "    return {'record': '/tmp/x.json', 'outputs': ['/tmp/x.glb'], '_text': 'readable summary'}\n"
        "def run_fake(args):\n"
        "    return {'record': 'fake', 'outputs': [], '_exit': 0}\n"
    )
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.setattr(cli, "COMMANDS", (("stub", "forge_gen_stub_cmd", "stub"),))
    monkeypatch.setattr(cli, "MOTION_COMMANDS", ())
    monkeypatch.delenv("FORGE_FAKE", raising=False)
    import io
    import contextlib

    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = cli.main(["stub", "--json"])
    assert code == 0
    lines = out.getvalue().strip().splitlines()
    assert lines[0] == "progress line"
    last = json.loads(lines[-1])
    assert last["ok"] is True and last["record"] == "/tmp/x.json" and "_text" not in last and "elapsed_s" in last

    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = cli.main(["stub"])
    assert code == 0 and out.getvalue().strip().splitlines()[-1] == "readable summary"

    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = cli.main(["stub", "--reject", "--json"])
    assert code == exit_codes.INPUT_REJECTED
    last = json.loads(out.getvalue().strip().splitlines()[-1])
    assert last == {"ok": False, "error": "input_rejected", "reason": "bad input", "image": "x.png", "message": "bad input", "elapsed_s": last["elapsed_s"]}

    monkeypatch.setenv("FORGE_FAKE", "1")
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = cli.main(["stub", "--json"])
    assert json.loads(out.getvalue().strip().splitlines()[-1])["record"] == "fake"

    monkeypatch.delenv("FORGE_FAKE")
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = cli.main(["--fake", "stub", "--json"])
    assert json.loads(out.getvalue().strip().splitlines()[-1])["record"] == "fake", "--fake before the command works too"


def test_project_flag_must_be_a_directory(tmp_path):
    done = _run("doctor", "--project", str(tmp_path / "nope"), "--json")
    assert done.returncode == exit_codes.USAGE
    assert json.loads(done.stdout.strip().splitlines()[-1])["error"] == "usage"


def test_from_payload_rebuilds_every_code():
    err = exit_codes.from_payload(4, {"error": "input_rejected", "reason": "r"}, fallback="f")
    assert isinstance(err, exit_codes.InputRejected) and err.reason == "r"
    err = exit_codes.from_payload(3, {"backend": "ardy", "hint": "h"}, fallback="f")
    assert isinstance(err, exit_codes.MissingBackend) and err.hint == "h"
    err = exit_codes.from_payload(6, {"tool": "ffmpeg"}, fallback="f")
    assert isinstance(err, exit_codes.MissingTool) and err.tool == "ffmpeg"
    err = exit_codes.from_payload(5, None, fallback="boom")
    assert isinstance(err, exit_codes.BackendFailed) and err.message == "boom"
    err = exit_codes.from_payload(9, {"log_tail": ["a"]}, fallback="odd")
    assert isinstance(err, exit_codes.BackendFailed) and err.log_tail == ["a"]
    assert exit_codes.from_payload(2, {}, fallback="u").code == 2
