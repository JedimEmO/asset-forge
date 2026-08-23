"""A designed voice: its record from its numbers, its refusals, and the name a line resolves to it by."""

from __future__ import annotations

import argparse
import json

import pytest

from forge_gen import cli, exit_codes, placeholders, records
from forge_gen.audio import speech, voice
from forge_gen.exit_codes import InputRejected


def _args(**overrides) -> argparse.Namespace:
    base = {
        "name": "crypt_warden",
        "describe": "Deep, slow, weathered male voice, English, low pitch, unhurried, grave and calm",
        "line": None,
        "seed": None,
        "temperature": None,
        "top_p": None,
        "top_k": None,
        "rep_penalty": None,
        "out_dir": None,
        "overwrite": False,
        "model": None,
        "created_by": "human",
    }
    base.update(overrides)
    return argparse.Namespace(**base)


@pytest.fixture
def project(tmp_path, monkeypatch):
    """A project root the records are relative to, forgotten afterwards."""
    root = tmp_path / "project"
    root.mkdir()
    records.set_project(root)
    yield root
    records.set_project(None)


def test_build_record_is_a_voice_record_with_every_knob_and_no_inputs(project):
    ref = project / "assets-src" / "voices" / "crypt_warden" / "ref.wav"
    placeholders.silence_wav(ref, seconds=8.0, rate=24000)
    rec = voice.build_record(
        name="crypt_warden",
        instruction="deep and slow",
        text=voice.DEFAULT_LINE,
        out_path=ref,
        model=voice.DEFAULT_MODEL,
        seed=7,
        created_by="human",
        commit="58b20a0d",
        python="3.12.7",
        torch="2.9.1+cu128",
        model_revision="97521ec2b6f3ec5026ac1f5751f8fc302d82c2d4",
    )
    text = records.dumps(rec)
    back = json.loads(text)
    assert back["kind"] == "voice" and back["tool"] == voice.TOOL
    assert back["backend"]["name"] == "moss_tts" and back["backend"]["model"] == voice.DEFAULT_MODEL
    assert back["inputs"] == [], "a designed voice is handed nothing"
    assert back["params"] == {
        "audio_repetition_penalty": 1.1,
        "audio_temperature": 1.5,
        "audio_top_k": 50,
        "audio_top_p": 0.6,
        "instruction": "deep and slow",
        "model": voice.DEFAULT_MODEL,
        "name": "crypt_warden",
        "seed": 7,
        "text": voice.DEFAULT_LINE,
    }
    assert back["outputs"][0]["path"] == "assets-src/voices/crypt_warden/ref.wav"
    assert back["outputs"][0]["sha256"].startswith("sha256:")
    assert back["measured"] == {"channels": 1, "duration_s": 8.0, "sample_rate": 24000}
    assert back["fake"] is False


def test_plan_seeds_every_run_validates_the_name_and_refuses_an_existing_voice(project):
    spec = voice.plan(_args())
    assert isinstance(spec["seed"], int) and spec["seed"] >= 0, "a fresh seed is drawn and recorded"
    assert spec["text"] == voice.DEFAULT_LINE
    assert spec["out"] == str(project / "assets-src" / "voices" / "crypt_warden" / "ref.wav")
    assert spec["record"] == str(project / "assets-src" / "voices" / "crypt_warden" / "voice.json")
    assert spec["sampling"] == voice.SAMPLING
    assert spec["project"] == str(project)

    spec = voice.plan(_args(seed=11, line="State your business.", temperature=1.2, top_p=0.5, top_k=20, rep_penalty=1.05))
    assert spec["seed"] == 11 and spec["text"] == "State your business."
    assert spec["sampling"] == {"audio_temperature": 1.2, "audio_top_p": 0.5, "audio_top_k": 20, "audio_repetition_penalty": 1.05}

    with pytest.raises(InputRejected, match=r"\[a-z0-9_\]\+"):
        voice.plan(_args(name="Crypt Warden"))
    with pytest.raises(InputRejected, match="--describe is empty"):
        voice.plan(_args(describe="   "))
    with pytest.raises(InputRejected, match="--seed must be >= 0"):
        voice.plan(_args(seed=-1))
    with pytest.raises(InputRejected, match="--top-p"):
        voice.plan(_args(top_p=1.5))

    ref = project / "assets-src" / "voices" / "crypt_warden" / "ref.wav"
    placeholders.silence_wav(ref, seconds=8.0)
    with pytest.raises(InputRejected, match="changes every line cloned from it") as caught:
        voice.plan(_args())
    assert caught.value.fields["voice"] == "crypt_warden"
    assert voice.plan(_args(overwrite=True))["out"] == str(ref)

    elsewhere = voice.plan(_args(name="other", out_dir=str(project / "elsewhere")))
    assert elsewhere["out"] == str(project / "elsewhere" / "other" / "ref.wav")


