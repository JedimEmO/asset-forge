"""Exercise actual X11 keyboard/mouse input; isolated software-rendered test only."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
from Xlib import X, XK, display
from Xlib.ext import xtest
from PIL import Image


def test(binary,out):
    out=out.resolve();out.mkdir(parents=True,exist_ok=False)
    with (out/'xvfb.log').open('w') as log:
        xvfb=subprocess.Popen(['Xvfb',':189','-screen','0','1440x900x24','-nolisten','tcp'],stdout=log,stderr=subprocess.STDOUT)
    game=None
    try:
        time.sleep(1);assert xvfb.poll() is None
        env=dict(os.environ,DISPLAY=':189',VK_ICD_FILENAMES='/usr/share/vulkan/icd.d/lvp_icd.json',WGPU_BACKEND='vulkan',XDG_DATA_HOME=str(out/'profile'))
        env.pop('WAYLAND_DISPLAY',None)
        with (out/'input.log').open('w') as log:
            game=subprocess.Popen([str(binary.resolve()),'--quiet','--report',str(out/'input.json')],cwd=Path.home(),env=env,stdout=log,stderr=subprocess.STDOUT)
        d=display.Display(':189');window=None
        for _ in range(100):
            for child in d.screen().root.query_tree().children:
                if child.get_wm_name()=='RELAY RUN':window=child;break
            if window:break
            assert game.poll() is None, (out/'input.log').read_text()
            time.sleep(0.1)
        assert window is not None
        window.set_input_focus(X.RevertToParent,X.CurrentTime);d.sync();time.sleep(3)
        def key(name,held=0.14):
            code=d.keysym_to_keycode(XK.string_to_keysym(name))
            xtest.fake_input(d,X.KeyPress,code);d.sync();time.sleep(held)
            xtest.fake_input(d,X.KeyRelease,code);d.sync();time.sleep(0.12)
        def snapshot():
            key('F8');time.sleep(0.2)
            rows=[json.loads(line.split('INPUT_SNAPSHOT ',1)[1]) for line in (out/'input.log').read_text().splitlines() if 'INPUT_SNAPSHOT ' in line]
            assert rows, (out/'input.log').read_text()
            return rows[-1]
        def capture(name):
            geom=window.get_geometry();raw=window.get_image(0,0,geom.width,geom.height,X.ZPixmap,0xffffffff)
            Image.frombytes('RGB',(geom.width,geom.height),raw.data,'raw','BGRX').save(out/name)
        results={}
        results['title']=snapshot();assert results['title']['phase']=='Title'
        key('Return');time.sleep(0.6);results['start']=snapshot();assert results['start']['phase']=='Playing'
        capture('started.png')
        key('d',0.5);results['strafe']=snapshot();assert results['strafe']['x']>0.5
        key('space',0.06);results['jump']=snapshot();assert results['jump']['y']>0.4
        key('q');results['nova']=snapshot();assert results['nova']['charge']<25.
        xtest.fake_input(d,X.MotionNotify,x=780,y=445);d.sync();time.sleep(0.2)
        xtest.fake_input(d,X.ButtonPress,1);d.sync();time.sleep(0.5)
        xtest.fake_input(d,X.ButtonRelease,1);d.sync();results['fire']=snapshot();assert results['fire']['shots']>0
        key('Shift_L');results['dash']=snapshot();assert results['dash']['dash_cd']>0
        key('Escape');results['pause']=snapshot();assert results['pause']['phase']=='Paused'
        capture('paused.png');time.sleep(0.6);results['paused_later']=snapshot();assert results['pause']['distance']==results['paused_later']['distance']
        key('Return');time.sleep(0.4);results['resume']=snapshot();assert results['resume']['phase']=='Playing' and results['resume']['distance']>results['pause']['distance']
        (out/'result.json').write_text(json.dumps(results,indent=2)+'\n')
        print(json.dumps(results,indent=2))
    finally:
        if game is not None and game.poll() is None:game.terminate();game.wait()
        xvfb.terminate();xvfb.wait()

if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--binary',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args();test(a.binary,a.out)
