"""Validate codec reconstruction, not only the PCM handed to the encoder."""
import array
import math
import shutil
import wave
from pathlib import Path

import pytest

from forge_gen.audio import check_pcm, transcode_ogg
from forge_gen.audio.music import check_encoded_track, measure_wav
from forge_gen.exit_codes import BackendFailed


def tone(path, amplitude, *, square=False):
    samples = array.array('h', (int(amplitude * 32767 * ((1 if i % 96 < 48 else -1) if square else math.sin(i * 2 * math.pi / 96))) for i in range(48000)))
    with wave.open(str(path), 'wb') as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(48000)
        f.writeframes(samples.tobytes())


@pytest.fixture
def ffmpeg():
    binary = shutil.which('ffmpeg')
    if not binary:
        pytest.skip('ffmpeg required for the real codec regression')
    return Path(binary)


def test_vorbis_overshoot_is_rejected_even_when_input_pcm_passes(tmp_path, ffmpeg):
    wav, ogg = tmp_path / 'input.wav', tmp_path / 'track.ogg'
    tone(wav, 0.97, square=True)
    check_pcm(wav)
    transcode_ogg(ffmpeg, wav, ogg)
    with pytest.raises(BackendFailed, match='encoded track track.ogg is clipped'):
        check_encoded_track(ffmpeg, ogg, tmp_path / 'decoded.wav', 1.0)
    assert ogg.exists(), 'rejected candidate remains available for diagnosis'


def test_accepted_track_measurements_come_from_decoded_container(tmp_path, ffmpeg):
    wav, ogg, decoded = (tmp_path / name for name in ('input.wav', 'track.ogg', 'decoded.wav'))
    tone(wav, 0.2)
    transcode_ogg(ffmpeg, wav, ogg)
    measured = check_encoded_track(ffmpeg, ogg, decoded, 1.0)
    assert measured == measure_wav(decoded)
    assert measured['sample_rate'] == 48000
    assert measured['channels'] == 1
    assert 0.99 < measured['duration_s'] < 1.02


def test_corrupt_container_is_not_reported_as_success(tmp_path, ffmpeg):
    ogg = tmp_path / 'broken.ogg'
    ogg.write_bytes(b'not audio')
    with pytest.raises(BackendFailed, match='ffmpeg exited'):
        check_encoded_track(ffmpeg, ogg, tmp_path / 'decoded.wav', 1.0)


def test_encoded_loop_exact_period_is_checked(tmp_path, ffmpeg):
    wav, ogg = tmp_path / 'input.wav', tmp_path / 'loop.ogg'
    tone(wav, 0.2)
    transcode_ogg(ffmpeg, wav, ogg)
    check_encoded_track(ffmpeg, wav, tmp_path / 'decoded.wav', 1.0, exact_frames=48000)
    with pytest.raises(BackendFailed, match='--format wav'):
        check_encoded_track(ffmpeg, ogg, tmp_path / 'decoded.wav', 1.0, exact_frames=48000)
