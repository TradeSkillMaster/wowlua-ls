**Bug Fixes**

- Redeclaring an existing alias (built-in or from another file) with only literal `---|` members now adds them to it instead of replacing it.
- Fixed false `access-private`/`access-protected` through a `---@class`-declared local whose name differs from the class. Completion and hover now list those fields.
- Retail 12.1.5 APIs (`C_Intl.GetCurrentLocale`, `SetRoundLayoutToNearestPixel`, …) are no longer marked Forever-only.
