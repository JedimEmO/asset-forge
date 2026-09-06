"""Verify the saved combat batch as an outside consumer."""
import hashlib,json,sys
from pathlib import Path
from PIL import Image
ROOT=Path(__file__).resolve().parents[3]
sys.path.insert(0,str(ROOT/"python"))
from forge_gen.glb import verify_glb

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()

def main():
 batch=ROOT/"designs/scrapyard/batch-02"
 vfx=json.loads((batch/"vfx/vfx.json").read_text())
 for e in vfx["effects"]:
  p=batch/"vfx"/e["path"];assert sha(p)==e["sha256"],p
  im=Image.open(p);assert im.mode=="RGBA" and list(im.size)==e["size_px"]
  assert e["frames"]==4 and e["fps"]>0 and not e["loop"]
  a=im.getchannel("A").point(lambda x:255 if x>=4 else 0);w,h=im.size
  for i,expected in enumerate(e["visible_bounds_px_per_cell"]):
   x=i%2;y=i//2;box=a.crop((x*w//2,y*h//2,(x+1)*w//2,(y+1)*h//2)).getbbox()
   assert list(box)==expected and box[0]>2 and box[1]>2 and box[2]<w//2-2 and box[3]<h//2-2
 for name,count in (("scavenger-combat",3),("rusher-combat",2),("armed-scavenger-combat",3)):
  p=batch/(name+".glb");r=verify_glb(p);assert r["animations"]==count and r["skins"]==1
 a=json.loads((batch/"armed-scavenger-combat.attachment.json").read_text())
 assert sha(batch/"armed-scavenger-combat.glb")==a["output_sha256"]
 for i in a["inputs"]:assert sha(ROOT/i["path"])==i["sha256"]
 g=json.loads((batch/"grounding.json").read_text())
 for clip in g["clips"]:
  for p,digest in clip["inputs_sha256"].items():assert sha(ROOT/p)==digest,p
  times=[s["t"] for s in clip["samples"]];assert times==sorted(set(times))
  for s in clip["samples"]:assert s["vertical_offset_m"]>=0 and s["minimum_foot_y_m"]+s["vertical_offset_m"]>=-0.000002
 print("Batch verified: 3 VFX atlases / 12 contained frames, 3 self-contained character packages, unchanged attachment inputs, 5 grounding tracks.")
if __name__=="__main__":main()
