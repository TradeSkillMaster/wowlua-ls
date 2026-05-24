### New

- Completions now suggest inherited callback event parameters — when typing inside an `@event`-narrowed callback, the completion list includes parameters from parent class event signatures ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/events.html))

### Bug Fixes

- Fixed `params<T>` type resolution failing inside event-narrowed branches, which could produce incorrect parameter types for callbacks registered with typed event handlers
