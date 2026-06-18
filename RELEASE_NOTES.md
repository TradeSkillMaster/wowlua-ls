### Improvements

- Code folding now extends through a block's closing keyword (`end`, `}`, `until …`). Line-folding editors like VS Code collapse the closer along with the body, while character-precise editors like IntelliJ keep it rendered inline after the placeholder (`if foo then … end`).

### Bug Fixes

- Fixed a deadlock that could freeze IntelliJ while the language server was busy. The server now buffers its stdin so it keeps draining client input even under load, with a watchdog that detects stalls in the main loop.
- Fixed a false `undefined-field` diagnostic on fields initialized in a nested `@class` constructor when that class is accessed from another file.
