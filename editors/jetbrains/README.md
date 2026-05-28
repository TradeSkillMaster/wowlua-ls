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

- Lua syntax highlighting (keywords, strings, comments, numbers)
- Comment toggling (Ctrl+/ / Cmd+/)
- Brace matching

## Requirements

- JetBrains IDE 2024.1 or newer
- [LSP4IJ](https://plugins.jetbrains.com/plugin/23257-lsp4ij) plugin (install from the JetBrains Marketplace)
- JDK 17+ (for building)
- `wowlua_ls` binary ([releases](https://github.com/TradeSkillMaster/wowlua-ls/releases) or `cargo build --release` from the repo root)

## Setup

### 1. Install the `wowlua_ls` binary

Either download a release binary or build from source:

```bash
cd /path/to/wowlua-ls
cargo build --release
```

Then add the binary to your PATH, or configure the path in the plugin settings (see below).

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

### 4. Configure (optional)

If `wowlua_ls` is not on your PATH, configure the binary location:

**Settings → Tools → WoW Lua LS → Server path**

## Notes

- This plugin registers the `.lua` file extension. If you have another Lua plugin installed (EmmyLua, Luanalysis), the IDE will ask you to choose which one handles `.lua` files.
- Code folding and structure view come from the LSP server, not the plugin's own parser.

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
│   ├── WowLuaLanguage.kt            # Language singleton
│   ├── WowLuaFileType.kt            # File type for .lua
│   ├── WowLuaFile.kt                # PSI file
│   ├── WowLuaIcons.kt               # Icon constants
│   ├── WowLuaTokenTypes.kt          # Lexer token types
│   ├── WowLuaLexer.kt               # Lua lexer for syntax highlighting
│   ├── WowLuaParser.kt              # Minimal pass-through parser
│   ├── WowLuaParserDefinition.kt    # Parser definition
│   ├── WowLuaSyntaxHighlighter.kt   # Syntax highlighter + factory
│   ├── WowLuaLspServerProvider.kt   # LSP4IJ language server factory
│   ├── WowLuaSettings.kt            # Persistent settings (server path)
│   ├── WowLuaSettingsConfigurable.kt # Settings UI
│   ├── WowLuaCommenter.kt           # Comment toggling
│   └── WowLuaBraceMatcher.kt        # Brace matching
└── resources/
    ├── META-INF/plugin.xml           # Plugin descriptor
    └── icons/
        ├── lua.svg                   # File type icon
        └── pluginIcon.svg            # Plugin marketplace icon
```
