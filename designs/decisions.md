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

**The compile goes, not the allocator.** MOSS-SoundEffect v2's DiT
`torch.compile`s itself, and the compiled path runs under inductor's cudagraph
trees, which call `torch._C._cuda_checkPoolLiveAllocations` — unsupported by
the cudaMallocAsync allocator the ComfyUI host runs with. Two levers close the
crash: `--disable-cuda-malloc` on the unit, or `TORCHDYNAMO_DISABLE=1` on the
unit. **Why the second:** the allocator is shared by every workflow the host
runs, and the image models' 23.3 GB peak on a 24 GB card was measured *with*
this allocator — changing it would put every reference image back into
unmeasured territory to save a compile nobody has yet seen finish (it died at
~60 s, twice). A host-wide knob is changed for the model that needs it only
when no narrower knob exists, and here one did. The value is the same one
`backends/moss_sfx` carried as an `[env]` key before the move; what changed is
that it is now a property of the host and lives in the unit file with its
reason beside it. 2026-08-30.

**A generator that cannot fail loudly must gate its own output.** The
TTS-Audio-Suite node caught its own `AttributeError`, logged it with an emoji,
returned a silent tensor and let the graph complete — so ComfyUI reported
success, `forge gen speech` printed `OK`, and a `forge_record: 2` was written
for 1.000 s of digital zeros. Every number in that record was true; the file
was worthless. `just audio` caught it one step later, which is one step too
late: the record had already been written, and a record is the thing this
repository treats as the truth. **The lesson is that `measure_wav` is not a
gate and `finish` should not say OK on a file the audio inspector would refuse
— silence, a full-scale run, a truncated tail.** The same check already exists
in `forge audio inspect`; what was missing is the call, before the record is
written, in the one place that knows the run happened. **It is there now**:
`forge_gen.audio.check_pcm` runs on the transcoded PCM in all four verbs —
the fake path included, which is what makes `ci-fake` a control for it —
holding a render to `metrics.rs`'s own three numbers (silence under 0.001,
a run of three samples at 0.999 of full scale, and a length more than a
quarter short of what was asked for), refusing with exit 5 and the
measurement in the message, and leaving no record behind. It refuses the
nine ACE-Step renders from that afternoon and passes every sound in the
library, which is the pair of facts that says it is the same gate.
2026-08-30.

**One fact, one place, both doors — and a default is where a fact goes to
die.** Five of the phase-1 audit's findings were one shape. The terminal
door submitted every `forge gen sfx` with `backend: null` while the MCP
door named `moss_sfx`, so a row said `executor: "env", need_gb: null` for a
run whose own record said `comfy`, and neither the foreign-holder block nor
the comfy free/restart/withhold ladder ever ran for it. Every door built
its queue options with `..default()`, so `[hardware] tier` never arrived
and a `--tier fake` project ran the *real* generator with `FORGE_FAKE`
unset. The release ladder compared free VRAM against a number read seconds
earlier — which an earlier job's leftovers are inside of — so 16.44 →
16.38 GB was "the card is back" with 7.3 GB of MOSS resident. `forge
setup`'s screen billed 9.5 GB in front of a 73.7 GB download because the
figures were constants beside the files that state them. **Why they are one
lesson:** in each case the fact existed, in one place, correctly — a map, a
`forge.toml`, a card, a `backend.toml` — and the second reader had a
default instead of a reference. A default is indistinguishable from an
answer at the call site and it never gets a review comment, which is why
these lived through the phase's own tests. The fixes are all the same
shape: `backend_for_verb` read by both doors, `LocalQueueOptions::for_project`
built by every door, a floor read from the card's own `vram_total`, and a
test that holds the disk table to each `backend.toml`. **And the gate
lesson underneath:** `ci-fake` and `mcp-session` both export `FORGE_FAKE=1`,
so neither could ever see the tier default — a gate that sets the thing it
is meant to prove cannot prove it, and `cli.rs` now has a leg that removes
`FORGE_FAKE` from the environment on purpose. 2026-08-30.

**A door that reads must not write, and a door that stops must not leave a
child.** Two halves of the same carelessness about who owns the state
directory. `forge jobs`, `forge job show|log` and `forge serve --status`
opened a full queue: reconciliation rewrote rows, and a `blocked` row an
ended MCP session had left came back as `queued` at the head of the FIFO,
so the next `forge gen` in that project ran a stranger's forgotten job on
the card first. And `stop` set a flag: the daemon exited with the generator
alive in its own process group, dropping `card.lock` while the card was
still held, and a later start stamped the row `interrupted` although
nothing had interrupted it. So: read verbs open the store with no worker
and reconcile nothing; only the daemon adopts rows a previous process left,
because only it will be there when they finish; a stop cancels its child by
recorded pid and waits for the terminal row before the process exits; and
`reconcile` leaves a `running` row alone while its pid is alive, with
`card_is_held` blocking the next card job behind that pid by name.
**Why cancel rather than drain:** a `forge stop` that blocks for a
four-minute render is a stop nobody believes, `^C` on a followed job has
always meant "give the card back", and the row says `cancelled` with the
note rather than a state nobody can act on. 2026-08-30.

**A blanket `--yes` at the second door undoes the gate at the first.**
`forge setup` refuses a bare `--yes` from a human — and then ran
`bash backends/comfy/install.sh --yes`, whose `confirm_license` took it as
consent to the Shakker-Labs FLUX.1-dev ControlNet under a **non-commercial**
licence that is not one of the five ids, was never on the screen and landed
in no receipt. The same call fetched 73.7 GB of image weights for a project
that makes music. An installer is handed `--yes` only when this machine's
receipt covers every id its own `confirm_license` asks about
(`BackendNeed::installer_prompts`), the comfy host is told which model
group the chosen kinds need (`--models qwen_image`, `--models none`), and
`--no-flux-controlnet` is always passed. **Why the FLUX ControlNet did not
become a sixth licence id:** an id in the table is shown on the screen of
every project whose kinds touch the comfy host, so a music project would be
asked to read a non-commercial notice for a model no kind in the map uses;
declining it at the door is the smaller and truer statement, and the group
stays fetchable by hand for the tracked template that is the losing side's
evidence. 2026-08-30.

