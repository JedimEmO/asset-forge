"""Original 16-bar industrial arcade loop, 120 BPM, E minor.

No audio inputs. Circular event mixing preserves release/delay tails at the
loop boundary. Run at repository root; promote the output through Forge.
"""
from pathlib import Path
import hashlib
import json
import wave
import numpy as np

RATE, BPM, BARS = 48000, 120, 16
BEAT = 60 / BPM
SECONDS = BARS * 4 * BEAT
COUNT = round(SECONDS * RATE)
TAU = 2 * np.pi


def render():
    mix = np.zeros((COUNT, 2), dtype=np.float64)
    rng = np.random.default_rng(42017)

    def event(time, samples, volume, pan=0.):
        indices = (round(time*RATE)+np.arange(len(samples))) % COUNT
        angle = (pan+1)*np.pi/4
        mix[indices, 0] += volume*np.cos(angle)*samples
        mix[indices, 1] += volume*np.sin(angle)*samples

    def clock(duration):
        return np.arange(round(duration*RATE))/RATE

    def envelope(t, attack, decay, end):
        return np.minimum(1., t/attack)*np.exp(-t/decay)*np.minimum(1., np.maximum(0., (end-t)/.008))

    def freq(note):
        return 440*2**((note-69)/12)

    def tone(note, duration, bright=False):
        t = clock(duration)
        phase = TAU*freq(note)*t
        voice = np.sin(phase)+.25*np.sin(2*phase)+.12*np.sin(3*phase)
        if bright:
            voice += .06*np.sin(5*phase)
        return voice*envelope(t,.006,duration*.26,duration)

    for bar in range(BARS):
        start = bar*4*BEAT
        # Tight four-on-the-floor kick with syncopated additional hits.
        for step in [0, 4, 8, 12] + ([14] if bar%4 == 3 else []):
            t = clock(.38)
            kick = np.sin(TAU*(48*t+5*(1-np.exp(-t/.035))))*envelope(t,.001,.085,.38)
            kick += .11*rng.uniform(-1,1,len(t))*envelope(t,.001,.008,.38)
            event(start+step*BEAT/4,kick,.53)
        for beat in [1,3]:
            t = clock(.22)
            noise = rng.uniform(-1,1,len(t))
            hp = noise-np.roll(noise,1)
            snare = (.4*hp+.3*np.sin(TAU*185*t))*envelope(t,.001,.045,.22)
            event(start+beat*BEAT,snare,.31,.08)
        for step in range(16):
            duration = .09 if step%4 == 2 else .042
            t = clock(duration)
            noise = rng.uniform(-1,1,len(t))
            hat = (noise-np.roll(noise,1))*envelope(t,.0005,.011,duration)
            event(start+step*BEAT/4,hat,.055 if step%2 else .075,(-.28 if step%2 else .28))
        # E-C-G-D progression and a rhythmic octave bass line.
        root = [40,36,43,38][(bar//2)%4]
        pattern = [(0,0),(2,0),(3,12),(6,0),(8,0),(10,7),(11,12),(14,0)]
        for step, interval in pattern:
            event(start+step*BEAT/4,tone(root+interval,.27),.24)
        # Slow harmonic bed; circular placement keeps tails at the seam.
        t = clock(2.8)
        notes = [root+24,root+27,root+31] if root in [40,38] else [root+24,root+28,root+31]
        chord = sum(np.sin(TAU*freq(n)*t) for n in notes)/3
        chord *= np.sin(np.pi*np.minimum(t/2.8,1))**2
        event(start,chord,.065,-.15)
        # Call-and-response hook, with stereo delay returning across loop ends.
        motif = [64,67,71,74,71,67,62,59]
        if bar%4 != 3:
            for j, step in enumerate([1,4,7,10]):
                note = motif[(bar+j)%len(motif)]
                lead = tone(note,.32,True)
                event(start+step*BEAT/4,lead,.095,-.18)
                event(start+step*BEAT/4+.375,lead,.030,.45)
        if bar%4 == 3:
            for j in range(4):
                t = clock(.14)
                clang = (np.sin(TAU*1733*t)+.4*np.sin(TAU*2381*t))*envelope(t,.001,.028,.14)
                event(start+(3+j/4)*BEAT,clang,.065,(-.4+j*.25))
    mix = np.tanh(mix*1.1)
    mix *= .48/np.max(np.abs(mix))
    return np.round(mix*32767).astype('<i2')


def main():
    path = Path('out/audio/synth/scrapyard_combat_loop.wav')
    path.parent.mkdir(parents=True, exist_ok=True)
    samples = render()
    with wave.open(str(path),'wb') as wav:
        wav.setparams((2,2,RATE,0,'NONE','not compressed'))
        wav.writeframes(samples.tobytes())
    info = {'source':str(Path(__file__)), 'source_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(), 'method':'Original analytic synthesis; circular event mixing; no audio inputs.', 'output':str(path), 'sha256':hashlib.sha256(path.read_bytes()).hexdigest(), 'bpm':BPM, 'bars':BARS, 'duration_s':SECONDS, 'sample_rate':RATE, 'channels':2, 'loop':True, 'boundary_step_linear':(np.abs(samples[0].astype(float)-samples[-1].astype(float))/32768).tolist()}
    path.with_suffix('.source.json').write_text(json.dumps(info,indent=2)+'\n')


if __name__ == '__main__':
    main()
