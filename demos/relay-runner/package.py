"""Package Relay Run from an unchanged stage-1 delivery. Existing packages are refused."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil

ROOT=Path(__file__).resolve().parent

def package(delivery,binary,output,showcase,pixal_trial):
    delivery=delivery.resolve();binary=binary.resolve(strict=True);output=output.resolve()
    if output.exists():raise FileExistsError(output)
    output.mkdir(parents=True)
    shutil.copytree(delivery/'assets',output/'assets')
    shutil.copytree(delivery/'provenance',output/'provenance')
    showcase=showcase.resolve(strict=True)
    shutil.copytree(showcase/'assets',output/'assets/showcase')
    shutil.copytree(showcase/'assets-src',output/'provenance/showcase/assets-src')
    # Keep the selected lift record when an audition lives outside assets-src.
    records=output/'provenance/showcase/selected-records';records.mkdir(parents=True)
    for record in (showcase/'out').glob('*.lift.json'):
        shutil.copy2(record,records/record.name)

    # Experimental generation evidence stays separate from Forge's reconstructed sidecar.
    pixal_trial=pixal_trial.resolve(strict=True)
    evidence=output/'provenance/pixal3d';evidence.mkdir()
    for name in ('experiment.json','review.json','budget-60000.glb','views-60000-cull-on.png'):
        shutil.copy2(pixal_trial/'results/pixal-budget1'/name,evidence/name)
    shutil.copytree(pixal_trial/'results/pixal-budget1-adapter',evidence/'adapter')
    shutil.copy2(pixal_trial/'upstream/LICENSE',evidence/'PIXAL3D-LICENSE')
    shutil.copy2(showcase/'out/props/relay_drone-pixal60k.prop.json',evidence/'relay_drone-pixal60k.prop.json')
    for name,run,adapter in [('cargo','pixal-cargo3','polish-cargo3-adapter'),('station','pixal-station1','polish-props-adapter')]:
        if not (showcase/f'assets/models/relay_{name}.glb').exists():
            continue
        destination=evidence/name;destination.mkdir()
        for filename in ('experiment.json','review.json','budget-60000.glb','views.png'):
            shutil.copy2(pixal_trial/'results'/run/filename,destination/filename)
        shutil.copytree(pixal_trial/'results'/adapter,destination/'adapter')
        shutil.copy2(showcase/f'out/props/relay_{name}-aligned-views.png',destination/'aligned-views.png')
        for suffix in ('prop.json','glb'):
            shutil.copy2(showcase/f'out/props/relay_{name}-pixal60k-aligned.{suffix}',destination/f'normalized.{suffix}')
    shutil.copy2(ROOT.parents[1]/'python/forge_gen/blender/prop.py',evidence/'prop-normalizer.py')
    shutil.copy2(ROOT/'EXPERIMENTAL-ASSETS.md',output/'EXPERIMENTAL-ASSETS.md')
    performance=output/'provenance/performance-assets';performance.mkdir()
    shutil.copytree(showcase/'out/keyed/relay-reload-s22',performance/'reload-keyed')
    shutil.copytree(ROOT.parents[1]/'out/sweeps/relay-reload-s21',performance/'reload-text-auditions')
    shutil.copy2(ROOT.parents[1]/'assets-src/takes/scrapyard_rifle_aim.npz',performance/'reload-base-aim.npz')
    shutil.copy2(ROOT.parents[1]/'assets/clips/scrapyard_rifle_aim.json',performance/'reload-base-aim.clip.json')
    shutil.copy2(showcase/'out/sheets/relay_reload-body.png',performance/'reload-body.png')
    shutil.copytree(showcase/'out/audio/voice',performance/'announcer-auditions')
    shutil.copy2(showcase/'out/relay-performance-review.json',performance/'review.json')
    shutil.copytree(showcase/'out/audio/sfx',performance/'sfx-auditions')
    shutil.copy2(ROOT/'src/voice_fx.rs',performance/'voice_fx.rs')
    shutil.copy2(ROOT/'src/vfx.rs',performance/'vfx.rs')
    shutil.copy2(ROOT/'src/lighting.rs',performance/'lighting.rs')
    shutil.copy2(ROOT/'src/post.rs',performance/'post.rs')
    shutil.copy2(showcase/'out/relay-v13-review.json',performance/'v13-review.json')
    hanabi=next((Path.home()/'.cargo/registry/src').glob('*/bevy_hanabi-0.19.0'))
    shutil.copy2(hanabi/'LICENSE-MIT',performance/'HANABI-LICENSE-MIT')


    for name in ('delivery.json' ,'LICENSE-MIT','LICENSE-APACHE','SCRAPYARD-NOTICES.md','CONTRACT.md'):
        shutil.copy2(delivery/name,output/name)
    shutil.copy2(binary,output/'relay-runner')
    shutil.copy2(ROOT/'README.md',output/'README.md')
    fonts=output/'assets/fonts';fonts.mkdir()
    for name in ('Lato-Regular.ttf','Lato-Bold.ttf'):
        shutil.copy2(Path('/usr/share/fonts/truetype/lato')/name,fonts/name)
    shutil.copy2('/usr/share/doc/fonts-lato/copyright',fonts/'LICENSE.txt')
    shutil.copy2(ROOT/'PLAY.sh',output/'PLAY.sh')
    shutil.copy2(ROOT/'desktop-audio.conf',output/'desktop-audio.conf')
    (output/'PLAY.sh').chmod(0o755)
    files={p.relative_to(output).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(output.rglob('*')) if p.is_file()}
    (output/'game-delivery.json').write_text(json.dumps({'name':'Relay Run','status':'playable prototype','source_delivery_sha256':hashlib.sha256((delivery/'delivery.json').read_bytes()).hexdigest(),'files':files},indent=2)+'\n')
    print(output)

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    for name in ('delivery','binary','output','showcase','pixal-trial'):p.add_argument('--'+name,type=Path,required=True)
    a=p.parse_args();package(a.delivery,a.binary,a.output,a.showcase,a.pixal_trial)
