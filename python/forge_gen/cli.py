"""The ``forge-gen`` command tree: one subcommand per generator, one contract for all.

``python3 python/forge_gen <cmd> [args] [--json] [--fake]`` — the Rust
``forge gen <cmd>`` is exactly this with ``--json``. With ``--json`` the
last stdout line is one JSON object (progress lines before it, diagnostics
on stderr); without it a human-readable summary. Exit codes are
``exit_codes.py``'s table, and every refusal — a command's, or argparse's
own exit 2 — is printed as that object on the way out.

Each command lives in its own module with ``add_parser(subparsers)``,
``run(args) -> dict`` and ``run_fake(args) -> dict``; this file only
registers them. Registration is lazy and forgiving on purpose: the modules
arrive from several hands, and a missing one must not take ``--help`` down
with it — it registers as a subcommand that says "not implemented" and
exits 2.

Nothing here imports torch, numpy or bpy, and neither may any module at
import time: a traceback from ``--help`` because torch is not in the system
python is the failure this layering exists to rule out.
"""

from __future__ import annotations

import argparse
import importlib
import json
import os
import sys
import time
import traceback
from types import ModuleType

from forge_gen import __version__, exit_codes, placeholders, records
from forge_gen.exit_codes import ForgeGenError, UsageError

#: ``(subcommand, module, one-line help)`` for every top-level command.
COMMANDS: tuple[tuple[str, str, str], ...] = (
    ("doctor", "forge_gen.doctor", "Is every backend, tool and model where it should be?"),
    ("mesh", "forge_gen.mesh", "Reference PNG -> textured mesh via TRELLIS.2 (a lift)"),
    ("prop", "forge_gen.blender.prop", "Lifted glb -> normalized prop in headless Blender"),
    ("prepare", "forge_gen.blender.prepare", "Lifted glb -> normalised mesh + a skeleton, no weights"),
    ("skin", "forge_gen.skin", "Prepared glb -> SkinTokens weights on a skeleton fitted to this body"),
    ("ref-import", "forge_gen.reference", "A drawn PNG -> a checked, recorded reference under assets-src/refs/"),
    ("export", "forge_gen.blender.export", "Rigged .blend -> self-contained body .glb"),
    ("rig-build", "forge_gen.blender.rig_build", "Rebuild the profile's rig.blend from its fixture clip"),
    ("sfx", "forge_gen.audio.sfx", "One sound effect from a prompt (MOSS-SoundEffect)"),
    ("music", "forge_gen.audio.music", "One track from a prompt (ACE-Step; the server stays resident)"),
    ("speech", "forge_gen.audio.speech", "One spoken line (MOSS-TTS)"),
    ("voice", "forge_gen.audio.voice", "Design a character's voice from a description (MOSS-VoiceGenerator)"),
)

#: The ``motion`` group's subcommands.
MOTION_COMMANDS: tuple[tuple[str, str, str], ...] = (
    ("sweep", "forge_gen.motion.sweep", "Audition animation prompts (ARDY)"),
    ("keys", "forge_gen.motion.keys", "Keyframe-constrained generation (ARDY; --preset recoil)"),
    ("review", "forge_gen.motion.review", "Metrics table and contact sheet for a sweep"),
)


class _Parser(argparse.ArgumentParser):
    """argparse whose refusals keep the ``--json`` contract.

    argparse exits 2 through ``error()`` before the run ever starts, which
    used to be the one refusal with no JSON last line. ``json_mode`` is set
    from the raw argv (the parse that would read ``--json`` properly is the
    one that is failing), and subparsers inherit the class through
    ``add_subparsers``'s default ``parser_class=type(self)``.
    """

    #: Whether ``--json`` was on the raw command line; set by :func:`main`.
    json_mode = False

    def error(self, message: str):
        self.print_usage(sys.stderr)
        sys.stderr.write(f"{self.prog}: error: {message}\n")
        if _Parser.json_mode:
            sys.stdout.write(json.dumps({"ok": False, "error": "usage", "message": message}, ensure_ascii=False) + "\n")
            sys.stdout.flush()
        raise SystemExit(exit_codes.USAGE)


class _Absent:
    """A subcommand whose module is not there (yet): registers, refuses, never crashes."""

    def __init__(self, name: str, module: str, help_text: str, reason: str) -> None:
        self.name = name
        self.module = module
        self.help = help_text
        self.reason = reason

    def add_parser(self, subparsers) -> None:
        subparsers.add_parser(
            self.name,
            help=f"{self.help} [not implemented: {self.reason}]",
            description=f"{self.help}\n\nNot implemented: {self.reason}",
        )

    def run(self, args) -> dict:
        raise UsageError(f"{self.name} is not implemented: {self.reason}", command=self.name, module=self.module)

    run_fake = run