def test_run_fake_writes_a_clonable_placeholder_and_its_record(project):
    result = voice.run_fake(_args(seed=3))
    ref = project / "assets-src" / "voices" / "crypt_warden" / "ref.wav"
    record = ref.with_name("voice.json")
    assert result["ok"] and result["outputs"] == [str(ref)] and result["record"] == str(record)
    assert result["voice"] == "crypt_warden" and result["seed"] == 3
    rec = records.load(record)
    assert rec["fake"] is True and rec["backend"]["commit"] == "fake"
    assert rec["params"]["seed"] == 3
    low, high = speech.REFERENCE_GOOD_S
    assert low <= rec["measured"]["duration_s"] <= high, "the placeholder passes the cloner's length gate"
    # And the cloner accepts it by name.
    assert speech.check_reference(speech.resolve_voice("crypt_warden")) == ref.resolve()


def test_a_bare_voice_name_resolves_to_the_designed_voice_and_its_record(project):
    with pytest.raises(InputRejected, match="forge gen voice crypt_warden") as caught:
        speech.resolve_voice("crypt_warden")
    assert caught.value.fields["voice"] == "crypt_warden"

    ref = project / "assets-src" / "voices" / "crypt_warden" / "ref.wav"
    placeholders.silence_wav(ref, seconds=8.0)
    assert speech.resolve_voice("crypt_warden") == ref
    assert speech.resolve_voice(" crypt_warden ") == ref
    assert speech.voice_record_beside(ref) is None, "a brought clip has no record"
    record = ref.with_name("voice.json")
    record.write_text("{}")
    assert speech.voice_record_beside(ref) == record
    assert speech.voice_record_beside(project / "calm.wav") is None
    assert speech.voice_record_beside(None) is None

    # A path is a path: nothing resolved, nothing refused here.
    brought = project / "brought" / "calm.wav"
    assert speech.resolve_voice(str(brought)) == brought
    assert speech.resolve_voice("assets-src/voices/crypt_warden/ref.wav").name == "ref.wav"


def test_a_line_cloned_from_a_designed_voice_records_the_voice_record(project):
    voice.run_fake(_args(seed=3))
    ref = project / "assets-src" / "voices" / "crypt_warden" / "ref.wav"
    out = project / "out" / "audio" / "voice" / "greeting.wav"
    args = argparse.Namespace(
        text="Few come this deep.",
        out=str(out),
        record=None,
        voice="crypt_warden",
        voice_text=None,
        language="en",
        backend="moss_tts",
        seed=None,
        lines_file=None,
        out_dir=None,
        model=None,
        created_by="human",
    )
    spec = speech.plan(args)
    assert spec["reference"] == str(ref)
    assert spec["voice_record"] == str(ref.with_name("voice.json"))
    result = speech.run_fake(args)
    assert result["voice"] == "crypt_warden"
    rec = records.load(out.with_suffix(".json"))
    roles = {item["role"]: item for item in rec["inputs"]}
    assert roles["reference"]["path"] == "assets-src/voices/crypt_warden/ref.wav"
    assert roles["reference"]["sha256"] == records.sha256_file(ref)
    assert roles["voice_record"]["path"] == "assets-src/voices/crypt_warden/voice.json"
    assert roles["voice_record"]["sha256"] == records.sha256_file(ref.with_name("voice.json"))
    assert rec["params"]["voice"] == "crypt_warden"
    assert rec["params"]["reference"] == "assets-src/voices/crypt_warden/ref.wav"
    assert rec["params"]["voice_record"] == "assets-src/voices/crypt_warden/voice.json"

    # A brought clip: the record names no voice record.
    brought = project / "calm.wav"
    placeholders.silence_wav(brought, seconds=6.0)
    args.voice = str(brought)
    assert speech.plan(args)["voice_record"] is None


def test_voice_is_registered_and_a_fake_run_goes_through_the_tree(project, monkeypatch, capsys):
    monkeypatch.setenv("FORGE_FAKE", "1")
    code = cli.main(["voice", "warden", "--describe", "deep and slow", "--seed", "5", "--json"])
    assert code == exit_codes.OK
    last = json.loads(capsys.readouterr().out.strip().splitlines()[-1])
    assert last["voice"] == "warden" and last["seed"] == 5
    assert (project / "assets-src" / "voices" / "warden" / "voice.json").is_file()

    code = cli.main(["voice", "warden", "--describe", "deep and slow", "--json"])
    assert code == exit_codes.INPUT_REJECTED, "an existing voice is refused without --overwrite"
    last = json.loads(capsys.readouterr().out.strip().splitlines()[-1])
    assert last["error"] == "input_rejected" and last["voice"] == "warden"
