local _, ns = ... ---@class ClassCtorFieldsNS

local StatusType = ns.StatusType
local ItemKind = ns.ItemKind

local _a = StatusType.Active
--                    ^ hover: (field) Active: string  diag: none
local _b = StatusType.Pending
--                    ^ hover: (field) Pending: number  diag: none
local _c = StatusType.Enabled
--                    ^ hover: (field) Enabled: boolean  diag: none
-- Constructor fields should not produce undefined-field
local _d = StatusType.Inactive
--                    ^ hover: (field) Inactive: string  diag: none

-- @field annotations should still work alongside constructor fields
local _e = ItemKind.Weapon
--                  ^ hover: (field) Weapon: string  diag: none
local _f = ItemKind.Special
--                  ^ hover: (field) Special: function  diag: none

-- Global assignment class constructor fields should also work
---@type GlobalClassCtor
local _gobj = {} ---@type GlobalClassCtor
local _g = _gobj.Foo
--               ^ hover: (field) Foo: string  diag: none
local _h = _gobj.Bar
--               ^ hover: (field) Bar: number  diag: none
