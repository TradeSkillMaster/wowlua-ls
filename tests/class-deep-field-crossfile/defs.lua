-- Fields written through a nested path under a `@class` global reach other
-- files: through a constructor entry (`Config`) or a sub-namespace assigned on
-- the class (`Util`), as the same writes on a plain global do.

---@class DeepFieldRoot
DeepFieldRoot = { Config = { mode = "full" } }
DeepFieldRoot.Config.scale = 2
DeepFieldRoot.Util = {}
DeepFieldRoot.Util.name = "x"
DeepFieldRoot.Util.Sub = {}
DeepFieldRoot.Util.Sub.depth = 3

-- `Missing` is neither a field of the class nor assigned on it: not invented.
DeepFieldRoot.Missing.value = 1

-- A write inside a function only registers that the field exists, with no type
-- to give it: not applied.
function DeepFieldRoot.Resize(value)
    DeepFieldRoot.Util.width = value
end

-- A segment a call assigns keeps the call's type.
DeepFieldRoot.store = CreateDeepFieldStore()
DeepFieldRoot.store.ready = true

-- A `@field`-declared `table` keeps its declared type.
---@class DeepFieldDeclared
---@field Opts table
DeepFieldDeclared = { Opts = {} }
DeepFieldDeclared.Opts.width = 10

---@class DeepFieldProfile
---@field name string

---@class DeepFieldDb
---@field profile DeepFieldProfile

-- A segment a `@field` or the write's own annotation types as a class keeps
-- that class's fields.
---@class DeepFieldApp
---@field db DeepFieldDb
DeepFieldApp = {}
DeepFieldApp.db = {}
DeepFieldApp.db.profile = {}
DeepFieldApp.db.extra = 5
---@type DeepFieldDb
DeepFieldApp.typedDb = {}
DeepFieldApp.typedDb.profile = {}
DeepFieldApp.typedDb.extra = 5
