-- Shadowing an imported stub open string-enum alias.
--
-- `UnitToken` is a stub alias defined as `string` + `---|"player"`-style lines, so
-- its completion literals are imported into every file via `ext.alias_string_literals`.
-- Redefining it locally to a *non-string* type must CLEAR those imported literals —
-- they no longer describe the alias. Regression: the clear was gated on the
-- redefinition also resolving to `string`, so a non-string redefinition skipped both
-- the insert and the clear, leaving the stub's 20 unit tokens stale and still offered
-- as bogus completions inside a `UnitToken`-typed string argument.

---@alias UnitToken number

---@param u UnitToken
local function localTaxi(u) end

-- No completions (the imported stub literals were cleared), and passing a string to
-- the now-`number` parameter is a genuine type error.
localTaxi("")
--         ^ comp: none  diag: type-mismatch
