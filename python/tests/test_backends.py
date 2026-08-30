"""backends.py reads a backend.toml and refuses the ones that would fail three steps later."""

from __future__ import annotations


import pytest

from forge_gen import backends
from forge_gen.exit_codes import MissingBackend
from tests.conftest import GOOD_TOML


def test_good_toml_parses_every_field(backends_tree):
    assert backends.backends_dir() == backends_tree
    backend = backends.load_backend("ardy")
    assert backend.name == "ardy" and backend.dir == backends_tree / "ardy"
    assert backend.role == "motion" and backend.env_kind == "venv" and backend.python == "3.12"
    assert backend.commit.startswith("693f74d") and backend.cuda == "12.8" and backend.vram_gb == 16.0
    assert backend.entry == "motion.session" and backend.cwd == "checkout" and backend.resident is False
    assert backend.env["TEXT_ENCODERS_DIR"] == "${TEXT_ENCODERS}" and backend.env["HF_HUB_OFFLINE"] == "1"
    assert [m.id for m in backend.models] == [
        "nvidia/ARDY-Core-RP-20FPS-Horizon40", "llama3-llm2vec-merged", "facebook/dinov3-vitl16-pretrain-lvd1689m",
    ]
    assert [m.store for m in backend.models] == ["hf", "text_encoders_dir", "hf"]
    assert backend.models[2].gated and backend.models[2].accept_url.startswith("https://")
    assert backend.notices == ["Llama 3: Built with Meta Llama 3"]
    assert backend.server is None
    assert backend.installed() is None
    assert "install.sh" in backend.install_hint()


def test_list_and_all_backends_keep_known_order_then_extras(backends_tree):
    extra = backends_tree / "zeta"
    extra.mkdir()
    (extra / "backend.toml").write_text(GOOD_TOML.replace('name = "ardy"', 'name = "zeta"'))
    (backends_tree / "not-a-backend").mkdir()
    assert backends.list_names() == [*backends.KNOWN, "zeta"]
    found = backends.all_backends()
    assert isinstance(found["ardy"], backends.Backend)
    assert isinstance(found["zeta"], backends.Backend)
    assert isinstance(found["trellis2"], MissingBackend), "described nowhere: missing, not an exception escaping"
    assert "not-a-backend" not in found


@pytest.mark.parametrize(
    ("before", "after", "complaint"),
    [
        ('name = "ardy"', 'name = "ardie"', "calls itself"),
        ('env_kind = "venv"', 'env_kind = "poetry"', "env_kind"),
        ('cwd = "checkout"', 'cwd = "here"', "cwd"),
        ('commit = "693f74d13b3d04a0a22ce127ee79c929dd89756b"', 'commit = "latest"', "git hash"),
        ('entry = "motion.session"', 'entry = "motion session"', "module name"),
        ('store = "text_encoders_dir"', 'store = "s3"', "store"),
        ('HF_HUB_OFFLINE = "1"', "HF_HUB_OFFLINE = [1]", "must be text"),
    ],
)
def test_bad_values_are_refused_by_name(backends_tree, before, after, complaint):
    text = GOOD_TOML.replace(before, after)
    assert text != GOOD_TOML
    (backends_tree / "ardy" / "backend.toml").write_text(text)
    with pytest.raises(backends.BackendConfigError) as caught:
        backends.load_backend("ardy")
    assert complaint in str(caught.value)
    assert caught.value.code == 3
    assert caught.value.payload()["error"] == "broken_backend"


def test_unparseable_toml_and_missing_fields(backends_tree):
    path = backends_tree / "ardy" / "backend.toml"
    path.write_text("name = [")
    with pytest.raises(backends.BackendConfigError, match="does not parse"):
        backends.load_backend("ardy")
    path.write_text('name = "ardy"\n')
    with pytest.raises(backends.BackendConfigError, match="upstream|commit|executor|env_kind|entry"):
        backends.load_backend("ardy")


def test_absent_dir_and_absent_backend(tmp_path, monkeypatch):
    monkeypatch.setenv("FORGE_BACKENDS", str(tmp_path / "nope"))
    with pytest.raises(MissingBackend, match="no backends directory"):
        backends.load_backend("ardy")
    monkeypatch.setenv("FORGE_BACKENDS", str(tmp_path))
    with pytest.raises(MissingBackend, match="not a backend this toolkit knows"):
        backends.load_backend("gpt")
    with pytest.raises(MissingBackend, match="not described here"):
        backends.load_backend("trellis2")