def _load(name: str, module: str, help_text: str) -> ModuleType | _Absent:
    """Import a command module, or stand in for it with a reason.

    Absence (the module or its package is not there) is quiet. Any other
    import failure is somebody's bug and is said on stderr — but still does
    not take the tree down, because ``forge doctor`` runs through here.
    """
    try:
        return importlib.import_module(module)
    except ModuleNotFoundError as err:
        if err.name and (module == err.name or module.startswith(err.name + ".")):
            return _Absent(name, module, help_text, f"{module} is absent")
        sys.stderr.write(f"forge-gen: {module} did not import: {err}\n")
        return _Absent(name, module, help_text, f"{module} fails to import ({err})")
    except Exception as err:  # noqa: BLE001 - a broken module must not break --help
        sys.stderr.write(f"forge-gen: {module} did not import: {err.__class__.__name__}: {err}\n")
        return _Absent(name, module, help_text, f"{module} fails to import ({err.__class__.__name__}: {err})")


class _Subparsers:
    """The ``subparsers`` a command module sees: ``add_parser`` with the common flags folded in.

    Every parser a module creates gets the shared ``--json/--fake/--project/
    --created-by`` flags and a ``_module`` default naming who runs it, so the
    modules never see those flags and cannot forget them.
    """

    def __init__(self, real, common: argparse.ArgumentParser, module) -> None:
        self._real = real
        self._common = common
        self._module = module

    def add_parser(self, name: str, **kwargs) -> argparse.ArgumentParser:
        parents = list(kwargs.pop("parents", []))
        parents.append(self._common)
        kwargs.setdefault("formatter_class", argparse.RawDescriptionHelpFormatter)
        parser = self._real.add_parser(name, parents=parents, **kwargs)
        parser.set_defaults(_module=self._module)
        return parser

    def __getattr__(self, item):
        return getattr(self._real, item)


def _common_flags(parser: argparse.ArgumentParser, *, suppress: bool) -> None:
    default = argparse.SUPPRESS if suppress else None
    parser.add_argument(
        "--json",
        action="store_true",
        default=argparse.SUPPRESS if suppress else False,
        help="last stdout line is one JSON object (what `forge gen` passes)",
    )
    parser.add_argument(
        "--fake",
        action="store_true",
        default=argparse.SUPPRESS if suppress else False,
        help="write placeholder outputs that pass the same validators, no backend needed (FORGE_FAKE=1 implies it)",
    )
    parser.add_argument(
        "--project",
        default=default,
        metavar="DIR",
        help="the project root; paths inside records are written relative to it",
    )
    parser.add_argument(
        "--created-by",
        default=default,
        metavar="WHO",
        help="human | agent:<name> | unknown — who asked for the run (default: unknown)",
    )


def _register(subparsers, entries, common: argparse.ArgumentParser) -> None:
    for name, module_name, help_text in entries:
        module = _load(name, module_name, help_text)
        proxy = _Subparsers(subparsers, common, module)
        try:
            module.add_parser(proxy)
        except Exception as err:  # noqa: BLE001 - see _load
            sys.stderr.write(f"forge-gen: {module_name}.add_parser failed: {err.__class__.__name__}: {err}\n")
            _Absent(name, module_name, help_text, f"add_parser failed ({err.__class__.__name__}: {err})").add_parser(proxy)


