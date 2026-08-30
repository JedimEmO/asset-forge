"""The ComfyUI graph client: load a tracked template, patch it, run it, fetch what it made.

    graph, sha = comfy.load_template(backend, "sfx.api.json")
    graph = comfy.patch(graph, {"prompt": "a heavy iron door", "seconds": 3.0, "seed": 815273})
    entry = comfy.wait_for(base, comfy.submit(base, graph, client_id), timeout=600)
    written = comfy.fetch(base, entry, out_path)

**Stdlib only.** `test_cli.py`'s contract is that ``--help`` never imports
torch, and the whole reason the graph client is Python rather than Rust is
that ``records.py`` must stay the one writer of a generator record: a Rust
graph client would be a third writer of one schema, and this repository's
ledger already records what three readers of one record format did to each
other. With the graph here, both executors are one mechanism — spawn
``forge gen <verb>``, stream the log, hold the last JSON line — so
``just sfx`` with no daemon up runs the same code the daemon runs, and
``run_fake`` never imports this module at all, which is what keeps
``ci-fake`` a control.

**Exactly one caller per endpoint.** This module owns ``POST /upload/image``,
``POST /prompt``, ``GET /history/{id}``, ``GET /view`` and — for
``doctor.py`` alone, once per doctor run — ``GET /object_info``. It does
**not** read ``/system_stats`` and does **not** call ``/free``: the card is
the lease holder's business, done in Rust, because the card must answer
with no Python alive.

**Patch points live inside the template.** A node whose ``_meta.title``
begins ``PATCH:<key>`` has that input patched with the job's value for
``<key>``; anything after the key is prose for whoever opens the graph in
the UI. There is no sidecar manifest of node ids — that would be one fact
written in two files that nothing holds together, which is
``hosting.md``'s four-places-or-nowhere entry in miniature. A marker inside
the graph cannot drift from what it annotates and survives a re-export from
ComfyUI. A template missing a key the verb requires is a refusal **before
the GPU**, naming the key and the file.

**What is hashed is the tracked file**, never the patched graph: a reader
can go and find a tracked file, and nobody can check a hash of bytes that
were never written down. What was patched into it is knobs, and knobs go in
the record's ``params``.
"""

from __future__ import annotations

import hashlib
import json
import os
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from pathlib import Path

from forge_gen import backends as backends_mod
from forge_gen.backends import Backend, BackendConfigError
from forge_gen.exit_codes import BackendFailed

#: Where ComfyUI answers unless the environment, the project or the host's
#: ``[server]`` table says otherwise.
DEFAULT_URL = "http://127.0.0.1:8188"

#: The environment variable that wins over everything.
URL_ENV = "FORGE_COMFY_URL"

#: The job id ``forge serve`` sets on every child, used to name the files
#: ComfyUI writes into its own output directory.
JOB_ENV = "FORGE_JOB_ID"

#: What marks a node's input as patchable, in its ``_meta.title``.
PATCH_PREFIX = "PATCH:"

#: The keys a ``/history`` output list can hold a produced file under.
OUTPUT_LISTS = ("images", "audio", "gifs", "videos", "files", "text")


class TemplateError(BackendConfigError):
    """A tracked template does not say what the verb needs of it.

    Exit 3 with ``broken_backend``, like a ``backend.toml`` that does not
    parse: the backend as described cannot run, the fix is a line in a
    tracked file, and none of it is the caller's input. Raised **before**
    anything is posted, so a template that cannot run costs no GPU second
    and no place in the queue.

    ``backend`` defaults to the host, because a graph that cannot be built
    is the host's job undone; a caller that knows which generator's template
    it is says so.
    """

    def __init__(self, message: str, *, backend: str = "comfy", hint: str | None = None, **fields: object) -> None:
        super().__init__(message, backend=backend, hint=hint, **fields)


# ------------------------------------------------------------------- where --


