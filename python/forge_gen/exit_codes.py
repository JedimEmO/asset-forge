"""The exit codes `forge gen` speaks, and the exceptions that carry them.

The Rust side mirrors this table as an enum, and every refusal a command
makes is one of these — so a caller that reads the code knows whether to
install something (3), fix its input (4), read a log (5) or put a tool on
PATH (6) without parsing a message. Each exception carries the JSON object
the CLI prints as its last stdout line under ``--json``; ``payload()`` is
that object, and ``ok`` is always ``False`` in it.

Stdlib only: this is imported by the outer launcher and by every inner
module, under five different interpreters.
"""

from __future__ import annotations

#: Everything went as asked.
OK = 0
#: The command line was wrong (argparse's own code, kept).
USAGE = 2
#: The backend this command needs is not installed — generation is off.
MISSING_BACKEND = 3
#: The input was read and refused: a PNG with no flat border, a mesh that
#: is not in the T-pose, a prompt that is empty.
INPUT_REJECTED = 4
#: The backend ran and failed; the log tail says why.
BACKEND_FAILED = 5
#: A host tool (Blender, ffmpeg, nvidia-smi) is not where it was looked for.
MISSING_TOOL = 6


class ForgeGenError(Exception):
    """Base of every refusal: an exit code and the JSON object that explains it."""

    #: The exit code; subclasses pin it.
    code: int = BACKEND_FAILED
    #: The ``error`` word in the payload.
    error: str = "backend_failed"

    def __init__(self, message: str, **fields: object) -> None:
        super().__init__(message)
        self.message = message
        self.fields = fields

    def payload(self) -> dict:
        """The object printed as the last stdout line under ``--json``."""
        out: dict = {"ok": False, "error": self.error}
        out.update(self.fields)
        out.setdefault("message", self.message)
        return out


class UsageError(ForgeGenError):
    """The arguments do not make sense together, or a required one is absent."""

    code = USAGE
    error = "usage"


class MissingBackend(ForgeGenError):
    """No interpreter for the backend: not installed, or the link is stale.

    ``hint`` is the command that installs it. This is raised *before* any
    GPU work — resolving absence costs about 100 ms and never imports torch.
    """

    code = MISSING_BACKEND
    error = "missing_backend"

    def __init__(self, message: str, *, backend: str, hint: str | None = None, **fields: object) -> None:
        super().__init__(message, backend=backend, hint=hint, **fields)
        self.backend = backend
        self.hint = hint


class InputRejected(ForgeGenError):
    """The input was examined and refused; ``reason`` names the defect.

    The fix is upstream of the command — the reference image, the prompt, the
    mesh — never a flag that makes the gate look away.
    """

    code = INPUT_REJECTED
    error = "input_rejected"

    def __init__(self, reason: str, **fields: object) -> None:
        super().__init__(reason, reason=reason, **fields)
        self.reason = reason


class BackendFailed(ForgeGenError):
    """The backend ran and did not finish; ``log_tail`` is its last lines."""

    code = BACKEND_FAILED
    error = "backend_failed"

    def __init__(self, message: str, *, log_tail: list[str] | None = None, **fields: object) -> None:
        super().__init__(message, log_tail=list(log_tail or []), **fields)
        self.log_tail = list(log_tail or [])


class MissingTool(ForgeGenError):
    """A host program is absent; ``tool`` names it and ``hint`` says where it is looked for."""

    code = MISSING_TOOL
    error = "missing_tool"

    def __init__(self, message: str, *, tool: str, hint: str | None = None, **fields: object) -> None:
        super().__init__(message, tool=tool, hint=hint, **fields)
        self.tool = tool
        self.hint = hint


#: Exit code → the exception class that carries it, for relaying an inner
#: process's refusal out of the outer one unchanged.
BY_CODE: dict[int, type[ForgeGenError]] = {
    USAGE: UsageError,
    MISSING_BACKEND: MissingBackend,
    INPUT_REJECTED: InputRejected,
    BACKEND_FAILED: BackendFailed,
    MISSING_TOOL: MissingTool,
}


def from_payload(code: int, payload: dict | None, *, fallback: str) -> ForgeGenError:
    """Rebuild the exception an inner process raised from its exit code and JSON line.

    An inner module that refused with exit 4 printed ``{"error":
    "input_rejected", "reason": ...}`` as its last line; the outer relays
    exactly that so the caller sees one refusal, not a refusal wrapped in a
    "backend failed". Anything unrecognised is a backend failure with
    whatever was printed as its log.
    """
    payload = dict(payload or {})
    payload.pop("ok", None)
    payload.pop("error", None)
    message = str(payload.pop("message", fallback))
    if code == INPUT_REJECTED:
        return InputRejected(str(payload.pop("reason", message)), **payload)
    if code == MISSING_BACKEND:
        return MissingBackend(message, backend=str(payload.pop("backend", "?")), hint=payload.pop("hint", None), **payload)
    if code == MISSING_TOOL:
        return MissingTool(message, tool=str(payload.pop("tool", "?")), hint=payload.pop("hint", None), **payload)
    if code == USAGE:
        return UsageError(message, **payload)
    tail = payload.pop("log_tail", None)
    return BackendFailed(message, log_tail=tail if isinstance(tail, list) else None, **payload)
