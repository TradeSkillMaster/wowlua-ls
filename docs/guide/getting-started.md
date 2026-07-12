# Getting Started

## Installation

### VS Code

Install the **wowlua-ls** extension from the VS Code marketplace. It bundles the language server binary — no separate install needed.

### JetBrains IDEs

Install **WoW Lua Language Server** from the [JetBrains Marketplace](https://plugins.jetbrains.com/plugin/31581-wow-lua-language-server) (or **Settings → Plugins → Marketplace**, search for "WoW Lua"). Works in any JetBrains IDE 2025.2 or newer; it uses the [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) plugin as its LSP client, which the Marketplace installs automatically as a dependency. The plugin bundles the language server binary — no separate install needed.

### Neovim

Neovim has built-in LSP client support — no plugin required, just configuration.

**1. Get the binary.** Either download a release from [GitHub Releases](https://github.com/TradeSkillMaster/wowlua-ls/releases), or build from source:

```bash
git clone https://github.com/TradeSkillMaster/wowlua-ls.git
cd wowlua-ls
cargo build --release
# Binary is at target/release/wowlua_ls
```

**2. Configure the LSP client.** Add this to your Neovim config (requires Neovim 0.11+):

```lua
vim.lsp.config('wowlua_ls', {
  cmd = { '/path/to/wowlua_ls' },
  filetypes = { 'lua' },
  root_markers = { '.wowluarc.json', '.toc', '.git' },
  workspace_required = false,
})
vim.lsp.enable('wowlua_ls')
```

Replace `/path/to/wowlua_ls` with the actual path to the binary. `workspace_required = false` lets the server attach to standalone Lua files outside a project.

#### LazyVim

If you use [LazyVim](https://www.lazyvim.org/), add a plugin spec:

```lua
-- ~/.config/nvim/lua/plugins/wowlua-ls.lua
return {
  {
    "neovim/nvim-lspconfig",
    opts = {
      servers = {
        wowlua_ls = {
          cmd = { "/path/to/wowlua_ls" },
          filetypes = { "lua" },
          root_markers = { ".wowluarc.json", ".toc", ".git" },
          workspace_required = false,
        },
      },
    },
  },
}
```

#### nvim-lspconfig (standalone)

If you use [nvim-lspconfig](https://github.com/neovim/nvim-lspconfig) without a distro, you can register the server manually:

```lua
local configs = require('lspconfig.configs')

if not configs.wowlua_ls then
  configs.wowlua_ls = {
    default_config = {
      cmd = { '/path/to/wowlua_ls' },
      filetypes = { 'lua' },
      root_dir = require('lspconfig.util').root_pattern('.wowluarc.json', '.toc', '.git'),
      single_file_support = true,
    },
  }
end

require('lspconfig').wowlua_ls.setup({})
```

::: tip
If you also use a general-purpose Lua language server (e.g. lua_ls), you may want to disable it for WoW addon projects to avoid duplicate diagnostics. You can do this by checking for `.wowluarc.json` in your lua_ls config's `root_dir` or using a filetype autocmd.
:::

### Other editors

Build the language server from source:

```bash
git clone https://github.com/TradeSkillMaster/wowlua-ls.git
cd wowlua-ls
cargo build --release
```

The binary is at `target/release/wowlua_ls`. Configure your editor to run it as an LSP server over stdio for Lua and TOC files.

## Your first project

Open a WoW addon folder in your editor. wowlua-ls will automatically:

1. Scan all `.lua`, `.xml`, and `.toc` files in the workspace
2. Load the built-in WoW API stubs (retail + classic)
3. Resolve cross-file classes, globals, and addon namespaces
4. Extract frame and template types from XML files
5. Provide [interactive editing support](/guide/toc-files) for TOC files
6. Start reporting diagnostics

No configuration file is required to get started. The defaults are sensible for most addons.

## Adding a configuration file

For project-specific settings, create a `.wowluarc.json` in your addon's root directory:

```json
{
  "ignore": ["Libs/"],
  "flavors": ["retail", "classic"],
  "diagnostics": {
    "enable": ["need-check-nil"]
  }
}
```

This tells wowlua-ls:

- **Skip `Libs/`** — don't analyze third-party library code
- **Target retail and classic** — warn about flavor-specific APIs
- **Enable nil checking** — report `need-check-nil` warnings (off by default)

See [Configuration](/guide/configuration) for the full reference.

## Adding annotations

wowlua-ls infers a lot on its own, but annotations make it smarter. Start with the high-value ones:

### Annotate your public API

```lua
---@param itemId number
---@param count number?
---@return boolean success
---@return string? error
function MyAddon:BuyItem(itemId, count)
    if not self.store then
        return false, "store not initialized"
    end
    -- ...
    return true
end
```

The LS now knows the parameter types, that `count` is optional, and that the function returns a boolean plus an optional error string. Every caller gets type checking and hover information.

### Annotate your classes

```lua
---@class AuctionEntry
---@field itemId number
---@field buyout number
---@field seller string
---@field duration number?
```

Now `AuctionEntry` is a named type. You can reference it in `@param` and `@return` annotations across your codebase, and the LS will provide completion and type checking for its fields.

### When to use `@type`

`@type` forces a variable's type. It's most useful when the LS can't infer what you need. For cases where the LS already has the answer, it's up to you — the annotation won't hurt, but you can save yourself the effort:

```lua
-- Useful: tells the LS what this empty table will hold
---@type AuctionEntry[]
local entries = {}

-- Useful: the LS can't infer the type from nil alone
---@type Frame?
local cachedFrame = nil

-- Optional: the LS already infers this from the assignment
---@type number
local x = 5
```

## What to annotate next

Once you've annotated your core classes and public functions, the LS handles most of the rest through inference. When you see a `?` type on hover (meaning the LS couldn't figure it out), that's a signal to add an annotation.