def build_parser() -> argparse.ArgumentParser:
    """The whole tree."""
    parser = _Parser(
        prog="forge-gen",
        description=(
            "asset-forge's generator launcher: every command resolves its backend before any GPU work, "
            "writes its outputs where --out says and a forge_record beside them, and speaks one JSON "
            "line and one exit-code table to the Rust side."
        ),
        epilog=(
            "exit codes: 0 ok, 2 usage, 3 missing backend, 4 input rejected, 5 backend failed, 6 missing tool.\n"
            "A missing backend exits 3 in ~100 ms; `forge-gen doctor` says what to install."
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--version", action="version", version=f"forge-gen {__version__}")
    _common_flags(parser, suppress=False)
    common = argparse.ArgumentParser(add_help=False)
    _common_flags(common, suppress=True)

    subparsers = parser.add_subparsers(dest="command", metavar="<command>", title="commands")
    _register(subparsers, COMMANDS, common)

    motion = subparsers.add_parser(
        "motion",
        help="Animation: sweep | keys | review (ARDY)",
        description="Animation takes: audition prompts, constrain with keyframes, review what came out.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    motion_sub = motion.add_subparsers(dest="motion_command", metavar="<sweep|keys|review>", title="motion commands")
    motion.set_defaults(_group=motion)
    _register(motion_sub, MOTION_COMMANDS, common)
    return parser


def emit(payload: dict, *, as_json: bool) -> None:
    """Print the result: one JSON line, or a readable summary.

    Keys starting with ``_`` are the command's private channel to this
    function and never reach the JSON: ``_text`` is a preformatted human
    summary printed instead of the key dump, ``_exit`` the exit code.
    """
    public = {key: value for key, value in payload.items() if not key.startswith("_")}
    if as_json:
        sys.stdout.write(json.dumps(public, ensure_ascii=False) + "\n")
        sys.stdout.flush()
        return
    text = payload.get("_text")
    if isinstance(text, str):
        sys.stdout.write(text if text.endswith("\n") else text + "\n")
        sys.stdout.flush()
        return
    for key, value in public.items():
        if isinstance(value, list) and value and all(isinstance(item, str) for item in value):
            sys.stdout.write(f"{key}:\n")
            for item in value:
                sys.stdout.write(f"  {item}\n")
        elif isinstance(value, (dict, list)):
            sys.stdout.write(f"{key}: {json.dumps(value, ensure_ascii=False)}\n")
        else:
            sys.stdout.write(f"{key}: {value}\n")
    sys.stdout.flush()


def main(argv: list[str] | None = None) -> int:
    """Parse, dispatch, print, and turn every refusal into its exit code."""
    # Before anything else, in syntax every old python parses: the rest of
    # this layer needs tomllib (3.11+), and without the guard a 3.10 host
    # gets twelve import-noise lines and an exit 2 that names nothing.
    if sys.version_info < (3, 11):
        sys.stderr.write(
            "forge-gen needs python3 >= 3.11; this is %s at %s -- "
            "put a newer python3 first on PATH (tomllib arrived in 3.11)\n"
            % (".".join(str(v) for v in sys.version_info[:3]), sys.executable)
        )
        return exit_codes.MISSING_TOOL
    raw = list(sys.argv[1:] if argv is None else argv)
    _Parser.json_mode = "--json" in raw
    parser = build_parser()
    args, extras = parser.parse_known_args(argv)
    if extras and not isinstance(getattr(args, "_module", None), _Absent):
        parser.error(f"unrecognized arguments: {' '.join(extras)}")
    as_json = bool(getattr(args, "json", False))
    if getattr(args, "command", None) is None:
        parser.print_help()
        return exit_codes.USAGE
    module = getattr(args, "_module", None)
    if module is None:
        group = getattr(args, "_group", None)
        if group is not None:
            group.print_help()
        return exit_codes.USAGE

    project = getattr(args, "project", None)
    if project:
        if not os.path.isdir(project):
            err = UsageError(f"--project {project} is not a directory")
            emit(err.payload(), as_json=as_json)
            sys.stderr.write(f"forge-gen: {err.message}\n")
            return err.code
        records.set_project(project)

    started = time.monotonic()
    try:
        runner = module.run_fake if placeholders.requested(args) else module.run
        result = runner(args)
        if not isinstance(result, dict):
            result = {"result": result}
        code = int(result.pop("_exit", exit_codes.OK))
        result.setdefault("ok", code == exit_codes.OK)
        result.setdefault("elapsed_s", round(time.monotonic() - started, 3))
        emit(result, as_json=as_json)
        return code
    except ForgeGenError as err:
        payload = err.payload()
        payload.setdefault("elapsed_s", round(time.monotonic() - started, 3))
        sys.stderr.write(f"forge-gen: {err.error}: {err.message}\n")
        hint = payload.get("hint")
        if hint:
            sys.stderr.write(f"forge-gen: hint: {hint}\n")
        if as_json:
            emit(payload, as_json=True)
        return err.code
    except KeyboardInterrupt:
        sys.stderr.write("forge-gen: interrupted\n")
        return 130
    except Exception as err:  # noqa: BLE001 - the last line must still be JSON
        traceback.print_exc()
        if as_json:
            emit(
                {"ok": False, "error": "internal", "message": f"{err.__class__.__name__}: {err}"},
                as_json=True,
            )
        return exit_codes.BACKEND_FAILED


if __name__ == "__main__":
    sys.exit(main())
