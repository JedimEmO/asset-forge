"""Verify delivery integrity and consumer inputs without a display or generator."""
import hashlib
import json
from pathlib import Path
import struct
import sys


def read(path):
    data=path.read_bytes()
    assert data[:4]==b'glTF' and struct.unpack_from('<I',data,8)[0]==len(data), path
    n=struct.unpack_from('<I',data,12)[0]
    return json.loads(data[20:20+n])


def verify(root):
    receipt=json.loads((root/'delivery.json').read_text())
    for name, sha in receipt['files'].items():
        path=(root/name).resolve()
        assert path.is_relative_to(root.resolve()), name
        assert hashlib.sha256(path.read_bytes()).hexdigest()==sha, name
    assets=root/'assets'
    config=json.loads((assets/'fixture.json').read_text())
    grounding=json.loads((assets/'grounding.json').read_text())
    for actor in config['actors']:
        manifest=json.loads((assets/actor['namespace']/'library.json').read_text())
        assert manifest['schema']==2
        for kind in ('bodies','models','clips','audio'):
            for entry in manifest[kind]:
                assert 'sha256:'+hashlib.sha256((assets/actor['namespace']/entry['path']).read_bytes()).hexdigest()==entry['sha256']
        doc=read(assets/actor['bundle'])
        assert len(doc['skins'])==1
        assert all('uri' not in i for i in doc.get('images',[])+doc['buffers'])
        assert [a['name'] for a in doc['animations']]==[c['animation'] for c in actor['clips']]
        record=json.loads((assets/actor['bundle']).with_suffix('.bundle.json').read_text())
        assert record['motion_scale']==actor['motion_scale']
        for clip in actor['clips']:
            entry=next(c for c in manifest['clips'] if c['name']==clip['library'])
            assert entry['duration_s']>0
            assert all(0<=e['time_s']<=entry['duration_s'] for e in entry['events'])
            row=next(c for c in grounding['clips'] if c['animation']==clip['animation'])
            assert len(row['samples'])>2
            assert all(s['minimum_foot_y_m']+s['vertical_offset_m']>=-0.000002 for s in row['samples'])
    vfx=json.loads((assets/'vfx/vfx.json').read_text())
    for effect in vfx['effects']:
        assert effect['alpha']=='straight' and effect['order']=='row-major'
        assert 0<effect['frames']<=effect['grid'][0]*effect['grid'][1] and effect['fps']>0
        assert all(0<=v<=1 for v in effect['pivot_normalized_top_left'])
        assert hashlib.sha256((assets/'vfx'/effect['path']).read_bytes()).hexdigest()==effect['sha256']
    print(f"Delivery verified: {len(receipt['files'])} files, six named animations, grounding, VFX, unchanged library hashes")



def float_accessor(path, index):
    data=path.read_bytes()
    doc=read(path)
    a=doc['accessors'][index]
    assert a['componentType']==5126 and 'sparse' not in a
    view=doc['bufferViews'][a['bufferView']]
    width={'SCALAR':1,'VEC3':3,'VEC4':4}[a['type']]
    start=20+struct.unpack_from('<I',data,12)[0]+8+view.get('byteOffset',0)+a.get('byteOffset',0)
    stride=view.get('byteStride',width*4)
    return [struct.unpack_from('<'+'f'*width,data,start+i*stride) for i in range(a['count'])]


def verify_scale(assets):
    bundle=assets/'scale-probe.glb'
    record=json.loads(bundle.with_suffix('.bundle.json').read_text())
    scale=record['motion_scale']
    assert 0<scale<1, 'probe must exercise a non-identity body motion scale'
    source=assets/'scavenger'/Path(record['clips'][0]['path']).relative_to('assets')
    src,dst=read(source),read(bundle)
    before,after=src['animations'][0],dst['animations'][0]
    channels={(src['nodes'][c['target']['node']]['name'],c['target']['path']):c for c in before['channels']}
    scaled=0
    for c in after['channels']:
        name=dst['nodes'][c['target']['node']]['name']; kind=c['target']['path']
        old=channels[(name,kind)]
        a=float_accessor(source,before['samplers'][old['sampler']]['output'])
        b=float_accessor(bundle,after['samplers'][c['sampler']]['output'])
        factor=scale if name=='Hips' and kind=='translation' else 1
        scaled+=int(factor!=1)
        assert len(a)==len(b)
        assert all(abs(x*factor-y)<2e-6 for aa,bb in zip(a,b) for x,y in zip(aa,bb)), (name,kind)
    assert scaled==1
    print(f'Non-identity scale verified: {scale}; Hips translation once, rotations unchanged')

if __name__=='__main__':
    root=Path(sys.argv[1]).resolve()
    verify(root)
    verify_scale(root/'assets')
