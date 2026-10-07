**Bug Fixes**

- `SecondsFormatter`, `AbbreviatedNumberFormatter` and `NumericRuleFormatter` are now accepted where a `NumericFormatter` is expected.
- Leaving out widget-method arguments that the wiki marks optional no longer reports `missing-parameter` (for example, the color on `Cooldown:SetSwipeTexture` or `relativeTo` on `Line:SetStartPoint`).
- `date("*t")` fields are now typed as integers instead of `integer|string`.
- Methods passed by name to AceTimer's `ScheduleTimer`/`ScheduleRepeatingTimer` or AceDB's `RegisterCallback` no longer report `unused-function`, and a misspelled method name is now flagged.
