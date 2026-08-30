"""Every command module's run_fake, end to end: outputs validate, records parse, guards hold.

Ten generator commands had zero tests; this file gives each one the cheap
half — the ``--fake`` path needs no backend, no GPU and no Blender, yet it
exercises argument settling, output writing and the record schema. Each
smoke test asserts the record round-trips through ``records.load`` +
``records.normalize`` (the byte contract with the Rust reader) and that the
artefacts pass the validator their real counterparts must. The guard tests
pin the other new behaviour: ``--fake`` refuses to overwrite anything a
``--fake`` run did not write.
"""

from __future__ import annotations

import argparse
import json
import wave

import pytest

from forge_gen import cli, glb, npz, placeholders, records
from forge_gen.exit_codes import UsageError
from tests.conftest import REPO


def parse(argv: list[str]):
    """The production parser, so args carry exactly what a real invocation gets."""
    args = cli.build_parser().parse_args(argv)
    return args._module, args


def run_fake(*argv: str) -> dict:
    module, args = parse([*argv, "--fake"])
    return module.run_fake(args)


def load_and_normalize(record_path) -> dict:
    """The record parses, declares itself fake, and re-normalises to the same content."""
    rec = records.load(record_path)
    assert rec["fake"] is True
    assert rec["backend"]["commit"] == placeholders.FAKE_COMMIT
    again = records.normalize(rec)
    assert records.dumps(again) == records.dumps(rec), "normalize must be idempotent on what write wrote"
    return rec


@pytest.fixture(autouse=True)
def humanoid(monkeypatch):
    monkeypatch.setenv("FORGE_RIG_PROFILE", str(REPO / "rigs" / "humanoid"))
    monkeypatch.delenv("FORGE_FAKE", raising=False)


@pytest.fixture
def ref_png(tmp_path):
    return placeholders.tile_png(tmp_path / "ref.png")


@pytest.fixture
def lift_glb(tmp_path):
    return placeholders.placeholder_glb(tmp_path / "lift.glb", name="lift")


# ------------------------------------------------------------------ meshes --


def test_mesh_fake_writes_a_glb_and_a_null_knob_record(ref_png, tmp_path):
    out, rec_path = tmp_path / "out.glb", tmp_path / "out.lift.json"
    result = run_fake("mesh", str(ref_png), "--out", str(out), "--record", str(rec_path))
    assert glb.verify_glb(out)["meshes"] == 1
    rec = load_and_normalize(rec_path)
    assert rec["kind"] == "lift"
    # Nothing ran: every knob the caller did not state is null — no seed 42,
    # no resolution 1024, and above all no texture baker that never loaded.
    assert all(value is None for value in rec["params"].values()), rec["params"]
    assert result["outputs"] == [str(out)]


def test_mesh_fake_keeps_only_the_stated_knobs(ref_png, tmp_path):
    rec_path = tmp_path / "s.lift.json"
    run_fake("mesh", str(ref_png), "--out", str(tmp_path / "s.glb"), "--record", str(rec_path), "--seed", "7", "--preset", "character")
    params = records.load(rec_path)["params"]
    assert params["seed"] == 7 and params["preset"] == "character", "a stated knob is a fact about the request"
    assert params["texture_baker"] is None and params["resolution"] is None


def test_prop_fake(lift_glb, tmp_path):
    out, rec_path = tmp_path / "prop.glb", tmp_path / "prop.json"
    run_fake("prop", str(lift_glb), "--out", str(out), "--record", str(rec_path), "--height", "0.9")
    assert glb.verify_glb(out)["meshes"] == 1
    rec = load_and_normalize(rec_path)
    assert rec["kind"] == "prop" and rec["inputs"][0]["role"] == "mesh"


