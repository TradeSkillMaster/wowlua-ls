local _, ns = ... ---@class ClassCtorFieldsNS

---@class StatusType
local StatusType = {
    Active = "active",
    Inactive = "inactive",
    Pending = 1,
    Enabled = true,
}

ns.StatusType = StatusType

---@class ItemKind
---@field Special fun()
local ItemKind = {
    Weapon = "weapon",
    Armor = "armor",
}

ns.ItemKind = ItemKind

---@class GlobalClassCtor
GlobalClassCtor = {
    Foo = "foo",
    Bar = 42,
}

-- Expression-based constructor fields (comparisons, logical chains, etc.)
---@class ExprFields
ns.ExprFields = {
    CompareResult = (1 == 2),
    LogicalAnd = true and false,
    ChainedExpr = (1 == 2) and (3 < 100),
    Negated = not true,
    Concat = "a" .. "b",
    Arithmetic = 1 + 2,
    HashLen = #"test",
    OrFallback = nil or "default",
    NegExpr = -(1 + 2),
    Literal = "simple",
}
