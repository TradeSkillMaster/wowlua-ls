---@diagnostic disable: create-global
-- Globals assigned a call, a reference, or a table constructor, read from
-- user.lua in another file.

local _, ns = ...

---@class AssignedRowTemplate
local AssignedRowTemplate = {}
function AssignedRowTemplate:SetRowData() end

-- Calls are typed from the call's resolved return, so a generic callee binds
-- (`Frame`), a template keeps its intersection, and a chained call is typed from
-- its outermost call.
MainFrame = CreateFrame("Frame")
RowFrame = CreateFrame("Frame", nil, UIParent, "AssignedRowTemplate")
MainFrameName = CreateFrame("Frame"):GetName()
ParsedCount = tonumber("5")

-- A table constructor's named entries are the global's fields. A `nil` entry is
-- a field assigned later, so it is untyped rather than `nil`.
Options = {
    Size = 12,
    Title = "Main",
    Colors = { Primary = "red" },
    Callback = nil,
    Refresh = nil,
}
-- A deep write extends a constructor sub-table.
Options.Colors.Secondary = "blue"

-- A method replaces a same-named constructor entry.
---@param force boolean
---@return boolean
function Options.Refresh(force) return force end

-- A `nil` entry is untyped in namespace and self-field constructors too.
ns.Hooks = { onLoad = nil }

---@class HookOwner
local HookOwner = {}
ns.HookOwner = HookOwner
function HookOwner:Init()
    self.opts = { onDone = nil }
end

-- A global table typed as a differently-named class resolves as that class.
---@class RenamedClass
RenamedGlobal = {}
---@return number
function RenamedGlobal:GetValue() return 1 end

---@class TypedShape
---@field size number
---@field Resize fun(self: TypedShape, n: number)

---@type TypedShape
TypedGlobal = { size = 1 }
-- A `---@type` global's own members don't join its class, and `---@type table`
-- doesn't alias the stdlib `table`.
TypedGlobal.label = "shape"
---@type table
SavedTable = { version = 1 }
SavedTable.lastLogin = 2

-- A typed write replaces a constructor entry: a literal, a call, or a reference.
Handlers = { log = nil, sub = { frame = nil }, count = nil }
Handlers.log = print
Handlers.sub.frame = CreateFrame("Frame")
Handlers.count = tonumber("5")

-- A table replacing a constructor sub-table keeps what deep writes put in it.
Registry = { Kinds = {} }
Registry.Kinds.Special = { id = 1 }
Registry.Kinds = { Basic = 1 }

-- `X = Y` chains share a call-typed global's type however they're ordered.
AliasA = AliasB
AliasB = AliasC
AliasC = RowFrame

-- A later assignment types a global an earlier one left untyped.
LateTyped = Options.Missing
LateTyped = C_Timer.After

-- Constructors of the same global merge across files (see defs_more.lua).
AddonDB = AddonDB or { profile = { scale = 1 } }

-- References to a global scanned later (defs_more.lua), a global typed only by
-- its call, table fields, stub tables, and the addon namespace.
CountAlias = LaterCount
FrameAlias = MainFrame
SizeRef = Options.Size
PrimaryColorRef = Options.Colors.Primary
BackpackRef = Enum.BagIndex.Backpack
AfterRef = C_Timer.After
ns.Api = { version = 2 }
ApiRef = ns.Api
