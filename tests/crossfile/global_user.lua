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

-- Global function return type
local appName = GetAppName()
--    ^ hover: (local) appName: string  def: local

-- Global function multiple returns
local num, info = GetInfo()
--    ^ hover: (local) num: number  def: local

-- Global function returning cross-file class
local cfg = GetConfig()
--    ^ hover: (local) cfg: GlobalConfig {  def: local
local dbg = cfg.debug
--    ^ hover: (local) dbg: boolean  def: local
local lvl = cfg.level
--    ^ hover: (local) lvl: number  def: local
