### Bug Fixes

- Frame and widget methods such as `RegisterEvent` are no longer shadowed by an untyped `any?` field in generated stubs.
- Fixed false `field-type-mismatch` errors on locals that alias a value narrowed by a `type()` guard across branch merges.
- Go-to-definition on a method of a built-in namespace table (e.g. `Settings`) that your workspace also redefines now offers both the stub and workspace sites.
- Fixed cross-file "undefined type" errors in JetBrains multi-root (attached-project) workspaces.

### Improvements

- The JetBrains plugin now uses LSP4IJ as its sole backend; the native backend and its settings toggle are gone, and LSP4IJ installs automatically as a required dependency.
