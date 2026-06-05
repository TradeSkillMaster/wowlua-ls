-- Consumer side: verify incrementally-built table fields resolve correctly
-- and no false-positive field-type-mismatch fires on the constructor.

---@type addonTableIncremental
local ns = nil

-- All fields (both constructor and incremental) should be accessible
local _r = ns.Constants.IsRetail
--                      ^ hover: (field) IsRetail: boolean  def: external
local _e = ns.Constants.Events
--                      ^ hover: (field) Events: table  def: external
local _f = ns.Constants.DefaultFont
--                      ^ hover: (field) DefaultFont: string  def: external
local _l = ns.Constants.LayerStep
--                      ^ hover: (field) LayerStep: number  def: external

