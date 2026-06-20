---@diagnostic disable: missing-return, unused-local, create-global
-- Framework file: a generic manager + a defclass factory + an expression method.
-- A constructor self-field assigned a chained generic call (see user.lua) is typed
-- `any` by the coarse cross-file scan because the generic `T` can't be inferred
-- there. The deferred harvest recovers the precise type args from the per-file
-- engine so `expression<T & Builtins, R>` sees the bound state's fields.

---@class HarvestBase
HarvestBase = {}

---@class HarvestBuiltins
---@field rand fun(): number

---@class HarvestMgr<T: HarvestBase>
HarvestMgr = {}

---@generic T: HarvestBase
---@param state T
---@return HarvestMgr<T>
function HarvestMgr.Create(state) end

---@return self
function HarvestMgr:Suppress(x) return self end

---@generic R
---@param key string
---@param expr expression<T & HarvestBuiltins, R>
function HarvestMgr:SetFromExpr(key, expr) end

---@class HarvestObj
---@constructor __init
HarvestObj = {}

---@generic T
---@defclass T
---@param name `T`
---@return T
function HarvestObj.DefineClass(name) end

-- Builder schema whose `CreateState()` returns `built: HarvestBase` — the precise
-- built subtype (named by `@built-name`) is only resolved late in the fixpoint, so
-- a generic inferred from it (`HarvestMgr.Create(state)`) starts at the base
-- `HarvestBase` and refines to the subtype. Used by builder_module_user.lua to
-- guard against caching that stale base binding for the expression context.
---@class HarvestSchema
HarvestSchema = {}

---@built-name 1
---@return self
function HarvestSchema.Create(name) return HarvestSchema end

---@param key string
---@builds-field 1 boolean
---@return self
function HarvestSchema:AddBool(key) return self end

---@return self
function HarvestSchema:Commit() return self end

---@return built: HarvestBase
function HarvestSchema:CreateState() return {} end
