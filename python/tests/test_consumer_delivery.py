"""Consumer scale and delivery refusal checks, without a GPU or model invocation."""
import importlib.util
import json
from pathlib import Path
import struct

import pytest

ROOT=Path(__file__).resolve().parents[2]
SPEC=importlib.util.spec_from_file_location('consumer_verify',ROOT/'release/consumer/verify.py')
VERIFY=importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFY)


def glb(path, translation, rotation):
    blob=struct.pack('<7f',*translation,*rotation)
    d={'asset':{'version':'2.0'},'nodes':[{'name':'Hips'}],
       'buffers':[{'byteLength':len(blob)}],
       'bufferViews':[{'buffer':0,'byteOffset':0,'byteLength':12},{'buffer':0,'byteOffset':12,'byteLength':16}],
       'accessors':[{'bufferView':0,'componentType':5126,'type':'VEC3','count':1},{'bufferView':1,'componentType':5126,'type':'VEC4','count':1}],
       'animations':[{'channels':[{'sampler':0,'target':{'node':0,'path':'translation'}},{'sampler':1,'target':{'node':0,'path':'rotation'}}],
                      'samplers':[{'output':0},{'output':1}]}]}
    encoded=json.dumps(d).encode();encoded+=b' '*((-len(encoded))%4)
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_bytes(struct.pack('<III',0x46546c67,2,28+len(encoded)+len(blob))+struct.pack('<II',len(encoded),0x4e4f534a)+encoded+struct.pack('<II',len(blob),0x004e4942)+blob)


@pytest.mark.parametrize('translation,rotation,passes',[
    ([1,2,3],[0,0,0,1],True),
    ([0.5,1,1.5],[0,0,0,1],False), # applied twice
    ([2,4,6],[0,0,0,1],False), # not applied
    ([1,2,3],[0,1,0,0],False), # changed rotation
])
def test_nonidentity_scale_checks_actual_channels(tmp_path,translation,rotation,passes):
    glb(tmp_path/'scavenger/clips/run.glb',[2,4,6],[0,0,0,1])
    glb(tmp_path/'scale-probe.glb',translation,rotation)
    (tmp_path/'scale-probe.bundle.json').write_text(json.dumps({'motion_scale':0.5,'clips':[{'path':'assets/clips/run.glb'}]}))
    if passes: VERIFY.verify_scale(tmp_path)
    else:
        with pytest.raises(AssertionError): VERIFY.verify_scale(tmp_path)


def test_delivery_refuses_corrupt_payload_before_loading_metadata(tmp_path):
    (tmp_path/'file').write_text('changed')
    (tmp_path/'delivery.json').write_text(json.dumps({'files':{'file':'0'*64}}))
    with pytest.raises(AssertionError,match='file'): VERIFY.verify(tmp_path)


def test_delivery_refuses_path_outside_delivery(tmp_path):
    (tmp_path/'delivery.json').write_text(json.dumps({'files':{'../outside':'0'*64}}))
    with pytest.raises(AssertionError,match='outside'): VERIFY.verify(tmp_path)
