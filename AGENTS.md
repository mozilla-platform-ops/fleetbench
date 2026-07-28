<!-- br-agent-instructions-v1 -->

## Commit Conventions

- Do not add `Co-Authored-By` (or any other coauthor) lines to commit messages.
- When a commit addresses a beads issue, append the issue ID in parentheses at the end of the commit subject line, e.g. `Add v2 schema types for collector output (fleetbench-collector-v0-8ne.3)`. No "Closes" / "Fixes" prefix needed.

---

## Releases

Releases are published automatically by `.github/workflows/release.yml` when
an annotated `v*` tag is pushed. The workflow rejects a tag whose version does
not match `collector/Cargo.toml`.

1. Create and claim a release Beads issue.
2. Choose the next version and update both `collector/Cargo.toml` and
   `collector/Cargo.lock`. Update downloader defaults and current runbook
   examples that should use the new release; do not rewrite historical logs.
3. Run the relevant checks (at minimum `cargo test`, plus syntax checks for
   any changed launchers or Python wrappers).
4. Close the release issue, run `br sync --flush-only`, stage only the scoped
   changes, commit using the issue-ID convention, and push the branch.
5. Create and push the annotated tag:

   ```bash
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin main vX.Y.Z
   ```

6. Monitor the GitHub Actions release workflow, confirm every job succeeds,
   then verify the GitHub release is published with the expected platform
   binaries and `SHA256SUMS` (especially the Linux asset used by host-wide
   runners).
7. Replace the workflow-generated release notes with a useful human summary.
   Follow the recent-release format: `# Fleetbench vX.Y.Z`, short focused
   `##` sections with outcome-oriented bullets, a `## Downloads` section, and
   a final `**Full Changelog**` comparison link. For example:

   ```bash
   gh release edit vX.Y.Z --title vX.Y.Z --notes-file <prepared-notes.md>
   ```

   Keep the notes specific to the release: explain user-visible behavior,
   new launchers or compatibility modes, and the release version callers
   should use. Do not leave only GitHub's generated commit list.

---

## Beads Workflow Integration

This project uses [beads_rust](https://github.com/Dicklesworthstone/beads_rust) (`br`/`bd`) for issue tracking. Issues are stored in `.beads/` and tracked in git.

### Essential Commands

```bash
# View ready issues (open, unblocked, not deferred)
br ready              # or: bd ready

# List and search
br list --status=open # All open issues
br show <id>          # Full issue details with dependencies
br search "keyword"   # Full-text search

# Create and update
br create --title="..." --description="..." --type=task --priority=2
br update <id> --status=in_progress
br close <id> --reason="Completed"
br close <id1> <id2>  # Close multiple issues at once

# Sync with git
br sync --flush-only  # Export DB to JSONL
br sync --status      # Check sync status
```

### Workflow Pattern

1. **Start**: Run `br ready` to find actionable work
2. **Claim**: Use `br update <id> --status=in_progress`
3. **Work**: Implement the task
4. **Complete**: Use `br close <id>`
5. **Sync**: Always run `br sync --flush-only` at session end

### Key Concepts

- **Dependencies**: Issues can block other issues. `br ready` shows only open, unblocked work.
- **Priority**: P0=critical, P1=high, P2=medium, P3=low, P4=backlog (use numbers 0-4, not words)
- **Types**: task, bug, feature, epic, chore, docs, question
- **Blocking**: `br dep add <issue> <depends-on>` to add dependencies

### Session Protocol

**Before ending any session, run this checklist:**

```bash
git status              # Check what changed
git add <files>         # Stage code changes
br sync --flush-only    # Export beads changes to JSONL
git commit -m "..."     # Commit everything
git push                # Push to remote
```

### Best Practices

- Check `br ready` at session start to find available work
- Update status as you work (in_progress → closed)
- Create new issues with `br create` when you discover tasks
- Use descriptive titles and set appropriate priority/type
- Always sync before ending session

<!-- end-br-agent-instructions -->
