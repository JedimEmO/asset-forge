# Orchestrator answers (2026-08-30, late) — for implementers A, B and C

Written by the session running this workflow, in reply to implementer A's
two questions; placed in every worktree because all three need the same
answers. `designs/skin.md` (committed on forge2 as 0d8af3c) remains the
contract; this file only points at the lines that answer.

**1. The verb spellings are the design's, and A's assumption is right:**
`prepare`, `skin`, `ref-import` — see skin.md line ~92 (`forge gen
prepare`, registered in cli.py COMMANDS), ~181 (`forge gen skin`,
python/forge_gen/skin.py), ~468 (`("ref-import", "forge_gen.reference",
…)`). B owns cli.py and lands prepare/skin; C hands B the ref-import
COMMANDS row as a patch note (skin.md ~711).

**2. The character loop in mcp-session: option (b).** skin.md ~725 already
rules it: the mcp-check `expected=` and the mcp-session character loop
land LAST, in one commit. In a worktree build that means: A lands the six
tools, the 25-name pins in its own two files, and the legs that pass
today (promote_body refused-then---overwrite, promote_model, render_model,
verify). Write the full fake-tier character loop as an `#[ignore]`d test
with a one-line note saying the MERGE agent removes the ignore once B's
`prepare`/`skin` and C's `ref-import` verbs exist — a red test must not
land on any branch, and the merge stage owns cross-branch green. The
justfile's mcp-session growth is C's; the merge agent reconciles the two
pinned lists with A's crate pins.

**3. (Unasked, for symmetry.)** B: your `--fake` paths for prepare/skin are
what A's ignored loop and C's justfile loop will run; keep the placeholder
refusal rules (a fake never overwrites a real file). C: your ref-import
fake writes the PNG + ref.json + SOURCES.md row through the same door.