def _project_comfy_url(project: str | os.PathLike | None) -> str | None:
    """``[hardware] comfy_url`` from the project's ``forge.toml``, when it says one."""
    if project is None:
        return None
    path = Path(project) / "forge.toml"
    if not path.is_file():
        return None
    try:
        import tomllib  # noqa: PLC0415 - stdlib, and only when a project was named

        with open(path, "rb") as handle:
            data = tomllib.load(handle)
    except (OSError, ValueError):
        # A forge.toml this module cannot read is `forge doctor`'s business
        # to complain about, not this one's: fall through to the host's own
        # [server] table rather than refuse a generate over it.
        return None
    url = (data.get("hardware") or {}).get("comfy_url")
    return str(url) if url else None


def host_backend(backend: Backend, root: str | os.PathLike | None = None) -> Backend:
    """The service a backend runs on: itself when it is the host, else what ``host`` names."""
    if backend.server or not backend.host or backend.host == backend.name:
        return backend
    return backends_mod.load_backend(backend.host, root)


def base_url(
    backend: Backend,
    project: str | os.PathLike | None = None,
    root: str | os.PathLike | None = None,
) -> str:
    """Where to post, with no trailing slash.

    ``$FORGE_COMFY_URL`` first — one export moves every backend at once,
    which is what a second machine or a tunnel needs — then the project's
    ``[hardware] comfy_url``, then the host backend's ``[server]`` table,
    then :data:`DEFAULT_URL`.
    """
    override = os.environ.get(URL_ENV)
    if override:
        return override.rstrip("/")
    stated = _project_comfy_url(project)
    if stated:
        return stated.rstrip("/")
    server = host_backend(backend, root).server or {}
    host = server.get("host")
    port = server.get("port")
    if host and port:
        return f"http://{host}:{port}"
    return DEFAULT_URL


