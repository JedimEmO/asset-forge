# Decisions

The lessons ledger. Judgement calls recorded with their reasons so nobody
re-litigates them by accident; where this file and a spec disagree, this
file wins. One entry per lesson: the decision, the story that earned it,
the date it was learned. Most were learned in the repository this one was
distilled from, and keep their dates.

**The reference image is input.** A reference PNG is brought, not made:
the record claims its integrity (sha256 + a row in `assets-src/SOURCES.md`)
and never its regeneration. **Why:** the image came from a model with no
seed, no checkpoint hash and no promise to paint the same pixels twice;
everything downstream of the PNG *is* reproducible, and a record that claims
exactly that much is one nobody has to argue with. 2026-08-18.

**Look before you rig.** `forge views` on the raw lift — front, back,
sides, three head close-ups, culling off — is the step between lift and
rig, not an optional one. **Why:** a head reported hollow from behind was
chased through three re-rigs before anyone rendered the raw export; every
gate had passed it, because a gate measures what is there and the defect
was what was not. One front view underdetermines the back of a shape.
2026-08-20.

**Never repair a derived artefact by hand.** Absent geometry is fixed by
the seed, failing that by the reference; a baked clip by its recipe; a
sidecar by the writer. **Why:** a `.blend` edited by hand becomes the one
file in the chain that cannot be re-derived, and every later re-lift
silently loses the fix. 2026-08-20.

**T-pose is upstream.** The rig step measures the mesh against the frozen
rest pose and refuses anything not near it; its message names the
reference image. Weight-side compensation is banned. **Why:** weights bent
around a bad pose ship a body that deforms wrong under every clip, and the
failure is invisible until animation review. 2026-08-18.

**Thresholds about a mesh are in the mesh's units.** The dust filter drops
islands under 0.025 m across, post-normalize. **Why:** the first filter
dropped islands under a fraction of the face count; on a 24k-triangle body
a 239-face patch is one percent of the faces and a quarter of a skull, and
it deleted the back of the head. 2026-08-20.

**Keep the generator's normals; ship double-sided.** No normal
recalculation on import; materials export `doubleSided`. **Why:** a lifted
mesh is an open shell, recalculating normals on an open surface has no
outside to agree on, and a flipped patch is culled by the renderer into a
hole that was not in the geometry. The matte register costs nothing for
it. 2026-08-20.

**Detached shells ride the surface they sit on.** Every island under 2 %
of the mesh is re-weighted rigidly from the nearest weighted vertex,
resolved nearest-first. **Why:** bone heat solves per connected piece, and
a piece no bone passes through is weighted by whichever bone it happens to
see — a shoulder lamp went 100 % to a thumb and orbited the arm.
Nearest-first so a lamp on a pad borrows from the pad, not the sleeve
under it. Large rescued counts on plated bodies are the asset's shape, not
a defect; the gate is the abort on a fifth of the mesh weightless.
2026-08-22.

**One register per class.** Bodies lift at 1024³ / 25 000 vertices /
1024², rig at a 60 000-triangle budget; props at 1024³ / 6 000 / 1024².
Fidelity is an argument stated in the command so the record shows it was
chosen. **Why:** a 1 500-vertex body register melted gauntlets into cones,
fused shoulder pads into torsos and muddied the bake; one re-lift of the
*same* reference at 25 000 brought the face back, so the register was the
defect and not the drawing. Judge the register before blaming the image.
2026-08-20, settled 2026-08-22.

**Sweep the seed; commit the seed that shipped.** A lift that is wrong in
its geometry is re-lifted at other seeds at the same settings, and the
record names the one kept. **Why:** seed 42 lifted a front view into a
mask; 7 and 1234 gave closed skulls. The seed is a knob like any other
and the record is where knobs live. 2026-08-20.