def test_rig_fake(lift_glb, tmp_path):
    out, rec_path = tmp_path / "hero.blend", tmp_path / "hero.rig.json"
    run_fake("rig", str(lift_glb), "--out", str(out), "--record", str(rec_path), "--name", "hero")
    assert placeholders.is_placeholder(out)
    rec = load_and_normalize(rec_path)
    assert rec["kind"] == "rig"
    assert all(value is None for value in rec["measured"].values()), "nothing was measured"


def test_export_fake(tmp_path):
    blend = placeholders.placeholder_blend(tmp_path / "hero.blend")
    out, rec_path = tmp_path / "hero.glb", tmp_path / "hero.export.json"
    run_fake("export", str(blend), "--out", str(out), "--record", str(rec_path))
    info = glb.verify_glb(out)
    assert info["skins"] == 1, "a body placeholder that binds nothing would not exercise the promote gate"
    rec = load_and_normalize(rec_path)
    assert rec["kind"] == "export"


def test_rig_build_fake_writes_a_record_at_last(tmp_path):
    result = run_fake("rig-build", "--out-dir", str(tmp_path))
    assert result["record"] is not None, "rig-build was the one generator command with no record"
    rec = load_and_normalize(result["record"])
    assert rec["kind"] == "rig" and rec["params"]["mode"] == "build"
    assert rec["inputs"][0]["role"] == "clip" and rec["inputs"][0]["sha256"], "the fixture clip is hashed as the input"
    assert {entry["path"] for entry in rec["outputs"]} == {str(tmp_path / "rig.blend"), str(tmp_path / "rig.glb")}
    assert placeholders.is_placeholder(tmp_path / "rig.blend")
    assert glb.verify_glb(tmp_path / "rig.glb")


# ------------------------------------------------------------------- audio --


def assert_audible_placeholder(path):
    assert placeholders.is_placeholder(path)
    with wave.open(str(path)) as handle:
        frames = handle.readframes(handle.getnframes())
    assert max(abs(int.from_bytes(frames[i : i + 2], "little", signed=True)) for i in range(0, len(frames), 2)) > 1000, (
        "the placeholder must not be silence: `forge audio inspect` calls a silent file defective"
    )


def test_sfx_fake(tmp_path):
    out = tmp_path / "door.wav"
    result = run_fake("sfx", "--prompt", "a heavy door slams", "--out", str(out))
    assert_audible_placeholder(out)
    rec = load_and_normalize(result["record"])
    assert rec["kind"] == "sfx" and rec["params"]["seed"] is not None


def test_music_fake(tmp_path):
    out, rec_path = tmp_path / "theme.wav", tmp_path / "theme.music.json"
    run_fake("music", "--prompt", "calm exploration", "--duration", "30", "--out", str(out), "--record", str(rec_path))
    assert_audible_placeholder(out)
    rec = load_and_normalize(rec_path)
    assert rec["kind"] == "music"
    assert rec["backend"]["executor"] == "comfy", "a fake stands in for the path that would have run"
    assert rec["backend"]["workflow_sha256"] is None, "nothing loaded a template; null means unknown"


