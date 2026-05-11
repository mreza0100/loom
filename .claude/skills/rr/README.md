# RR — Research & Report

**Version:** 1.1.0 · **License:** MIT · **Repo:** [github.com/mreza0100/rr](https://github.com/mreza0100/rr)

A Claude Code skill for structured, multi-agent research pipelines. Instead of a single search and a one-paragraph answer, RR builds knowledge in batches — each batch shaped by what the previous batch found — and finishes with a synthesized report and an actionable plan.

## Why this exists

When you ask an LLM to "research X," it runs one search, reads the top results, and gives you a summary. That's fine for simple lookups. But for real research — comparing options, auditing a codebase, investigating regulations, evaluating trade-offs — you need a pipeline that:

1. **Maps the landscape** first (what are the sub-questions?)
2. **Fans out** into parallel research agents (one per sub-question)
3. **Pressure-tests** findings (looks for counter-evidence before stopping)
4. **Synthesizes** everything into a verdict + plan (not just raw findings)

RR does this. It delegates research to spawned agents so the main conversation stays clean, persists the full research record to a file, and delivers a terse executive summary.

## Two modes

| Mode | Trigger | What happens |
|------|---------|-------------|
| **RR** | `RR <topic>` | Spawns research agents, runs the pipeline, delivers a report |
| **RRP** | `RRP <topic>` | Writes a self-contained prompt you can run in another chat |

## Three research surfaces

| Surface | Tools used | When to pick |
|---------|-----------|-------------|
| **internet** | WebSearch, WebFetch | External topics — libraries, regulations, market research |
| **codebase** | Read, Grep, Glob, Bash | Internal topics — "how is auth wired", "audit our data flow" |
| **both** | All of the above | Mixed — "best practice X and how we currently do it" |

RR infers the surface from the topic. If ambiguous, it asks.

## Installation

### As a Claude Code skill

```bash
# From your project root
mkdir -p .claude/skills/rr
cp SKILL.md .claude/skills/rr/SKILL.md
```

Then use it in Claude Code:

```
RR <topic>              — run a full research pipeline
RRP <topic>             — write a portable research prompt
research <topic>        — alias for RR
look into <topic>       — alias for RR
```

## Pipeline architecture

```
User: "RR <topic>"
  │
  ├─ Step 1: Refine the goal (what do they actually want?)
  ├─ Step 2: Determine storage path
  ├─ Step 3: Spawn scout agent (maps the landscape)
  │    └─ Returns: landscape summary + 2-6 sub-questions
  ├─ Step 4: Fan out N parallel agents (one per sub-question)
  │    └─ Each returns: full findings in chat (no files)
  ├─ Step 4.5: Write ONE aggregate file
  │    └─ Prompt → Fan-out plan → Scout findings → Per-question findings → Verdict → Plan
  └─ Step 5: Deliver terse summary to user
       └─ Verdict + key findings + plan + file path
```

## Key design decisions

**Delegate, don't inline.** Research generates a lot of tool noise. RR spawns agents so the main conversation stays clean and focused.

**Dynamic batches, not a fixed plan.** Each batch is shaped by what the previous batch found. If batch 3 was decided before batch 1 ran, it's not RR.

**One file, one run.** The entire pipeline produces exactly one research file — no scout files, no per-agent files. The orchestrator writes it at the end from the agents' chat results.

**Pressure-test pass.** After the goal is answered, one extra batch looks for counter-evidence, newer sources, or contradicting patterns. The first plausible answer is often wrong.

**Plan, not just findings.** A wall of facts is half the deliverable. Every RR run must produce a concrete, opinionated recommendation.

## Updating

Compare the `version` field in your installed `SKILL.md` frontmatter against the repo's latest:

```bash
cd /path/to/rr-repo && git pull
cp SKILL.md /your/project/.claude/skills/rr/SKILL.md
```

## License

MIT
