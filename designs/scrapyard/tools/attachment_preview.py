"""Build a consumer attachment preview from unchanged library inputs.

This composes a static prop under a named socket. It never overwrites a library
asset. The adjacent recipe records input hashes and the explicit local offset.
"""
import argparse, hashlib, json, math, struct
from pathlib import Path

def read(path):
    b=Path(path).read_bytes()
    assert b[:4]==b"glTF" and struct.unpack_from("<I",b,8)[0]==len(b)
    chunks={}; off=12
    while off<len(b):
        n,t=struct.unpack_from("<II",b,off);chunks[t]=b[off+8:off+8+n];off+=8+n
    return json.loads(chunks[0x4e4f534a]),chunks[0x004e4942]

def build(body,prop,sockets,socket_name,out,translation,rotation):
    assert all(math.isfinite(x) for x in translation+rotation)
    assert abs(sum(x*x for x in rotation)-1)<1e-5,"Rotation must be normalized"
    assert Path(out).resolve() not in (Path(body).resolve(),Path(prop).resolve()),"Preview must not replace an input"
    d,b=read(body); p,pb=read(prop)
    assert not p.get("skins") and not p.get("animations")
    assert not p.get("extensionsRequired"),"Unsupported required prop extension"
    kinds=("bufferViews","accessors","images","samplers","textures","materials","meshes","nodes")
    offsets={k:len(d.get(k,[])) for k in kinds}
    b+=b"\0"*((-len(b))%4);byte_offset=len(b)
    for v in p.get("bufferViews",[]): v["byteOffset"]=v.get("byteOffset",0)+byte_offset;v["buffer"]=0
    for a in p.get("accessors",[]):
        assert "sparse" not in a
        if "bufferView" in a:a["bufferView"]+=offsets["bufferViews"]
    for i in p.get("images",[]):
        assert "uri" not in i
        i["bufferView"]+=offsets["bufferViews"]
    for t in p.get("textures",[]):
        if "source" in t:t["source"]+=offsets["images"]
        if "sampler" in t:t["sampler"]+=offsets["samplers"]
    def textures(obj):
        if isinstance(obj,dict):
            for k,v in obj.items():
                if k.endswith("Texture") and isinstance(v,dict) and "index" in v:v["index"]+=offsets["textures"]
                else:textures(v)
        elif isinstance(obj,list):
            for v in obj:textures(v)
    for m in p.get("materials",[]): textures(m)
    for m in p.get("meshes",[]):
        for q in m["primitives"]:
            q["attributes"]={k:v+offsets["accessors"] for k,v in q["attributes"].items()}
            if "indices" in q:q["indices"]+=offsets["accessors"]
            if "material" in q:q["material"]+=offsets["materials"]
            assert not q.get("targets")
    for n in p["nodes"]:
        if "mesh" in n:n["mesh"]+=offsets["meshes"]
        if "children" in n:n["children"]=[x+offsets["nodes"] for x in n["children"]]
        n["name"]="AttachedRifle_"+n.get("name","Node")
    socket=next(s for s in json.loads(Path(sockets).read_text())["sockets"] if s["name"]==socket_name)
    bone=next(n for n in d["nodes"] if n.get("name")==socket["bone"])
    for k in kinds:d.setdefault(k,[]).extend(p.get(k,[]))
    sid=len(d["nodes"]);cid=sid+1
    bone.setdefault("children",[]).append(sid)
    d["nodes"].append({"name":"PreviewSocket","translation":socket["translation"],"rotation":socket["rotation"],"children":[cid]})
    d["nodes"].append({"name":"RifleAttachmentCorrection","translation":translation,"rotation":rotation,"children":[x+offsets["nodes"] for x in p["scenes"][p.get("scene",0)]["nodes"]]})
    b+=pb;d["buffers"]=[{"byteLength":len(b)}]
    d["asset"]["generator"]="scrapyard consumer attachment recipe v1"
    d["extensionsUsed"]=sorted(set(d.get("extensionsUsed",[])+p.get("extensionsUsed",[])))
    j=json.dumps(d,separators=(",",":")).encode();j+=b" "*((-len(j))%4);b+=b"\0"*((-len(b))%4)
    result=struct.pack("<III",0x46546c67,2,28+len(j)+len(b))+struct.pack("<II",len(j),0x4e4f534a)+j+struct.pack("<II",len(b),0x004e4942)+b
    out=Path(out);out.parent.mkdir(parents=True,exist_ok=True);out.write_bytes(result)
    record={"purpose":"Consumer socket attachment review, not a replacement library body","inputs":[{"path":str(x),"sha256":hashlib.sha256(Path(x).read_bytes()).hexdigest()} for x in (body,prop,sockets)],"socket":socket_name,"translation":translation,"rotation_xyzw":rotation,"output_sha256":hashlib.sha256(result).hexdigest()}
    out.with_suffix(".attachment.json").write_text(json.dumps(record,indent=2)+"\n")

if __name__=="__main__":
    a=argparse.ArgumentParser();a.add_argument("--body",required=True);a.add_argument("--prop",required=True);a.add_argument("--sockets",default="rigs/humanoid/sockets.json");a.add_argument("--socket",default="hand_r");a.add_argument("--out",required=True);a.add_argument("--config");a.add_argument("--translation",nargs=3,type=float,default=[0,0,0]);a.add_argument("--rotation",nargs=4,type=float,default=[0,0,0,1]);v=a.parse_args()
    if v.config:
        c=json.loads(Path(v.config).read_text());v.socket=c["socket"];v.translation=c["translation"];v.rotation=c["rotation_xyzw"]
    build(v.body,v.prop,v.sockets,v.socket,v.out,v.translation,v.rotation)