def test_env_force_is_parsed_apart_and_overlap_is_refused(backends_tree):
    ardy = backends_tree / "ardy"
    ardy_toml = (ardy / "backend.toml").read_text()
    (ardy / "backend.toml").write_text(ardy_toml + '\n[env.force]\nCC = "${PREFIX}/bin/gcc"\n')
    backend = backends.load_backend("ardy")
    assert backend.env_force == {"CC": "${PREFIX}/bin/gcc"}
    assert "CC" not in backend.env and "force" not in backend.env
    # The same key in both tables is a contradiction, said by name.
    (ardy / "backend.toml").write_text(ardy_toml + '\n[env.force]\nHF_HUB_OFFLINE = "1"\n')
    with pytest.raises(backends.BackendConfigError, match="HF_HUB_OFFLINE"):
        backends.load_backend("ardy")
    (ardy / "backend.toml").write_text(ardy_toml.replace('[env]\n', '[env]\nforce = "not-a-table"\n'))
    with pytest.raises(backends.BackendConfigError, match=r"\[env.force\] must be a table"):
        backends.load_backend("ardy")


def test_install_hint_is_absolute(backends_tree):
    backend = backends.load_backend("ardy")
    assert str(backends_tree / "ardy" / "install.sh") in backend.install_hint(), "the hint must work from any cwd"


def test_notices_may_be_plain_strings_and_env_numbers_become_text(backends_tree):
    text = GOOD_TOML.replace('[[notices]]\ntitle = "Llama 3"\ntext = "Built with Meta Llama 3"', "")
    text = text.replace('name = "ardy"', 'name = "ardy"\nnotices = ["one", "two"]', 1)
    text = text.replace('HF_HUB_OFFLINE = "1"', "HF_HUB_OFFLINE = 1\nFLAG = true")
    (backends_tree / "ardy" / "backend.toml").write_text(text)
    backend = backends.load_backend("ardy")
    assert backend.notices == ["one", "two"]
    assert backend.env["HF_HUB_OFFLINE"] == "1" and backend.env["FLAG"] == "1"


#: A backend.toml in the second form: no interpreter, a host, a graph.
COMFY_TOML = """
name = "moss_sfx"
role = "sfx"
executor = "comfy"
host = "comfy"
upstream = "https://github.com/diodiogod/TTS-Audio-Suite"
license = "Apache-2.0 (MOSS-SoundEffect-v2); TTS-Audio-Suite: MIT"
vram_gb = 8
entry = "audio.sfx"

[comfy]
workflows = ["sfx.api.json"]
nodes = ["OneNode", "TheUnloader"]
unload_node = "TheUnloader"

[[comfy.packs]]
repo = "https://github.com/diodiogod/TTS-Audio-Suite"
commit = "b7e41a2cb7e41a2c"
dir = "TTS-Audio-Suite"
license = "MIT"
pips = ["ftfy"]
nodes = ["OneNode", "TheUnloader"]

[[models]]
id = "OpenMOSS-Team/MOSS-SoundEffect-v2.0"
store = "comfy:models/tts"
license = "Apache-2.0"
"""


def _write(tree, name, text):
    directory = tree / name
    directory.mkdir(exist_ok=True)
    (directory / "backend.toml").write_text(text)
    return directory


def test_the_executor_is_derived_when_a_file_does_not_state_one(backends_tree):
    """Every backend.toml written before the second form keeps working unedited."""
    assert backends.load_backend("ardy").executor == "env"
    _write(backends_tree, "blender", 'name = "blender"\nupstream = "x"\ncommit = "none"\nenv_kind = "none"\nentry = "blender"\n')
    tool = backends.load_backend("blender")
    assert tool.executor == "tool" and tool.is_tool and not tool.is_comfy


