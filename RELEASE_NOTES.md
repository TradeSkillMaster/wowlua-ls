### Bug Fixes

- Fixed a false positive in the `unused-function` diagnostic where functions dispatched dynamically via `keyof` indexing were incorrectly reported as unused.
