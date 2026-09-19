---@meta _
-- Secret-value semantics (retail 12.x) that Blizzard's machine-readable API
-- documentation doesn't carry. Secrecy of ordinary APIs is generated from the
-- docs' `SecretReturns`/`SecretWhen*`/`SecretArguments` keys; these entries cover
-- the builtins that *test* secrecy (their result narrows the argument), the two
-- that convert between secret and ordinary values, and the Lua string functions
-- Blizzard documents as accepting secrets from addon code.
--
-- Which builtins accept secret arguments is curated in
-- `stub_gen/secret_stubs.rs` (`DOCUMENTED_ARGS_OVERRIDES`), not here: the
-- generator rewrites Blizzard's documented policy onto the vendor stubs, so the
-- decision has to be made where that rewrite happens.

---Returns true if a supplied value is a secret value.
---
---[Documentation](https://warcraft.wiki.gg/wiki/API_issecretvalue)
---@param value LuaValueReference
---@return boolean isSecret
---@secret-guard value is-secret
function issecretvalue(value) end

---Returns true if the immediate calling function has appropriate permissions to access and operate on a specific value.
---
---[Documentation](https://warcraft.wiki.gg/wiki/API_canaccessvalue)
---@param value LuaValueReference
---@return boolean canAccessValue
---@secret-guard value accessible
function canaccessvalue(value) end

---Returns true if the immediate calling function has appropriate permissions to access and operate on all supplied values.
---
---[Documentation](https://warcraft.wiki.gg/wiki/API_canaccessallvalues)
---@param ... LuaValueReference values
---@return boolean canAccessAllValues
---@secret-guard ... accessible
function canaccessallvalues(...) end

---Returns true if any supplied value is a secret value.
---
---[Documentation](https://warcraft.wiki.gg/wiki/API_hasanysecretvalues)
---@param ... LuaValueReference values
---@return boolean isAnyValueSecret
---@secret-guard ... any-secret
function hasanysecretvalues(...) end

---Converts all supplied values to secret values, preventing most operations on them from occurring on tainted code paths.
---
---[Documentation](https://warcraft.wiki.gg/wiki/API_secretwrap)
---@generic T
---@param value T
---@param ... any
---@return secret<T> ... wrapped
function secretwrap(value, ...) end

---Unwraps all supplied secrets, converting them back to regular values.
---
---[Documentation](https://warcraft.wiki.gg/wiki/API_secretunwrap)
---@generic T
---@param value secret<T>
---@param ... any
---@return T ... unwrapped
function secretunwrap(value, ...) end

---Returns a formatted version of its variable number of arguments following the description given in its first argument.
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-string.format"])
---
---@param s string|number
---@param ... any
---@return string
---@nodiscard
---@secret-args tainted
function string.format(s, ...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_strjoin)
---@param delim string|number
---@param ... string|number
---@return string
---@nodiscard
---@secret-args tainted
function string.join(delim, ...) end

---[Documentation](https://warcraft.wiki.gg/wiki/API_string.concat)
---@param ... string
---@return string
---@nodiscard
---@secret-args tainted
function string.concat(...) end
