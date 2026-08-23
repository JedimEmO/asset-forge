# Style guide — template

Copy this file to `designs/style.md` in your project and fill it in. It is
the art direction every reference image is judged against, and it exists
because a look that is never chosen is residue: whatever the image model
drew first, whatever material the importer handed a materialless mesh,
whatever proportions "read as human". Choose, write it down, and judge every
picture against it before a GPU minute is spent.

Everything here that is a number is **an example** — the register one
project settled on after two passes, kept so the tables are not empty.
Replace the values; keep the shape of the document.

## Where the style is enforced

**At the reference image, by eye.** A character or a prop is lifted from one
committed picture (`assets-src/refs/{characters,props}/<name>.png`), and
every rule below is a rule about what that picture must show. Nothing
downstream repaints, re-proportions or re-lights a lifted mesh: the tools
force one matte material (`[material]` in the rig profile) and otherwise
ship what the lift produced. A character that is off register is fixed in
the image and lifted again; a mesh is never edited to meet the style.

The pipeline's own gates (`forge gen rig`'s fit check, `forge rig check`)
measure only what the rig needs. They do not judge style and never will;
that is a human looking at a reference, a seven-view sheet, a walk sheet
and the studio.

## Hard constraints inherited from the rig

These are not style choices; they are the profile, and the style has to
live inside them.

- **The skeleton is frozen.** The profile's rest pose (a T-pose at a
  reference stature — 1.80 m in the shipped humanoid) is what every clip
  is baked against. Style comes from *flesh around unchanged bones*: an
  inflated skull, big hands and boots on a body whose arm span is roughly
  its height. The auto-rig refuses a mesh whose arms do not reach the
  skeleton's wrists (reach 0.80–1.45 of wrist span, arm tips within 0.15 m
  of wrist height) and names the image as the fix. A headdress that is a
  tenth of the figure's height is paid for with `--stature`, not by
  squashing the body.
- **One painted diffuse per mesh, one matte material.** The texture ships
  as the lift baked it, under the profile's `metallic` and `roughness`
  (0.0 / 0.9 shipped), normal and metallic-roughness maps stripped. The
  lighting is painted into the diffuse and nothing argues with it.
- **Dressed in the image.** There is no wardrobe: an outfit is painted into
  the reference and lifted with it; equipment (a sword, a pistol) is a
  separate prop that rides a socket. A character is never judged naked, so
  neither is a reference.
- **The register, not the image, is the first suspect.** Bodies lift at
  1024³ / 25 000 vertices / 1024²; props at 1024³ / 6 000 / 1024². A melted
  hand or a fused shoulder pad at a lower register is the register's
  fault; re-lift the same picture at the right one before redrawing it.

## Proportion targets (example register)

Head height (HH) = chin to crown. Stature is the profile's; stylization is
a bigger head, never a shorter body. Judged on the reference, in the
picture, before the lift. The third column is how to say it to whoever or
whatever makes the image — an artist, a prompt — because "stylized" means
nothing and "about six head heights" means one thing.

| Metric | Target (example) | How to phrase it for the image |
|---|---|---|
| Effective head heights | **6.0** (±0.3) | "about six head heights tall" — image models draw squat by default; say the number |
| Shoulder span / head width | ~2.1 (a heavy: 2.4–2.6; a gaunt figure: 1.8–1.9) | "shoulders just over two heads wide" |
| Shoulders vs hips (span ratio) | ~1.25 (a heavy: 1.45–1.55) | "shoulders clearly wider than hips" |
| Forearm bulk / upper-arm bulk | ≥ 0.85 | "tube arms, no taper" |
| Hand length / face height | **0.95–1.20** | "hands as big as the face" — the mitt honestly outsizes the face; hands are the signature |
| Foot length | ≥ hand length | "boots as long as the hands" |
| Neck | short thick spacer, mostly buried in collar or trapezius | "almost no neck" — never a beauty line |

Limb rule: **cylinders, not tapers.** Mass sits proximal and distal (thigh,
boot flare, bracer flare), never pinched mid-limb. Taper ≤ 10–15 % per
segment; boots flare back out to 1.1–1.3× the ankle.

## The 80-pixel silhouette test

Rendered at ~80 px tall — a contact-sheet cell, a distant enemy — a body
must still read as **four blobs**: head, shoulder span, hands, boots. A
stick figure fails. A figure that passes at 80 px has the proportions
above; one that needs 400 px to read has a real head on a real neck and
belongs to a different game. Run it on the reference before lifting and on
`forge views` after.

## Textures must already look lit in a flat viewer

Engine light is seasoning, not the meal. The reference is **already lit**:
a baked top-left key, ambient occlusion painted into the pits (armpit,
crotch, neck hollow, under the jaw, under a pauldron), cloth folds and trim
painted, a painted face. Value contrast is mandatory: lightest highlight
versus deepest recess **ΔV ≥ 0.45**, in posterized steps rather than
airbrush. "Flat matte painted texture with the lighting painted in,
posterized colour steps" belongs in every prompt for this reason. Material:
matte, sRGB texture, 3–5 hues per character.

Check it where it will be seen: open the lift in `forge views` or the
studio. If the mesh looks flat there, the picture was flat; repaint the
picture.

## Palette bands (example)

HSV, as read off the screen:

| Surface | S | V | Note |
|---|---|---|---|
| skin | 0.35–0.55 | 0.55–0.75 | warm, never plastic pink |
| primary cloth | 0.55–0.85 | mid | |
| leather | 0.35–0.55 | 0.25–0.45 | |
| metal | 0.10–0.30 | — | tinted, never pure grey; a painted specular stroke |
| accents | 0.8+ | — | sparse; a second register can spend the whole accent budget on one hue (neon, say) and keep the rest of the bands |

## Face and head rules

Faces are texture posters on a skull block:

- **Eyes, brows, mouth and nostrils are paint**, not geometry. Painted eyes
  with a specular dot, iris ~0.20 of face height, eye line ~50 % of face
  height. Low-contrast sclera — bright white eyeballs read undead.
- **The face is most of the head.** Hairline-to-chin ~60–70 % of head
  height with a real jaw; a giant bald cranium with features crowded at
  the chin is the named failure mode. One hard brow ledge; block jaw.
- **Hair is geometry mass, not scalp paint.** Every character wears hair
  (or a hood, a helmet, a crown) that shapes the silhouette.
- **The back of the head exists.** A single front view underdetermines the
  skull and a lift can leave the rear absent; `forge views` with culling
  off is the check and a new seed is the fix — never a patch in Blender.

## Density is not a style lever

Triangles earn their keep in **silhouette breakers** — boot tops, cuffs, the
jaw, the brow ledge — not in knuckles or pores. That is what a painted
reference with big simple shapes gives the lift to work with, and it holds
at any register. The register is fixed per class (above) and chosen for
what the lift can reproduce, not for the look; dropping it to enforce
chunkiness melted hands into cones and fused shoulders into torsos. Shape
the silhouette in the picture; leave the vertex count alone.

## Proof set

Name the shipped bodies that *are* the style — one per corner of the
register — so a new reference is judged beside something and not against
a paragraph. In the example register: a footman-grade base in the fantasy
look, the same proportions in a second look, a heavy, a gaunt figure, all
inside the tables above. List yours here with the reference PNG each was
lifted from:

| Body | Corner | Reference |
|---|---|---|
| `<name>` | base | `assets-src/refs/characters/<name>.png` |
| `<name>` | heavy | |
| `<name>` | gaunt | |

The sample library's `vex_runner` is a body in one project's register and
nothing more; it proves the tools, not a style.
