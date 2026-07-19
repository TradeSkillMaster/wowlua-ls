-- Regression (review WARNING): a field assigned from an *anonymous* callee — a
-- call whose callee has no name identifier, e.g. the ternary-dispatch idiom
-- `(cond and F1 or F2)("Class")` — must NOT be mis-typed to the string-named
-- `@class`. The coarse scan's `first_string_literal_arg` defclass hint is paired
-- with a non-empty callee chain, and an anonymous callee yields an empty chain,
-- so the hint is dropped and the field settles on a neutral `table` placeholder.
-- A genuine NAMED-defclass call (`F1("Widget")`) keeps its class-name hint.
---@diagnostic disable: unused-local, missing-return
local name, ns = ...

---@class Widget
local Widget = {}

---@param key string
---@return Widget
local function F1(key) end
---@param key string
---@return Widget
local function F2(key) end

---@type boolean
local cond

-- Anonymous callee: empty chain -> hint suppressed -> neutral placeholder.
ns.anon = (cond and F1 or F2)("Widget")
-- ^ hover: (field) anon: table

-- Named callee: the defclass class-name hint still applies.
ns.named = F1("Widget")
-- ^ hover: (field) named: Widget
