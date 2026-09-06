"""The isolated speaker must stay isolated, with truthful inputs and refusals."""
import argparse
import json
from pathlib import Path

import pytest
from forge_gen import records
from forge_gen.audio import speech, speech_isolated
from forge_gen.exit_codes import MissingBackend, InputRejected


def test_legacy_alias_uses_isolated_interpreter(monkeypatch, tmp_path):
    reference = tmp_path/'ref.wav'
    reference.write_bytes(b'reference')
    spec = dict(backend='moss_tts', model=speech.DEFAULT_MODEL, reference=str(reference))
    seen = []
    monkeypatch.setattr(speech_isolated.launcher, 'resolve_interpreter', lambda b: seen.append(b.name))
    monkeypatch.setattr(speech_isolated, 'ffmpeg_bin', lambda: Path('/ffmpeg'))
    def launch(backend, module, argv, **kwargs):
        payload = json.loads(Path(argv[0]).read_text())
        assert backend.name == 'moss_speech'
        assert module == 'audio.speech_isolated'
        assert payload['backend'] == 'moss_speech'
        assert payload['sampling']['audio_temperature'] == 1.0
        assert payload['sampling']['text_temperature'] == 1.5
        assert kwargs['timeout'] == 10
        return {'ok': True}
    monkeypatch.setattr(speech_isolated.launcher, 'run_inner_checked', launch)
    assert speech_isolated.run(spec, timeout=10) == {'ok': True}
    assert seen == ['moss_speech']
    def absent(backend):
        raise MissingBackend('isolated interpreter missing', backend=backend.name)
    monkeypatch.setattr(speech_isolated.launcher, 'resolve_interpreter', absent)
    with pytest.raises(MissingBackend):
        speech_isolated.run(spec, timeout=10)


def test_adopted_model_code_is_hashed_not_assumed_at_pin(tmp_path):
    code = tmp_path/'model.py'; code.write_text('first')
    cache = tmp_path/'.cache'; cache.mkdir(); (cache/'receipt').write_text('ignored')
    before = speech_isolated.model_facts(tmp_path)
    code.write_text('changed')
    after = speech_isolated.model_facts(tmp_path)
    assert set(before) == {'model.py'}
    assert before['model.py'] != after['model.py']


def test_fake_speech_records_actual_executor_without_loading_models(tmp_path):
    args = argparse.Namespace(backend=None, text='Hold the line.', out=str(tmp_path/'line.wav'),
        lines_file=None, out_dir=None, record=None, voice=None, voice_text=None,
        language='en', seed=101, model=None, created_by='agent:test')
    result = speech.run_fake(args)
    record = records.load(result['record'])
    assert record['backend']['name'] == 'moss_speech'
    assert record['backend']['executor'] == 'env'
    assert record['params']['audio_top_p'] == 0.95
    with pytest.raises(InputRejected, match='needs --voice'):
        speech_isolated.run(speech.plan(args), timeout=1)


def test_truncated_token_lists_are_not_decoded_as_complete_lines():
    import numpy as np
    from forge_gen.exit_codes import BackendFailed
    speech_isolated.check_complete([(0, np.array([[1, 0], [99, 0]]))], 99)
    for result in ([], [(0, np.empty((0, 2)))], [(0, np.array([[1, 0], [2, 0]]))]):
        with pytest.raises(BackendFailed, match="without its end token"):
            speech_isolated.check_complete(result, 99)
