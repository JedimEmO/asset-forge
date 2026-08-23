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
    with pytest.raises(backends.BackendConfigError, match="upstream|commit|env_kind|entry"):
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


def test_notices_may_be_plain_strings_and_env_numbers_become_text(backends_tree):
    text = GOOD_TOML.replace('[[notices]]\ntitle = "Llama 3"\ntext = "Built with Meta Llama 3"', "")
    text = text.replace('name = "ardy"', 'name = "ardy"\nnotices = ["one", "two"]', 1)
    text = text.replace('HF_HUB_OFFLINE = "1"', "HF_HUB_OFFLINE = 1\nFLAG = true")
    (backends_tree / "ardy" / "backend.toml").write_text(text)
    backend = backends.load_backend("ardy")
    assert backend.notices == ["one", "two"]
    assert backend.env["HF_HUB_OFFLINE"] == "1" and backend.env["FLAG"] == "1"


def test_default_backends_dir_is_beside_the_package(monkeypatch, repo_root):
    monkeypatch.delenv("FORGE_BACKENDS", raising=False)
    assert backends.backends_dir() == repo_root / "backends"
