**New**

- Secret-value analysis for retail 12.x. APIs that return secrets while addon restrictions are active are typed `secret<T>`, secrecy is tracked through locals, fields, and returns, and the operations that error on a secret are reported by six new diagnostics: `secret-arithmetic`, `secret-comparison`, `secret-condition`, `secret-table-key`, `secret-argument`, and `secret-access`. Each one offers a quick fix that wraps the statement in a `canaccessvalue` guard. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/secrets.html))
- Secrets are cleared where you've proven they're safe: `issecretvalue` / `canaccessvalue` guards, `C_Secrets` calls, and restriction checks all clear secrecy in the code they guard. Hover on an API shows when it returns secrets and what it requires. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/secrets.html#guarding-by-context))
- Zed extension. Publishing to Zed's extension registry is pending; until it lands, install it with **Install Dev Extension** pointed at `editors/zed`. It downloads the server binary on first use. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/getting-started.html))
- Flavor guards are now inferred from `WOW_PROJECT_ID` comparisons: `local isRetail = WOW_PROJECT_ID == WOW_PROJECT_MAINLINE` narrows without a `@flavor-narrows` annotation, including when the boolean is read from another file. ([docs](https://tradeskillmaster.github.io/wowlua-ls/guide/flavor-filtering.html))

**Bug Fixes**

- Fixed runaway memory growth on large workspaces that could exhaust system memory.
- Diagnostics no longer vary between runs on unchanged code.
- Flavor narrowing now applies to the right-hand side of `and`, so `WOW_PROJECT_ID == WOW_PROJECT_MAINLINE and RetailOnlyAPI()` no longer reports `wrong-flavor-api`.

**Improvements**

- Lower memory use on large workspaces, both during the startup scan and while idle afterwards.
