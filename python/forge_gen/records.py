"""Writing generator records — ``forge_record: 2`` — from every generator.

Every run of a generator leaves one of these beside what it produced: which
backend at which commit, what it was handed (hashed), every knob it was given
(``null`` where it was not), what it produced (hashed), and whether it was a
``--fake`` placeholder. The Rust side (``crates/forge_library/src/
generator_record.rs``) is the type that defines what these files mean; this
module is the Python half of that contract: one place that knows the key
order and the atomic write, so eight command modules cannot drift into eight
dialects of the same schema.

Two rules carry over from the Rust types:

* **``None`` means unknown, and a default is never written as a measurement.**
  A writer that cannot say "I do not know" will lie instead.
* **Key order is the Rust field order**, and the free-form ``params`` and
  ``measured`` objects are written with their keys *sorted* — the reader holds
  them in a sorted map and re-emits them that way — so a record re-serialised
  by Rust is the same bytes, and a ``git diff`` shows a change of content
  rather than a change of writer.

Paths inside a record are relative to the project root when one is known and
the file is inside it, else absolute. The project is whatever ``--project``
named (or ``set_project``); Python otherwise never knows the project.

Stdlib only: this is imported by scripts running under five different
interpreters, one of them Blender's.
"""

from __future__ import annotations

import datetime
import hashlib
import json
import os
from pathlib import Path

#: Schema version this module writes. Mirrors ``forge_library::generator_record::RECORD_SCHEMA``.
SCHEMA = 2

#: The oldest schema this module reads. Mirrors ``RECORD_SCHEMA_MIN``.
#:
#: **Both readers accept 1 and 2, and only 2 is ever written.** Nothing
#: under ``assets/`` or ``assets-src/`` was rewritten when the schema went
#: to 2: adding four nulls to a shipped sidecar is churn with no new fact
#: in it, and a v1 record still says everything it said before.
SCHEMA_MIN = 1

#: The record kinds the Rust reader knows, as ``RecordKind`` spells them.
#: ``prepare`` is the normalised mesh plus a bare skeleton that ``forge gen
#: skin`` then hashes as its ``mesh`` input, so the chain from a lift to a
#: body is ``lift -> prepare -> rig`` by hash and nothing in it is a claim
#: about a file nobody can name. ``ref`` is the first link: the drawn PNG a
#: lift starts from, brought through ``forge gen ref-import`` and never made
#: here.
KINDS = ("ref", "lift", "prop", "prepare", "rig", "export", "take", "sfx", "music", "speech", "voice")

#: Top-level keys, in the Rust field order. ``write`` refuses a record that
#: has any other key or lacks any of these.
KEYS = (
    "forge_record",
    "kind",
    "tool",
    "created",
    "created_by",
    "backend",
    "inputs",
    "params",
    "outputs",
    "measured",
    "fake",
    "note",
)

#: The ``backend`` block's keys, in order. The four ``forge_record: 2``
#: added are appended after the original six, so the key order stays a
#: prefix of what it was and a v1 record read by a v2 reader needs no
#: reordering to become one.
BACKEND_KEYS = (
    "name",
    "commit",
    "python",
    "torch",
    "model",
    "model_revision",
    "executor",
    "comfyui_commit",
    "workflow_sha256",
    "packs",
)

#: How much of a file to hash at a time — big enough that the syscalls vanish,
#: small enough that a 90-second track is not copied into memory to be hashed.
_CHUNK = 64 * 1024

#: The project root paths are made relative to, when one was named.
_PROJECT: Path | None = None


def set_project(root: str | os.PathLike | None) -> Path | None:
    """Remember the project root ``--project`` named (``None`` forgets it)."""
    global _PROJECT  # noqa: PLW0603 - one process, one project
    _PROJECT = Path(root).resolve() if root else None
    return _PROJECT


def project() -> Path | None:
    """The project root in force, if any."""
    return _PROJECT


def sha256_file(path: str | os.PathLike) -> str:
    """``sha256:<hex>`` of a file, in the form the records carry.

    The prefix is not decoration: it makes a future change of algorithm a
    visible difference in the record instead of a silent one.
    """
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(_CHUNK), b""):
            digest.update(block)
    return "sha256:" + digest.hexdigest()


def today() -> str:
    """Today as ``YYYY-MM-DD``, the form every record uses."""
    return datetime.date.today().isoformat()


