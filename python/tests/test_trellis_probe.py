"""Doctor must enter the same lazy pipeline import that a real lift needs."""
import importlib.util
import json
import sys
from types import SimpleNamespace


def test_a_broken_lazy_pipeline_is_not_reported_as_an_installed_backend(repo_root, monkeypatch, capsys):
    spec = importlib.util.spec_from_file_location('trellis_probe', repo_root/'backends/trellis2/probe.py')
    probe = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(probe)
    monkeypatch.setattr(sys, 'path', sys.path.copy())
    monkeypatch.setitem(sys.modules, 'torch', SimpleNamespace(
        __version__='2.6.0', version=SimpleNamespace(cuda='12.4'),
        cuda=SimpleNamespace(is_available=lambda: True)))
    def import_result(name):
        if name == 'trellis2.pipelines':
            return False, None, 'pipeline import failed'
        return True, None, None
    monkeypatch.setattr(probe, '_try_import', import_result)
    monkeypatch.setattr(probe, '_nvcc_version', lambda: '12.4')
    assert probe.main() == 1
    report = json.loads(capsys.readouterr().out)
    assert not report['ok']
    assert report['errors']['trellis2.pipelines'] == 'pipeline import failed'


def test_imports_alone_do_not_hide_a_missing_cuda_toolchain(repo_root, monkeypatch, capsys):
    spec = importlib.util.spec_from_file_location('trellis_probe_missing_toolchain', repo_root/'backends/trellis2/probe.py')
    probe = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(probe)
    monkeypatch.setattr(sys, 'path', sys.path.copy())
    monkeypatch.setitem(sys.modules, 'torch', SimpleNamespace(
        __version__='2.6.0', version=SimpleNamespace(cuda='12.4'),
        cuda=SimpleNamespace(is_available=lambda: True)))
    monkeypatch.setattr(probe, '_try_import', lambda name: (True, None, None))
    monkeypatch.setattr(probe, '_nvcc_version', lambda: None)
    monkeypatch.delenv('CC', raising=False)
    monkeypatch.delenv('CXX', raising=False)
    assert probe.main() == 1
    report = json.loads(capsys.readouterr().out)
    assert all(report['imports'].values())
    assert not report['ok']
    assert {'cuda_toolchain', 'CC', 'CXX'} <= report['errors'].keys()
