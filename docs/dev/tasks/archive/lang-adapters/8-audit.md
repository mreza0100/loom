> Author: code-auditor

# Audit — lang-adapters

**Verdict:** NEEDS A SWEEP (no blocking issues)
**Date:** 2026-05-11

## Summary

12 findings, 0 critical, 2 high (Java reverse edges), 4 medium, 6 low/info.

## High Severity

1. **EDGE-1:** Java `interface B extends A` missing `extended_by` reverse edge — breaks impact() traversal for interface hierarchies
2. **EDGE-2:** Java `class C implements I` missing `implemented_by` reverse edge — breaks interface blast radius

## Medium Severity

3. **NAMING-1:** Java/Go import edges use full path as `source_name` instead of local binding — may prevent import map resolution
4. **EDGE-3:** Go struct embedding drops package-qualified types (`io.Reader`, `sync.Mutex`)
5. **EDGE-4:** Go/Java interfaces don't extract method stubs as symbols
6. **SMELL-2:** Grammar init failures (`RuntimeError`) escape `ImportError` guard in __init__.py

## Low Severity / Quick Wins

7-12: Dead `_children_by_type` in all adapters, dead `is_class` param in csharp.py, `callee[0].isupper()` empty guard, unreachable punct guard, undocumented `kind="macro"`, Parser re-instantiation per call.

## Blocking Issues

None.

## Recommended Follow-ups

1. `/jc`: Fix Java reverse edges (EDGE-1 + EDGE-2) — 4 lines
2. `/jc`: Broaden except clause in __init__.py (SMELL-2) — 1 line
3. `/jc`: Dead code cleanup (DEAD-1 + DEAD-2) — 15 lines removed
4. Future `/build`: Java/Go import source_name audit + Go embedded type handling
