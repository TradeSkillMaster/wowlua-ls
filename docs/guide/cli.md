# CLI Tools

wowlua-ls includes command-line tools for linting and documentation generation outside an editor, useful for CI pipelines and batch analysis.

## `check`: Lint a project

Scan an addon directory and report all diagnostics:

```bash
wowlua_ls check path/to/addon
```

By default, only warnings are shown. Include hints (unused locals, inject-field, style issues):

```bash
wowlua_ls check path/to/addon --severity hint
```

Exit code is `1` if any diagnostics are found, making it suitable for CI:

```yaml
# GitHub Actions example
- name: Lint
  run: wowlua_ls check . --severity warning
```

## `dump-stubs`: Dump global stub types

Output every global name from the precomputed stubs and its resolved type, one per line (tab-separated). Useful for diffing before and after stub regeneration:

```bash
wowlua_ls dump-stubs > before.txt
# ... regenerate stubs ...
wowlua_ls dump-stubs > after.txt
diff before.txt after.txt
```

## `doc`: Generate API documentation

Generate markdown API documentation snippets from annotated Lua source:

```bash
wowlua_ls doc path/to/addon --out-dir docs/api
```

This scans the project for `@class` definitions and produces one `.md` snippet per class plus an `index.md` in the output directory. Classes from WoW API stubs are excluded. Only classes defined within the scanned directory are included.

### Using with VitePress

Per-class files are headless snippets (no `# Title`) designed for embedding via VitePress's include directive. Write your own page with custom prose, then include the generated API reference:

```markdown
# ReactiveState

Overview of what ReactiveState does, with examples...

<!--@include: ./api/ReactiveState.md-->
```

The included content starts at `## Fields` / `## Methods` level, so it integrates naturally under your page's `# Title`.

### CI example

```yaml
# GitHub Actions: generate API docs
- name: Generate API docs
  run: wowlua_ls doc . --out-dir docs/api
```
