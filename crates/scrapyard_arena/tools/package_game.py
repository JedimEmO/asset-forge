"""Build a relocatable Linux game folder from reviewed runtime assets.

The source checkout stays unchanged. Run from anywhere with Python3; the script
builds the game, copies the exact consumer inputs, and records SHA256 hashes.
"""
from pathlib import Path
import hashlib,json,shutil,subprocess,tempfile

ROOT=Path(__file__).resolve().parents[3]
DEST=ROOT/'out/scrapline-linux'
subprocess.run(['cargo','build','-p','scrapyard_arena','--bin','scrapline'],cwd=ROOT,check=True)
DEST.parent.mkdir(parents=True,exist_ok=True)
OUT=Path(tempfile.mkdtemp(prefix='.scrapline-build-',dir=DEST.parent))
files=[
 'designs/scrapyard/batch-02/armed-scavenger-combat.glb',
 'designs/scrapyard/batch-02/rusher-combat.glb',
 'assets/models/scrapyard_magnet.glb',
 'designs/scrapyard/batch-02/vfx/muzzle_flash.png',
 'designs/scrapyard/batch-02/vfx/metal_impact.png',
 'designs/scrapyard/batch-02/vfx/pickup_burst.png',
 'assets/audio/music/scrapyard_combat_loop.wav',
 *[f'assets/audio/sfx/scrapyard_{name}.wav' for name in ['shot','hit','explosion','pickup','dash','upgrade','ui']],
 'LICENSE-MIT','LICENSE-APACHE',
 'assets-src/audio/scrapyard_synth.py','assets-src/audio/scrapyard_music.py',
]
for relative in files:
    source=ROOT/relative;target=OUT/relative
    target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(source,target)
shutil.copy2(ROOT/'target/debug/scrapline',OUT/'scrapline')
if shutil.which('strip'):subprocess.run(['strip','--strip-debug',str(OUT/'scrapline')],check=True)
guide=(ROOT/'crates/scrapyard_arena/README.md').read_text()
controls=guide[guide.index('| Control |'):guide.index('## Check the game')]
(OUT/'PLAYER-GUIDE.md').write_text('# SCRAPLINE // LAST SHIFT\n\nRun `./PLAY.sh` in this folder, then press Enter to start a shift.\nThe folder includes every game asset and can be moved together.\n\n'+controls+'\nSee [asset notices](ASSET-NOTICES.md) for source and license details.\n')
shutil.copy2(ROOT/'crates/scrapyard_arena/ASSET-NOTICES.md',OUT/'ASSET-NOTICES.md')
shutil.copy2(ROOT/'crates/scrapyard_arena/VERIFICATION.md',OUT/'VERIFICATION.md')
(OUT/'PLAY.sh').write_text('#!/bin/sh\nset -eu\ncd -- "$(dirname -- "$0")"\nexec ./scrapline "$@"\n')
(OUT/'PLAY.sh').chmod(0o755)
manifest={str(p.relative_to(OUT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(OUT.rglob('*')) if p.is_file() and p.name!='manifest.json'}
(OUT/'manifest.json').write_text(json.dumps({'game':'SCRAPLINE // LAST SHIFT','files':manifest},indent=2)+'\n')
if DEST.exists():
    previous=Path(tempfile.mkdtemp(prefix='scrapline-previous-',dir=DEST.parent))
    previous.rmdir()
    DEST.rename(previous)
    try:OUT.rename(DEST)
    except BaseException:
        previous.rename(DEST)
        raise
    print(f'Previous package preserved at {previous}')
else:OUT.rename(DEST)
print(f'Packaged {len(manifest)} files at {DEST}')