def host_commit(host: Backend) -> str | None:
    """HEAD of the host's checkout — what the running service is — or ``None`` when git will not say.

    The pin in ``backend.toml`` is *what it should be*; a record says what
    ran, so an unreadable checkout is ``null`` and not the pin.
    """
    checkout = host.checkout
    if not checkout.exists():
        return None
    try:
        done = subprocess.run(
            ["git", "-C", str(checkout.resolve()), "rev-parse", "--verify", "HEAD"],
            capture_output=True,
            text=True,
            timeout=20,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if done.returncode != 0:
        return None
    return done.stdout.strip() or None


def packs_block(backend: Backend) -> dict[str, str]:
    """``{repo: commit}`` for the record — an empty object for native nodes.

    Empty and ``None`` say different things: ``{}`` is "this ran on nodes
    that ship with ComfyUI", and ``None`` (what an env run writes) is "the
    question does not apply".
    """
    spec = backend.comfy
    if spec is None:
        return {}
    return {pack.repo: pack.commit for pack in spec.packs}


def output_prefix(stem: str) -> str:
    """``filename_prefix`` for a save node: one directory per job, so two jobs cannot collide.

    ComfyUI writes into its own output directory and we fetch from it; the
    prefix exists so that directory stays readable by a human and so a
    second job for the same name does not land on the first.
    """
    job = os.environ.get(JOB_ENV) or f"local-{os.getpid()}"
    safe = "".join(c if c.isalnum() or c in "-_." else "_" for c in stem) or "out"
    return f"forge/{job}/{safe}"


# -------------------------------------------------------------- the template --


def load_template(backend: Backend, name: str) -> tuple[dict, str]:
    """``(graph, "sha256:…")`` of a tracked API-format template.

    The hash is of the **file on disk**, taken before anything is patched
    into the copy that is returned.
    """
    spec = backend.comfy
    if spec is not None and spec.workflows and name not in spec.workflows:
        raise TemplateError(
            f"{backend.name} has no workflow {name!r} — [comfy] workflows lists "
            f"{', '.join(spec.workflows)}",
            backend=backend.name,
        )
    path = backend.workflow(name)
    try:
        raw = path.read_bytes()
    except OSError as err:
        raise TemplateError(
            f"{backend.name}: the tracked template {path} cannot be read: {err}",
            backend=backend.name,
            hint="[comfy] workflows names it; restore it from the toolkit checkout",
        ) from err
    try:
        graph = json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as err:
        raise TemplateError(f"{path} is not an API-format workflow: {err}", backend=backend.name) from err
    if not isinstance(graph, dict) or not graph:
        raise TemplateError(
            f"{path} is not an API-format workflow: expected an object of node id -> node",
            backend=backend.name,
        )
    return graph, "sha256:" + hashlib.sha256(raw).hexdigest()


def _is_link(value) -> bool:
    """Whether an input value is a wire from another node rather than a knob."""
    return isinstance(value, list) and len(value) == 2 and isinstance(value[0], str)


def _field(node: dict, key: str, node_id: str, where: str | None) -> str:
    """Which input a ``PATCH:<key>`` node has patched.

    The input named exactly ``key`` when there is one; else the node's only
    knob (every other input being a wire). Anything else is a template
    defect named here rather than a wrong value posted to the card.
    """
    inputs = node.get("inputs") or {}
    if key in inputs and not _is_link(inputs[key]):
        return key
    knobs = [name for name, value in inputs.items() if not _is_link(value)]
    if len(knobs) == 1:
        return knobs[0]
    raise TemplateError(
        f"{where or 'the template'}: node {node_id} is marked {PATCH_PREFIX}{key} but has "
        f"{'no input to patch' if not knobs else 'several: ' + ', '.join(sorted(knobs))} — "
        f"name the input {key!r} or mark a node with one knob"
    )


def patch_points(graph: dict, where: str | None = None) -> dict[str, tuple[str, str]]:
    """``{key: (node_id, field)}`` from the ``_meta.title`` markers inside the graph.

    A title carries one marker per knob — ``PATCH:seed``, or
    ``pose conditioning; PATCH:strength PATCH:end_percent`` — and everything
    else in it is prose for whoever opens the graph in the UI. A node with
    several markers must name each input, because two keys landing on one
    input is a template that says one thing twice.
    """
    points: dict[str, tuple[str, str]] = {}
    for node_id, node in graph.items():
        if not isinstance(node, dict):
            continue
        title = str((node.get("_meta") or {}).get("title") or "")
        keys = [token[len(PATCH_PREFIX) :] for token in title.split() if token.startswith(PATCH_PREFIX)]
        if PATCH_PREFIX in title and not [k for k in keys if k]:
            raise TemplateError(
                f"{where or 'the template'}: node {node_id} is titled {title!r} with no key after {PATCH_PREFIX}"
            )
        for key in keys:
            if key in points:
                raise TemplateError(
                    f"{where or 'the template'}: {PATCH_PREFIX}{key} is on two nodes "
                    f"({points[key][0]} and {node_id}) — one knob, one place"
                )
            field = _field(node, key, node_id, where)
            taken = [k for k, (n, f) in points.items() if n == node_id and f == field]
            if taken:
                raise TemplateError(
                    f"{where or 'the template'}: node {node_id} patches {field!r} for both "
                    f"{taken[0]} and {key} — name each input"
                )
            points[key] = (node_id, field)
    return points


def patch(graph: dict, inputs: dict, where: str | None = None) -> dict:
    """A copy of the graph with every patch point filled, or a refusal before the GPU.

    Every ``PATCH:`` marker in the template is required and every key given
    must be one: a knob the template does not carry would be silently
    dropped, and a marker nobody filled would run at whatever the template
    happened to be saved with. Both refusals name the file, because the fix
    is in it.
    """
    points = patch_points(graph, where)
    unknown = sorted(set(inputs) - set(points))
    if unknown:
        raise TemplateError(
            f"{where or 'the template'} has no {PATCH_PREFIX} point for "
            f"{', '.join(unknown)} — it patches {', '.join(sorted(points)) or 'nothing'}"
        )
    missing = sorted(set(points) - set(inputs))
    if missing:
        raise TemplateError(
            f"{where or 'the template'} marks {', '.join(missing)} for patching and this run states "
            f"{', '.join(sorted(inputs)) or 'nothing'} — a marker nobody fills runs at whatever the "
            "template was saved with"
        )
    patched = json.loads(json.dumps(graph))
    for key, (node_id, field) in points.items():
        patched[node_id]["inputs"][field] = inputs[key]
    return patched


# ------------------------------------------------------------------ the wire --


def _get(url: str, timeout: float = 30.0):
    with urllib.request.urlopen(url, timeout=timeout) as response:  # noqa: S310 - loopback, no scheme from user input
        return json.load(response)


def _get_bytes(url: str, timeout: float = 300.0) -> bytes:
    with urllib.request.urlopen(url, timeout=timeout) as response:  # noqa: S310
        return response.read()


def _post_json(url: str, payload: dict, timeout: float = 60.0):
    body = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=timeout) as response:  # noqa: S310
        text = response.read().decode("utf-8")
    return json.loads(text) if text.strip() else {}


