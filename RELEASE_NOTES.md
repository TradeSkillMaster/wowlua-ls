### Bug Fixes

- Fix `@type` override not persisting when a local is reassigned, and fix `fun()` sibling narrowing in tuple-union return types

### Improvements

- Add table constructor completions for cross-file `table<K,V>` locals — when bracket-assigning into a typed table (`NPCs[id] = { … }`), field completions from the value class are now suggested (#49)
