#!/usr/bin/env python3
"""Stage the reviewed consumer fixture; refuse existing destinations and preserve inputs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def stage(install, acceptance, scavenger, out):
    install, acceptance, scavenger, out = [p.resolve() for p in (install, acceptance, scavenger, out)]
    if out.exists():
        raise FileExistsError(out)
    out.mkdir(parents=True)
    env = {k: v for k, v in os.environ.items() if k not in ('FORGE_HOME', 'FORGE_TOOLKIT')}
    forge = install / 'bin/forge'
    for name in ('Cargo.toml', 'Cargo.lock', 'src', 'grounding.py', 'verify.py', 'run_review.py', 'README.md'):
        source = Path(__file__).parent / name
        if source.is_dir(): shutil.copytree(source, out / name)
        else: shutil.copy2(source, out / name)
    shutil.copy2(install / 'designs/consumer-contract.md', out / 'CONTRACT.md')
    assets = out / 'assets'
    assets.mkdir()
    actors = []
    for namespace, project, body, clips in (
        ('rusher', acceptance, 'release_rusher', ['release_idle', 'release_run', 'release_swipe']),
        ('scavenger', scavenger, 'scrapyard_scavenger', ['scrapyard_rifle_aim', 'scrapyard_rifle_run', 'scrapyard_rifle_fire']),
    ):
        # Copy complete manifests and the files they name, not a hand-edited subset.
        subprocess.run([str(forge), '--project', str(project), 'manifest', '--check'], env=env, check=True)
        shutil.copytree(project / 'assets', assets / namespace)
        shutil.copytree(project / 'assets-src', out / 'provenance' / namespace / 'assets-src')
        shutil.copy2(project / 'forge.toml', out / 'provenance' / namespace / 'forge.toml')
        bundle = assets / f'{namespace}.glb'
        subprocess.run([str(forge), '--project', str(project), 'bundle', body,
                        '--clips', ','.join(clips), '--out', str(bundle)], env=env, check=True)
        record = json.loads(bundle.with_suffix('.bundle.json').read_text())
        manifest = json.loads((assets / namespace / 'library.json').read_text())
        body_entry = next(b for b in manifest['bodies'] if b['name'] == body)
        actors.append(dict(namespace=namespace, bundle=bundle.name, body=body,
                           motion_scale=body_entry['motion_scale'],
                           clips=[dict(library=n, animation=a) for n, a in zip(clips, record['output']['animations'])]))
    subprocess.run([str(forge), '--project', str(scavenger), 'bundle', 'scrapyard_rusher',
                    '--clips', 'scrapyard_rusher_rush', '--out', str(assets / 'scale-probe.glb')], env=env, check=True)
    shutil.copytree(scavenger / 'designs/scrapyard/batch-02/vfx' , assets / 'vfx')
    shutil.copy2(scavenger / 'designs/scrapyard/batch-02/rifle-attachment.json', assets / 'attachment.json')
    shutil.copy2(scavenger / 'designs/scrapyard/batch-02/image-prompts.json', out / 'provenance/vfx-image-prompts.json')
    for name in ('LICENSE-MIT', 'LICENSE-APACHE'):
        shutil.copy2(install / name, out / name)
    shutil.copy2(scavenger / 'crates/scrapyard_arena/ASSET-NOTICES.md', out / 'SCRAPYARD-NOTICES.md')
    config = dict(schema=1, actors=actors, prop='rusher/models/magnet.glb',
                  weapon='scavenger/models/scrapyard_rifle.glb')
    (assets / 'fixture.json').write_text(json.dumps(config, indent=2)+'\n')
    subprocess.run([sys.executable, str(out / 'grounding.py'), '--install', str(install), '--assets', str(assets)], check=True)
    receipt = dict(schema=1, purpose='Consumer delivery; original manifests and provenance remain unchanged',
                   distribution_sha256=digest(install / 'distribution.json'),
                   files={p.relative_to(out).as_posix(): digest(p) for p in sorted(out.rglob('*')) if p.is_file()})
    (out / 'delivery.json').write_text(json.dumps(receipt, indent=2)+'\n')
    subprocess.run([sys.executable, str(out / 'verify.py'), str(out)], check=True)
    return out


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('install', 'acceptance', 'scavenger', 'out'):
        p.add_argument('--'+name, type=Path, required=True)
    a = p.parse_args()
    print(stage(a.install, a.acceptance, a.scavenger, a.out))
