"""Isolated MOSS speech. The outer process never imports model libraries."""
from __future__ import annotations

import json
import platform
import sys
import tempfile
from pathlib import Path

from forge_gen import backends, launcher, records
from forge_gen.audio import check_pcm, ffmpeg_bin, transcode_wav
from forge_gen.audio import speech
from forge_gen.exit_codes import BackendFailed, ForgeGenError, InputRejected

# Explicit defaults from the pinned checkpoint's _build_generation_config.
# These differ from the old Comfy graph's sampler.
SAMPLING = speech.SAMPLING


def run(spec: dict, *, timeout: float) -> dict:
    if spec["model"] != speech.DEFAULT_MODEL:
        raise InputRejected(f"isolated speech supports {speech.DEFAULT_MODEL}, not {spec['model']!r}")
    if not spec["reference"]:
        raise InputRejected("speech needs --voice <name> or a reference clip")
    # The checkpoint accepts a language tag, but not a reference transcript.
    # Keep the supplied transcript in the record and state whether it was used.
    ffmpeg_bin()
    backend = backends.load_backend("moss_speech")
    launcher.resolve_interpreter(backend)
    spec = dict(spec, backend="moss_speech", sampling=dict(SAMPLING))
    with tempfile.TemporaryDirectory(prefix="forge-speech-spec-") as directory:
        path = Path(directory)/"spec.json"
        path.write_text(json.dumps(spec), encoding="utf-8")
        return launcher.run_inner_checked(backend, "audio.speech_isolated", [str(path)], timeout=timeout)


def model_facts(directory: Path) -> dict:
    """Hash actual adopted files; a descriptor pin is not an observed revision."""
    files = sorted(p for p in directory.rglob("*") if p.is_file() and not {".cache", "__pycache__", ".git"}.intersection(p.relative_to(directory).parts))
    return {str(p.relative_to(directory)): records.sha256_file(p) for p in files}


def check_complete(result, end_token: int) -> None:
    """The checkpoint returns (prefix length, token matrix), not a tensor."""
    if not result or any(len(tokens) == 0 or int(tokens[-1, 0]) != end_token for _, tokens in result):
        raise BackendFailed("speech ended without its end token; split a token-limited line into shorter sentences")


def generate(spec: dict) -> dict:
    records.set_project(spec.get("project"))
    import torch
    import soundfile
    import transformers
    from transformers import AutoModel, AutoProcessor

    if transformers.__version__ != "5.0.0" or torch.__version__ != "2.9.1+cu128":
        raise BackendFailed("moss_speech requires transformers 5.0.0 and torch 2.9.1+cu128; run its installer")
    backend = backends.load_backend("moss_speech")
    model_dir = backend.checkpoints/"MOSS-TTS-Local-Transformer"
    codec_dir = backend.checkpoints/"MOSS-Audio-Tokenizer"
    for directory in (model_dir, codec_dir):
        if not (directory/"config.json").is_file():
            raise InputRejected(f"speech model missing at {directory}; run forge setup")
    # Hash once per batch, including the Python code loaded by AutoModel.
    files = {"speech": model_facts(model_dir), "codec": model_facts(codec_dir)}
    torch.backends.cuda.enable_cudnn_sdp(False)
    torch.manual_seed(spec["seed"])
    processor = AutoProcessor.from_pretrained(model_dir, codec_path=str(codec_dir), trust_remote_code=True)
    processor.audio_tokenizer = processor.audio_tokenizer.to("cuda")
    model = AutoModel.from_pretrained(model_dir, trust_remote_code=True, local_files_only=True,
                                     attn_implementation="sdpa", torch_dtype=torch.bfloat16).to("cuda").eval()
    with tempfile.TemporaryDirectory(prefix="forge-speech-reference-") as directory:
        reference = Path(spec["reference"])
        # Explicit PCM decoding avoids torchaudio's torchcodec dependency and
        # supports every container the public command promises.
        pcm = Path(directory)/"reference.wav"
        transcode_wav(ffmpeg_bin(), reference, pcm)
        samples, rate = soundfile.read(pcm, dtype="float32", always_2d=True)
        codes = processor.encode_audios_from_wav([torch.from_numpy(samples.T)], rate)[0]
    rendered = []
    sampling = dict(spec["sampling"], n_vq_for_inference=model.channels - 1)
    for job in spec["jobs"]:
        torch.manual_seed(spec["seed"])
        message = processor.build_user_message(text=job["text"], reference=[codes], language=spec["language"])
        batch = processor([[message]], mode="generation")
        with torch.no_grad():
            result = model.generate(input_ids=batch["input_ids"].to("cuda"),
                                    attention_mask=batch["attention_mask"].to("cuda"), **sampling)
        check_complete(result, model.config.audio_end_token_id)
        messages = [item for item in processor.decode(result) if item is not None]
        if not messages or not messages[0].audio_codes_list:
            raise BackendFailed("speech returned no audio")
        audio = messages[0].audio_codes_list[0].detach().float().cpu().numpy()
        out = Path(job["out"])
        out.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix=".forge-speech-", dir=out.parent) as directory:
            staged = Path(directory)/"line.wav"
            soundfile.write(str(staged), audio.T if audio.ndim > 1 else audio,
                            int(processor.model_config.sampling_rate), subtype="PCM_16")
            check_pcm(staged, what="the line")
            staged.replace(out)
        record = speech.build_record(
            text=job["text"], out_path=out, model=spec["model"], reference=spec["reference"],
            language=spec["language"], voice_text=spec["voice_text"], voice_record=spec["voice_record"],
            seed=spec["seed"], sampling=sampling, created_by=spec["created_by"],
            executor="env", backend_name="moss_speech", python=platform.python_version(), torch=torch.__version__,
        )
        record["params"]["reference_transcript_used"] = False
        record["params"]["runtime"] = {"transformers": transformers.__version__, "attention": "sdpa",
                                      "dtype": "bfloat16", "cudnn_sdp": False, "model_files": files}
        records.write(record, job["record"])
        rendered.append(job)
    return speech._success(spec, rendered)


if __name__ == "__main__":
    try:
        if len(sys.argv) != 3 or sys.argv[1] != "--inner":
            raise InputRejected("use forge gen speech")
        print(json.dumps(generate(json.loads(Path(sys.argv[2]).read_text()))))
    except ForgeGenError as error:
        print(json.dumps(error.payload()))
        sys.exit(error.code)
