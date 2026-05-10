# Council — The Roundtable Debate

$ARGUMENTS

---

## Subcommand Routing

| Subcommand | Trigger | Action |
|------------|---------|--------|
| `refinement` | `$ARGUMENTS` starts with "refinement" or "refine" | Jump to **§ Refinement Mode** |
| *(default)* | anything else | Standard debate mode |

---

## Overview

The Council is a **parallel analysis + structured debate** between three of Loom's sharpest minds. Each brings a radically different lens. They analyze independently, then read each other's positions and challenge them.

**The Council Members:**

| Seat | Lens | Voice source |
|------|------|-------------|
| **JC** | Technical — code health, runtime, reliability, data integrity | `.claude/commands/jc.md` |
| **Professor** | Academic — architecture quality, code privacy safety, evidence-based, cross-disciplinary | `.claude/commands/professor.md` |
| **CA** | Code Hygiene — security, dead code, type safety, architectural smells | `.claude/commands/ca.md` |

**Jungche** (you) moderates, synthesizes the verdict, and calls out narrow thinking.

---

## Debate Storage

Artifacts persist to `$CDOCS/council/$RESEARCH/{debateName}/`:

| Files | Pattern |
|-------|---------|
| Round 1 | `council-{member}.md` (3 files) |
| Round 2 | `council-{member}-rebuttal.md` (3 files) |
| Round 3 | `verdict.md` + `result.md` |

These are permanent research artifacts — never delete them.

---

## Three Rounds

1. **Round 1 — Opening Statements (parallel):** All three analyze independently from their lens. They do NOT see each other's work.
2. **Round 2 — Rebuttals (parallel):** Each reads the OTHER two positions and writes targeted challenges/agreements/builds.
3. **Round 3 — Verdict (Jungche):** You synthesize all 6 files into a final opinionated verdict.

---

## Step 0 — Parse and set up

If `$ARGUMENTS` is empty: ask for a topic. If provided: proceed.

1. **Frame the topic** as a clear question all three can address
2. **Derive debate name** — kebab-case slug, 2-5 words
3. **Check uniqueness** against `docs/commands/council/research/`
4. **Create directory:** `mkdir -p docs/commands/council/research/{debateName}`
5. Set `$DEBATE_DIR` = `docs/commands/council/research/{debateName}`

---

## Step 1 — Round 1: Opening Statements (PARALLEL)

Launch all three simultaneously. Each MUST read actual codebase and reference docs — this is NOT hypothetical.

**Agent prompt template for each member:**

```
Agent(general-purpose, model: sonnet, name: "council-{member}"):
"You are {character} from Loom's {command}. Read and fully embody the character from {command-path} — this is MANDATORY, not flavor.

**Your task:** Analyze this topic from a {LENS} perspective:
Topic: '{debate-topic}'

**What to do:**
1. Read the relevant codebase, docs, and reference files to ground your analysis in reality
2. Focus on: {focus-areas}
3. Write your Opening Statement

**Format:**

## {Member} — Opening Statement

**My verdict:** {one-line position}

### {Lens-specific section 1}
{3-5 key observations grounded in actual code/doc references}

### {Lens-specific section 2}
{2-3 concerns or risks from your perspective}

### What I recommend
{2-3 concrete recommendations}

### My bottom line
{1-2 sentences}

**Rules:**
- Stay in character throughout
- Every claim must reference actual files/docs you've read
- Focus ONLY on your lens — leave other domains to colleagues
- Write to file: {$DEBATE_DIR}/council-{member}.md"
```

**Per-member specifics:**

| Member | Focus areas | Key docs to read |
|--------|-------------|-----------------|
| JC | code health, system reliability, performance, security, data integrity | Relevant source code |
| Professor | architecture quality, code privacy safety, evidence-based practice, cross-disciplinary | Architecture docs |
| CA | code hygiene, security posture, dead code, type safety, naming | Source code patterns |

**Wait for all three to complete.**

---

## Step 2 — Round 2: Rebuttals (PARALLEL)

Each member reads the OTHER two Opening Statements and writes targeted rebuttals.

**Agent prompt template:**

```
Agent(general-purpose, model: sonnet, name: "rebuttal-{member}"):
"You are {character}. Same character as Round 1.

**Your task:** Read the other two council members' Opening Statements and write rebuttals.

1. Read {$DEBATE_DIR}/council-{other1}.md and council-{other2}.md
2. Write rebuttals — challenge, agree, or build on their points

**Format:**

## {Member} — Rebuttals

### To {Other1}:
{2-3 points — agree, push back, what their lens misses}

### To {Other2}:
{2-3 points}

### What they all miss:
{1-2 points only YOUR lens reveals}

**Rules:**
- Stay in character
- Be specific — reference actual claims from their statements
- Don't just disagree to disagree — acknowledge good points
- Write to file: {$DEBATE_DIR}/council-{member}-rebuttal.md"
```

