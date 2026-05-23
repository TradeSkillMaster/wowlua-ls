# Neovim Diagnostic Integration

Hard-won learnings from debugging the push/pull diagnostic interaction between wowlua-ls and Neovim's LSP client (v0.12.2+).

## Neovim's Dual Namespace Problem

Since Neovim PR #37938 (Feb 2026, v0.12.2+), push (`publishDiagnostics`) and pull (`textDocument/diagnostic`) diagnostics use **separate namespaces**. Both are displayed simultaneously. If a server sends the same diagnostics via both mechanisms, they appear doubled.

**Implication**: A server that advertises `diagnosticProvider` (pull) must NOT also push `publishDiagnostics` to Neovim for the same files, or diagnostics will be doubled. Gate all push calls behind `!client.diagnostic_refresh`.

## `workspace_diagnostics` Must Be `false`

This is the critical finding. In Neovim's `diagnostic.lua`, the `on_refresh()` handler (triggered by `workspace/diagnostic/refresh`) has a branch:

```lua
if client:supports_method('workspace/diagnostic') then
    M._workspace_diagnostics({ client_id = ctx.client_id })
else
    for bufnr in pairs(client.attached_buffers or {}) do
        if bufstates[bufnr] and bufstates[bufnr].pull_kind == 'document' then
            M._refresh(bufnr)
        end
    end
end
```

When the server advertises `workspace_diagnostics: true`:
- Neovim ONLY calls `_workspace_diagnostics()` on refresh
- It does NOT re-pull `textDocument/diagnostic` for open buffers
- In-buffer diagnostics stay **permanently stale** until the next edit triggers `didChange`

When the server advertises `workspace_diagnostics: false`:
- Neovim takes the `else` branch and calls `_refresh(bufnr)` for each attached buffer
- This triggers `textDocument/diagnostic` re-pulls, updating in-buffer diagnostics

**Our server does not implement `workspace/diagnostic`** (we return an error for that request). So `workspace_diagnostics: true` was doubly wrong — Neovim tried to pull workspace diagnostics, got an error, and never fell back to per-buffer pulls.

## When Neovim Pulls `textDocument/diagnostic`

Neovim pulls on:
1. `didOpen` — via `LspNotify` autocmd
2. `didChange` — via `LspNotify` autocmd
3. `workspace/diagnostic/refresh` — but ONLY if `workspace_diagnostics: false` (see above)

Neovim does NOT pull on:
- `didSave`
- After receiving `publishDiagnostics`
- After `workspace/diagnostic/refresh` when `workspace_diagnostics: true`

## Phase 4 Push Must Be Gated

Our Phase 4 (debounced 500ms reanalysis after edits) publishes fresh diagnostics. This push must be gated behind `!client.diagnostic_refresh`:
- For pull clients (Neovim with `diagnosticProvider`): send `workspace/diagnostic/refresh` notification instead, which triggers Neovim to re-pull
- For push-only clients: send `publishDiagnostics` directly

## Line-Shifting: Edit Zone Exclusivity

When Neovim sends a `didChange` notification for deleting a line (`dd`), the LSP edit range is:
```
start: { line: N, character: 0 }
end:   { line: N+1, character: 0 }
```

The end position `(line N+1, char 0)` is an **exclusive boundary** — line N+1 is NOT part of the deleted range. Our edit zone calculation must account for this:

```rust
let edit_end_line = if range.end.character == 0 && range.end.line > range.start.line {
    range.end.line - 1  // exclusive end — don't include that line in the edit zone
} else {
    range.end.line
};
```

Without this fix, deleting a line drops diagnostics from the line below the deletion (which should have been shifted up, not removed).

## Parse Errors in Line-Shifted Results

Parse errors (`code: None` in our diagnostic model, vs semantic diagnostics which have string codes) can appear on lines far from the actual mistake. For example, deleting a `)` produces parse errors that cascade to distant lines.

When line-shifting cached diagnostics (before fresh analysis completes), we drop parse errors since their positions are unreliable after edits:
```rust
items.retain(|d| d.code.is_some());
```

This applies in both the `didChange` push path and the `textDocument/diagnostic` pull handler.

## rust-analyzer Has the Same Problem

rust-analyzer also advertises `diagnosticProvider` with `workspace_diagnostics: false` and pushes via `publishDiagnostics`. It encounters the same Neovim namespace conflict (tracked in Neovim issue #37936). Our approach matches theirs.

## Summary of Our Diagnostic Architecture

1. **didOpen**: Push diagnostics immediately (push-only clients) or let Neovim pull (pull clients)
2. **didChange**: Line-shift cached diagnostics (drop parse errors, adjust edit zone), push for push-only clients; pull clients get pulled by Neovim's `LspNotify` autocmd
3. **Phase 4** (500ms debounce): Full reanalysis → push for push-only clients, send `workspace/diagnostic/refresh` for pull clients
4. **textDocument/diagnostic** (pull handler): Return current diagnostics, applying line-shift if pending edit hasn't been reanalyzed yet
