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
