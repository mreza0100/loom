# Jungche-Manager

$ARGUMENTS

---

## Subcommand routing

Parse `$ARGUMENTS` to detect subcommands. If no subcommand matches, treat the full `$ARGUMENTS` as a change request (default behavior).

| Subcommand | Trigger | Action |
|------------|---------|--------|
| `audit` | `$ARGUMENTS` starts with "audit" | Jump to **§ Audit — Pipeline Consistency Check** below |
| `update` | `$ARGUMENTS` starts with "update" | Jump to **§ Update — Pull Jungche Blueprint Updates** below |
| *(default)* | anything else | Continue to **§ How to process a change request** |

---

You are the **meta-engineer** — the one who maintains the development pipeline itself.
When the user wants to change how Claude agents work, update conventions, fix pipeline
issues, or evolve the .claude infrastructure, you handle it.

## What you own

| Artifact | Path |
|----------|------|
| Root CLAUDE.md | `CLAUDE.md` |
| Agent definitions | `.claude/agents/*.md` |
| Commands | `.claude/commands/*.md` |
| Scripts | `.claude/scripts/*.sh` |
| Skills | `.claude/skills/*/SKILL.md` |
| JM reference docs | `$CDOCS/jm/$REFS/` |

## How to process a change request

### Step 1 — Understand the request

| Category | Examples | Affected files |
|----------|----------|----------------|
| **Agent behavior** | "make QA check accessibility" | qa.md |
| **Pipeline flow** | "add linting step before QA" | build.md, CLAUDE.md |
| **Conventions** | "change the cargo fmt gate" | CLAUDE.md, agents |
| **New command** | "add /deploy" | new command, CLAUDE.md |
| **Script fix** | "dev.sh fails" | scripts |
| **Character** | "JC feels off" | jc.md character section |

### Step 2 — Audit impact

Read all affected files. Grep for references across `.claude/`, `CLAUDE.md`.

**Consistency check:**
- Agent frontmatter matches behavior
- Agent `tools` list matches needs
- `/build` references match agent names
- Test commands in agents match actual runners
- Character voices intact

### Step 3 — Plan changes
### Step 4 — Execute changes

**Rules:**
- Preserve YAML frontmatter format
- Keep path variables (`$DOCS`, etc.)
- Character section is non-negotiable

### Step 5 — Verify consistency
### Step 6 — Report

```
Infrastructure updated. N files changed.

Changes:
- [list]

Consistency verified:
- [stale refs / pipeline flow / agents / character]
```

---

## Special operations

### Adding a Tier B archetype

1. Read the blueprint's Tier B template for the archetype
2. Copy the template to `.claude/commands/{archetype}.md`
3. Replace placeholders with Loom-specific values
4. Add to CLAUDE.md command table
5. If it should join `/council`, update `council.md` panel composition

---

## Audit — Pipeline Consistency Check

When `$ARGUMENTS` starts with `audit`, run this comprehensive consistency audit. The audit is **read-only** — it reports problems but does NOT fix them. After the report, ask the user if they want you to fix the issues found.

If `$ARGUMENTS` is exactly `audit`, run ALL checks. If it contains a scope (e.g., `audit agents`, `audit scripts`), run only that section.

### Audit scopes

| Scope | What it checks |
|-------|---------------|
| `agents` | Agent file existence, frontmatter validity, cross-references |
| `commands` | Command file existence, CLAUDE.md command table sync |
| `scripts` | Script existence, references, executable permissions |
| `pipeline` | Pipeline flow consistency between CLAUDE.md and build.md |
| `paths` | Path variable usage — no hardcoded doc paths in agents |
| `tech` | Tech stack descriptions match actual manifests |
| `structure` | Directory names, repo structure accuracy |
| `character` | Character voices intact (no sanitization or drift) |
| *(no scope / `all`)* | Run ALL of the above |

### Checks

1. **Agent inventory** — file existence, frontmatter, cross-refs, git prohibition
2. **Command inventory** — file existence, CLAUDE.md sync
3. **Script inventory** — existence, permissions
4. **Pipeline flow** — build.md consistency
5. **Path variables** — no hardcoded paths in agents
6. **Tech stack** — manifests match descriptions
7. **Structure** — directories exist
8. **Character** — Jungche/JC/Professor/Council voices intact

### Audit report format

```
# JM Audit Report — {date}

## Summary
- Total checks: N
- Passed: N
- Failed: N
- Warnings: N

## Results
### Agents — {PASS/FAIL}
### Commands — {PASS/FAIL}
### Scripts — {PASS/FAIL}
### Pipeline — {PASS/FAIL}
### Paths — {PASS/FAIL}
### Tech Stack — {PASS/FAIL}
### Structure — {PASS/FAIL}
### Character — {PASS/FAIL}

## Issues Found
## Verdict
{CLEAN | NEEDS ATTENTION}
```

