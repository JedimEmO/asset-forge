"""Exercise actual X11 keyboard/mouse input against the Bevy game in Xvfb.

Run from repository root: xvfb-run -a python3 crates/scrapyard_arena/tools/verify_runtime.py
The tested window is identified by exact title, focused, and all input targets
that isolated display. Capture scenarios never modify the player's run record.
"""
from pathlib import Path
import json, subprocess, time
from Xlib import X, XK, display
from Xlib.ext import xtest

ROOT=Path(__file__).resolve().parents[3]
OUT=ROOT/'out/scrapline'
OUT.mkdir(parents=True,exist_ok=True)

def run(name,scenario,actions):
    report=OUT/f'{name}.json'
    report.unlink(missing_ok=True)
    log=(OUT/f'{name}.log').open('w')
    command=[str(ROOT/'target/debug/scrapline'),'--scenario',scenario,'--frames','900','--screenshot',str(OUT/f'{name}.png'),'--report',str(report)]
    proc=subprocess.Popen(command,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT)
    d=display.Display()
    try:
        window=None
        for _ in range(100):
            for child in d.screen().root.query_tree().children:
                if child.get_wm_name()=='SCRAPLINE // LAST SHIFT':window=child;break
            if window:break
            if proc.poll() is not None:raise RuntimeError('game exited before window appeared')
            time.sleep(.1)
        assert window is not None,'game window not found'
        window.set_input_focus(X.RevertToParent,X.CurrentTime)
        d.sync();time.sleep(2.0)
        def key(name,down=True):
            code=d.keysym_to_keycode(XK.string_to_keysym(name))
            xtest.fake_input(d,X.KeyPress if down else X.KeyRelease,code);d.sync()
        def tap(name):key(name);time.sleep(.07);key(name,False)
        def move(x,y):window.warp_pointer(x,y);d.sync()
        def click(x,y):move(x,y);xtest.fake_input(d,X.ButtonPress,1);d.sync();time.sleep(.08);xtest.fake_input(d,X.ButtonRelease,1);d.sync()
        actions(key,tap,move,click)
        proc.wait(timeout=35)
        assert proc.returncode==0,f'game failed: {proc.returncode}'
        result=json.loads(report.read_text())
        text=(OUT/f'{name}.log').read_text()
        assert 'panicked at' not in text and 'ERROR' not in text,text
        print(name,json.dumps(result),flush=True)
        return result
    finally:
        if proc.poll() is None:proc.terminate();proc.wait(timeout=5)
        d.close();log.close()

def movement(key,tap,move,click):
    tap('Return');time.sleep(.3)
    move(930,355)
    key('d');time.sleep(.45);tap('space');time.sleep(.45);key('d',False)
    click(930,355)
    time.sleep(.3)
    # Pause freezes the measurable position; resume and pause again exercises both transitions.
    tap('Escape');time.sleep(.3);tap('Return');time.sleep(.2);tap('Escape')

r=run('input-movement','title',movement)
assert r['phase']=='Paused',r
assert r['player_pos'][0]>6.5 and abs(r['player_pos'][1])<.5,r
assert r['aim'][0]>.8,r

def upgrade(key,tap,move,click):
    click(640,400);time.sleep(.3);tap('Escape')
r=run('input-upgrade','upgrade',upgrade)
assert r['phase']=='Paused',r
assert any(u['upgrade']=='Barrier cell' and u['rank']==1 for u in r['build']),r
assert r['bullets']==0,'upgrade click must not leak into firing'
print('PASS: native start, movement, mouse aim, dash, pause/resume, and upgrade click',flush=True)
