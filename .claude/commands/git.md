# Git — Gitter Gateway

Talk to gitter: $ARGUMENTS

---

## Overview

`/git` is the gateway to **gitter** — the only agent allowed to run git commands. Known subcommands get routed to specific gitter phases. Anything else gets forwarded to gitter as a freeform request — gitter figures it out.

**Usage:** `/git <subcommand or freeform request>`

---

## Subcommand routing

Parse `$ARGUMENTS` to determine if it matches a known subcommand:

| Input pattern | Subcommand | Action |
|---------------|-----------|--------|
| starts with `push` | PUSH | Stage + commit + push everything (see below) |
| starts with `pull` | PULL | Pull latest from remote (see below) |
| anything else | FREEFORM | Forward to gitter as-is (see below) |
| *(empty)* | FREEFORM | Forward empty request — gitter will ask what's needed |

---

## Subcommand: `push`

```
Agent(gitter): "Phase: PUSH.
  Arguments: {any extra text after 'push'}

  Stage, commit, and push all changes.
  Read .claude/agents/gitter.md Phase 4: PUSH and follow every step."
```

---

## Subcommand: `pull`

```
Agent(gitter): "Phase: PULL.

  Pull latest from remote.
  Read .claude/agents/gitter.md Phase 5: PULL and follow every step."
```

---

## Freeform: anything that isn't a known subcommand

If `$ARGUMENTS` doesn't match any known subcommand above, forward the entire request to gitter. Gitter is a git expert — it can handle status checks, branch operations, log queries, diff reviews, conflict resolution advice, and anything else git-related.

```
Agent(gitter): "The user ran /git with the following request:

  $ARGUMENTS

  You are the git operations specialist. Handle this request using your expertise.
  Read your full agent definition at .claude/agents/gitter.md for context.

  If the request maps to one of your known phases (MERGE, DOCS-COMMIT, JC-COMMIT, PUSH, PULL),
  follow that phase's protocol. Otherwise, use your git knowledge to fulfill the request directly.

  Rules:
  - You may run any git read commands (status, log, diff, branch, show, etc.) freely
  - For write operations (commit, push, reset, etc.), follow your safety protocols
  - Report results clearly back to the user"
```

---

## Rules

- **ALL git operations go through gitter** — this command NEVER runs git commands directly
- **Known subcommands route to specific phases** — `push` -> PUSH phase
- **Unknown requests go freeform** — gitter is smart enough to handle anything git-related
- **Pass user arguments through verbatim** — don't interpret or filter, let gitter decide
