# Codex Integration Layer

Optional dual-runtime setup for the Loom pipeline. Everything in `.codex/` is a config layer that points to `.claude/` as the single source of truth. Delete this entire directory and the pipeline runs fine on Claude Code alone.

---

## What this is

OpenAI's Codex CLI can mirror the same Jungche pipeline that Claude Code runs. The `.codex/` directory contains:

- **`config.toml`** — global Codex settings (personality override, sandbox, Teams enablement)
- **`agents/*.toml`** — wrappers that tell Codex "read this `.claude/` manual and follow it"
- **`skills/`** — interactive `$name` invocations (Codex's equivalent of Claude's `/name` slash commands)
- **`AGENTS.md`** (at repo root) — a symlink to `CLAUDE.md` so Codex reads the same root instructions

Codex never gets its own copy of pipeline logic. Every `.toml` file says "read `.claude/commands/X.md`" or "read `.claude/agents/X.md`" — the markdown manual is always the source of truth.

---

## Division of labor

When **Claude orchestrates** (the default):
- Claude spawns child agents for every pipeline step
- Claude's gitter agent handles all git operations
- Codex is not involved at all

When **Codex orchestrates** (opt-in — e.g., `$build`, `$wave`):
- Codex reads the same `.claude/commands/*.md` manuals
- Codex spawns child agents via Codex Teams (`Agent(role, "...")`)
- Codex owns git inline (reads `.claude/agents/gitter.md` protocol, executes bash git commands)
- Claude's gitter agent is NOT involved — Codex is self-contained

---

## File structure

```
loom/
├── CLAUDE.md                    ← source of truth (Claude reads this)
├── AGENTS.md                    ← symlink → CLAUDE.md (Codex reads this)
├── .claude/                     ← pipeline source of truth
│   ├── agents/                  ← agent manuals (read by both runtimes)
│   ├── commands/                ← command manuals (read by both runtimes)
│   ├── scripts/                 ← worktree.sh, alloc-ports.sh, dev.sh
│   └── skills/                  ← Claude skills + shared research skills
├── .codex/                      ← OPTIONAL Codex config layer
│   ├── config.toml              ← global Codex settings
│   ├── README.md                ← you are here
│   ├── agents/                  ← .toml wrappers pointing to .claude/ manuals
│   │   ├── build.toml           ← command wrapper (/build)
│   │   ├── jc.toml              ← command wrapper (/jc)
│   │   ├── wave.toml            ← command wrapper (/wave)
│   │   ├── dev.toml             ← command wrapper (/dev)
│   │   ├── git.toml             ← command wrapper (/git)
│   │   ├── jm.toml              ← command wrapper (/jm)
│   │   ├── professor.toml       ← command wrapper (/professor)
│   │   ├── council.toml         ← command wrapper (/council)
│   │   ├── ca.toml              ← command wrapper (/ca)
│   │   ├── qa.toml              ← command wrapper (/qa)
│   │   ├── bm.toml              ← command wrapper (/bm)
│   │   ├── gitter.toml          ← git operator
│   │   ├── developer.toml       ← role agent wrapper
│   │   ├── planner.toml         ← role agent wrapper
│   │   ├── architect.toml       ← role agent wrapper
│   │   └── qa-agent.toml        ← role agent wrapper
│   └── skills/                  ← $name interactive invocations
│       ├── build/SKILL.md       ← mirrors /build
│       ├── jc/SKILL.md          ← mirrors /jc
│       ├── wave/SKILL.md        ← mirrors /wave
│       ├── dev/SKILL.md         ← mirrors /dev
│       ├── git/SKILL.md         ← mirrors /git
│       ├── jm/SKILL.md          ← mirrors /jm
│       ├── professor/SKILL.md   ← mirrors /professor
│       ├── council/SKILL.md     ← mirrors /council
│       ├── ca/SKILL.md          ← mirrors /ca
│       ├── qa/SKILL.md          ← mirrors /qa
│       ├── bm/SKILL.md          ← mirrors /bm
│       ├── rr -> symlink        ← shared research skill
│       ├── rnd -> symlink       ← shared iterative skill
│       └── 360 -> symlink       ← shared analysis skill
```

---

## Git ownership rules

**When Codex orchestrates a full pipeline** (`$build`, `$wave`):
- Codex reads `.claude/agents/gitter.md` as its git protocol manual
- Codex executes gitter phases inline via bash git commands
- Anywhere the command manual says "Use the gitter agent" → "Execute gitter.md Phase X inline"

**When Codex runs as a scoped implementer** (role agent, delegated task):
- Codex has NO git access
- Only the orchestrating runtime handles git

**When Codex runs `$jc`** (hotfix mode):
- Codex executes gitter.md JC-COMMIT inline — LOCAL commits only
- Codex MUST NOT push — JC-COMMIT is local. Push is a separate explicit action via `$git push`.

---

## What NOT to do

- **Don't make Codex a requirement** — every pipeline operation works with Claude Code alone
- **Don't duplicate logic in .toml files** — they point to `.claude/` manuals, not restate them
- **Don't let Codex edit `.claude/` or `CLAUDE.md`** — those are the source of truth, edited only by `/jm`
- **Don't let Claude edit `.codex/`** — it's Codex's config layer, co-owned by `/jm`
