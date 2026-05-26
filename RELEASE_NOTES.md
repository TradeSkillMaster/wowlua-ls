### Bug Fixes
- Fix `params<F>` narrowing inside `@event` action handlers — parameter type projections now resolve correctly when the handler is registered via an event annotation.

### Improvements
- Add `Param.type_name` to the plugin API, allowing plugins to inspect the resolved type name of function parameters. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/plugins.html))