**A fit check pulling against its sibling is a reference problem.** When
wrist span and arm height cannot both pass, the image's proportions are
wrong: re-proportion the reference (taller, more heads, shoulders higher),
then bring stature down. **Why:** both checks measure the same T-pose, and
an edit to the mesh or the thresholds to satisfy one breaks the other.
2026-08-22.

**Never add a parent above the hips.** No root bone, no renamed bone, no
bone inserted mid-chain; leaf bones are allowed and reported. **Why:** the
engine binds a curve by hashing the bone's full name path and reports
nothing — no warning at any log level — when the hash finds no target;
the character holds its rest pose forever, indistinguishable from a bad
export. 2026-08.

**No retargeting, by construction.** The rig profile is a strict superset
of the motion generator's skeleton — same names, same hierarchy, rest pose
preserved — so a raw take binds 27 of 27 curves with zero orphaned. **Why:**
every retargeting layer is a place for a clip to be subtly wrong in a way
no number catches; a skeleton that *is* the generator's has no such
place. 2026-08.

**`null` means unknown.** A default is never written as if it were a
measurement; a migration nulls what it cannot know rather than carry it
forward. **Why:** thirty shipped clips once recorded `seed 0, duration
4.0` because those were the writer's argparse defaults, and every one had
been promoted out of a sweep with different values. 2026-08.

**Provenance markers are never laundered.** `recorded` is written by the
generator as it generates; `reconstructed` is rebuilt after the fact;
`unknown` is a file with a hash and nothing else. A marker moves down, never
up. **Why:** the marker is the reader's instruction on how much of the
record to believe, and the only honest direction for a rebuild is less.
2026-08.

**Never inherit an unstated argument.** A recipe states every knob; nothing
is read from the sweep or from the clip the promote is about to overwrite.
**Why:** an earlier baker resolved a missing trim by inheriting it from the
sidecar of the clip it was replacing, and silently gave a new take the old
clip's cuts. 2026-08.

**One native bake, every door calls it.** A clip file is a pure function of
take, recipe and rig profile; the CLI, the MCP promote and the studio all
call the same function, and a test bakes one take through two doors and
compares bytes. **Why:** when each door had its own bake they disagreed,
and the disagreement was found in a shipped file. 2026-08.

**Sidecars are truth; the catalog is a projection.** The catalog is rebuilt
by scanning sidecars in milliseconds so it cannot go stale; the manifest
is the consumer-facing projection, committed and checked (`forge manifest
--check`). **Why:** three readers of one record format disagreed, two of
them line parsers that could not see inside a nested object. 2026-08.

**Schema numbers bump to say more, never to say less.** A record format
bumps when it must be able to say something new, because an older build
that drops an unknown field unauthors it on its next rewrite. A consumer
format bumps so an old reader says "you are behind" rather than "corrupt".
**Why:** a body entry that lost a field read as corruption to the previous
reader, when "behind" was the true thing to say. 2026-08-22.

**A stale manifest fails the consumer with a message that names nothing.**
`just manifest` then `just manifest-check` after any hand change under
`assets/`. **Why:** clips removed by hand left entries in the manifest and
every consumer-side test died with a panic that named neither the clip nor
the file. 2026-08-08.

**Clipping is a run, not a count.** Only sustained flat-topping warns.
**Why:** peak-normalising to 0 dBFS puts a sample at the rail by
construction, and counting full-scale samples flagged five of eight shipped
effects as broken. 2026-08.

**Loudness is approximate and says so.** K-weighting is a high-pass
stand-in. **Why:** it is enough to compare assets in one library and not
enough to certify a master, and the column header should not promise the
second. 2026-08.

**Judge a take on the skeleton it ships on.** The review sheet renders the
take on the real body, not a stick figure. **Why:** a diagonal cut reviewed
as a strike was a shove on the body — the stick figure had no shoulders to
show it. 2026-08.

**Prompt a grounded activity, not a geometry.** Say what the character is
doing and how many hands are on it; never describe the pose. **Why:** weak
priors give arm-waves, a pose description gives a still, and "standing
life" is not in the model — an idle must be an activity too. 2026-08.

