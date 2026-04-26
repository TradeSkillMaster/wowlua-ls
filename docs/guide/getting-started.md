# Getting Started

## Installation

### VS Code

Install the **wowlua-ls** extension from the VS Code marketplace. It bundles the language server binary — no separate install needed.

### Other editors

Build the language server from source:

```bash
git clone https://github.com/TradeSkillMaster/wowlua-ls.git
cd wowlua-ls
cargo build --release
```

The binary is at `target/release/wowlua_ls`. Configure your editor to run it as an LSP server over stdio for Lua files.

## Your first project

Open a WoW addon folder in your editor. wowlua-ls will automatically:

1. Scan all `.lua` files in the workspace
2. Load the built-in WoW API stubs (retail + classic)
3. Resolve cross-file classes, globals, and addon namespaces
4. Start reporting diagnostics

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