def test_the_music_graph_is_built_and_patched_without_a_host(tmp_path, repo_root):
    """The whole of `forge gen music` up to `POST /prompt`, on the tracked template.

    No ComfyUI, no card: what this proves is that the template the toolkit
    ships carries every knob the verb states and no knob it does not, which
    is the refusal that must happen *before* the card is leased.
    """
    from forge_gen import backends, comfy
    from forge_gen.audio import music

    backend = backends.load_backend("acestep", repo_root / "backends")
    assert backend.executor == "comfy" and backend.host == "comfy"
    graph, digest = comfy.load_template(backend, music.WORKFLOW)
    assert digest.startswith("sha256:")

    args = argparse.Namespace(
        out=str(tmp_path / "theme.ogg"), record=str(tmp_path / "theme.json"),
        prompt="dark ambient boss theme, low strings, taiko", lyrics_file=None,
        duration=90.0, seed=815273, bpm=96, keyscale="F minor", timesignature="4",
        thinking=True, format=None, stop_server=False, timeout=60.0, created_by="human",
    )
    request = music.check_inputs(args)
    inputs = music.template_inputs(request, seed=request["seed"], prefix="forge/j-1/theme")
    patched = comfy.patch(graph, inputs, "music.api.json")

    # Every stated knob landed on a node, and the two seeds are one seed.
    assert patched["5"]["inputs"]["tags"] == args.prompt
    assert patched["5"]["inputs"]["bpm"] == 96 and patched["5"]["inputs"]["keyscale"] == "F minor"
    assert patched["5"]["inputs"]["seed"] == 815273 and patched["11"]["inputs"]["seed"] == 815273
    assert patched["5"]["inputs"]["duration"] == 90.0 and patched["10"]["inputs"]["seconds"] == 90.0
    assert patched["13"]["inputs"]["filename_prefix"] == "forge/j-1/theme"
    # And the register the template states for itself is untouched.
    assert patched["11"]["inputs"]["steps"] == 8 and patched["11"]["inputs"]["cfg"] == 1.0

    # A key the model would refuse never reaches the host.
    args.keyscale = "H sharp minor"
    with pytest.raises(Exception, match="not one the model takes"):
        music.check_inputs(args)


def test_every_audio_verb_states_exactly_the_knobs_its_template_marks(tmp_path, repo_root):
    """The contract between a module and its graph, checked on the shipped files.

    A template that marks a knob the verb does not fill would run at
    whatever it was saved with; a verb that states a knob the template does
    not mark would have it silently dropped. Both are refusals inside
    `comfy.patch`, and this is what makes them fail here — in a test with no
    host and no card — instead of in front of someone with the card leased.
    """
    from forge_gen import backends, comfy
    from forge_gen.audio import music, sfx, speech, voice

    tree = repo_root / "backends"
    ref = placeholders.placeholder_wav(tmp_path / "voices" / "warden" / "ref.wav", seconds=8.0)

    cases = [
        ("acestep", music, lambda spec: music.template_inputs(spec, seed=1, prefix="p")),
        ("moss_sfx", sfx, lambda spec: sfx.template_inputs(spec, spec["jobs"][0], prefix="p")),
        ("moss_tts", speech, lambda spec: speech.template_inputs(spec, spec["jobs"][0], reference_name="r.wav", prefix="p")),
        ("moss_tts", voice, lambda spec: voice.template_inputs(spec, prefix="p")),
    ]
    specs = {
        music: music.check_inputs(argparse.Namespace(
            out=str(tmp_path / "t.wav"), record=str(tmp_path / "t.json"), prompt="taiko",
            lyrics_file=None, duration=30.0, seed=1, bpm=None, keyscale=None,
            timesignature=None, thinking=True, format=None, stop_server=False)),
        sfx: sfx.plan(argparse.Namespace(
            prompt="a door", out=str(tmp_path / "d.wav"), record=None, seconds=1.0, seed=1,
            steps=100, cfg=4.0, batch_file=None, out_dir=None, model=None)),
        speech: speech.plan(argparse.Namespace(
            text="Stand down.", out=str(tmp_path / "l.wav"), record=None, voice=str(ref),
            voice_text=None, language="en", backend="moss_tts", seed=1, lines_file=None,
            out_dir=None, model=None)),
        voice: voice.plan(argparse.Namespace(
            name="warden", describe="deep, slow, grave", line=None, seed=1,
            out_dir=str(tmp_path / "designed"), overwrite=False, model=None,
            temperature=None, top_p=None, top_k=None, rep_penalty=None)),
    }
    for backend_name, module, fill in cases:
        backend = backends.load_backend(backend_name, tree)
        graph, _ = comfy.load_template(backend, module.WORKFLOW)
        marked = set(comfy.patch_points(graph, module.WORKFLOW))
        stated = set(fill(specs[module]))
        assert stated == marked, (
            f"{module.WORKFLOW}: the verb states {sorted(stated)} and the template marks {sorted(marked)}"
        )
        # And the patch itself goes through, which is the real proof.
        comfy.patch(graph, fill(specs[module]), module.WORKFLOW)


