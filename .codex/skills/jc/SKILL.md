---
name: jc
description: "Live debug, diagnose & fix on main. Invoked as $jc <bug description>. Hotfixes only."
---

Read `.claude/commands/jc.md` in full — it is your complete role manual. Follow it verbatim.

**Argument:** bug description, service name, or diagnostic request.

## Codex-only differences

- When jc.md says "use gitter JC-COMMIT" — execute gitter.md JC-COMMIT phase inline via bash git commands.
- JC-COMMIT is LOCAL ONLY. Do NOT push. Ever.