def actor(text: str | None) -> str:
    """Normalise a ``--created-by`` value to ``human``/``agent:<n>``/``unknown``.

    Anything unrecognised is treated as an agent name rather than as unknown:
    an unfamiliar name is still a fact, and discarding it would be exactly the
    laundering this schema exists to stop.
    """
    text = (text or "").strip()
    if not text or text == "unknown":
        return "unknown"
    if text == "human":
        return "human"
    if text.startswith("agent:"):
        return text
    return "agent:" + text


def record_path(path: str | os.PathLike, project_root: str | os.PathLike | None = None) -> str:
    """The form a path takes inside a record.

    Relative to the project root (POSIX separators) when the file sits inside
    it; absolute otherwise. A record that said ``../../../tmp/x.glb`` would be
    relative to wherever the reader happened to stand.
    """
    root = Path(project_root).resolve() if project_root else _PROJECT
    absolute = Path(path).resolve()
    if root is not None:
        try:
            return absolute.relative_to(root).as_posix()
        except ValueError:
            pass
    return str(absolute)


def backend_block(
    name: str | None = None,
    commit: str | None = None,
    python: str | None = None,
    torch: str | None = None,
    model: str | None = None,
    model_revision: str | None = None,
    executor: str | None = "env",
    comfyui_commit: str | None = None,
    workflow_sha256: str | None = None,
    packs: dict | None = None,
) -> dict:
    """The ``backend`` block with every key present, in the Rust order.

    Nulls are written rather than omitted: a record that lists what it does
    not know is one a human can read and see the gaps in.

    The last four are ``forge_record: 2``'s. ``executor`` is written for
    **every** record, ``env`` ones included — a record that says nothing
    about its executor is one nobody can group later — and it defaults to
    ``"env"`` because that is what a generator calling this function from
    inside its own interpreter is; the comfy path states its own. For an
    ``env`` run the other three stay ``None``, because an env run genuinely
    has no workflow and no host. ``workflow_sha256`` hashes the **tracked template
    file**, never the patched graph: a reader can go and find a tracked
    file, and nobody can check a hash of bytes that were never written down.
    ``packs`` is ``{repo: commit}``, an empty object for native nodes.
    """
    return {
        "name": name,
        "commit": commit,
        "python": python,
        "torch": torch,
        "model": model,
        "model_revision": model_revision,
        "executor": executor,
        "comfyui_commit": comfyui_commit,
        "workflow_sha256": workflow_sha256,
        "packs": packs,
    }


def new_record(kind: str, tool: str, *, created_by: str | None = None, created: str | None = None) -> dict:
    """A fresh record of one ``kind`` with every key present, in the Rust order.

    ``backend`` starts as all-null, ``params``/``measured`` empty, ``fake``
    false; the command fills what it knows and leaves the rest ``None``.
    """
    if kind not in KINDS:
        raise ValueError(f"record kind {kind!r} is not one of {', '.join(KINDS)}")
    return {
        "forge_record": SCHEMA,
        "kind": kind,
        "tool": tool,
        "created": created or today(),
        "created_by": actor(created_by),
        "backend": backend_block(),
        "inputs": [],
        "params": {},
        "outputs": [],
        "measured": {},
        "fake": False,
        "note": None,
    }


def add_input(
    rec: dict,
    role: str,
    path: str | os.PathLike | None = None,
    *,
    source: str | None = None,
    prompt: str | None = None,
    project_root: str | os.PathLike | None = None,
) -> dict:
    """Append one input: a file (hashed now, so the record says what was read) or a prompt.

    ``role`` is what the input was for — ``image``, ``mesh``, ``blend``,
    ``prompt``, ``reference``, ``voice_record`` — and is how the Rust
    projections find it.
    """
    entry = {
        "role": role,
        "path": record_path(path, project_root) if path is not None else None,
        "sha256": sha256_file(path) if path is not None else None,
        "source": source,
        "prompt": prompt,
    }
    rec["inputs"].append(entry)
    return entry


def add_output(rec: dict, path: str | os.PathLike, *, project_root: str | os.PathLike | None = None) -> dict:
    """Append one output, hashed and sized as it is on disk right now.

    Call it after the file is final: a hash of a half-written file is a claim
    the next reader will find false.
    """
    entry = {
        "path": record_path(path, project_root),
        "sha256": sha256_file(path),
        "bytes": os.path.getsize(path),
    }
    rec["outputs"].append(entry)
    return entry