def test_a_comfy_backend_with_an_interpreter_is_refused(backends_tree):
    """A comfy backend has no interpreter, and saying it has one is a half-truth refused at parse time."""
    _write(backends_tree, "moss_sfx", COMFY_TOML)
    backend = backends.load_backend("moss_sfx")
    assert backend.executor == "comfy" and backend.is_comfy and not backend.is_tool
    assert backend.host == "comfy"
    assert backend.commit is None, "no checkout of its own; null means unknown"
    assert backend.comfy.workflows == ["sfx.api.json"]
    assert backend.comfy.unload_node == "TheUnloader"
    assert [p.dir for p in backend.comfy.packs] == ["TTS-Audio-Suite"]
    assert backend.comfy.packs[0].pips == ["ftfy"]
    assert backend.workflow("sfx.api.json") == backends_tree / "moss_sfx" / "workflows" / "sfx.api.json"
    # `entry` is emphatically not part of the rule: it is a module path
    # relative to forge_gen and the comfy half runs in the outer process.
    assert backend.entry == "audio.sfx"

    for extra, complaint in (
        ('entry = "audio.sfx"\n\n[env]\nPYTHONNOUSERSITE = "1"\n', "an [env] table"),
        ('entry = "audio.sfx"\npython = "3.12"\n', "a python key"),
        ('entry = "audio.sfx"\nenv_kind = "venv"\n', "disagree"),
    ):
        # Spliced in at the top level, not appended: a key after [[models]]
        # would belong to that table and prove nothing.
        _write(backends_tree, "moss_sfx", COMFY_TOML.replace('entry = "audio.sfx"\n', extra))
        with pytest.raises(backends.BackendConfigError, match=complaint.replace("[", r"\[")):
            backends.load_backend("moss_sfx")

    # And the host it runs on must be named: "comfy" is a directory, not a mood.
    _write(backends_tree, "moss_sfx", COMFY_TOML.replace('host = "comfy"\n', ""))
    with pytest.raises(backends.BackendConfigError, match="must name the host"):
        backends.load_backend("moss_sfx")


def test_a_stated_executor_that_agrees_with_env_kind_is_fine(backends_tree):
    text = (backends_tree / "ardy" / "backend.toml").read_text()
    (backends_tree / "ardy" / "backend.toml").write_text(text.replace('name = "ardy"', 'name = "ardy"\nexecutor = "env"'))
    assert backends.load_backend("ardy").executor == "env"
    (backends_tree / "ardy" / "backend.toml").write_text(text.replace('name = "ardy"', 'name = "ardy"\nexecutor = "tool"'))
    with pytest.raises(backends.BackendConfigError, match="disagree"):
        backends.load_backend("ardy")
    (backends_tree / "ardy" / "backend.toml").write_text(text.replace('env_kind = "venv"', 'executor = "env"'))
    with pytest.raises(backends.BackendConfigError, match="needs an env_kind"):
        backends.load_backend("ardy")


def test_comfy_store_resolves_under_the_host_prefix(backends_tree, tmp_path):
    """``comfy:models/<dir>`` is one model list again, resolved where the host's paths config points."""
    _write(backends_tree, "moss_sfx", COMFY_TOML)
    _write(
        backends_tree,
        "comfy",
        'name = "comfy"\nrole = "host"\nexecutor = "tool"\nenv_kind = "none"\n'
        'upstream = "https://github.com/comfyanonymous/ComfyUI"\ncommit = "169fcf35"\nentry = "comfy"\n'
        '[comfy]\nbase_directory = "data"\n',
    )
    prefix = tmp_path / "prefix"
    (prefix / "venv" / "bin").mkdir(parents=True)
    (prefix / "venv" / "bin" / "python").write_text("#!/bin/sh\n")
    (backends_tree / "comfy" / ".env").symlink_to(prefix / "venv")

    host = backends.load_backend("comfy")
    assert backends.comfy_data_dir(host) == prefix / "data", "$PREFIX/<base_directory>, beside the venv"

    model = backends.load_backend("moss_sfx").models[0]
    assert model.is_comfy and model.comfy_path == "models/tts" and model.comfy_folder == "tts"
    assert backends.comfy_model_path(model, host) == prefix / "data" / "models" / "tts"

    model.file = "split/x.safetensors"
    assert backends.comfy_model_path(model, host) == prefix / "data" / "models" / "tts" / "x.safetensors"
    model.local = "renamed.safetensors"
    assert backends.comfy_model_path(model, host).name == "renamed.safetensors"

    # An hf model is not a comfy one, and nothing pretends otherwise.
    assert backends.comfy_model_path(backends.load_backend("ardy").models[0], host) is None


@pytest.mark.parametrize("store", ["comfy:", "comfy:/models/tts", "comfy:models/../../etc", "comfy:models/tts/"])
def test_a_comfy_store_that_could_leave_the_host_tree_is_refused(backends_tree, store):
    _write(backends_tree, "moss_sfx", COMFY_TOML.replace('store = "comfy:models/tts"', f'store = "{store}"'))
    with pytest.raises(backends.BackendConfigError, match="relative path inside the host"):
        backends.load_backend("moss_sfx")


def test_default_backends_dir_is_beside_the_package(monkeypatch, repo_root):
    monkeypatch.delenv("FORGE_BACKENDS", raising=False)
    assert backends.backends_dir() == repo_root / "backends"
