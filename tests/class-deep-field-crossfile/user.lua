---@diagnostic disable: unused-local

local scale = DeepFieldRoot.Config.scale
--    ^ hover: (local) scale: number
local mode = DeepFieldRoot.Config.mode
--    ^ hover: (local) mode: string
local debug = DeepFieldRoot.Config.debug
--    ^ hover: (local) debug: boolean
local name = DeepFieldRoot.Util.name
--    ^ hover: (local) name: string
local depth = DeepFieldRoot.Util.Sub.depth
--    ^ hover: (local) depth: number

local missing = DeepFieldRoot.Missing
--                            ^ diag: undefined-field

---@param n number
local function takesNumber(n) return n end
takesNumber(DeepFieldRoot.Util.width)
local opts = DeepFieldDeclared.Opts
--                             ^ hover: (field) Opts: table
local storeCount = DeepFieldRoot.store.count
--    ^ hover: (local) storeCount: number
local profileName = DeepFieldApp.db.profile.name
--    ^ hover: (local) profileName: string
local dbExtra = DeepFieldApp.db.extra
--                              ^ diag: undefined-field
local typedName = DeepFieldApp.typedDb.profile.name
--    ^ hover: (local) typedName: string
local typedExtra = DeepFieldApp.typedDb.extra
--                                      ^ diag: undefined-field
