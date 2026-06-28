-- Regression: pcall's `(true, returns<F>) | (false, string)` tuple-union with
-- sibling narrowing must read EACH call's own generic binding `F`, not the most
-- recent sibling call's. A `local status` redeclared (one shared symbol,
-- versioned) across several `status, x = pcall(...)` lines used to make an
-- earlier call's result inherit a LATER call's `returns<F>` — e.g. `decoded`
-- (from `pcall(decode, ..)`, returns string) was mistyped `table?` (the return
-- of the unrelated `pcall(deser, ..)` below it), producing a false
-- type-mismatch when passed back into a string-typed callee.
-- Fixed in analysis/resolve.rs::find_generic_subs_from_inner (match the sibling
-- FunctionCall whose callee == this OverloadNarrow's func_expr).
---@diagnostic disable: redefined-local, unused-local, unreachable-code

---@param s string
---@return string
local function decode(s) return s end

---@param s string
---@return table?
local function deser(s) return nil end

local function importData(input)
  local data
  if input == "{" then
    local status
    status, data = pcall(deser, input)
  else
    local status, decoded = pcall(decode, input)
    if not status then error("bad") return end
    local probe = decoded
--                ^ hover: (local) decoded: string
    -- `decoded` stays `string` (returns<decode>), not `table?` (returns<deser>
    -- from the `data` pcall below); passing it to `decode` is then clean.
    local status, decompressed = pcall(decode, decoded)
    if not status then error("bad") return end
    status, data = pcall(deser, decompressed)
  end
  return data
end
