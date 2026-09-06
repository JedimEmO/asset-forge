"""The runtime installer must preserve dependencies and refuse changed inputs."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile

import pytest


@pytest.fixture
def runtime(repo_root, tmp_path, monkeypatch):
    spec = importlib.util.spec_from_file_location('trellis_runtime', repo_root/'backends/trellis2/install_runtime.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    monkeypatch.setattr(module.platform, 'system', lambda: 'Linux')
    monkeypatch.setattr(module.platform, 'machine', lambda: 'x86_64')
    libraries = tmp_path/'dependency env'
    site = libraries/'lib/python3.11/site-packages'
    site.mkdir(parents=True)
    (site/'keep.txt').write_text('existing dependencies')
    monkeypatch.setattr(module, 'python_info', lambda python: dict(version=[3, 11, 16], purelib=str(site)))
    archive = io.BytesIO()
    with tarfile.open(fileobj=archive, mode='w:gz') as bundle:
        for name, content in [('python/bin/python3.11', b'test executable'),
                              ('python/lib/python3.11/site-packages/README', b'test site')]:
            info = tarfile.TarInfo(name)
            info.size = len(content)
            bundle.addfile(info, io.BytesIO(content))
    data = archive.getvalue()
    monkeypatch.setattr(module, 'SHA256', hashlib.sha256(data).hexdigest())
    monkeypatch.setattr(module.urllib.request, 'urlopen', lambda *args, **kwargs: io.BytesIO(data))
    return module, libraries, tmp_path/'runtime'


def test_a_bad_download_installs_nothing_and_preserves_dependencies(runtime, monkeypatch):
    module, libraries, destination = runtime
    monkeypatch.setattr(module, 'SHA256', '0'*64)
    with pytest.raises(ValueError, match='SHA256 mismatch'):
        module.install(libraries, destination)
    assert not destination.exists()
    assert (libraries/'lib/python3.11/site-packages/keep.txt').read_text() == 'existing dependencies'


def test_runtime_reuse_is_offline_and_bound_to_its_dependency_env(runtime, monkeypatch):
    module, libraries, destination = runtime
    assert module.install(libraries, destination) == destination
    receipt = json.loads((destination/'forge-runtime.json').read_text())
    assert receipt['libraries'] == str(libraries)
    pth = destination/'lib/python3.11/site-packages/forge-trellis-dependencies.pth'
    assert pth.read_text().strip() == str(libraries/'lib/python3.11/site-packages')
    def no_download(*args, **kwargs):
        pytest.fail('a verified runtime should not download again')
    monkeypatch.setattr(module.urllib.request, 'urlopen', no_download)
    assert module.install(libraries, destination) == destination
    other = libraries.parent/'another env'
    other.mkdir()
    with pytest.raises(ValueError, match='different runtime or dependency env'):
        module.install(other, destination)
    assert (libraries/'lib/python3.11/site-packages/keep.txt').read_text() == 'existing dependencies'


def test_a_modified_runtime_is_not_silently_reused(runtime):
    module, libraries, destination = runtime
    module.install(libraries, destination)
    (destination/'bin/python3.11').write_bytes(b'changed executable')
    with pytest.raises(ValueError, match='changed since installation'):
        module.install(libraries, destination)
