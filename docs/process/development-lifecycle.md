# Development Lifecycle

## Purpose

This document defines the default delivery path for work governed by `forj`.

## Default flow

1. Shape the work in a spec.
2. Convert the spec into an implementation plan.
3. Open a tracking issue with scope and acceptance criteria.
4. Implement on a branch tied to the issue.
5. Open a pull request that links the spec, plan, and issue.
6. Run verification and document the results.
7. Run agentic review and summarize the findings.
8. Resolve review findings or document why they are accepted.
9. Merge only after the gate is satisfied.

## Required artifacts

- Spec
- Plan
- Tracking issue
- Pull request
- Verification summary
- Agentic review summary

## Minimum quality bar

- The spec is clear enough that another engineer could explain the intent.
- The plan is concrete enough that another engineer could execute it.
- The issue scope is bounded.
- The pull request explains what changed and how it was verified.
- The review summary identifies actual risks, not empty approval.

## Allowed exceptions

Exceptions are allowed for:

- emergency fixes
- tiny maintenance updates
- repo-administration changes

Exceptions should still be documented in the pull request, including which artifact was skipped and why.
