---@diagnostic disable: unused-local
-- Cross-file defclass enum field go-to-definition test:
-- Accesses defclass fields defined in nested_enum_user.lua from another file.
---@type MY_ENUM
local enum_ref

local v = enum_ref.FLAT
--                 ^ def: external
local w = enum_ref.GROUP_A.ITEM_1
--                         ^ def: external
