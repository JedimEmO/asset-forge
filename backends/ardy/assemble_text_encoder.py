"""Assemble the LLM2Vec text encoder ARDY expects, under one directory.

    <env>/bin/python backends/ardy/assemble_text_encoder.py --dest DIR [--device cuda|cpu] [--keep-going]

ARDY conditions on LLM2Vec embeddings of Llama-3-8B-Instruct and resolves
two names under ``TEXT_ENCODERS_DIR`` (``ardy/model/load_model.py``):

    McGill-NLP/LLM2Vec-Meta-Llama-3-8B-Instruct-mntp              a FULL model
    McGill-NLP/LLM2Vec-Meta-Llama-3-8B-Instruct-mntp-supervised   a PEFT adapter

The hub repo of the first holds only an adapter (the MNTP LoRA) plus the
bidirectional-Llama modeling code; the full model it wants is that adapter
merged into Llama-3-8B-Instruct. Meta's own repo is gated, and NousResearch
mirrors the identical weights without the click-through, so this script:

1. downloads ``NousResearch/Meta-Llama-3-8B-Instruct`` (the base),
   ``McGill-NLP/LLM2Vec-Meta-Llama-3-8B-Instruct-mntp`` (into
   ``McGill-NLP/mntp-adapter-src``) and ``...-mntp-supervised``;
2. rewrites the MNTP adapter's ``base_model_name_or_path`` to the local base,
   so loading it attaches to the mirror and not to the gated repo;
3. loads the base *through the adapter directory* as ARDY's bidirectional
   ``LlamaBiModel`` — transformers' PEFT integration attaches the adapter
   on load — calls ``merge_and_unload`` and saves the result as the full
   model under the first name, beside the custom modeling files and the
   tokenizer copied from the adapter repo;
4. rewrites the supervised adapter's ``base_model_name_or_path`` to the
   merged path, so the record of what it sits on is the truth.

Layout written (≈31 GB)::

    DIR/NousResearch/Meta-Llama-3-8B-Instruct/                  the base, kept for re-runs
    DIR/McGill-NLP/mntp-adapter-src/                             the MNTP repo as downloaded (+ rewritten adapter_config)
    DIR/McGill-NLP/LLM2Vec-Meta-Llama-3-8B-Instruct-mntp/        merged full model (model.safetensors, config, tokenizer, modeling_*.py)
    DIR/McGill-NLP/LLM2Vec-Meta-Llama-3-8B-Instruct-mntp-supervised/   the supervised adapter (+ rewritten adapter_config)

Idempotent: a finished step is skipped. Runs inside the ardy env (needs
``ardy``, ``transformers==5.8.1``, ``peft``, ``torch``, ``huggingface_hub``);
the merge takes ~16 GB of RAM on CPU or of VRAM on ``--device cuda``.

Licence: the base is under the Llama 3 Community License ("Built with Meta
Llama 3" — the backend notice carries it); LLM2Vec's adapters are MIT.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
from pathlib import Path

BASE_ID = "NousResearch/Meta-Llama-3-8B-Instruct"
MNTP_ID = "McGill-NLP/LLM2Vec-Meta-Llama-3-8B-Instruct-mntp"
SUPERVISED_ID = "McGill-NLP/LLM2Vec-Meta-Llama-3-8B-Instruct-mntp-supervised"

#: Where the MNTP repo's raw download lives — it is an adapter, and the name
#: ARDY resolves must be the merged full model.
MNTP_SRC_SUBDIR = "McGill-NLP/mntp-adapter-src"

#: Files the merged directory takes from the adapter repo: the bidirectional
#: modeling code the hub config names, and the tokenizer with the LLM2Vec
#: padding setup.
SIDE_FILES = (
    "modeling_llama_encoder.py",
    "attn_mask_utils.py",
    "tokenizer.json",
    "tokenizer_config.json",
    "special_tokens_map.json",
)

#: One file, like the reference layout: sharding a 15 GB model into 5 GB
#: pieces buys nothing on a local disk.
MAX_SHARD = "20GB"


def log(text: str) -> None:
    sys.stderr.write(f"assemble_text_encoder: {text}\n")
    sys.stderr.flush()


def download(repo_id: str, dest: Path) -> Path:
    """Snapshot ``repo_id`` into ``dest`` (a real directory, not a cache link). Idempotent."""
    from huggingface_hub import snapshot_download

    dest.parent.mkdir(parents=True, exist_ok=True)
    marker = dest / "config.json"
    if not marker.is_file():
        marker = dest / "adapter_config.json"
    if marker.is_file():
        log(f"{repo_id}: present at {dest}")
    else:
        log(f"{repo_id} -> {dest}")
    # snapshot_download is itself resumable; calling it on a complete
    # directory is a metadata check and nothing more.
    snapshot_download(repo_id=repo_id, local_dir=str(dest))
    return dest


def rewrite_base_path(adapter_dir: Path, base: Path) -> None:
    """Point an adapter's ``base_model_name_or_path`` at a local directory."""
    config_path = adapter_dir / "adapter_config.json"
    with open(config_path, encoding="utf-8") as handle:
        config = json.load(handle)
    wanted = str(base.resolve())
    if config.get("base_model_name_or_path") == wanted:
        return
    config["base_model_name_or_path"] = wanted
    with open(config_path, "w", encoding="utf-8") as handle:
        json.dump(config, handle, indent=2)
        handle.write("\n")
    log(f"{config_path}: base_model_name_or_path -> {wanted}")


