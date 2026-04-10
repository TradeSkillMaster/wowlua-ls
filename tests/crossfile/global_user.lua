-- Cross-file global function and variable test: usage

-- Global variable access
local ver = MY_VERSION
--    ^ hover: (global) ver: string  def: local
local cnt = MY_COUNT
--    ^ hover: (global) cnt: number  def: local
local en = MY_ENABLED
--     ^ hover: (global) en: boolean  def: local

-- Global table method calls (dot syntax)
local len = UtilLib.GetLength("hello")
--    ^ hover: (global) len: number  def: local

-- Global table method calls (colon syntax)
local sum = UtilLib:Add(1, 2)
--    ^ hover: (global) sum: number  def: local

-- Global function return type
local appName = GetAppName()
--    ^ hover: (global) appName: string  def: local

-- Global function multiple returns
local num, info = GetInfo()
--    ^ hover: (global) num: number  def: local

-- Global function returning cross-file class
local cfg = GetConfig()
--    ^ hover: (global) cfg: GlobalConfig {  def: local
local dbg = cfg.debug
--    ^ hover: (global) dbg: boolean  def: local
local lvl = cfg.level
--    ^ hover: (global) lvl: number  def: local
