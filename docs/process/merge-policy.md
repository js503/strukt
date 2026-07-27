# Merge Policy

## Required before merge

1. A spec exists and is linked in the pull request.
2. A plan exists and is linked in the pull request.
3. A tracking issue exists and is linked in the pull request.
4. Verification is described in the pull request.
5. Agentic review has been completed and summarized in the pull request.
6. Human review is complete when required by the team.
7. Any material process deviation is documented in the pull request.

## Agentic review

Agentic review should focus on:

- behavioral regressions
- mismatches between implementation and spec
- missing tests or weak verification
- risky edge cases
- accidental scope drift

## Notes

- The repository workflow enforces the PR checklist, not the depth of the review itself.
- Branch protection should be configured in GitHub to require the PR workflow checks before merge.
- If work intentionally skips a spec, plan, or issue, that exception should be explicit and justified in the pull request.