def _unreachable(base: str, err: Exception) -> BackendFailed:
    return BackendFailed(
        f"the ComfyUI host at {base} did not answer: {err}",
        hint="systemctl --user status forge-comfy  — and `just doctor` says what the host is missing",
    )


def upload_image(base: str, path: str | os.PathLike, name: str) -> str:
    """``POST /upload/image`` with overwrite; the name ComfyUI filed it under.

    A multipart body by hand, because the whole client is stdlib.
    """
    source = Path(path)
    boundary = f"----forge{uuid.uuid4().hex}"
    parts = []
    for key, value in (("overwrite", "true"), ("type", "input")):
        parts.append(f'--{boundary}\r\nContent-Disposition: form-data; name="{key}"\r\n\r\n{value}\r\n'.encode())
    parts.append(
        f'--{boundary}\r\nContent-Disposition: form-data; name="image"; filename="{name}"\r\n'
        f"Content-Type: application/octet-stream\r\n\r\n".encode()
    )
    parts.append(source.read_bytes())
    parts.append(f"\r\n--{boundary}--\r\n".encode())
    request = urllib.request.Request(
        f"{base}/upload/image",
        data=b"".join(parts),
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
    )
    try:
        with urllib.request.urlopen(request, timeout=120) as response:  # noqa: S310
            answer = json.loads(response.read().decode("utf-8"))
    except (urllib.error.URLError, OSError) as err:
        raise _unreachable(base, err) from err
    filed = answer.get("name", name)
    subfolder = answer.get("subfolder") or ""
    return f"{subfolder}/{filed}" if subfolder else filed


def submit(base: str, graph: dict, client_id: str) -> str:
    """``POST /prompt``; the prompt id. A refused graph is a backend failure with the host's own words."""
    try:
        answer = _post_json(f"{base}/prompt", {"prompt": graph, "client_id": client_id})
    except urllib.error.HTTPError as err:
        detail = err.read().decode("utf-8", "replace")[:2000]
        raise BackendFailed(
            f"ComfyUI refused the graph ({err.code}): {detail}",
            hint="a node class or a model file the template names is not on the host — `just doctor`",
        ) from err
    except (urllib.error.URLError, OSError) as err:
        raise _unreachable(base, err) from err
    prompt_id = answer.get("prompt_id")
    if not prompt_id:
        raise BackendFailed(f"ComfyUI refused the graph: {json.dumps(answer)[:2000]}")
    return str(prompt_id)


