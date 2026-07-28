### Bug Fixes

- Vendored libraries that are symlinked or hardlinked into multiple addons (e.g. a shared folder linked into each addon's `Libs/`) are no longer scanned more than once, eliminating duplicate definitions and inconsistent hover / go-to-definition results.
