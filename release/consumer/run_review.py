"""Run the fixture from an unrelated cwd using software Vulkan and a private Xvfb."""
import argparse
import os
from pathlib import Path
import subprocess
import time


def run(binary, delivery, display, icd):
    delivery=delivery.resolve(); binary=binary.resolve(); icd=icd.resolve(strict=True)
    assert not (delivery/'runtime.log').exists(), 'preserve the prior review: stage a new delivery'
    with (delivery/'xvfb.log').open('x') as log:
        xvfb=subprocess.Popen(['Xvfb',display,'-screen','0','1200x800x24','-nolisten','tcp'],stdout=log,stderr=subprocess.STDOUT)
        try:
            time.sleep(1)
            if xvfb.poll() is not None: raise RuntimeError('Xvfb failed; choose an unused display')
            env=dict(os.environ,DISPLAY=display,VK_ICD_FILENAMES=str(icd),WGPU_BACKEND='vulkan')
            env.pop('WAYLAND_DISPLAY',None)
            with (delivery/'runtime.log').open('x') as output:
                result=subprocess.run([str(binary),str(delivery)],cwd=Path.home(),env=env,stdout=output,stderr=subprocess.STDOUT,timeout=600)
            result.check_returncode()
            print((delivery/'runtime.json').read_text())
        finally:
            xvfb.terminate();xvfb.wait()


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--delivery',type=Path,required=True)
    p.add_argument('--display',default=':187')
    p.add_argument('--icd',type=Path,default=Path('/usr/share/vulkan/icd.d/lvp_icd.json'))
    a=p.parse_args();run(a.binary,a.delivery,a.display,a.icd)
