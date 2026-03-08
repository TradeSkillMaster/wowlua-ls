-- Cross-file test: file B uses a different variable name but sees file A's fields
local addonName, addon = ...
local v = addon.version
--    ^ hover: v: number  def: local
local t = addon.title
--    ^ hover: t: string  def: local
local lib = addon.Lib
--    ^ hover: lib: MyLib  def: local
addon.Lib:GetName()
--        ^ hover: GetName: fun()  def: external
local e = addon.Lib.enabled
--    ^ hover: e: boolean  def: local
local loc = addon.Locale
--    ^ hover: loc: {  def: local
addon.Locale.GetTable()
--           ^ hover: GetTable: fun(): table  def: external
local comp = addon.MyComponent
--    ^ hover: comp: MyComponent  def: local
local act = addon.MyComponent.active
--    ^ hover: act: boolean  def: local