**A server with no project is a session, not a refusal — and the tool that
makes a project is inside it.** `forge mcp` in a directory with no
`forge.toml` used to exit 2 before the handshake, with `forge init` as the
way out: a shell command, handed to the one kind of client this whole
surface exists for, which has no shell. So `init_project` — the tool whose
entire job is to make a project — was reachable only from a server already
bound to some *other* project, and the gate could not see it because
`mcp-session` ran `forge init` from a shell first and then called
`init_project` with `adopt: true`. The server now starts over a `NoQueue`
that writes nothing anywhere, serves `init_project`, `licences` (whose ids
are the toolkit's, not a project's) and a `doctor` that answers "no project
here", and refuses the other fifteen with a frame naming the first. **Why a
provisional `Project` rather than an `Option`:** every tool and every helper
takes a project, and threading an `Option` through all of them to express a
state in which almost nothing may run would put a `None` branch in fifteen
places to be got wrong once; one boolean beside an in-memory layout puts the
decision in the one place the refusal is written. A `forge.toml` that does
not *parse* is still an error, because "there is no project here" and "your
project is broken" are different answers and only one of them is fixed by
`init_project`. And `init_project` on a path this server is not serving says
so: a running server holds one project, settled at startup, and it cannot
follow. 2026-08-30.

**The MCP `setup` tool plans, gates and records; it does not install, and
its description says so.** `serve.md` §7 has it returning a job. It returns
the plan and the one command a human runs, and the honest fix for that
mismatch was the *description*, not a hurried job: the queue schedules
`forge gen` command lines and an installer is not one, so making the install
a job means a third executor shape with its own log, exit-code and cancel
story — and an answer to what a cancelled 35 GB download leaves behind.
**Why the description and not the code:** a tool whose description says
"install" and whose behaviour is "return a sentence" costs an agent a turn
and its trust; a tool that says what it does costs neither, and the job can
land later without breaking a promise anybody relied on. What the tool does
do is the half that must not be automated anyway — the licence gate — and
the acceptance it records means the human's install does not ask again.
2026-08-30.

**An unrepeatable measurement is worth more than a repaired one.** Two
transformers-5 shims were written into the TTS-Audio-Suite clone to see how
far the speech path could get; both worked, and the third result — 12.8 s of
fluent babble for a four-word line — is what said stop. They were reverted and
the clone put back at its pin, because a promoted line made through a
hand-patched checkout would carry a record naming
`packs {TTS-Audio-Suite: fab00263}` for bytes that pin cannot produce: the
one-way rule applied to a dependency. What the experiment bought is the
entry in `hosting.md` naming both API breaks by name, which is the thing a
future pin bump can be checked against. Investigate in the checkout; ship
only from the pin. 2026-08-30.

**The queue lives in the daemon, and a model host is not a scheduler.**
`forge serve` owns the FIFO, the job table and the card lease; ComfyUI owns
the graphs it runs well. The alternative was on the table and was cheaper:
ComfyUI already has a worker thread, a queue endpoint and a history, so a
toolkit that spoke only to it would have needed no daemon at all. **Why
not:** every one of this toolkit's generators would then have to be a node
pack, and the research the plan was written on says they are not — TRELLIS.2
wrappers carry the same CUDA build fight with Windows-first wheels,
SkinTokens packs are weeks old, one of three HY-Motion packs already 404s,
and ARDY has no node at all. A queue that only schedules half the work
schedules nothing: the card would be held by an `env` lift the host had
never heard of. So the properties a user actually asked for — one queue, one
card, never two generates, a job you can watch, a remote card — belong to the
process that can see both executors, and the host is demoted to what it is
good at, a place to run a graph. The consequence to hold on to is that
**ComfyUI is a backend of the daemon and never its scheduler**, so nothing in
the toolkit may depend on a node pack existing for a UX property to hold.
2026-08-30.

**A generate is a job, and both doors are clients of the same one.** `forge
gen` submits and follows a log; the MCP `generate_audio` returns a job id and
the literal `wait` call to make next. Neither runs a generator in the caller's
own process when a daemon is up. **Why:** a lift is minutes and a sweep is
longer, and a tool call that blocks for minutes is a tool call an agent's
client times out, retries and thereby double-books the card — the risk table
names it, "a generate blocks an MCP call for minutes", and jobs are what
retires it. The second reason is bigger than the first: the two doors were
racing. A terminal `just character` and an agent's `generate_mesh` are two
processes reaching for one 24 GB card, and the only way they can be made to
take turns is if the thing they both talk to is the same queue holding the
same `flock`. Jobs are the shape that makes "one queue" observable — a row
with a state, an exit, a log and a record path, which `status` and `list_runs`
read after a context reset instead of the agent remembering. And the door that
asked is the one that knows: a shell that says `FORGE_FAKE=1` gets a
placeholder even when the daemon it submitted to was started from another
terminal, because a queue option is carried by the submission and not by the
daemon's environment. 2026-08-30.

**The door for a mesh promote opens.** The first form of the MCP surface had
`promote_clip` and `promote_audio` and, deliberately, no promote for a body or
a model: a mesh was the thing an agent had to hand back to a human. Phase 2
gives it `promote_body` and `promote_model`. **Why the rule goes:** the human
was never absent — the harness that issues every one of these calls is a human
in the loop, which is the same ground the older "no review queue" decision
already stands on, and a rule that pretends otherwise only makes the agent ask
a human to type the command the agent composed. What actually protects the
library is not the doorman but the gates: the export gate (armature node,
depth, rest pose, ≤ 4 influences, a self-contained container), `forge rig
check` on the walk, and the refused taken name that must be told `overwrite`
and then echoes what it replaced. All three run inside `promote_body`, and
none of them is weaker for being called by an agent. **What stays
un-automatable is a different thing entirely** — accepting a licence, which is
why `setup`'s `accept` is an explicit argument naming ids that `licences`
returned in full, and why the DINOv3 token stops flat at a human. The eye is
the other: "look before you promote" is not a gate and never was, and it is
the reason `render_model` exists beside the promote rather than instead of it.
2026-08-30.

