---@diagnostic disable: unused-local
-- Cross-file reads of the globals assigned in defs.lua / defs_more.lua.

local _, ns = ...

local frame = MainFrame
--    ^ hover: (local) frame: Frame
local row = RowFrame
--    ^ hover: (local) row: Frame & AssignedRowTemplate
RowFrame:SetRowData()
--       ^ hover: (method) function AssignedRowTemplate:SetRowData()
local name = MainFrameName
--    ^ hover: (local) name: string
local count = ParsedCount
--    ^ hover: (local) count: number?

local options = Options
--    ^ hover: (local) options: {\n  Callback: any,\n  Colors: {Primary: string, Secondary: string},\n  Refresh: fun(force: boolean): boolean,\n  Size: number,\n  Title: string\n}
local title = Options.Title
--                     ^ hover: (field) Title: string  def: external
local primary = Options.Colors.Primary
--    ^ hover: (local) primary: string
local secondary = Options.Colors.Secondary
--    ^ hover: (local) secondary: string
local refreshed = Options.Refresh(true)
--    ^ hover: (local) refreshed: boolean

-- A `nil` constructor entry is untyped: calling it or passing it to a typed
-- parameter is not flagged.
---@param s string
local function takesString(s) end
takesString(Options.Callback)
Options.Callback()
ns.Hooks.onLoad()
takesString(ns.Hooks.onLoad)
---@param owner HookOwner
local function runHooks(owner)
    owner.opts.onDone()
    takesString(owner.opts.onDone)
end

local value = RenamedGlobal:GetValue()
--    ^ hover: (local) value: number
local shape = TypedGlobal
--    ^ hover: (local) shape: TypedShape
TypedGlobal:Resize(2)
--          ^ hover: (method) function TypedShape:Resize(n: number)
local typedLabel = TypedGlobal.label
--    ^ hover: (local) typedLabel: string
---@param other TypedShape
local function readOther(other)
    print(other.label)
    --          ^ diag: undefined-field
end
local tableLastLogin = table.lastLogin
--                           ^ diag: undefined-field
local savedVersion = SavedTable.version
--    ^ hover: (local) savedVersion: number

local handlerLog = Handlers.log
--    ^ hover: (local) function handlerLog(...: any)
local handlerFrame = Handlers.sub.frame
--    ^ hover: (local) handlerFrame: Frame
local handlerCount = Handlers.count
--    ^ hover: (local) handlerCount: number?

local special = Registry.Kinds.Special
--    ^ hover: (local) special: {\n  id: number\n}
local basic = Registry.Kinds.Basic
--    ^ hover: (local) basic: number

local aliasA = AliasA
--    ^ hover: (local) aliasA: Frame & AssignedRowTemplate
local lateTyped = LateTyped
--    ^ hover: (local) function lateTyped(seconds: number, callback: TimerCallback)

local scale = AddonDB.profile.scale
--    ^ hover: (local) scale: number
local dbCount = AddonDB.global.count
--    ^ hover: (local) dbCount: number

local countAlias = CountAlias
--    ^ hover: (local) countAlias: number?
local frameAlias = FrameAlias
--    ^ hover: (local) frameAlias: Frame
local size = SizeRef
--    ^ hover: (local) size: number
local primaryRef = PrimaryColorRef
--    ^ hover: (local) primaryRef: string
local backpack = BackpackRef
--    ^ hover: (local) backpack: number
local after = AfterRef
--    ^ hover: (local) function after(seconds: number, callback: TimerCallback)
local api = ApiRef
--    ^ hover: (local) api: {\n  version: number\n}
local version = ApiRef.version
--    ^ hover: (local) version: number
