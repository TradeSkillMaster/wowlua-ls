# TOC File Support

wowlua-ls provides full language server features for WoW `.toc` files — syntax highlighting, hover documentation, completions, go-to-definition, and diagnostics.

## What it does

Every WoW addon has a `.toc` file that declares metadata (title, dependencies, saved variables) and lists the Lua/XML files to load. Editing these files without tooling means memorizing field names, manually checking file paths, and catching typos only at runtime.

With TOC support enabled, you get:

- **Syntax highlighting** — Header keys, values, comments, directives, and file paths are colorized
- **Hover documentation** — Hover any standard field name to see what it does, or hover an Interface version to see which expansion it maps to
- **Completions** — Type `## ` and get suggestions for all standard fields; type a value and get context-aware options (game types, boolean values, file paths)
- **Go-to-definition** — Click a file path to jump directly to that `.lua` or `.xml` file
- **Diagnostics** — Catch common mistakes before you reload your addon

## Example

```
## Interface: 110100
## Title: My Addon
## Notes: Does useful things
## SavedVariables: MyAddonDB
## Dependencies: Ace3
## AllowLoadGameType: mainline, cata
## IconTexture: Interface\Icons\INV_Misc_QuestionMark
## LoadOnDemand: 1

# Core files
Core/Init.lua
Core/Config.lua

# UI
UI/MainFrame.xml
UI/MainFrame.lua
```

Hovering `Interface` shows its documentation. Hovering `110100` shows "The War Within 11.1.x". Hovering `mainline` shows "Retail (The War Within, etc.)".

Completions after `## ` suggest `Title`, `Notes`, `Author`, `SavedVariables`, etc. — filtered to exclude fields you've already declared.

## Supported fields

wowlua-ls recognizes all standard TOC fields:

| Field | Description |
|---|---|
| `Interface` | WoW client interface version (required) |
| `Title` | Addon display name in the addon list |
| `Notes` | Short description tooltip |
| `Author` | Addon author(s) |
| `Version` | Version string |
| `SavedVariables` | Account-wide persisted globals |
| `SavedVariablesPerCharacter` | Per-character persisted globals |
| `Dependencies` | Required addon dependencies (alias: `RequiredDeps`) |
| `OptionalDeps` | Optional load-order dependencies |
| `LoadOnDemand` | Load only when explicitly requested (`1`/`0`) |
| `DefaultState` | Whether enabled by default (`enabled`/`disabled`) |
| `LoadWith` | Load this addon when any listed addon loads |
| `LoadManagers` | Addons that manage loading this addon |
| `IconTexture` | Addon compartment icon |
| `AddonCompartmentFunc` | Compartment click handler |
| `AddonCompartmentFuncOnEnter` | Compartment hover-enter handler |
| `AddonCompartmentFuncOnLeave` | Compartment hover-leave handler |
| `AllowLoadGameType` | Restrict to specific game flavors: `mainline`, `classic`, `vanilla`, `cata`, `wrath`, `tbc`, `mists` |
| `OnlyBetaAndPTR` | Restrict to test realms only |
| `Secure` | Blizzard-signed secure code marker |

Additionally:
- **`X-*` fields** — Custom addon metadata (e.g. `X-Website`, `X-Curse-Project-ID`) are recognized and won't trigger unknown-field warnings
- **Locale suffixes** — Fields like `Title-deDE`, `Notes-enUS`, `Category-enUS` are recognized as localized variants

## Per-line directives

TOC files support directives that prefix individual file paths:

```
[AllowLoadGameType mainline]Retail/FrameOverrides.lua
[AllowLoadGameType classic]Classic/Compatibility.lua
[Family]Shared/Utils.lua
[Game]Locale/Strings.lua
```

| Directive | Description |
|---|---|
| `[AllowLoadGameType ...]` | Only load this file on specified game flavors |
| `[Family]` | Path variable expanding to game family subdirectory |
| `[Game]` | Path variable expanding to specific game subdirectory |

Hover a directive to see its documentation.

## Diagnostics

TOC files have their own set of diagnostics:

| Code | Severity | Description |
|---|---|---|
| `toc-missing-interface` | Warning | Required `## Interface:` field is missing |
| `toc-duplicate-header` | Warning | Same header key appears more than once |
| `toc-unknown-header` | Hint | Header not in the known catalog and not an `X-*` custom field |
| `toc-invalid-interface` | Error | Interface value is not a valid numeric version |
| `toc-nonexistent-file` | Warning | Referenced file does not exist on disk |
| `toc-invalid-value` | Warning | Value doesn't match the expected format for its field |

## Go-to-definition

Click any file path in a TOC file to navigate directly to that file. This works for both `.lua` and `.xml` references. Paths are resolved relative to the TOC file's directory.

Files referenced through `[Family]` or `[Game]` path variables cannot be resolved (they depend on the client's install configuration).

## No configuration needed

TOC support is enabled automatically when you open a `.toc` file. No `.wowluarc.json` settings are required. The language server activates on `.toc` files the same way it does for `.lua` files.
