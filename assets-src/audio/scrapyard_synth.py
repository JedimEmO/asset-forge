"""Original arcade sound design. No sampled or generated audio inputs.

Run from the repository root: python3 assets-src/audio/scrapyard_synth.py
Writes new PCM sources under out/audio/synth. Forge promotion is separate.
The recipe JSON is a source manifest, not a Forge generator record.
"""
from pathlib import Path
import hashlib
import json
import math
import random
import struct
import wave

RATE = 48000
TAU = math.tau
OUT = Path('out/audio/synth')
SPECS = {
    'shot': (0.24, 0.70, 'Dry electric rifle crack with metallic bolt tail'),
    'hit': (0.22, 0.58, 'Short bright armor impact with inharmonic ringing'),
    'explosion': (0.9, 0.75, 'Compact bass impact, electrical fizz and debris'),
    'pickup': (0.42, 0.48, 'Three rising bell notes: E6 G6 B6'),
    'dash': (0.28, 0.50, 'Fast filtered air jet and descending electronic zip'),
    'upgrade': (1.0, 0.52, 'Ascending E minor arpeggio resolving to a bright E chord'),
    'ui': (0.085, 0.35, 'Quiet two-frequency digital confirmation tick'),
}


def bell(t, f, decay):
    if t < 0:
        return 0.0
    return (math.sin(TAU*f*t) + .24*math.sin(TAU*f*2*t) + .08*math.sin(TAU*f*3*t)) * math.exp(-t/decay) * min(1., t/.002)


def render(name, duration):
    rng = random.Random(42017 + list(SPECS).index(name))
    values = []
    low = 0.0
    for i in range(round(duration*RATE)):
        t = i/RATE
        white = rng.uniform(-1, 1)
        low += .08*(white-low)
        high = white-low
        if name == 'shot':
            value = .70*high*math.exp(-t/0.023) + .55*math.sin(TAU*(145*t + 3.0*(1-math.exp(-t/.012))))*math.exp(-t/.028)
            value += .17*bell(t, 1480, .035) + .07*bell(t-.045, 930, .018)
        elif name == 'hit':
            value = .48*high*math.exp(-t/.011)
            value += .32*bell(t, 1267, .033) + .22*bell(t, 1913, .028) + .12*bell(t, 2789, .02)
        elif name == 'explosion':
            value = 1.0*low*math.exp(-t/.13) + .20*high*math.exp(-t/.065)
            value += .33*math.sin(TAU*(47*t + 2.6*(1-math.exp(-t/.05))))*math.exp(-t/.105)
            for delay, freq in [(0.055, 740), (.12, 1231), (.19, 587), (.25, 1733)]:
                value += .055*bell(t-delay, freq, .045)
        elif name == 'pickup':
            value = sum(.22*bell(t-j*.065, f, .058) for j, f in enumerate([1318.51, 1567.98, 1975.53]))
        elif name == 'dash':
            env = min(1., t/.004)*math.exp(-t/.043)
            value = .6*high*env + .14*math.sin(TAU*(2100*t-2600*t*t))*env
        elif name == 'upgrade':
            value = sum(.15*bell(t-j*.075, f, .105) for j, f in enumerate([659.25, 783.99, 987.77, 1318.51]))
            value += sum(.09*bell(t-.34, f, .12) for f in [659.25, 830.61, 987.77, 1318.51])
        else:
            value = (.25*math.sin(TAU*1600*t)+.14*math.sin(TAU*2400*t))*math.exp(-t/.009)
        # A designed attack and release avoid discontinuities in every oscillator.
        value *= min(1., t/.0007) * min(1., max(0., (duration-t)/.008))
        values.append(value)
    # DC-block within the source renderer, before level design and PCM export.
    clean = []
    previous_x = previous_y = 0.0
    for x in values:
        y = x-previous_x+.995*previous_y
        clean.append(y)
        previous_x, previous_y = x, y
    peak = max(abs(x) for x in clean)
    gain = SPECS[name][1]/peak
    return [round(x*gain*32767) for x in clean]


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    recipe = {'source': str(Path(__file__)), 'source_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'sample_rate': RATE, 'seed_base': 42017, 'method': 'Original analytic oscillators, envelopes and seeded noise; no audio inputs.', 'sounds': {}}
    for name, (duration, level, description) in SPECS.items():
        path = OUT/f'scrapyard_{name}.wav'
        samples = render(name, duration)
        with wave.open(str(path), 'wb') as wav:
            wav.setparams((1, 2, RATE, 0, 'NONE', 'not compressed'))
            wav.writeframes(struct.pack('<'+'h'*len(samples), *samples))
        recipe['sounds'][name] = {'path': str(path), 'duration_s': duration, 'peak_linear': level, 'description': description, 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
    (OUT/'source-manifest.json').write_text(json.dumps(recipe, indent=2)+'\n')


if __name__ == '__main__':
    main()
