**New**

- Field visibility (`@private` / `@protected`) now applies to library and addon-namespace tables — the pattern where a table *is* the class. The declaring file can freely read and write its own private/protected fields (including the reload-safe `Lib = Lib or {}` idiom), while other files get `access-private` / `access-protected`. Visibility can now also be declared inline on the assignment instead of in the `@class` block. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/classes.html#library-and-namespace-tables))
