-- Cross-file global function and variable test: usage

-- Global variable access
local ver = MY_VERSION
--    ^ hover: (local) ver: string  def: local
local cnt = MY_COUNT
--    ^ hover: (local) cnt: number  def: local
local en = MY_ENABLED
--     ^ hover: (local) en: boolean  def: local

-- Global table method calls (dot syntax)
local len = UtilLib.GetLength("hello")
--    ^ hover: (local) len: number  def: local

-- Global table method calls (colon syntax)
local sum = UtilLib:Add(1, 2)
--    ^ hover: (local) sum: number  def: local

-- Global table method with named returns
UtilLib.Search("hello")
--      ^ hover: (field) function Search(text: string)\n  -> found: boolean, position: number  def: external

-- Global function return type
local appName = GetAppName()
--    ^ hover: (local) appName: string  def: local

-- Global function multiple returns
local num, info = GetInfo()
--    ^ hover: (local) num: number  def: local

-- Global function with named return values
local hasItem = HasSlotItem(1)
--    ^ hover: (local) hasItem: boolean  def: local
HasSlotItem(1)
-- ^ hover: (global) function HasSlotItem(slot: number)\n  -> hasItem: boolean  def: external

local iName, iCount = GetItemDetails(123)
--    ^ hover: (local) iName: string  def: local
GetItemDetails(123)
-- ^ hover: (global) function GetItemDetails(id: number)\n  -> itemName: string, itemCount: number  def: external

-- Global function returning cross-file class
local cfg = GetConfig()
--    ^ hover: (local) cfg: GlobalConfig {  def: local
local dbg = cfg.debug
--    ^ hover: (local) dbg: boolean  def: local
local lvl = cfg.level
--    ^ hover: (local) lvl: number  def: local

-- _G.field globals: table with methods
local valid = MyGlobalAPI:IsValid("test")
--    ^ hover: (local) valid: boolean  def: local

-- _G.field globals: standalone function
local result = GlobalHelper(42)
--      ^ hover: (local) result: number  def: local

-- _G.field globals: variable
local gc = GLOBAL_CONST
--    ^ hover: (local) gc: string  def: local

-- Global function that never returns a value → inferred nil
local act = DoAction()
--    ^ hover: (local) act: nil  def: local
local actBare = DoActionBare()
--    ^ hover: (local) actBare: nil  def: local

-- Global table method with no return → inferred nil
local task = UtilLib:RunTask()
--    ^ hover: (local) task: nil  def: local

-- Field-position token must NOT resolve to a same-named global.
-- DataLib.inner resolves as generic "table", so the chain walk for
-- DataLib.inner.MY_ENABLED_INFO fails. MY_ENABLED_INFO must NOT fall
-- back to the global MY_ENABLED_INFO.
local di = DataLib.inner.MY_ENABLED_INFO
--                       ^ def: None
