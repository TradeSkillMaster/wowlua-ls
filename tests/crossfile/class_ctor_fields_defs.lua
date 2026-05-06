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
