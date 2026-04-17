-- Cross-file test: file B uses a different variable name but sees file A's fields
local addonName, addon = ...
local v = addon.version
--    ^ hover: (global) v: number  def: local
--              ^ def: external
local t = addon.title
--    ^ hover: (global) t: string  def: local
--              ^ def: external
local lib = addon.Lib
--    ^ hover: (global) lib: MyLib {  def: local
addon.Lib:GetName()
--        ^ hover: (method) function MyLib:GetName()  def: external
local e = addon.Lib.enabled
--    ^ hover: (global) e: boolean  def: local
local loc = addon.Locale
--    ^ hover: (global) loc: {  def: local
addon.Locale.GetTable()
--           ^ hover: (field) function GetTable()  def: external
local comp = addon.MyComponent
--    ^ hover: (global) comp: MyComponent {  def: local
local act = addon.MyComponent.active
--    ^ hover: (global) act: boolean  def: local
-- Method chain: ChainApp should NOT resolve to MyLib class
local chainApp = addon.ChainApp
--    ^ hover: (global) chainApp: ChainApp {  def: local
local chainLoc = addon.ChainApp.Locale
--    ^ hover: (global) chainLoc: {  def: local
addon.ChainApp.Locale.GetTable()
--                     ^ hover: (field) function GetTable()  def: external
