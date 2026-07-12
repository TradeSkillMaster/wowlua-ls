# WoW Lua Language Server - JetBrains Plugin

IntelliJ-based plugin that connects any JetBrains IDE (IntelliJ IDEA, PyCharm, WebStorm, GoLand, etc.) to the [wowlua_ls](https://github.com/TradeSkillMaster/wowlua-ls) language server via LSP.

## Features

All features are provided by the `wowlua_ls` language server:

- 9,000+ WoW API stubs built in (retail, classic, classic era)
- Event handler typing with per-event payload params
- XML frame scanning for frame definitions, virtual templates, and mixins
- TOC file support - hover, completions, go-to-definition, and diagnostics
- Metatable inference, correlated narrowing, mixin and template support
- Flavor filtering - warns on APIs unavailable in your target game version
- 70 diagnostics for type safety, nil checking, annotation correctness, and WoW-specific checks
- Diagnostic plugins for project-specific conventions
- Powerful generics, builder patterns, signature help with overload resolution
- Code completion, go-to-definition, find references, rename, semantic tokens

The plugin adds:

- Lua and TOC syntax highlighting via the shared TextMate grammar (same grammar as the VS Code extension)
- LSP client wiring via [LSP4IJ](#lsp-backend)

## Requirements

- A JetBrains IDE 2025.2 or newer, with the [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) plugin. LSP4IJ is a required dependency, so installing from the Marketplace pulls it in automatically.
- JDK 21+ (for building)
- `wowlua_ls` binary ([releases](https://github.com/TradeSkillMaster/wowlua-ls/releases) or `cargo build --release` from the repo root)

## LSP backend

The plugin drives the language server through Red Hat's [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) client on every IDE. LSP4IJ works across all JetBrains IDEs (including Community-based ones), serves files outside the project content (e.g. go-to-definition targets inside WoW API stubs), and scopes servers strictly per project. You can turn the server off without uninstalling the plugin from LSP4IJ's **Languages & Frameworks → Language Servers** settings.

## Setup

### 1. Install the `wowlua_ls` binary

Either download a release binary or build from source:

```bash
cd /path/to/wowlua-ls
cargo build --release
```

Then add the binary to your PATH.

### 2. Build the plugin

```bash
cd editors/jetbrains

# Generate Gradle wrapper (requires Gradle 8.x installed)
gradle wrapper

# Build the plugin
./gradlew buildPlugin
```

The plugin ZIP will be at `build/distributions/wowlua-ls-0.0.1.zip`.

### 3. Install

1. Open your JetBrains IDE
2. Go to **Settings → Plugins → ⚙️ → Install Plugin from Disk...**
3. Select the ZIP file from step 2

## Notes

- The plugin does not register a `.lua` file type or language of its own — syntax coloring comes from TextMate bundles, and all analysis comes from the LSP server. It coexists with other Lua plugins at the file-type level, though running two language servers on the same files is not recommended.
- Code folding and structure view come from the LSP server, not from an IDE-side parser.

## Development

To run the plugin in a sandbox IDE for development:

```bash
./gradlew runIde
```

This launches a fresh IntelliJ instance with the plugin loaded. Open any directory containing `.lua` files to test.

## Project structure

```
src/main/
├── kotlin/com/tradeskillmaster/wowluals/
│   ├── lsp4ij/
│   │   └── WowLuaLanguageServerFactory.kt # LSP4IJ client wiring (server + enablement)
│   ├── WowLuaServerPath.kt                # wowlua_ls binary resolution
│   ├── WowLuaTextMateBundleProvider.kt    # Registers the bundled TextMate grammars
│   └── WowLuaSettings.kt                  # Persistent settings (LSP4IJ server enable/disable)
└── resources/META-INF/
    ├── plugin.xml                         # Plugin descriptor (LSP4IJ server registration)
    └── textmate.xml                       # TextMate bundle registration
```
