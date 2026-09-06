"""Measure a consumer grounding track without editing any shipped clip or body.

Apply vertical_offset_m to the character's visual root after animation blending.
This only lifts penetrating feet; it does not pull airborne feet onto the floor.
Samples use glTF skinning and animation interpolation on the actual fitted body.
"""
import hashlib,json,sys
from pathlib import Path
import numpy as np
from scipy.spatial.transform import Rotation,Slerp


def measure(body_path,clip_path,animation_index=0):
 from forge_gen.fit import _chunks,_accessor
 d,b=_chunks(Path(body_path));c,cb=_chunks(Path(clip_path))
 nodes=d["nodes"];names={n.get("name"):i for i,n in enumerate(nodes)}
 anim=c["animations"][animation_index];channels=[];times=set()
 for ch in anim["channels"]:
  sampler=anim["samplers"][ch["sampler"]];t=_accessor(c,cb,sampler["input"]).ravel();v=_accessor(c,cb,sampler["output"])
  assert sampler.get("interpolation","LINEAR")=="LINEAR"
  channels.append((names[c["nodes"][ch["target"]["node"]]["name"]],ch["target"]["path"],t,v))
  times.update(float(x) for x in t)
 samples=[]
 for n in nodes:
  if "mesh" not in n or "skin" not in n:continue
  skin=d["skins"][n["skin"]];ibm=_accessor(d,b,skin["inverseBindMatrices"]).reshape(-1,4,4).transpose(0,2,1)
  foot=np.array([("Foot" in nodes[j].get("name","") or "Toe" in nodes[j].get("name","")) for j in skin["joints"]])
  for p in d["meshes"][n["mesh"]]["primitives"]:
   a=p["attributes"];v=_accessor(d,b,a["POSITION"]);j=_accessor(d,b,a["JOINTS_0"]).astype(int);w=_accessor(d,b,a["WEIGHTS_0"])
   selected=(foot[j]*w).sum(axis=1)>=0.5
   assert selected.any()
   samples.append((np.column_stack((v[selected],np.ones(selected.sum()))),j[selected],w[selected],skin["joints"],ibm))
 rows=[]
 times.update(float(t) for t in np.arange(0,max(times),1/60))
 for time in sorted({round(t,6) for t in times}):
  state=[dict(n) for n in nodes]
  for i,path,t,v in channels:
   if len(t)==1 or time<=t[0]:value=v[0]
   elif time>=t[-1]:value=v[-1]
   else:
    k=int(np.searchsorted(t,time))-1
    if path=="rotation":value=Slerp(t[k:k+2],Rotation.from_quat(v[k:k+2]))([time]).as_quat()[0]
    else:value=v[k]+(v[k+1]-v[k])*((time-t[k])/(t[k+1]-t[k]))
   state[i][path]=value
  world={}
  def visit(i,parent):
   n=state[i]
   if "matrix" in n:m=np.array(n["matrix"]).reshape(4,4).T
   else:
    m=np.eye(4);m[:3,:3]=Rotation.from_quat(n.get("rotation",[0,0,0,1])).as_matrix()@np.diag(n.get("scale",[1,1,1]));m[:3,3]=n.get("translation",[0,0,0])
   world[i]=parent@m
   for child in n.get("children",[]):visit(child,world[i])
  for root in d["scenes"][d.get("scene",0)]["nodes"]:visit(root,np.eye(4))
  low=float("inf")
  for v,j,w,bones,ibm in samples:
   mats=np.stack([world[i] for i in bones])@ibm
   posed=(np.einsum("nkij,nj->nki",mats[j],v)*w[:,:,None]).sum(axis=1)
   low=min(low,float(posed[:,1].min()))
  offset=max(0.0,-low)
  rows.append({"t":round(time,6),"minimum_foot_y_m":round(low,6),"vertical_offset_m":round(offset,6)})
 return {"body":body_path,"clip":clip_path,"animation":anim["name"],"max_lift_m":max(r["vertical_offset_m"] for r in rows),"samples":rows,"inputs_sha256":{p:hashlib.sha256(Path(p).read_bytes()).hexdigest() for p in (body_path,clip_path)}}

if __name__ == "__main__":
 import argparse
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument("--install",type=Path,required=True)
 parser.add_argument("--assets",type=Path,required=True)
 args=parser.parse_args()
 sys.path.insert(0,str(args.install.resolve()/"python"))
 config=json.loads((args.assets/"fixture.json").read_text())
 rows=[]
 for actor in config["actors"]:
  bundle=args.assets/actor["bundle"]
  for i,clip in enumerate(actor["clips"]):
   row=measure(str(bundle),str(bundle),i)
   row["body"]=row["clip"]=actor["bundle"]
   row["inputs_sha256"]={actor["bundle"]:hashlib.sha256(bundle.read_bytes()).hexdigest()}
   rows.append(row)
 result={"schema":1,"method":"Bundle-space linear blend skinning; vertices with at least 0.5 foot/toe weight; baked keys plus 60 Hz. Scale already applied by bundle.","clips":rows}
 (args.assets/"grounding.json").write_text(json.dumps(result,indent=2)+"\n")