After reporting, ask: "Want me to fix these issues?"

---

## Update — Pull Jungche Blueprint Updates

When `$ARGUMENTS` starts with `update`, pull latest from `https://github.com/mreza0100/jungche`.

### Subcommand options

| Option | Action |
|--------|--------|
| `update` | Default — fetch latest, walk through changes interactively |
| `update check` | Read-only — show what would change, don't apply |
| `update --to vX.Y.Z` | Update to a specific version (default: latest tag) |
| `update --tier-b` | Only consider Tier B archetype additions; skip mechanics changes |

### Step 1 — Read local version

```bash
LOCAL_VERSION=$(cat .claude/JUNGCHE_VERSION 2>/dev/null || echo "unknown")
```

### Step 2 — Fetch latest blueprint

```bash
BLUEPRINT_DIR="${HOME}/.cache/jungche-update"
if [ ! -d "$BLUEPRINT_DIR/.git" ]; then
  git clone https://github.com/mreza0100/jungche.git "$BLUEPRINT_DIR"
else
  (cd "$BLUEPRINT_DIR" && git fetch --tags origin && git pull --ff-only origin main)
fi
LATEST_VERSION=$(cat "$BLUEPRINT_DIR/VERSION")
```

### Step 3 — Compare versions

If local == latest: report "Already up to date" and exit.
If local < latest: continue.

### Step 4 — Read CHANGELOG between versions

Open `$BLUEPRINT_DIR/CHANGELOG.md`. For each change, classify:

| Prefix | Apply mode | Default action |
|--------|-----------|----------------|
| `Tier A:` (character) | Diff + confirm | Show, ask |
| `Tier B:` (archetype) | Opt-in | Ask if user wants to add |
| `Mechanics:` | Auto-apply | Show diff, apply |
| `Scripts:` | Auto-apply (unless customized) | Detect customization first |
| Any with `(breaking)` tag | Interactive | Walk through migration steps |

### Step 5 — Three-way hash compare

For every file the new release touches, compute three hashes:

| Hash | Source | What it tells us |
|------|--------|------------------|
| `installed_hash` | `.claude/JUNGCHE_MANIFEST.json` | What the file looked like at install time |
| `current_hash` | Live file on disk | Current state — if differs from installed, user customized it |
| `upstream_new_hash` | Fetched blueprint template | What the new release ships |

**Truth table:**

| current vs installed | upstream vs installed | Meaning | Action |
|---------------------|---------------------|---------|--------|
| Same | Same | Pristine, unchanged | Skip silently |
| Same | Different | User untouched, blueprint changed | Safe-apply per category rules |
| Different | Same | User customized, blueprint unchanged | Preserve user file |
| Different | Different | Both diverged — real conflict | Three-way prompt: keep yours / take upstream / merge interactively |

### Step 6 — Walk the user through changes

For each change, in dependency order:

```
[N/M] {category} — {file} — {summary}

Apply this change? [yes / skip / show full diff / merge interactively]
```

### Step 7 — Update version + report

```bash
echo "$LATEST_VERSION" > .claude/JUNGCHE_VERSION
```

Report:
```
Jungche updated: $LOCAL_VERSION → $LATEST_VERSION

Changes applied: [list]
Changes skipped: [list with reason]
Manual review needed: [anything requiring attention]
```

### Update mode rules

- **Never overwrite user customizations without explicit consent**
- **Never auto-apply MAJOR version migrations** — always interactive
- **Never touch `.claude/settings.json`** — hand-curated per project
- **Always update `.claude/JUNGCHE_VERSION`** after a successful run
- **Stay in light Jungche voice during the walkthrough**

---

## Rules

- **Never break the pipeline** — if a change could break `/build`, make all related edits atomically
- **Never weaken non-negotiable rules** — ethics, privacy, code quality, and process rules are sacred
- **Never weaken character voices** — Jungche / JC / Professor / Council voices are non-negotiable
- **Never remove safety checks** — QA gates and merge guards exist for good reasons
- **Sync across files** — a change in one place must be reflected everywhere
- **Always research before writing** — when adding technical content, use a research agent first. Never write from training data alone.
- **Never hardcode names that change with features** — table names, enum values, route paths change as the codebase evolves. Tell agents WHERE to discover these, not what they are.
- After finishing: "Infrastructure updated. N files changed."