**Select on net turn, not cumulative turn.** A turning clip is ranked by
where it ends up facing. **Why:** cumulative turn rewards a take that
wobbles. 2026-08.

**Sweep directories carry the seed and the prompt hash.** **Why:** two
prompts sharing a slug overwrote each other's takes. 2026-08.

**Renders are for eyes, not CI.** Sheets, views and turntables are excluded
from the gate; a render's exit status is its only pass/fail. **Why:** a
frame is not byte-stable across GPUs, and a golden that changes with the
driver is a test of the driver. 2026-08.

**Gates at the API surface, not in the prompt.** What an agent must not do
is a tool that does not exist. **Why:** a prompt asking an agent to behave
is a request; a missing tool is a fact. 2026-08.

**Refusals are successful frames.** A wrong name returns the list of what
does exist. **Why:** a failure costs an agent a guess; a refusal with the
alternatives costs one turn. 2026-08.

**No review queue.** Reversal of the previous repository's staging queue:
promote writes directly to the library and refuses an existing name unless
told to overwrite, echoing the old and the new recipe. **Why:** the human
is already in the loop through the agent harness that issues every
command; a second queue with its own inbox was a second story about the
same decision, and the one that went stale. 2026-08-23.

**Audio generators render to `out/`.** Nothing lands under `assets/`
without a promote. **Why:** a sound is judged from its plot and its numbers
before it ships, and a generator that writes to the library skips the
look. 2026-08-23.

**nvdiffrast is non-commercial.** The lift's texture baker carries the
NVIDIA Source Code License; its install is consent-gated with the licence
printed, `forge doctor` warns while it is present, and every lift record
names it. Replacing it is a follow-up, not a v1 item. **Why:** a licence
fact that lives only in an installer is a fact nobody reading a record can
see. 2026-08-23.

**One way to make each kind of asset.** Dormant capability with fixture-only
tests is the thing to delete. **Why:** a second path is a second set of
habits to keep correct, and the one nothing ships through is the one that
rots. 2026-08-22.

**Voices are designed, not brought.** A character's voice is
`assets-src/voices/<name>/ref.wav` spoken by MOSS-VoiceGenerator from a
description at a seed, with `voice.json` beside it; every line is cloned
from that clip by name, and `forge verify` fails a voice clip that has
neither its record nor a `SOURCES.md` row. **Why:** a reference clip nobody
owns is unshippable — a line cloned from a recording found on the net is a
line whose provenance is a person who never agreed — and the sample library
went without a voice for exactly that reason. A designed one is
reproducible from description + seed (the same seed gave the same bytes
twice on this card, though the record claims only the hash), it is the
durable source two lines a month apart are both cloned from, and its
licence is the model's (Apache-2.0). The first real `just speech` also
found that the cloner cannot open a reference by path in this env
(torchaudio → torchcodec); the inner half reads the clip itself and hands
over codes. 2026-08-23.

**Retire in phases.** Each phase ends green and committed, with the reason
written here. **Why:** a retirement that spans a red tree is one nobody can
bisect, and a reason that lives only in a commit message is one nobody
finds. 2026-08-22.