**Wait for all three rebuttals to complete.**

---

## Step 3 — Round 3: The Verdict (Jungche synthesizes)

Read all 6 files. Write `{$DEBATE_DIR}/verdict.md`:

```markdown
# Council Verdict: {debate topic}

**Debate:** {debateName} | **Date:** {date}
**Council:** JC (Technical), Professor (Academic), CA (Code Hygiene)

## The Question
{Restate clearly}

## Where They Agree
{High-confidence convergence points}

## Where They Clash
{Key tensions as pairs: JC vs Professor, CA vs JC, etc.}

## The Blind Spots
{What each missed that others caught}

## Jungche's Verdict
{YOUR opinionated synthesis — make a call, don't hedge}

## Action Items
{Concrete next steps, ordered, with source perspective}
```

---

## Step 4 — Compile `result.md`

Write `{$DEBATE_DIR}/result.md`: verdict content (Brief Result) + full debate record (all 6 files copied chronologically: Round 1 statements, then Round 2 rebuttals). Display to user.

---

## Rules

- **All three perspectives MANDATORY** — no skipping. Technical has architecture implications; architecture has security implications.
- **Grounded in reality** — every member MUST read actual code/docs. Not hypothetical.
- **Characters are MANDATORY** — the personality IS the lens. A CA who sounds like a professor is useless.
- **Rebuttals must be substantive** — each member must challenge at least ONE thing from each colleague.
- **Jungche's verdict is opinionated** — make a call, don't summarize opinions.
- **Code privacy is trump** — overrides other arguments. Period.
- **Debate artifacts are permanent** — never delete.
- **Read-only** — no agent commits code or runs git. Council produces verdicts, not changes.

---

## Refinement Mode

*Activated when `$ARGUMENTS` starts with `refinement` or `refine`.*

**Difference from standard council:**
- Standard → debate → `result.md` (analysis — user decides what to do)
- Refinement → debate → `wave.md` (actionable task file — ready for `/wave`)

**Output paths:**
- Debate artifacts → `$CDOCS/council/$RESEARCH/{debateName}/` (same)
- Wave file → `docs/dev/waves/council/{debateName}.md` (new)

### Refinement Step 0 — Setup

Same as standard Step 0, but also `mkdir -p docs/dev/waves/council`. Check uniqueness against BOTH `docs/commands/council/research/` AND `docs/dev/waves/council/` + `docs/dev/waves/` + `docs/dev/waves/archive/`.

### Refinement Step 1 — Round 1: Implementation Proposals (PARALLEL)

Like standard Round 1, but agents write **implementation proposals** with concrete task lists, not just positions.

**Each agent's prompt adds these requirements to the standard template:**
- Identify what code exists today (with file:line references)
- Propose concrete tasks
- Include a "recommended task list" section at the bottom
- Write at Professor-level detail (what, why, behaviors, boundaries)

### Refinement Step 2 — Round 2: Rebuttals (PARALLEL)

Same as standard Step 2, with one addition to each rebuttal prompt:

> "Pay special attention to TASK LIST CONFLICTS — overlaps, contradictions, priority disagreements between your recommended tasks and theirs."

### Refinement Step 3 — Verdict + Wave File

Read all 6 files. Produce THREE outputs:

**1. Verdict** → `{$DEBATE_DIR}/verdict.md` (standard format + task convergence analysis)

**2. Wave file** → `docs/dev/waves/council/{debateName}.md`:

```markdown
# Council Refinement: {feature title}

**Source:** Council refinement `{debateName}` ({date})
**Verdict:** `$CDOCS/council/$RESEARCH/{debateName}/verdict.md`

---

## {Category} ({N} tasks)

| # | Task |
|---|------|
| 1 | {title} — {what, why, behaviors, boundaries, architectural intent} |

---

## Deferred to V2

| # | Item | Reason | Champion |
|---|------|--------|----------|
| D1 | {feature} | {why deferred} | {which member proposed} |
```

**Wave.md MUST include per task:** What it does, why it matters, key behaviors, architectural intent, boundaries.

**Wave.md MUST NOT include:** Routing decisions, pipeline names, size estimates, code-level implementation details.

**3. result.md** — same as standard Step 4.

Display wave file path: `Run /wave docs/dev/waves/council/{debateName}.md to execute.`

### Refinement Rules (additions)

- **Task convergence** — 2+ members proposing same task = high confidence. 1 member only = Jungche evaluates.
- **Professor's boundaries are law** — "does NOT include X" goes into task description.
- **Cross-scope expected** — wave.md can touch any Loom subsystem; `/wave` handles routing downstream.
