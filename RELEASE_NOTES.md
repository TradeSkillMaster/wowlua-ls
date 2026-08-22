**Bug Fixes**
- Fixed the language server hanging on load when an addon `@class` has its fields assigned across many files, caused by an unbounded re-entrancy cycle in the deferred cross-file field harvest.
