---
description: Draft release notes for an upcoming release
---

Draft release notes for an upcoming release.

$ARGUMENTS

Steps:

1. Find the most recent tag: `git tag --sort=-v:refname | head -1`
2. List all commits since that tag: `git log --format="%h %s" <last_tag>..HEAD`
3. If there are no commits since the last tag, tell the user there's nothing to release.
4. Read the commit messages and group them into categories:
   - **Bug Fixes** — anything that fixes incorrect behavior
   - **New** — new features or capabilities
   - **Improvements** — refactors, performance, cleanup, better error handling
   - **Docs** — documentation-only changes
   Skip empty categories. Collapse clusters of related commits (e.g. a series of diagnostic decoupling commits) into a single summary bullet. Drop commits that are pure internal churn with no user-visible effect.
5. Determine the version number following semver (https://semver.org/):
   - If the user provided a version number, use it.
   - Otherwise, auto-increment from the last tag: bump MAJOR for breaking changes, MINOR for new features/diagnostics, PATCH for bug-fix-only releases.
   - Always include the version as a `# vX.Y.Z` heading at the top of the release notes.
6. If the user provided extra context or arguments, incorporate them.
7. Present the draft to the user for review. Ask if they want any changes.
8. Once approved, replace the contents of `RELEASE_NOTES.md` at the repo root with only the new release's notes (do not keep previous releases).
