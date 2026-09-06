---
name: forge-voice
description: Design a character's voice from a description — MOSS-VoiceGenerator speaks one audition line into assets-src/voices/<name>/ref.wav with its record, judged from its plot and rerolled by seed until it is the character; every spoken line is then cloned from it by name through forge-audio's speech step. Use when the user wants a new voice for a character, says a project has no reference clip to clone, or wants a voice regenerated (another seed, a rewritten description).
---

# forge-voice

Read [the canonical procedure](../../../.agents/skills/forge-voice/SKILL.md)
in full before acting. Apply its workflow, review gates and record rules.
This entry point exists for Claude skill discovery; edit the canonical file
in `.agents/skills/` when the procedure changes.