**Nothing measured is nothing claimed, in both directions.** The comfy release
ladder reads `/system_stats`, and when the host does not answer at all the
ladder stops at step 1: no `POST /free`, no `systemctl restart`, no lease
withheld, `vram_after_gb: null` and a line saying the host did not answer.
`forge gpu --free` clears a withholding only when it *measured* a return
against the card's own floor. **Why:** driving `forge gpu --free` at a dead
port found both halves of the same error. Unreachable had been falling through
to "the card did not come back", so a bogus URL restarted the developer's
ComfyUI unit and then withheld the lease — which would have blocked every card
job, an `env` lift that never touches the host included, until somebody ran
`forge gpu --free`, which cannot answer either while the service is down. And
the same run printed "the card is back; any withheld lease is cleared" on the
strength of no measurement at all. A failed reading is not a bad reading: it
licenses neither the alarm nor the all-clear, and the row says `null`, which
is what `null` has always meant here. 2026-08-30.

**Three venvs retire because the host is already installed, not because a
host is better.** ACE-Step, MOSS-TTS and MOSS-SoundEffect lost their own
environments, their probes and — with the ACE-Step server — a pidfile, a
`--stop-server` and a soundfile patch; they run on the ComfyUI unit now,
ACE-Step native and the three MOSS models through TTS-Audio-Suite. **Why
these three and not the rest:** the measured cost of putting the pack on the
host was zero — 58 nodes registered on the first restart with not one pip
installed and torch untouched — while TRELLIS.2, ARDY and SkinTokens each
want an interpreter, a CUDA build and patches of their own, which is exactly
the fight the `env` executor exists to keep out of one process. A backend
moves onto the host when the host already hosts it well, one backend at a
time, with the pack pinned and its snapshot committed. What the move costs is
recorded honestly rather than argued away: the pack ships no unload node at
this pin, so `unload_node` is `null` for all three, `POST /free` does nothing
for what they loaded, and `systemctl --user restart forge-comfy` is the only
lever that returns their card — measured, 4.4 s. 2026-08-30.