def merged_complete(merged: Path) -> bool:
    weights = (merged / "model.safetensors").is_file() or (merged / "model.safetensors.index.json").is_file()
    return weights and all((merged / name).is_file() for name in SIDE_FILES) and (merged / "config.json").is_file()


def merge(base: Path, mntp_src: Path, merged: Path, *, device: str) -> None:
    """Base + MNTP adapter → one full model under ``merged``."""
    import torch

    try:
        from ardy.model.llm2vec.models.bidirectional_llama import LlamaBiModel
    except ImportError as err:
        raise SystemExit(
            f"ardy is not importable ({err}); run this under backends/ardy/.env/bin/python after `pip install -e` of the checkout"
        ) from err

    if merged_complete(merged):
        log(f"merged model present at {merged}")
        return
    if device == "cuda" and not torch.cuda.is_available():
        log("no CUDA device; merging on the CPU (~16 GB of RAM)")
        device = "cpu"
    rewrite_base_path(mntp_src, base)
    # Loading the ADAPTER directory: transformers' PEFT integration reads
    # adapter_config.json, loads the base it names (the local mirror, after
    # the rewrite above) and attaches the LoRA. This is the path LLM2Vec's
    # own from_pretrained takes for "config.json and adapter weights in the
    # same directory", followed by merge_and_unload.
    log(f"loading {base.name} through {mntp_src} as LlamaBiModel on {device} (bf16)")
    try:
        model = LlamaBiModel.from_pretrained(str(mntp_src), dtype=torch.bfloat16, device_map=device)
    except OSError:
        # transformers' own auto-redirect (integrations.peft.maybe_load_adapters)
        # only points at the base when mntp_src has no config.json of its
        # own — this adapter repo ships one (it names the architecture
        # class, not a checkpoint), so it is read as "a complete model with
        # an embedded adapter" and transformers looks for model.safetensors
        # right there instead. Loaded explicitly instead of depending on
        # that file-existence heuristic holding across transformers versions.
        log(f"{mntp_src} has its own config.json — transformers did not auto-redirect to the base; loading it explicitly")
        from peft import PeftModel

        model = LlamaBiModel.from_pretrained(str(base), dtype=torch.bfloat16, device_map=device)
        model = PeftModel.from_pretrained(model, str(mntp_src))
    if not hasattr(model, "peft_config"):
        # Older transformers did not attach on load; do it by hand.
        from peft import PeftModel

        model = PeftModel.from_pretrained(model, str(mntp_src))
    log("merge_and_unload")
    model = model.merge_and_unload()
    merged.mkdir(parents=True, exist_ok=True)
    log(f"saving the merged model to {merged} (one safetensors file)")
    model.save_pretrained(str(merged), safe_serialization=True, max_shard_size=MAX_SHARD)
    del model
    if device == "cuda":
        torch.cuda.empty_cache()
    for name in SIDE_FILES:
        source = mntp_src / name
        if source.is_file():
            shutil.copy2(source, merged / name)
        else:
            log(f"warning: {source} is not in the adapter repo; the merged model lacks it")
    # The reference layout's config carries no _name_or_path: LLM2Vec reads
    # it back from config.json and would otherwise wrap prompts in a chat
    # template when it names meta-llama's repo. Keep the merged config free
    # of it so the encoder behaves the same whether or not the hub is reachable.
    config_path = merged / "config.json"
    with open(config_path, encoding="utf-8") as handle:
        config = json.load(handle)
    if config.pop("_name_or_path", None) is not None:
        with open(config_path, "w", encoding="utf-8") as handle:
            json.dump(config, handle, indent=2)
            handle.write("\n")
    log(f"merged: {merged}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--dest", required=True, metavar="DIR", help="the TEXT_ENCODERS_DIR to lay out")
    parser.add_argument("--device", default="cuda", choices=("cuda", "cpu"), help="where to merge (default cuda, falling back to cpu)")
    parser.add_argument("--skip-download", action="store_true", help="do not touch the hub; fail if a piece is missing")
    args = parser.parse_args(argv)

    dest = Path(args.dest).expanduser().resolve()
    base = dest / BASE_ID
    mntp_src = dest / MNTP_SRC_SUBDIR
    merged = dest / MNTP_ID
    supervised = dest / SUPERVISED_ID

    if args.skip_download:
        for path in (base, mntp_src, supervised):
            if not path.is_dir():
                raise SystemExit(f"--skip-download but {path} is missing")
    else:
        download(BASE_ID, base)
        download(MNTP_ID, mntp_src)
        download(SUPERVISED_ID, supervised)

    merge(base, mntp_src, merged, device=args.device)
    rewrite_base_path(supervised, merged)

    if not merged_complete(merged):
        raise SystemExit(f"{merged} is incomplete after the merge")
    log(f"done: TEXT_ENCODERS_DIR={dest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
