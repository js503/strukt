# Review Standard

## Purpose

This document defines what an acceptable agentic pull request review should examine before merge.

## Required review areas

An agentic review should look for:

- functional bugs
- behavioral regressions
- edge cases that are unhandled or weakly handled
- mismatches between implementation and spec
- scope drift relative to the plan
- missing verification
- missing or weak tests
- maintainability risks that could break near-term development

## Review output standard

The review summary should include:

- overall risk level
- concrete findings, if any
- files or areas reviewed
- testing or verification gaps
- open questions or assumptions

## Review quality rules

- "Looks good" is not enough.
- Findings should be specific, defensible, and action-oriented.
- If there are no findings, the review should explicitly say that no material findings were identified.
- If verification was not run, that gap should be called out.

## Merge expectation

Merging should wait until:

- findings are resolved, or
- accepted risks are documented with explicit justification