def _sorted(value):
    """A copy with every object's keys sorted, as serde_json re-emits them."""
    if isinstance(value, dict):
        return {key: _sorted(value[key]) for key in sorted(value)}
    if isinstance(value, list):
        return [_sorted(item) for item in value]
    return value


def normalize(rec: dict) -> dict:
    """The record as it is written: Rust key order, nested blocks complete, free-form keys sorted.

    Refuses a record with a key the reader does not know — an unknown key is
    a field the Rust would silently drop, and the byte-equality test would
    then fail for a reason three files away.
    """
    unknown = set(rec) - set(KEYS)
    if unknown:
        raise ValueError(f"record has keys the reader does not know: {', '.join(sorted(unknown))}")
    missing = [key for key in KEYS if key not in rec]
    if missing:
        raise ValueError(f"record lacks {', '.join(missing)}")
    if rec["forge_record"] != SCHEMA:
        raise ValueError(f"record declares forge_record {rec['forge_record']!r}, this writer is {SCHEMA}")
    if rec["kind"] not in KINDS:
        raise ValueError(f"record kind {rec['kind']!r} is not one of {', '.join(KINDS)}")
    backend = dict(rec["backend"] or {})
    extra = set(backend) - set(BACKEND_KEYS)
    if extra:
        raise ValueError(f"backend block has keys the reader does not know: {', '.join(sorted(extra))}")
    out = {}
    for key in KEYS:
        if key == "backend":
            block = {name: backend.get(name) for name in BACKEND_KEYS}
            # `packs` is free-form like `params`: the Rust holds it in a
            # sorted map and re-emits it that way.
            if block["packs"] is not None:
                block["packs"] = _sorted(block["packs"])
            out[key] = block
        elif key == "inputs":
            out[key] = [
                {
                    "role": item["role"],
                    "path": item.get("path"),
                    "sha256": item.get("sha256"),
                    "source": item.get("source"),
                    "prompt": item.get("prompt"),
                }
                for item in rec["inputs"]
            ]
        elif key == "outputs":
            out[key] = [
                {"path": item["path"], "sha256": item.get("sha256"), "bytes": item.get("bytes")}
                for item in rec["outputs"]
            ]
        elif key in ("params", "measured"):
            out[key] = _sorted(rec[key] if rec[key] is not None else {})
        elif key == "fake":
            out[key] = bool(rec[key])
        else:
            out[key] = rec[key]
    return out


def dumps(rec: dict) -> str:
    """The record's bytes as text: two-space indent, trailing newline, UTF-8 as is."""
    # ensure_ascii=False so a prompt with an accent in it is written the way
    # serde_json writes it. The two writers produce the same bytes for the
    # same record, which is what keeps a re-save out of the diff.
    return json.dumps(normalize(rec), indent=2, ensure_ascii=False) + "\n"


def write(rec: dict, path: str | os.PathLike) -> Path:
    """Write a record atomically, matching what the Rust reader re-emits.

    A temporary sibling plus ``os.replace``: the studio polls these files and
    the Rust side reads them from another process, so a half-written record
    must not be observable. Two spaces of indent and a trailing newline
    because these are git-tracked and reviewed as diffs.
    """
    text = dumps(rec)
    target = Path(path)
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp = target.parent / f".{target.name}.{os.getpid()}.tmp"
    try:
        with open(tmp, "w", encoding="utf-8") as handle:
            handle.write(text)
        os.replace(tmp, target)
    except BaseException:
        if tmp.exists():
            tmp.unlink()
        raise
    return target


def load(path: str | os.PathLike) -> dict:
    """Read a record back, refusing one this build cannot read.

    :data:`SCHEMA_MIN` through :data:`SCHEMA` are accepted — every record
    shipped under ``assets/`` is a 1 and none of them was rewritten — and
    only :data:`SCHEMA` is ever written. A 1 read here is a 1: the four keys
    ``forge_record: 2`` added are absent, which is what ``null`` already
    means everywhere else in this file.
    """
    with open(path, encoding="utf-8") as handle:
        rec = json.load(handle)
    if not isinstance(rec, dict) or "forge_record" not in rec:
        raise ValueError(f"{path} is not a generator record: no forge_record field")
    schema = rec["forge_record"]
    if not isinstance(schema, int) or isinstance(schema, bool) or not SCHEMA_MIN <= schema <= SCHEMA:
        raise ValueError(f"{path} is forge_record {schema!r}; this build reads {SCHEMA_MIN}–{SCHEMA}")
    return rec