def wait_for(
    base: str,
    prompt_id: str,
    *,
    timeout: float,
    poll: float = 1.0,
    on_progress=None,
) -> dict:
    """Poll ``GET /history/{id}`` until the prompt is done; the history entry.

    ``on_progress(seconds, status)`` is called once per poll so the caller
    can print a line the log carries. A prompt that ends in ``error`` and
    one that never ends are both backend failures (exit 5): the model ran
    and something about the run broke.
    """
    started = time.monotonic()
    while True:
        try:
            history = _get(f"{base}/history/{prompt_id}")
        except (urllib.error.URLError, OSError) as err:
            raise _unreachable(base, err) from err
        entry = history.get(prompt_id) if isinstance(history, dict) else None
        elapsed = time.monotonic() - started
        if entry is not None:
            status = entry.get("status") or {}
            if status.get("status_str") == "error":
                raise BackendFailed(
                    f"prompt {prompt_id} failed on the host: {json.dumps(status)[:4000]}",
                    hint="the host's journal has the traceback: journalctl --user -u forge-comfy -n 200",
                )
            if status.get("completed") or status.get("status_str") == "success":
                return entry
        if on_progress is not None:
            on_progress(elapsed, entry)
        if elapsed > timeout:
            raise BackendFailed(
                f"prompt {prompt_id} did not finish in {timeout:.0f}s",
                hint="the host may be loading weights for the first time; journalctl --user -u forge-comfy -f",
            )
        time.sleep(poll)


def outputs(entry: dict) -> list[dict]:
    """Every file the run produced, as ``{"filename", "subfolder", "type"}``, in node order."""
    found: list[dict] = []
    for node_output in (entry.get("outputs") or {}).values():
        if not isinstance(node_output, dict):
            continue
        for key in OUTPUT_LISTS:
            for item in node_output.get(key) or []:
                if isinstance(item, dict) and item.get("filename"):
                    found.append(
                        {
                            "filename": str(item["filename"]),
                            "subfolder": str(item.get("subfolder") or ""),
                            "type": str(item.get("type") or "output"),
                        }
                    )
    return found


def fetch(base: str, entry: dict, dest: str | os.PathLike) -> list[Path]:
    """``GET /view`` every output and write it.

    An existing directory writes each file under its own name; anything
    else is a file path, and the first output goes there while any others
    land beside it as ``<stem>_1<suffix>`` — a graph that saves two things
    must not silently overwrite the one the caller asked for.
    """
    produced = outputs(entry)
    if not produced:
        raise BackendFailed(
            "the graph finished and saved nothing — no output node in the template wrote a file",
            hint="the template needs a save node (SaveAudio, SaveImage) downstream of what it makes",
        )
    target = Path(dest)
    directory = target.is_dir()
    written: list[Path] = []
    for index, item in enumerate(produced):
        query = urllib.parse.urlencode(item)
        try:
            blob = _get_bytes(f"{base}/view?{query}")
        except (urllib.error.URLError, OSError) as err:
            raise _unreachable(base, err) from err
        if directory:
            path = target / Path(item["filename"]).name
        elif index == 0:
            path = target
        else:
            path = target.with_name(f"{target.stem}_{index}{target.suffix}")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(blob)
        written.append(path)
    return written


def was_cached(entry: dict, node_id: str) -> bool:
    """Whether the host served this node from its cache, **observed** in ``/history``.

    Never inferred. The unit runs with ``--cache-none``, so an identical
    graph genuinely re-runs; a graph-hash-to-job index would claim a cache
    hit that never happened, and a record that claims a re-roll it did not
    do is the failure this whole design is arranged around.
    """
    for message in (entry.get("status") or {}).get("messages") or []:
        if not isinstance(message, (list, tuple)) or len(message) != 2:
            continue
        name, data = message
        if name != "execution_cached" or not isinstance(data, dict):
            continue
        if str(node_id) in [str(n) for n in data.get("nodes") or []]:
            return True
    return False


def object_info(base: str, node: str | None = None) -> dict:
    """``GET /object_info`` — **doctor's endpoint only**, fetched once per doctor run.

    The whole surface is a large answer on a cold host, which is why the
    caller fetches it once and shares it across every comfy backend rather
    than asking six times.
    """
    url = f"{base}/object_info/{urllib.parse.quote(node)}" if node else f"{base}/object_info"
    try:
        answer = _get(url, timeout=180.0)
    except (urllib.error.URLError, OSError) as err:
        raise _unreachable(base, err) from err
    return answer if isinstance(answer, dict) else {}


def client_id() -> str:
    """A fresh id per job, so the host's progress stream is not shared between runs."""
    return uuid.uuid4().hex
