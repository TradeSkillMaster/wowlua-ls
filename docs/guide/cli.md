# CLI Tools

wowlua-ls includes a command-line checker for linting addons outside an editor — useful for CI pipelines and batch analysis.

## `check` — Lint a project

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
