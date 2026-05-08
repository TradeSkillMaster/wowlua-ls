-- Cross-file test: file B uses a different variable name but sees file A's fields
local addonName, addon = ...
local v = addon.version
--    ^ hover: (local) v: number  def: local
--              ^ def: external
local t = addon.title
--    ^ hover: (local) t: string  def: local
--              ^ def: external
local lib = addon.Lib
--    ^ hover: (local) lib: MyLib {  def: local
addon.Lib:GetName()
--        ^ hover: (method) function MyLib:GetName()  def: external
local e = addon.Lib.enabled
--    ^ hover: (local) e: boolean  def: local
local loc = addon.Locale
--    ^ hover: (local) loc: {  def: local
addon.Locale.GetTable()
--           ^ hover: (field) function GetTable()  def: external
local comp = addon.MyComponent
--    ^ hover: (local) comp: MyComponent {  def: local
local act = addon.MyComponent.active
--    ^ hover: (local) act: boolean  def: local
-- Method chain: ChainApp resolves to a sub-table (auto-created under the addon ns).
-- It must NOT resolve to MyLib (the type of the inner function call's target).
local chainApp = addon.ChainApp
--    ^ hover: (local) chainApp: {  def: local
local chainLoc = addon.ChainApp.Locale
--    ^ hover: (local) chainLoc: {  def: local
addon.ChainApp.Locale.GetTable()
--                     ^ hover: (field) function GetTable()  def: external
-- Void method on addon namespace → inferred nil
local resetResult = addon:Reset()
--    ^ hover: (local) resetResult: nil  def: local
