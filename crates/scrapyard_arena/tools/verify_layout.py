"""Capture actual Bevy layout at small and large window sizes in isolated X11."""
from pathlib import Path
import subprocess,time
from Xlib import display,X
from PIL import Image
ROOT=Path(__file__).resolve().parents[3]
OUT=ROOT/'out/scrapline'
OUT.mkdir(parents=True,exist_ok=True)
for width,height,scenario in [(800,600,'title'),(800,600,'upgrade'),(1920,1080,'upgrade'),(1920,1080,'paused')]:
    name=f'layout-{scenario}-{width}'
    with (OUT/f'{name}.log').open('w') as log:
        proc=subprocess.Popen([str(ROOT/'target/debug/scrapline'),'--scenario',scenario,'--frames','240','--screenshot',str(OUT/f'{name}.png'),'--report',str(OUT/f'{name}.json')],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT)
        d=display.Display()
        try:
            window=None
            for _ in range(100):
                for child in d.screen().root.query_tree().children:
                    if child.get_wm_name()=='SCRAPLINE // LAST SHIFT':window=child;break
                if window:break
                time.sleep(.1)
            assert window is not None
            window.configure(width=width,height=height)
            window.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync()
            proc.wait(timeout=30)
            assert proc.returncode==0
            with Image.open(OUT/f'{name}.png') as image:assert image.size==(width,height),image.size
            assert 'ERROR' not in (OUT/f'{name}.log').read_text()
            print('Captured',name,flush=True)
        finally:
            if proc.poll() is None:proc.terminate();proc.wait(timeout=5)
            d.close()
print('PASS: all requested window sizes rendered without runtime errors',flush=True)
