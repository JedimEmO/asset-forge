#!/usr/bin/env python
"""Is a gated Hugging Face repo reachable from here? Say exactly what to do if not.

    python hf_gate.py facebook/dinov3-vitl16-pretrain-lvd1689m [--quiet]

Tries to fetch the repo's ``config.json`` through ``huggingface_hub`` (the
env's own copy, which is why this runs under the backend's interpreter and
not the system python). Exit 0 when it downloads; 1 with the four login
steps when the hub answers 401/403; 2 when ``huggingface_hub`` itself is
absent or the network is down — a different problem, named differently.

Stdlib apart from ``huggingface_hub``, which is imported inside ``main`` so
the usage message still prints when it is missing.
"""

from __future__ import annotations

import os
import sys
from pathlib import Path


def token_path() -> Path:
    if os.environ.get("HF_TOKEN_PATH"):
        return Path(os.environ["HF_TOKEN_PATH"]).expanduser()
    if os.environ.get("HF_HOME"):
        return Path(os.environ["HF_HOME"]).expanduser() / "token"
    return Path.home() / ".cache" / "huggingface" / "token"


def login_steps(repo_id: str) -> str:
    stored = "a token is stored" if token_path().is_file() else f"no token at {token_path()}"
    return "\n".join(
        [
            f"{repo_id} is gated; access was refused. Four steps:",
            f"  1. open https://huggingface.co/{repo_id} and accept the licence on the model page",
            "  2. make a read token at https://huggingface.co/settings/tokens",
            "  3. hf auth login --token <tok>     (never the interactive login: no TTY under an agent's shell)",
            f"  4. re-run this install, or `forge doctor`      ({stored})",
        ]
    )


def main(argv: list[str]) -> int:
    args = [a for a in argv[1:] if not a.startswith("--")]
    quiet = "--quiet" in argv
    if len(args) != 1:
        print(__doc__, file=sys.stderr)
        return 2
    repo_id = args[0]
    try:
        from huggingface_hub import hf_hub_download
        from huggingface_hub.errors import GatedRepoError, HfHubHTTPError, RepositoryNotFoundError
    except ImportError as err:
        print(f"hf_gate: huggingface_hub is not importable here ({err}); install it into the env first", file=sys.stderr)
        return 2
    try:
        path = hf_hub_download(repo_id, "config.json")
    except GatedRepoError:
        print(login_steps(repo_id), file=sys.stderr)
        return 1
    except RepositoryNotFoundError:
        print(f"hf_gate: {repo_id} does not exist, or the token cannot see it\n{login_steps(repo_id)}", file=sys.stderr)
        return 1
    except HfHubHTTPError as err:
        status = getattr(getattr(err, "response", None), "status_code", None)
        if status in (401, 403):
            print(login_steps(repo_id), file=sys.stderr)
            return 1
        print(f"hf_gate: the hub answered {status or '?'} for {repo_id}: {err}", file=sys.stderr)
        return 2
    except OSError as err:
        print(f"hf_gate: could not reach the hub for {repo_id}: {err}", file=sys.stderr)
        return 2
    if not quiet:
        print(f"hf_gate: {repo_id} is reachable ({path})")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