def test_a_music_record_from_a_comfy_run_says_what_ran(tmp_path):
    """build_record still takes a result and a backend block; both halves are new."""
    from forge_gen.audio import music

    args = argparse.Namespace(
        out=str(tmp_path / "theme.wav"), record=str(tmp_path / "theme.json"),
        prompt="taiko", lyrics_file=None, duration=30.0, seed=7, bpm=None,
        keyscale=None, timesignature=None, thinking=True, format=None,
        stop_server=False, timeout=60.0, created_by="human",
    )
    request = music.check_inputs(args)
    inputs = music.template_inputs(request, seed=7, prefix="forge/local/theme")
    result = {
        "prompt": request["prompt"], "lyrics": inputs["lyrics"],
        "seed_value": f"{inputs['plan_seed']},{inputs['seed']}",
        "dit_model": "ace_step_1.5_turbo_aio.safetensors", "lm_model": None,
        "metas": {"bpm": inputs["bpm"], "keyscale": inputs["keyscale"],
                  "timesignature": inputs["timesignature"], "genres": inputs["tags"]},
    }
    backend = {
        "name": "acestep", "commit": None, "python": None, "torch": None,
        "model": "ace_step_1.5_turbo_aio.safetensors", "model_revision": None,
        "executor": "comfy", "comfyui_commit": "169fcf35", "workflow_sha256": "sha256:9f1c",
        "packs": {},
    }
    rec = music.build_record(request, result, measured={"duration_s": 30.0}, backend=backend)
    rec["params"]["workflow"] = music.WORKFLOW
    written = json.loads(records.dumps(rec))
    assert written["forge_record"] == 2
    assert written["backend"]["commit"] is None, "no checkout of its own"
    assert written["backend"]["executor"] == "comfy"
    assert written["backend"]["packs"] == {}, "native nodes: empty, not null"
    # The params keys are the ones the library's shipped music records have.
    assert set(written["params"]) >= {
        "lm_model", "dit_model", "seed", "bpm", "keyscale", "timesignature",
        "genres", "lyrics", "duration_s", "format", "thinking", "workflow",
    }
    assert written["params"]["seed"] == "7,7", "the pair the two stages were given"
    assert written["params"]["bpm"] == 120 and written["params"]["keyscale"] == "C major", (
        "what the node was given, which is the node's default when the run said nothing"
    )


def test_stop_server_is_refused_by_name(tmp_path):
    from forge_gen.audio import music
    from forge_gen.exit_codes import UsageError

    args = argparse.Namespace(stop_server=True, out=None, record=None, prompt=None)
    with pytest.raises(UsageError) as caught:
        music.refuse_stop_server(args)
    assert "forge gpu --free" in caught.value.payload()["hint"]
    assert "systemctl --user stop forge-comfy" in caught.value.payload()["hint"]


def test_speech_fake(tmp_path):
    out = tmp_path / "line.wav"
    result = run_fake("speech", "--text", "Stand down.", "--out", str(out))
    assert_audible_placeholder(out)
    rec = load_and_normalize(result["record"])
    assert rec["kind"] == "speech"


def test_voice_fake(tmp_path):
    result = run_fake("voice", "warden", "--describe", "deep, slow, grave", "--seed", "1", "--out-dir", str(tmp_path))
    out = tmp_path / "warden" / "ref.wav"
    assert_audible_placeholder(out)
    with wave.open(str(out)) as handle:
        seconds = handle.getnframes() / handle.getframerate()
    assert 5.0 <= seconds <= 15.0, "the fake reference must pass the cloner's duration band"
    rec = load_and_normalize(result["record"])
    assert rec["kind"] == "voice"


# ------------------------------------------------------------------ motion --


