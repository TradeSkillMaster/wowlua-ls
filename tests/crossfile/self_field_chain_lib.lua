-- Cross-file chained-funcall self-field test: a self-field assigned from a
-- *chained* call (`self.x = Foo():Bar()`) whose receiver is itself a call.
-- The funcall self-field scanner can only resolve callees rooted at a plain
-- name chain, so it skips chained receivers; the bare scanner then registers
-- the field existence-only as `any` so cross-file reads on a re-declared
-- @class don't false-positive as `undefined-field`. (Registration for the
-- `self.db = LibStub("AceDB-3.0"):New(...)` pattern in real addons.) `any`,
-- not a guessed `table`: the chain's return type is unknown to the coarse
-- scan, and a concrete `table` placeholder leaks into reads — see
-- self_field_chain_user.lua.

---@class ChainLib
local Lib = {}
function Lib:Build() return self end

---@return ChainLib
local function GetLib() return setmetatable({}, { __index = Lib }) end

---@class ChainHost
local Host = {}

function Host:Setup()
    self.handle = GetLib():Build()
end
