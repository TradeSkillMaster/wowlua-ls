---@diagnostic disable: unused-local
-- Cross-file addon namespace alias test: local aliased from addon namespace field
-- Tests that function return types resolve when callee root is an addon-ns alias
local addonName, ns = ...
local AliasedFactory = ns.AliasedFactory

-- Field assigned from function call via addon-ns alias
---@type NsAliasHost
local host = nil

host.widget = AliasedFactory:CreateWidget()
local w = host.widget
--    ^ hover: (local) w: NsAliasWidget {  def: local

local lbl = host.widget.label
--    ^ hover: (local) lbl: string  def: local

-- Intermediate chain: alias root → sub-table → method call
host.result = AliasedFactory.Sub:Run()
local r = host.result
--    ^ hover: (local) r: NsAliasResult {  def: local

local rid = host.result.id
--    ^ hover: (local) rid: number  def: local

-- Negative test: name not on addon namespace should not spuriously resolve
local NoSuchLib = nil
host.bad = NoSuchLib
local b = host.bad
--    ^ hover: (local) b: ?  def: local
