---
name: git
description: "Gitter gateway. Invoked as $git <push|pull|freeform>."
---

Read `.claude/commands/git.md` in full — it is your complete role manual. Follow it verbatim.

**Argument:** push, pull, or freeform git request.

## Codex-only differences

- When git.md says "use the gitter agent" — execute gitter.md phases inline via bash git commands.
- Forbidden: `git reset --hard`, `git clean -fdx`, `git push --force`.
