---
name: wave
description: "Multi-task wave runner. Invoked as $wave <task list>. Groups tasks into pipelines."
---

Read `.claude/commands/wave.md` in full — it is your complete role manual. Follow it verbatim.

**Argument:** task list or wave description.

## Codex-only differences

- No `Skill(...)` calls — spawn child agents via Codex Teams `Agent(role, "...")`.
- Git work: execute gitter.md phases inline via bash git commands.