def test_sweep_fake_states_batch_and_grid(tmp_path):
    result = run_fake(
        "motion", "sweep", "--out-dir", str(tmp_path), "--prompt", "a person walks forward",
        "--duration", "2", "--samples", "2", "--seeds", "0", "1",
    )
    takes = result["takes"]
    assert len(takes) == 4
    for take in takes:
        assert placeholders.is_placeholder(take["path"])
        rec = load_and_normalize(take["record"])
        assert rec["kind"] == "take"
        params = rec["params"]
        assert params["batch_size"] == 8, "the argparse default, stated — sample k of a batch depends on it"
        assert params["grid"] == {"prompts": 1, "seeds": 2, "cfg": 1, "durations": 1, "samples": 2}
    assert placeholders.is_placeholder(tmp_path / "sweep.json")


def test_keys_fake_states_batch_and_grid(tmp_path):
    base = npz.write_take(tmp_path / "base.npz", frames=20, fps=20, prompt="idle")
    result = run_fake(
        "motion", "keys", "--base", str(base), "--prompt", "rifle recoil", "--preset", "recoil",
        "--out-dir", str(tmp_path), "--samples", "2", "--seed", "3",
    )
    takes = result["takes"]
    assert len(takes) == 2
    for take in takes:
        rec = load_and_normalize(take["record"])
        params = rec["params"]
        assert params["batch_size"] == 2, "keys draws every sample in one forward pass"
        assert params["grid"] == {"prompts": 1, "seeds": 1, "cfg": 1, "durations": 1, "samples": 2}
        assert params["keys_sha256"], "the synthesized spec is hashed like an authored one"


# ------------------------------------------------------------------ guards --


def test_fake_refuses_to_overwrite_a_real_blend(lift_glb, tmp_path):
    """The finding's exact shape: FORGE_FAKE=1 replaced a committed 3 MB .blend with 69 bytes."""
    out = tmp_path / "hero.blend"
    out.write_bytes(b"BLENDER-v405RENDH" + b"\x00" * 4096)
    with pytest.raises(UsageError, match="hero.blend"):
        run_fake("rig", str(lift_glb), "--out", str(out), "--record", str(tmp_path / "hero.rig.json"), "--name", "hero")
    assert out.stat().st_size > 4096 - 1, "the real file is untouched"


def test_fake_refuses_a_real_wav_glb_take_and_record(ref_png, tmp_path):
    real_wav = tmp_path / "door.wav"
    with wave.open(str(real_wav), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(8000)
        handle.writeframes(b"\x00\x01" * 800)
    with pytest.raises(UsageError, match="door.wav"):
        run_fake("sfx", "--prompt", "a door", "--out", str(real_wav))

    real_glb = tmp_path / "barrel.glb"
    glb.placeholder_glb(real_glb, generator="TRELLIS.2")  # a real generator string: not a placeholder
    with pytest.raises(UsageError, match="barrel.glb"):
        run_fake("mesh", str(ref_png), "--out", str(real_glb), "--record", str(tmp_path / "barrel.lift.json"))

    real_record = tmp_path / "sound.json"
    rec = placeholders.fake_record("sfx", "moss_sound_effect", backend="moss_sfx")
    rec["fake"] = False  # a real record beside a real render
    records.write(rec, real_record)
    with pytest.raises(UsageError, match="sound.json"):
        run_fake("sfx", "--prompt", "a door", "--out", str(tmp_path / "sound.wav"), "--record", str(real_record))


def test_fake_reruns_over_its_own_placeholders_are_fine(tmp_path):
    argv = ("motion", "sweep", "--out-dir", str(tmp_path), "--prompt", "walk", "--duration", "2", "--samples", "1")
    run_fake(*argv)
    run_fake(*argv)  # ci-fake reruns must stay green
    out = tmp_path / "door.wav"
    run_fake("sfx", "--prompt", "a door", "--out", str(out))
    run_fake("sfx", "--prompt", "a door", "--out", str(out))