**A `just` recipe's `*flags` re-splits quoted values.** `just promote-clip
x y --note "two words"` reaches the shell as three arguments, and an
apostrophe in the value ends in `Syntax error: Unterminated quoted string`.
The skills say so and send a multi-word `--note`/`--prompt` to the `forge`
binary directly. **Why:** learned on the end-to-end run, where a note on a
re-promoted walk failed before the door was reached; a recipe that takes
free text needs a positional, the way `sweep` and `sfx` take their prompt.
2026-08-23.

**Every step reads the project's rig profile.** `forge gen rig|export|prop`
(and `motion review`) defaulted to `<toolkit>/rigs/humanoid` while
`forge rig check`, promote and the manifest read the project's
`assets-src/rigs/<rig>` named in `forge.toml`. Identical files after
`forge init`, so nothing showed; a project that edited its copy would have
rigged against one contract and been checked against another. `forge gen`
now hands the project's profile to the Python side (`FORGE_RIG_PROFILE`,
unless the user already set it). **Why:** two readers of one fact with
different defaults is the shape of every silent mismatch this ledger
records. 2026-08-23.

**A recipe never calls `just` recursively.** `just sweep` used to end in
a bare `just review "$dir"`; run from a `forge init` project as
`just --justfile <toolkit>/justfile --working-directory . sweep …` that
inner call said `error: no justfile found` after every take was written.
Recipes now call the tool (`forge gen motion review …`) directly. The same
run found `promote-audio` stopped by its own `record="${{file}}"` (the
shell saw `$out/…` under `set -u`); fixed the same day. **Why:** the
justfile's header promises the `--justfile` form, and the end-to-end run
was the first time anyone used it — a recipe that works only from the
toolkit checkout is a recipe that works only for its author. 2026-08-23.

**Not published to crates.io.** The crates stay unpublished; a game
depends on the toolkit by git or path. **Why:** two of the seven library
names (`forge_manifest`, `forge_audio`) already exist on the registry
(crates.io treats `-` and `_` as one name), and renaming them to squat
free names would trade the code's real names for a registry nobody here
needs — the toolkit ships a binary, a Python layer and a rig profile
together, and a git or path dependency is the honest shape for a thing
that is only whole as a checkout. `just publish-check` stays as the
packaging-hygiene gate (each library crate builds in isolation), not as a
release step. 2026-08-23.

**The code licence does not cover the samples.** MIT OR Apache-2.0 is
scoped to the code, in so many words, at the top of both LICENSE files and
in README §Licence; the sample assets under `assets/` and `assets-src/`
are governed by `assets-src/SOURCES.md`, summarised per kind in
`assets/LICENSE.md`. **Why:** the sample meshes' textures were baked
through nvdiffrast (non-commercial), so an unqualified MIT grant over the
repository handed a stranger files the repo's own records say may not be
sold — the licence and the records told two different stories, and the
records were right. 2026-08-23.

**SkinTokens is a skinner, not a rigger: take the weights by joint order and
keep the profile's armature.** `demo.py --use_skeleton` hands back a skeleton
that is ours in count, order and parent array but not in name (`bone_0…`) or
in position — every joint moved, 7.6 mm mean and 12.1 mm worst against a
`rest_tolerance_m` of 0.1 mm, because the checkpoint tokenises joint
positions on a 256-level grid. So the returned skeleton is discarded whole;
the per-vertex joint indices are read as **skin-order indices**, given the
name at the same index in the skin that went in, and bound to the profile's
own untouched `rig.blend`. **Why:** the raw output fails `forge rig check`
56 findings deep — 0 bones driven, 27 orphaned curves, the character frozen
in its rest pose through every clip — and the identical file after the
by-order re-attach passes 10 of 10 with the walk driving 27 of 27 and 0
unweighted vertices. Nothing about the returned skeleton is repaired or
averaged towards ours, because a rig nudged "to fit" is the one defect this
ledger already records under a different name; it is thrown away and the
contract's own rest pose is used, which is the pose every shipped clip was
baked against. 2026-08-30.

**A rigid shell wants one bone, not a blend.** SkinTokens binds each
detached island of a plated body to a single joint (a `vex_runner` pauldron
shell: LeftArm 100 %); the bone-heat ladder blended the same shell across
three (LeftArm 50 %, LeftShoulder 36 %, LeftForeArm 14 %) and it shears on
any clip that counter-rotates the shoulders. **Why:** a shell is a rigid
object in the fiction and a smooth weight field is a lie about it — the
repository already knew this, which is why `rig.py` carried a `shells_rigid`
rescue; what the spike adds is that the skinner does it natively and better,
and that the difference is visible on `pistol_shoot`, not in the numbers.
2026-08-30.

**The reference image is drawn by Qwen-Image, and the style line decided it,
not the pose.** Both candidates were given one prompt — the style guide's
line in front of one courier description — and pose-conditioned on the
profile's own rest pose through a native ControlNet, four seeds each, on the
same card the same afternoon. Qwen-Image held the arms within 0.2–1.6° of
horizontal in 4 of 4 with flat palm-down hands, and drew flat, posterized,
already-lit surfaces in 4 of 4; FLUX.1-schnell held 3.0–6.4° with splayed
palm-forward hands and drew a photograph in 4 of 4, ignoring the style line
entirely. **Why the style line outweighs the four-times-faster model:** the
style guide says in so many words that the look is enforced at the reference
and nothing downstream repaints a lifted mesh, so a model that will not take
the project's style sentence cannot serve `generate_reference` at any speed.
The licences agree with the pictures — Qwen-Image's pose ControlNet is
Apache-2.0 and trained on the model it conditions, while every maintained
FLUX pose ControlNet is FLUX.1-dev Non-Commercial and off-base for schnell —
but that was not the deciding fact, and it is recorded here so nobody
re-opens this on the assumption that it was. What the spike also showed is
that **the fit gate does not choose an image model**: both winners lifted and
passed it (reach 1.21 and 1.20 of wrist span, arm tips 22 mm and 35 mm under
the wrists), because a 6° droop is well inside what the gate tolerates. The
gate is the floor, the eye on the contact sheet is the choice. 2026-08-30.

**A background the picture is judged on is not the background the keyer
sees.** Two failures from the same afternoon, both invisible until the
picture was measured: Qwen drew a soft contact shadow under the shoes on a
seed whose background reads as flat, and the shadow's core sits further from
the backdrop than the keyer's tolerance, so it survived, joined the shoes and
lifted as **a slab under both feet** — a connected island the dust filter
cannot drop and the fit gate does not look at; and FLUX drew a cream jacket
on a light-grey ground, where the border flood reached through the sleeves'
antialiasing and punched holes out of the torso. **Why:** "flat uniform light
grey background, no floor, no shadow" in a prompt is a request, and the thing
that decides whether a reference is liftable is `mesh.py`'s border flood run
on the actual pixels. So the reference door runs that keyer on the drawn PNG
before anything is lifted — the same check `import_reference` promises a
brought image — and the backdrop value is stated *against* the character
rather than as a constant. 2026-08-30.

**A `vram_gb` is a budget, not a measurement, and half of ours were wrong by
a factor of four.** Every VRAM figure in `CLAUDE.md`, `backends/README.md`,
`hosting.md`'s co-residency table and each `backend.toml` was written from a
README, an upstream claim or a planning estimate; on 2026-08-30 a 10 Hz
`nvidia-smi` sampler finally ran over four of them. TRELLIS.2 at 1024³ is
**4.7 GB**, not the ~22 GB every table said. SkinTokens is **3.3–4.4 GB**,
not upstream's "at least 14 GB". ARDY is **15.4 GB**, which is the ~16 GB
the table said. Qwen-Image fp8 really is 23.3 GB. So the two numbers that
shaped the plan's hardware tiers — "a lift needs the whole card" and "the
skinner is tight on 16 GB" — were both fiction, and the one thing that
genuinely eats a 24 GB card is the *image model*. **Why:** an unmeasured
budget reads exactly like a measurement once it is in a table, and this one
had been quoted into `just gpu`'s sizing, into the co-residency rules and
into the lean tier's whole shape. A figure nobody sampled belongs in a
`vram_gb` field (it is a budget, and a conservative one is correct there) and
**never in prose that reads as fact**; `hosting.md` now dates each measured
peak and says which rows are still estimates. 2026-08-30.

**The lean tier lifts at 1024³ too; 512³ is a speed knob, not a memory one.**
`forge2.md`'s lean column had "512³" as what a 16 GB card must fall back to.
Measured, a 1024³ lift peaks at 4.7 GB — a 16 GB card has three quarters of
itself spare during one — while 512³ saves 16 s and costs the face: the same
reference at the same seed came back with a doughy mask instead of a brow,
nose and mouth, fingers fused into a mitt, and the magenta visor baked out
purple and the scalp orange (`out/spike/lean/views_512.png` against
`views_1024.png`; both closed, both in the T-pose, both at the same bounds).
That is the 1 500-vertex register's failure again, milder. **Why:** the tier
question is "what will not fit", and 1024³ fits; a register chosen to save
memory that was never scarce buys nothing and spends the thing this library
is judged on. What the lean tier does have to change is the *reference image*
(Qwen-Image Q4_K_M GGUF, 13.07 GB, ~15.1 GB of run against fp8's 23.3 GB, and
111 s against 113 s — Q4 costs about nothing in style, pose or time) and it
has to admit that **ARDY at 15.4 GB is marginal on a real 16 GB part**, not
comfortable. 512³ stays available as what it actually is: a faster lift for a
crowd body nobody walks up to. 2026-08-30.

**The host keeps the GGUF pack, and a pack lives in four places or nowhere.**
The lean-tier spike cloned `city96/ComfyUI-GGUF` @ `6ea2651e` into the
ComfyUI host by hand, put `gguf` and `protobuf` in its venv and pulled the
13.07 GB `qwen-image-Q4_K_M.gguf`, while `snapshot.json` said
`git_custom_nodes: {}`, `backend.toml` said `packs = []`, `install.sh` never
fetched any of it and `hosting.md`'s pins row said "custom node packs: none"
— with `workflows/reference_qwen_gguf.api.json` tracked and unable to run
without the pack. The reconciliation goes towards **keeping** it: the ledger
above already made Q4_K_M the lean tier's reference image, so the pack is
part of the host's shape and not spike residue, and a stranger following
`install.sh` must end up with a host that can run every tracked template.
**Why the shape of the fix and not just the direction:** the four statements
about a host — `[[comfy.packs]]`, `install.sh`, `snapshot.json` and the pins
row — are one fact written four times, and `snapshot.json` is the Manager's
own answer, so it was **re-fetched from `GET /v2/snapshot/get_current`, not
hand-edited**; a snapshot typed by hand is the same defect as a hand-repaired
`.blend`. `probe.py` now holds each pack's clone to its pinned commit the way
it holds ComfyUI's, and `UnetLoaderGGUF` joined its wanted-node list, so the
next drift is a doctor line rather than a `POST /prompt` failure in front of
a stranger. 2026-08-30.

**Phase 0 answers all three of its questions with a yes, and the plan stands
as written.** Recorded as one verdict because the spikes were a gate: **the
skinner is go** — SkinTokens skinned `vex_runner`'s raw lift to the profile's
own armature by joint order, bound the detached pauldron shells rigidly to
one bone each, and the result passed `forge rig check` 10 of 10 with the walk
driving 27 of 27 and 0 unweighted vertices, so Phase 2 goes ahead whole and
the bone-heat ladder is condemned rather than kept; **the reference model is
`qwen_image`** — it took the style guide's sentence and held the arms within
1.6° of horizontal in 4 of 4 where FLUX.1-schnell drew a photograph in 4 of
4, so `backends/qwen_image/` is what Phase 3 builds and the FLUX template
stays only as the losing side's evidence; **the lean tier has measurements
where it had estimates** — four rows sampled at 10 Hz on the card, 1024³ on
both tiers, and Q4_K_M GGUF as lean's only real substitution. **Why the
verdict is its own entry and not just the three findings above it:** the risk
table named "SkinTokens skin-only is a demo mode with no numbers" as the one
risk that could shrink this plan to the daemon and the references, and a
reader who finds three findings but no ruling has to re-derive the go/no-go
from them — a gate that passed should say so once, in the file that wins over
the plan. What Phase 0 did *not* answer, and does not pretend to: no number
here was taken on a real 16 GB part, and ARDY at 15.4 GB is marginal on one.
2026-08-30.

**A project says what it makes, and doctor gets a fifth word for what it
does not.** `forge.toml` grew `[make]` and `[hardware]`, and every door
downstream reads them. The lesson underneath is about the exit code:
doctor used to hold *every* backend to `ok` and exit 1 otherwise, which
meant a project that only makes props was permanently red about ACE-Step
and MOSS — and a gate that is always red is a gate nobody reads. So `off`
is not a probe result and never votes: it is `[make]` not having chosen the
kind, printed with the line that turned it off (`off — [make] music =
false`), never probed at all, which is also what makes doctor fast on a
props-only machine. **Exit 1 only while a *chosen* backend is not ok.** Two
consequences worth naming: tier `fake` chooses nothing, so a machine with
no card reads all-off and exits 0 — that is how this gate stays green on a
runner — and a `forge.toml` written before the tables reads as *every kind
chosen, tier detected*, because a silent narrowing would have taken rows
away from projects that never asked for it. 2026-08-30.

**A licence is accepted at a door, never in a file.** The receipt lives at
`$FORGE_BACKENDS_HOME/licences.json`, beside the installs, because the
install is what is licensed and one machine's acceptance covers every
project on it. It is deliberately *not* in `forge.toml`: that file is
hand-edited, and an acceptance you can type is an acceptance nobody gave.
The same reasoning refuses a bare `--yes`: `--yes nvdiffrast --yes llama3`
names what is being agreed to, and a blanket yes to a list nobody read is
exactly the thing the gate exists to prevent. On the agent's side the gate
is a *refusal*, not a prompt — `setup` cannot succeed until `accept` names
every gated id, and the refusal lists the ids and the sentence that fixes
the call, because a tool that merely asks an agent to behave is a tool that
will one day be asked to behave differently. 2026-08-30.

**The licence text an agent is shown is the notice, not the statute.**
`licences` returns each component's whole notice — the same words
`install.sh` prints and a human is asked to accept, with the operative
clause quoted verbatim (nvdiffrast's section 3.3, Llama 3's "Built with
Meta Llama 3") and the canonical URL for the rest. Not a summary, because
you cannot accept what you were not shown; and not a paraphrase of a
5,000-word licence either, because a licence text reproduced from memory is
worse than a pointer to the real one. This is the form the installers have
used since the first one, and now one table feeds both doors. 2026-08-30.

**One screen before a byte downloads.** `forge setup` prints, per chosen
kind, the backends, their disk, the total and every licence fact — then
asks once. Every figure on that screen is disk, and each carries the file
it was read out of; **no VRAM number appears there at all**, because half
this repository's `vram_gb` figures turned out to be budgets reading as
facts (2026-08-30, Phase 0), and a bill is exactly where that mistake would
be repeated. Resumability is decided by asking doctor rather than by
comparing `installed.json`'s commit to the pin: a receipt says an env was
*made*, and `ok` says the weights are there too. 2026-08-30.

**`mcp-session` runs one script twice, and that is the point.** The phase's
claim is "one tool surface, two transports, one queue"; a transport nothing
exercises ships ungated, so the same script runs over stdio and over the
daemon's streamable HTTP, and both assert on the frame text an agent would
read rather than on any internal. It is a Rust integration test against
`env!("CARGO_BIN_EXE_forge")` because rmcp is already pinned in this
workspace — a second protocol implementation in CI would be a second thing
to keep current — and because `CARGO_BIN_EXE_forge` is the binary this
build produced, with no `just` step in front of it that could hand the test
last week's `target/debug/forge`. The two negative legs (a `wait` on an
unknown job, a second promote onto a taken name) are in the same script on
purpose: refusals rot silently, and nothing else in CI reads one.
2026-08-30.

**Blender is not in the kind → backend map, and is still in the chosen
set.** The map is about generators — what *makes* the thing — and Blender
makes nothing; it normalises a prop and prepares a body's geometry. Leaving
it out of the map entirely would have let a props-only project with no
Blender read green, so it is added to the chosen set the way the comfy host
is: by what chose it, not by being a generator. Same for `comfy` itself.
2026-08-30.

**The graph client is Python; the card protocol is Rust.** Loading a
workflow, patching its knobs, POSTing it and fetching what came out is
`python/forge_gen/comfy.py`; taking the card, reading free VRAM and calling
`POST /free` is `forge_serve::card`. **Why:** `records.py` is the one writer
that knows key order, sorted free-form maps, the atomic write and the
None-means-unknown rule, and `python_records.rs` pins it and the Rust reader
to the same bytes — a Rust graph client would be a third writer of one
schema, and this ledger already records what three readers of one record
format did to each other. With the graph in Python both executors are one
mechanism (spawn `forge gen <verb>`, stream the log, hold the last JSON
line), so `just sfx` with no daemon up runs the code the daemon runs;
`run_fake` never imports the client, which is the only reason `ci-fake` is a
control for the move rather than a second thing it can break; and the spike
code had already run on the real host. The card half cannot be Python for
the opposite reason: the lock must hold with no Python process alive —
after a crash, before the first job, and inside `forge gpu`. 2026-08-30.

**The card lock is a `flock(2)`, not a pid file.** `out/serve/card.lock` is
taken exclusively by whichever door is up — the daemon's worker, or
`commands/generate.rs` when there is no daemon — and `out/serve/card.json`
beside it is only a projection for humans and for `forge gpu`, the same
relation sidecars and the manifest already have. The reason is one sentence:
**the kernel drops a flock when the holder dies, and that is the promise a
pid file cannot make.** A SIGKILLed holder, an OOM kill or a closed laptop
leaves a stale `card.json` and a free lock, so the next acquirer simply
overwrites the sidecar; the pid-file version of this leaves a lock nobody
holds and a stranger's pid that a later run will eventually signal — which
is exactly the bug `music.py`'s resident server shipped with, and which went
out with it. An in-process worker being singular is *not* the card lock: it
holds nothing against a second terminal. 2026-08-30.

**An interrupted job is never completed by inference.** When the daemon
restarts, a `running` row becomes `interrupted` with `exit: null` and stays
there — for an `env` child and a `comfy` one alike. A restarted daemon
cannot `waitpid` on a process it did not fork and cannot read a pipe that
died with its parent, so it can observe neither the exit code nor the last
JSON line; and finishing a comfy row out of `GET /history/{prompt_id}` would
drag `/view` fetching, `measure_wav` and a record write into Rust — the
second record writer this whole design refuses. `null` means unknown,
applied to a process. Whatever record the generator did manage to write is
found by `list_runs`, which is the correct authority because it reads the
file rather than remembering, and the fix is a re-run from the spec. Rows
that were `queued` or `blocked` are re-queued in submitted order instead:
they never ran and derived nothing, and the one-way rule is about a file
that exists. 2026-08-30.

**A gate that can reach a real generator is not a gate.** `just ci` ran a
real ComfyUI graph on the card on 2026-08-30, from
`crates/forge/tests/cli.rs::a_stale_daemon_json_falls_back_in_process`: the
test asserted exit 3 for `forge gen sfx`, which was true while `moss_sfx`
was a venv nobody had installed on a runner, and stopped being true the
moment `moss_sfx` became a comfy backend — because the developer's own
ComfyUI unit *is* up, so the door resolved, the graph was posted and the
card was leased inside the gate. Nothing was harmed and the run happened to
fail on a missing pip, which is how it was noticed at all. The lesson is
that **a test must never rely on a backend being absent**: absence is a
property of the machine, and this phase changed which machines have it.
`FORGE_FAKE=1` is the way to exercise a generate in CI, and it costs
nothing here — a fake job takes the queue and the lease and writes its row
like any other (`serve.md` §1.2), which is exactly what that test is about.
2026-08-30.