**A reference that passes every gate can still lift to junk, and the fit
gate cannot know.** The first in-project reference (Qwen-Image seed 42, the
courier, drawn with the style guide's "flat matte, lighting painted in,
posterized" line and nothing else from the guide) went reference → lift →
prepare → SkinTokens → export gate → rig check with every gate green — fit
reach 1.21, 55 of 55 bones weighted, 0 unweighted, the walk driving 27 of
27 with 0 orphaned — and on the walk strip the arm was a sliver stretched
to 2.8 m on a 1.80 m body, the drawn contact shadow rode the feet as a
slab, and the shins were broken strips (`out/spike/courier_qwen_check.png`
beside `out/spike/refs/courier_qwen.png`). **Why:** a posterized picture
with 25-pixel shins gives TRELLIS.2 no shading to lift volume from, and the
fit gate measures reach and arm height, never limb volume. The guide
already asks for what was missing — a baked key with occlusion painted into
the pits, an inflated skull, big hands and boots — and the prompt left it
out. Three things follow for the reference door: the style prefix carries
the guide's volume sentences, not only its texture sentence; the pose image
leaves air under the feet and the keyer runs on the drawn PNG before a lift;
and the lift gets a gate the fit gate is not — a sliver check on the
prepared mesh (limb cross-sections against the profile's bone lengths, or
the seven views read for a limb thinner than a bone), and *not* posed
bounds against stature: the same evening a good body printed 2.56 × 2.83 ×
2.52 m on the walk because a walk travels, so that number does not separate
them. The strip on
the real body is the judge of a new door, not a rest-pose sheet; the spike
read the sheet and called it fine. The same door with the guide's volume
sentences in the prompt (seed 7, `out/spike/v2/`) lifted, skinned, walked
and shot correctly the same evening — the chain was never the fault. Two
wording facts from that re-roll: "an inflated skull" draws a literal skull
in four of four, so the prefix says "a large head"; and a faint contact
shadow still passes the keyer as a detached island above the 0.025 m dust
threshold and rides a foot bone, so the keyer pre-check on the drawn PNG is
a gate, not a convenience. 2026-08-30.

**The reference image stays brought.** The first entry in this ledger said
a reference PNG is input, not output; the second-shape review argued the
other way (make it here, in the project's style, so the fit gate's
"re-proportion the reference" becomes a re-roll), and the 2026-08-30 spike
built it: Qwen-Image under pose conditioning held the T-pose in 4 of 4 and
the style line in 4 of 4. The picture that passed then lifted to a sliver,
a re-roll with volume in the prompt lifted correctly, and the sum was
weighed: two minutes of the whole 24 GB card per picture, a gate that
cannot see what matters in one, and a person's eye still needed on every
seed — the same eye that paints one in Grok in less time, at no cost to the
card. **Why:** the toolkit's value is downstream of the PNG, and a
generator that is worse than the tool the user already has is a second
path that rots. The reference comes through one door, `import_reference`,
which holds it to a stated format, keys it, pre-checks it and records its
stated source; the maintainers make the sample library's references in
Grok and say so in `SOURCES.md`. The image models, their ControlNets and
their templates leave the host; the spike's `hosting.md` entries stay as
the record of what was measured. 2026-08-30.

**"Chunky" is volume, never proportion, and the format text says the
geometry outright.** Three Grok references drawn with "stylized game
character with chunky proportions" came out five heads tall with arm
spans a third wider than their height, and the fit gate refused all
three at reach 1.63–1.78 of wrist span; "taller" as an edit did not move
the lift (1.00 wide by 0.74 tall became 1.00 by 0.74); "head-to-toe equals
fingertip-to-fingertip, seven and a half heads" did, and the knight and
the robot then walked, aimed and rolled with 27 of 27 bound. The witch —
four heads, a hat a quarter of her height — was refused five times with
her arm tips 16–23 cm under the skeleton's wrists and never will fit this
profile. **Why:** every clip plays on one frozen skeleton, so a reference
is a picture of *that* skeleton in clothes: the door's description says
span equal to height and seven heads or more, in numbers, and a body plan
that cannot say that is a second profile or a fitted skeleton, which
`forge2.md` records as Phase 2's first question. 2026-08-30.

**The fit gate is conservative, and the strip says by how much.** The
four-head witch the gate refused at "tips 16–23 cm under the wrists" was
skinned past a loosened copy of the profile as an experiment (nothing
shipped touched, `out/grok/moss_witch_v4/`), and on the walk, the pistol
and the roll she reads as a short character in a slightly oversized rig:
arms hinged a hand's breadth above her real shoulders, sleeves bunched at
the hands where the wrist bones sit past her wrists, nothing torn, legs,
robe and hat right. **Why it matters:** the gate's 0.15 m arm-height
tolerance guards against weights bent around a bad *pose*, which this was
not — it was a body *shorter* than the skeleton, and rotation curves on a
too-long bone show as a high pivot, not a tear. So the tolerance is a
budget, not a measurement of where bodies break, and the question it
begs is Phase 2's first: whether the skeleton should fit the mesh. Until
then the gate stands at 0.15 m and the strip is the judge of any body
that argues with it. 2026-08-30.

**Bone lengths belong to the body, and the skinner's weights say what
they are.** Spiked 2026-08-30 on the witch the fit gate refused: names,
hierarchy and rest rotations kept (they moved by 3.1e-6), every bone's
local translation kept in direction and scaled in length by a ratio read
from SkinTokens' own weights — the weight-product centroid of each
transition band projected onto the frozen direction, measured per
landmark run (torso, shoulder, upper arm, forearm, hand, hip, thigh,
shin, foot) because no skinner draws a line inside a run, mirrored
left/right — and she walks, aims and rolls with her shoulders 13 cm lower,
where they are, feet within 1.5 cm of the floor, 27 of 27 bound, no
retarget, no clip rebaked. **Why it holds:** a clip carries rotation
curves and one Hips track; lengths were frozen for the audit's sake, and
the audit binds to the fixture mannequin, which does not change. **What
the spike also said:** fit once — the second pass walks the torso
downhill 74 mm a time, because weights are made against the skeleton
handed in; the only door with the 0.1 mm translation rule is the
exporter, so that is the door that changes; the contact-pose overshoot
is real (a two-handed grip lands at her face) and is the price every
shared-animation game pays until an IK pass. Two more things the same spike measured, so the
gate is not written yet: the arm boundaries inside a sleeve disagree
left to right by 17–19 % on the witch and 23–24 % on `vex_runner`, so a
10 % symmetry gate refuses every body including the one that ships (the
spike ran at 25 %, stated), and the root fits 5.8 cm high on
`vex_runner` (a `motion_scale` of 1.06 for a body that already works) —
the weights are a good prior for where a limb *ends* and a poor one for
where a body's *centre* is. So: limb runs from the weights, mirrored; the
root and the shoulder line from geometry (the crotch and the lowest
vertices, the arm tube's centroid), which the T-pose makes cheap; the
symmetry tolerance set from the two bodies measured, not guessed; and
feet-on-the-ground on contact frames added to rig check. 2026-08-30.

**A bundle is a merge, not a bake.** `forge bundle` writes one
self-contained `.glb` carrying a body's skin and any number of clips as
named animations, for handing an asset to somebody who has none of this
toolkit — and it re-derives nothing. **Why the contract makes it so:** a
baked clip carries a rotation curve per bone plus one translation track on
the root, and every channel targets its bone **by name path**; every body
carries the profile's names, hierarchy and rest rotations. So re-pointing
each channel at the body's node of the same name is the whole operation,
the sampler values travel byte for byte, and the file that comes out plays
what the library holds rather than something re-computed from it — which
also means a clip driving a bone the body lacks has nowhere to point and is
refused by name, rather than binding to nothing while the engine reports
that at no log level. Two things fall out. The one number a bundle applies
is `motion_scale` on the root track alone (metres against the reference
legs, the fitted-skeleton design), never on a rotation and never on another
bone's translation — a Blender-era clip's constant translation curves are
bone rest offsets, and scaling one of those moves a skeleton. And a bundle
is a **derived artefact**: it is a pure function of body, clips, order and
scale, its `<stem>.bundle.json` hashes every input, and the fix for
anything wrong with one is the same command run again — never a hand edit
of the `.glb` or of the record, the one-way rule that already covers every
`.blend` and every baked clip. Nothing about it reaches `assets/`: no
sidecar, no catalog entry, no manifest row, because the library already
holds every input it was made from. 2026-08-30.

**A number in a design is a proposal until a picture is measured against it,
and three of the reference door's moved.** `designs/skin.md` marked four of
`ref import`'s pre-check numbers **budget** and told the implementer to pin
them from the pictures on disk. Running the shipped gates over all twenty-one
(`out/refs_grok/`, `out/spike/refs/`, `out/spike/reference_v2/`,
`assets-src/refs/`) moved three of them:

- **heads: refuse below 3.0, not 4.0.** The design chose 4.0 "because the
  four-head witch now ships". She measures **3.37**. 4.0 would have refused
  the exact body Phase 2 exists to ship. 3.0 sits 12 % under her — the same
  headroom the sliver gate ships with — and still refuses a picture whose
  arms start a third of the way down the frame.
- **retained alpha: 0.10–0.85, not 0.15–0.85.** The seventeen keyable
  references read 0.123 to 0.278. A floor of 0.15 refuses `courier_v2_42.png`
  and `courier_v2_11.png`, both of which lifted.
- **subject fill: a printed note, never a refusal.** Measured 0.634 (a prop)
  to 0.95, with a character that lifted at 0.69. The number does not separate
  good from bad, and skin.md's own rule is that a gate nobody can calibrate
  ships as a note with the lesson dated — this is the lesson.

**Why:** a design is written from what the last run remembered, and the
pictures are what actually happened. Marking a number *budget* and then
shipping it unmeasured is how a budget becomes a measurement nobody took —
the `vram_gb` lesson of this same day, one directory over. The two that stayed
budgets (the floor band, the flood-through hole) say so in the door's own
source, because on those two the *bad* side genuinely is not on disk: a drawn
floor dark enough to matter fails `mesh.keyed`'s border check before the
pre-checks see it. 2026-08-30.

**"Heads" from a silhouette is crown-to-arm-line, and the definition is the
gate.** Three estimators were tried on the same seventeen pictures. A
neck-pinch estimator — the narrowest row below the widest part of the head —
disagreed with itself by a factor of two depending on where the search
started (`ember_knight_v3.png` read 6.58 or 10.74) and returned nothing at all
on six of them, because a crown that is a hat point or an antenna gives the
search no reference width. What is stable is the row where the silhouette's
width first reaches half its widest: in a T-pose that is where the arms come
out, it exists in every picture, and it is one expression with no thresholds
inside it. So `heads` here means *height over the height of everything above
the arm line* — and it counts a hat, which is why the witch reads 3.37 rather
than the 4 an artist would say. **Why that is the right answer anyway:** the
question the gate is asking is not "what would an artist call this" but "how
much of this picture is not body", because that is what decides whether there
is a torso for a skeleton to sit in. The record says `heads` and the door's
docstring says what it measured, so nobody has to guess which of the two it
meant. 2026-08-30.

**A clipped render is fixed by a knob in the graph, never by a gain on the
file — and the knob's default is a budget until three renders exist.**
ACE-Step 1.5 turbo normalises to peak: nine renders came off the host pinned
at 0.0 dBFS with runs of 10 to 186 full-scale samples, and the clipping gate
refused every one. Two ways out were on the table and only one is inside the
one-way rule. Normalising the WAV after the fact would make a shipped file
that no record re-derives — the hand-repaired artefact the whole ledger exists
to prevent — and loosening the clipping gate would trade a true measurement
for a green light. The fix is `AudioAdjustVolume` as node `14` of the tracked
graph, `PATCH:gain_db`, its value in `params.gain_db`: the file that comes out
is a function of the recipe again. **The part that is not done:** the default
ships as a **budget** of −3, because pinning it means rendering the busiest
arrangement at −2, −3 and −4 and reading `peak_dbfs` off each, and that needs
the card. It says "budget" in the door, in the ledger and in the template's
saved value, so nobody reads −3 as a thing that was measured. 2026-08-30.

**A notice that names the wrong lever is worse than no notice, because
somebody will pull it.** `backends/moss_tts/backend.toml` said "WHAT LIFTS
THIS: a TTS-Audio-Suite pin built against transformers >= 5" for a pin,
`fab00263`, that **is** v5.8.7 — already two minor releases past the one that
moved the pack to transformers 5. Anybody acting on that sentence would have
spent an afternoon bumping a pin to itself. The correction cost nothing and no
card: the pack's own `pyproject.toml`, `CHANGELOG.md`, `engine_registry.py`,
`unified_model_interface.py` and `utils/runtimes/` are all on disk, and
together they say that MOSS has a runtime *profile* with no packages in it, no
worker, no proxy, and an explicit
`RuntimeError("Isolated runtime is not implemented for engine 'moss_tts'")`.
**Why it matters beyond this pin:** a notice is the one place a `doctor: ok`
row can be contradicted, so it is read as authority. The rule this earns is
that a notice names what was **measured** and what would be **checked next**,
and never a remedy nobody tried — and that a remedy which can be checked by
reading the dependency's source is checked before it is written down.
2026-08-30.

**A recipe that is renamed dies by name for one release.** `just rig-mesh` is
gone and `just promote-mesh` became `just promote-body`; `promote-mesh`
survives as a recipe whose whole body is
`promote-mesh became promote-body when the skinner changed; the rig step is
now prepare + skin` and an exit 1. **Why:** `just` answers an unknown recipe
with "unknown recipe", which names nothing a person can do next, and the
command line they typed is the only evidence of what they were trying to do.
It is the courtesy `backends/comfy/install.sh` already gives `--models`, and
it costs four lines. 2026-08-30.

**A gate that cannot see the thing it is named for is not a gate.** `forge rig
check` gained two findings and lost none, and each replaced a different kind of
absence. `check_rest_directions` replaces the exporter's 0.1 mm rest-translation
rule, which was the only rule in the toolkit about where a bone sits and which
refused the fitted witch with 55 problems, worst 252.33 mm — for a skeleton that
was right. It is now a **direction** rule at one degree, with the length
unchecked: a clip carries rotation curves and one root track, so a shorter bone
plays every clip correctly and a turned bone binds perfectly and animates
wrongly, which is the failure nothing else can see. Measured drift on the
spike's whole fitted skeleton was 0.0000°, so a degree is a generous ceiling on
a quantity that moved by nothing. `check_contact_feet` replaces nothing, which
is the point: rig check passed the fitted witch 10 of 10 with joints a quarter
of a metre off, because none of its checks moves when a bone changes length. It
binds the profile's reference clip, CPU-skins it frame by frame, and measures
**the planted foot's own lowest vertex** — not the mesh's, which a trailing hand
or a hem answers instead, and the spike measured the two 8 cm apart on one body.
The whole-clip figure stays beside it as a note so the two are never read as one
number. **What the implementation had to correct:** a foot is planted on the
quarter of frames it is *slowest* on, by rank rather than by any threshold in
metres — the library bakes locomotion in place, so a planted foot travels
backwards at stride speed while the root stands still, and a fraction of the
swing foot's peak called 296 of the shipped walk's 310 frames a contact. A rank
is scale-free; a threshold in m/s calls every frame of an idle a contact and
none of a sprint. 2026-08-30.

**A schema-1 body's `motion_scale` is 1.0 because that is what those bodies were
made at, not because 1.0 is the identity.** Every body in a schema-1 library was
scaled to the profile before it was skinned. The bones beside it are re-derived
from each shipped `.glb`, never copied from the contract — a body legitimately
0.09 mm off the contract is shippable under the old exporter's rule and would
fail verify's own 0.1 mm re-derivation on the first run, with no door to fix it
that is not a hand edit. 2026-08-30.

**A test that reads the developer's disk measures the disk, not the door.**
`each_mesh_tool_refuses_by_naming_the_door_that_fixes_it` asserts that
`generate_mesh` on a machine without TRELLIS.2 refuses by naming doctor. It
passed on every runner and in every worktree, and failed in the checkout that
had actually adopted TRELLIS.2 — because `Backends::discover` falls through to
the toolkit's own `backends/`, where an adopted install leaves a `.checkout` a
worktree does not carry. The fix is one line in the test helper: the project's
`backends_dir` is pinned inside its tempdir and never created, so the unit tests
see one empty machine everywhere. **Why it matters:** a green suite that depends
on what the author has installed is a suite that says "works here", and the
gates in this toolkit exist to say more than that. 2026-08-30.

**One artefact, one generator.** The reference format text has one home
(`FORMAT` + `FORMAT_AMENDMENT` in `python/forge_gen/reference.py`) and the MCP
tool's description is generated from it — but two mechanisms arrived to generate
that one copy: a `crates/forge_mcp/build.rs` that reads the constants at build
time, and a `just ref-format rust` that printed a const for somebody to paste.
Only the build step has a consumer, and only the build step cannot go stale, so
the printer was deleted and a pytest now refuses to let a `rust` renderer come
back. **Why:** "the copies are generated" is a property of the artefact, not of
the code that can generate it; two generators is one source and one fixture
waiting to drift, which is exactly what the one-home rule was written against.
`just ref-format` still prints the text and the skill's markdown block, neither
of which is checked in anywhere. 2026-08-30.

**A door that derives its own destination has to be given the directory, not the
files.** `import_reference` was submitting `--out` and `--record` to a reference
importer that takes neither: it derives the PNG's path, the record beside it and
the ledger row's key from one sources directory, so that the row it writes
always names the file it wrote. The caller now passes `--sources` and states the
two paths it knows as the job's own claim, so the queue still holds a lease on
them. **The same shape, in the other door:** `skin_body` was writing the rig
record to `out/skin/`, where `promote_body` does not look — the rigged `.blend`
and its record are source, not intermediate, and they live beside each other
under `assets-src/blender/`. Both were caught by the character loop over MCP the
moment the three doors were in one tree, which is the argument for that test
existing at all: three implementers each held a correct half of a path contract.
2026-08-30.

**A door nobody ran end to end is a door that does not run.** `forge gen
skin` — the whole Phase 2 loop behind one command — died on its first real
call, at step 4 of 5, with `vex_runner.glb would write a stem 'vex_runner.p2'
that is not [a-z0-9_]+`. The door names its second-pass intermediate
`<name>.p2.glb` and then hands it to `forge gen prepare`, whose own gate
refuses an output stem that is not a library name. Both halves are correct and
together they are a door that cannot complete for **any** body, on any tier.
The fix is one character, `_p2` for `.p2`. **Why it got that far:** every gate
that could have caught it runs on placeholders — `ci-fake` and `mcp-session`
stub the generators, so the second prepare never runs — and the spike scripts
this door replaced used their own names. A pipeline whose middle steps are
stubbed in CI is tested at its ends; the only thing that tests the middle is
running it on the card, which is what this run was for. 2026-08-31.

**Slow is not planted, and a picture said so.** `check_contact_feet` picked a
foot's contact frames as the quarter it is slowest on horizontally, and that
rule refused three of the three fitted bodies on disk. On `moss_witch_v4_fitted`
it named `LeftFoot at 2.05 s` a contact frame **8.0 cm off the floor** while
the whole-clip lowest vertex was −1.5 cm; the strip at that time
(`out/p2run/witch_t205.png`) is a foot mid-swing, heel up, toe pointed. **Why
the rank was half a rule:** a foot's horizontal speed turns around *twice* per
stride — once at touch-down and once at the apex of the swing — so the slowest
frames of an in-place clip hold both, and the second is in the air. The
existing unit test guards the mirror-image mistake (a fast, low scuff is not a
contact either), so neither rank alone is the answer and ranking by height
first breaks the test. What holds both is a **filter, not a second rank**: a
frame is a candidate only while the foot is within the gate's *own* tolerance
of the floor or below it, and the slowest quarter of those is the stance. No
new number is introduced, a sinking foot stays a candidate because it is the
defect being measured, and a foot that never nears the floor falls back to its
closest frame, which is outside tolerance by construction. The witch then
reads +4.9 cm and passes, against the +3.0 cm the fitted-skeleton spike
measured. **What the fix does not buy, recorded so nobody assumes it:** the
gate is now a sinking-foot gate with a floating-foot backstop, not a symmetric
one — a foot that touches down once and hovers the rest of the time passes.
Catching that wants stance *duration* off a contact-labelled clip, which is a
clip-side fact this body-side gate does not hold. 2026-08-31.

**`contact_foot_tolerance_m` has two millimetres of headroom over the body
that ships, and that is a measurement nobody took.** With the contact-frame
rule corrected, the five bodies on this disk read: `vex_runner` as shipped
(bone heat, contract skeleton) **−4.8 cm**, `ember_knight` (SkinTokens,
fitted) **−3.8 cm**, `moss_witch_v4_fitted` **+4.9 cm**, `vex_runner`
re-skinned and fitted **−5.4 cm**, `drow_warlock_fitted` **−7.7 cm**. The
tolerance is 5.0 cm, so the shipped sample body sits 4 % inside it and a
re-skin of the *same lift* falls 8 % outside. The number came from the spike's
"−1.5 to +3.0 cm on the fitted witch", which was the **whole-clip** lowest
vertex and not the planted foot's own — a number measured one way and spent on
a gate that asks another. **Why it is left standing anyway:** moving it now
would be loosening a gate to ship the artifact that failed it, which is the
same defect as editing a record to pass one, and the deepest of the five
numbers is not the fit's doing — `walk` is baked from ARDY with no IK and puts
a booted 1.8 m body's planted foot about 5 cm through the floor on its worst
frame whatever weights it carries. So the tolerance keeps its value, the
measurement above is the record of what it separates, and the real answer is
upstream in the clip. 2026-08-31.

**The agent's character loop has no door between `skin_body` and
`promote_body`.** Driven for real over MCP streamable HTTP — `import_reference
→ generate_mesh → wait → prepare_body → skin_body → render_model →
promote_body` — the loop stops after the skin: `skin_body`'s own `then` says
"export the body, then `promote_body`", and there is no export tool among the
twenty-five. `promote_body` takes the *exported* glb and runs the export gate
and rig check on it; handed the skinned one it refuses, correctly, with `lowest
vertex at y=-0.074 m`. `crates/forge/tests/mcp_session.rs` proves the gap by
stepping outside the protocol in the middle of its own character loop to shell
`forge gen export`. **Why it matters more than one missing verb:** the shell
path has six doors and the MCP surface has five, so an agent given only the
tools cannot ship a body it just skinned, and the test that is supposed to hold
that path green is the thing routing around it. The fix is an `export_body`
tool with `mcp-check` re-pinned; it is not made here because this run was to
find out, and finding out is what it found. 2026-08-31.

**A fake tier stubs the one door that costs no card.** `import_reference` on
tier `fake` stores the original PNG bytes, hashes them, and writes every
measurement as `null` with the note "placeholder run: the picture was stored
and hashed, but nothing about it was measured". The record is honest — `null`
means unknown, which is the rule — but it means the keyer, the span, the head
count and the contact-shadow check, the whole reason the door exists, never run
in `ci-fake` or `mcp-session`. **Why this is worth a line:** every other
generator is stubbed because it needs a GPU; this one needs numpy and a PNG.
The tier is a statement about the *card*, and a door that does not touch the
card has no reason to answer to it. 2026-08-31.

**The re-skin of `vex_runner` was refused, and the shipped body stands.** The
first thing Phase 2 promised — the sample body re-skinned through the new doors
— ran clean through prepare (fit 1.358 m against its own shoulder line at
1.361 m), skin (55 of 55 bones weighted, 0 unweighted of 24 119, `motion_scale`
1.0000) and export, and `forge rig check` refused it on the contact-feet
finding at −5.4 cm against the contract's 5.0 cm. Nothing was promoted, the
committed `.blend` was restored, and the evidence is under `out/p2run/reskin/`.
**What the strip says, which is the part a number cannot:** on `walk` and on
`pistol_shoot` the fitted SkinTokens body and the shipped bone-heat body are
indistinguishable to the eye at four and six frames, front, left and
three-quarter (`out/p2run/cmp_frame0.png`, `out/p2run/cmp_pistol.png`) — the
pauldron improvement the Phase 0 spike measured on `pistol_shoot` does not read
on a body whose skeleton was also re-fitted. So the honest summary of the
re-skin is: no visible gain, one gate lost by 4 mm, and the library unchanged.
`ember_knight` is the body that proves the doors, because it is the one that
had nowhere else to come from. 2026-08-31.

**The shoulder line comes from the weights, and the geometry anchor the
ledger promised cannot be built.** The 2026-08-30 entry above ends "the root
and the shoulder line come from geometry (the crotch and the lowest vertices,
the arm tube's centroid)". Only the root's half was implemented, and the fit
record shipped claiming `"shoulder_line": "geometry"` for a line the weights
had decided — a record stating a design's intent rather than a door's
behaviour, on the first library body carrying its own bone lengths. The claim
is now `"weights"`, which is what happens, and `fit.py` reports the geometry
beside it as a cross-check that places nothing. **Why the anchor is not being
built instead.** `fitgeom.shoulder_y` is the median height of every vertex
further out than 0.55 of the half-span, and its own source says that fraction
is "narrow enough to leave the shoulders out of it": it is the **arm tube**,
a statement about a pose, not a measurement of the joint where an arm leaves
the torso. On `ember_knight` the band starts 50.5 cm out, past the elbow at
42.2 cm, and reads 1.3871 m against a shoulder the weights place at 1.5327 m.
And a run's whole freedom in this design is **one positive scalar along the
contract's frozen direction**, because a scalar cannot rotate a vector and the
rest rotations are what every baked clip binds to. `Spine3 → Arm` runs
diagonally, so anchoring its *height* means either rotating the segment —
forbidden — or solving the scalar from the height alone, which would put
`ember_knight`'s shoulder joint 8.7 cm from the mirror plane on a body whose
fingertips are 91.7 cm out: an arm leaving the torso from inside the chest, to
satisfy a number that was never about that joint. The root can be anchored
because the root has no parent segment and so no direction to break. **What is
kept from the idea:** the gap travels in the report and in the rig record —
14.6 cm on `ember_knight` (plate armour, a pauldron the skinner reads as
shoulder), 6.5 cm on the `vex_runner` re-skin, 1.7 cm on the witch of the
original spike — printed by the door and refused by nothing, because three
bodies is not a threshold. 2026-08-31.

**The arm-sliver check ships as a note, and no number separates the bodies on
this disk.** `designs/skin.md` pinned `[fit] limb_radius_min_fraction` at
**0.22** on four bodies — the courier sliver at 0.200–0.214 against
`vex_runner` at 0.245–0.610 — and pre-authorised its own demotion: "if any
good arm lands under 0.22 the gate becomes a note in the same commit, with the
lesson dated in `decisions.md`". The fifth body measured is that clause
arriving. `moss_witch_v4` reads **0.218 / 0.160** on the upper arms and 0.872 /
0.844 on the forearms, and she walks, aims and rolls — below every arm of the
body that walked as a sliver. Two others measured in between: `courier_flux`
0.244/0.253 with forearms at 0.164/0.166, `drow_warlock` 0.549/0.545 and
0.682/0.568. So the ranking is not monotone in "is this body any good", and no
threshold separates a body that ships from one that does not. **Why she reads
that low without being thin:** she is a narrow arm inside a wide sleeve, and
her 1.04 m half-span puts the contract's upper-arm run across the sleeve
rather than through the arm — the measurement is of the run's neighbourhood,
not of a limb. The 2026-08-30 entry above, written the same day, calls
it "the sliver gate" while deriving the head count's headroom from it: the
arithmetic stands and the word does not. `prepare` prints the four ratios and
the off-axis distance beside them and refuses on neither; the one refusing gate left on the prepared
mesh is the arm-height check against the body's own shoulder line, and the
judge of volume is the strip on the real body. The demotion is stated in
`prepare.py`'s docstring, in `profile.toml`, in the `prepare_body` tool's
description, in `forge-character` and in the README, so nothing promises a
refusal that does not happen. **Do not restore the refusal without a number
that separates** — the one that would have refused the witch is the one this
entry exists to prevent. 2026-08-31.

**`export_body` is the twenty-sixth tool, and the gate that was supposed to
hold the character path green was shelling the verb that was missing.** The
entry above found the gap and left it: `skin_body` writes a `.blend`,
`promote_body` takes an exported `.glb`, and nothing on the surface turned
one into the other, so an agent with no shell could skin a body and never
ship it. `export_body` now queues `forge gen export` exactly as
`just promote-body` does, `skin_body`'s `then` names it, and `mcp-check` is
re-pinned at twenty-six. **The part worth keeping as a lesson is the test.**
`crates/forge/tests/mcp_session.rs` stepped outside the protocol in the
middle of its own character loop to run `forge gen export` in a subprocess —
and it was green, every day, while the path it claimed to hold was broken at
exactly that step. A gate that reaches for a terminal in the middle of the
loop it is proving has stopped proving that loop: it proves the terminal.
Both loops are tool calls end to end now, and the `forge rig fixture` call
that remains in the shipping half is labelled as what it is — a fixture
nobody ships, not a door with no tool. 2026-08-31.

**What a door measured belongs in the reply, not only in the log.** A real
`import_reference` over MCP came back `state: done, message: null` — no span,
no head count, no keyer numbers, no notes — and the CLI printed `record`,
`output` and `elapsed`. The 3.37-heads note that decides whether a picture
is worth a card minute reached the job log and stopped there, and an agent
has no shell to read a log with. Everything needed was already on the row:
`Job::payload` keeps the child's whole JSON last line for exactly this
reason, and its own doc says so — "the MCP frames must print the generator's
own words, not a summary of them". So the done frame now carries `reported`,
which is that object **minus** what the frame already states, plus the
door's own rendering (`_text`) as a text block; `forge gen` prints the same
two things. **A projection by subtraction, not by a list of keys:** a door
that starts measuring something new is read without `jobs.rs` changing,
which is the whole argument for keeping the last line whole in the first
place. `skin_body`'s description promised a fit table it was not sending;
the door now puts the run table, the shoulder-line cross-check and the
`motion_scale` in its payload under `fit_table`, so the promise and the
frame agree. 2026-08-31.

**A tier is a statement about the card, so the door that needs no card runs
for real on tier `fake`.** The entry above recorded that `import_reference`
stubbed itself on the fake tier and left every measurement `null` — the
keyer, the span, the head count and the contact-shadow check never running
in `ci-fake` or `mcp-session`, which is the entire reason the door exists.
`run_fake` now asks one question — are Pillow, numpy and OpenCV importable
here? — and where they are, it **is** `run`: the same key, the same
refusals, a record with `fake: false`, because nothing about that run is a
placeholder. Where they are not (a bare runner, the machine tier `fake` was
invented for) there is nothing to re-exec into, since a fake tier installs
no backend: the bytes are stored, every field is `null`, and one sentence
says so in the record **and** in the frame and the summary — an agent told
only "done" would file an unliftable reference believing it passed.
**Two things this forced, both improvements.** `ci-fake` and `mcp-session`
now *draw* a T-posed figure at 1024 instead of handing the door a 4×4 grey
square or a single transparent pixel, so what CI exercises is a picture
being measured. And the door's own tests pin the answer to that question
with a fixture instead of discovering it: an unpinned test would have taken
one path on this disk and the other on the runner, which is the
"works here" defect one directory over. 2026-08-31.

**A budget with its pinning run written down is a debt, and this one was paid
in twenty seconds of card time.** `DEFAULT_GAIN_DB = -3` shipped as a budget
naming exactly what would settle it: three renders of the busiest arrangement
at −2, −3 and −4 with `peak_dbfs` read off each. On the first hour the card was
free they were made — one prompt, one seed, 30 s of ogg — and read −1.5, −2.6
and −3.5 dBFS with no full-scale samples, against 0.0 dBFS and 1711 of them
with no node at all. The offset from the nominal gain is a constant ~0.5 dB at
all three, which is the vorbis encode's overshoot, so the knob is linear over
this range; −3 is kept because it is the one that lands inside −2 ± 1 dBFS, and
`assets/audio/music/tavern.ogg` is that render, promoted. **Why the shape
matters more than the number:** a budget that names its measurement is a
sentence somebody can act on in twenty seconds, and a budget that only says
"provisional" is one nobody can close. The two reference thresholds still
marked budget here (the floor band, the flood-through hole) name theirs the
same way — a picture with a drawn floor and one with a hole through it — and
stay budgets until such a picture exists, because inventing one to measure
against would measure the invention. 2026-08-31.

**A host that is up and holding nothing still holds the card.** The ComfyUI
unit's idle CUDA context measures **0.4 GB** — 23.9 GB free with it stopped
against 23.5 GB running — so a 1024³ TRELLIS.2 lift at a 22 GB budget goes
`systemctl --user stop forge-comfy` first and `start` after, and SkinTokens at
3.3–4.4 GB does not. **Why no lock catches it:** the card lease serialises
*jobs*, and the unit's context belongs to a process that has no job — it is
allocated at start and returned at stop, so every door that asks "is anybody
holding the card" answers honestly and still leaves the biggest generate
thinner than it looks. The rule is the one `CLAUDE.md` already states about
studio windows, with a second tenant named: before the largest generate, stop
the things that are merely *resident*, not only the things that are running.
2026-08-31.

**A door that writes into the source tree needs an inverse, and `forge ref
import` has none.** One import leaves three things behind — the PNG under
`assets-src/refs/<kind>s/`, its `.ref.json`, and a row in
`assets-src/SOURCES.md` — and there is no `forge ref remove`. In this
repository the way back is `rm` twice and `git checkout -- assets-src/SOURCES.md`,
which works only because the ledger is tracked; in a project made by `forge
init` and not under git, the only way back is **typing in the one file the door
exists to keep hands out of**. **Why it is recorded rather than fixed here:**
the missing verb is small and the rule it has to keep is not — a remove that
takes a row out of a ledger has to refuse a reference something downstream was
lifted from, or it becomes the door that orphans a `lift.json`. Until it
exists, an import of the wrong picture is undone with `--overwrite` and the
right one, which leaves no orphan and is the move the door already supports.
2026-08-31.

**Four documents promised a verify rule and no gate ran it, because the door
wrote both halves.** `CLAUDE.md`, `designs/records.md`, `designs/skin.md` and
`forge2.md` all said a reference PNG passes `forge verify` on **either** a
`.ref.json` whose output hash is that PNG **or** a `SOURCES.md` row.
`verify::refs` only ever read the row: the record half was never built, and
nobody noticed for a phase, because `forge ref import` writes the record *and*
the row, so every picture on this disk passes by the half that exists. The
consequence was not theoretical in the other direction — a PNG edited in place
after its import passed verify, since the row says nothing about bytes. The
rule is now built as it was written, mirroring the voice check it was modelled
on, and the hash is what it buys. **Why the gap survived review:** a gate is
proved by a case that needs it, and the fixture that needs this one — a record
with no row — is exactly the case the door cannot produce. When a door writes
every input to a check, the check has to be tested against a state the door
never makes, or the suite is agreeing with itself. 2026-08-31.
