---@diagnostic disable: unused-local

-- ═══════════════════════════════════════════════════════════════════════════════
-- Basic: keyof constraint + T[K] return type
-- ═══════════════════════════════════════════════════════════════════════════════

---@class Config
---@field name string
---@field value number
---@field enabled boolean

---@generic T, K: keyof T
---@param obj T
---@param key K
---@return T[K]
local function getField(obj, key)
    return obj[key]
end

---@type Config
local cfg = { name = "test", value = 42, enabled = true }

-- Return type resolves per-field
local n = getField(cfg, "name")
--    ^ hover: (local) n: string
local v = getField(cfg, "value")
--    ^ hover: (local) v: number
local e = getField(cfg, "enabled")
--    ^ hover: (local) e: boolean

-- Invalid key triggers generic-constraint-mismatch
getField(cfg, "bogus")
--             ^ diag: generic-constraint-mismatch

-- Completions offer valid field names
getField(cfg, "")
--             ^ comp: enabled, name, value  diag: generic-constraint-mismatch

-- ═══════════════════════════════════════════════════════════════════════════════
-- Inherited fields from parent classes
-- ═══════════════════════════════════════════════════════════════════════════════

---@class BaseEntity
---@field id number
---@field created string

---@class UserEntity : BaseEntity
---@field username string
---@field email string

---@type UserEntity
local user = {}

-- Own fields resolve
local uname = getField(user, "username")
--    ^ hover: (local) uname: string

-- Inherited fields resolve
local uid = getField(user, "id")
--    ^ hover: (local) uid: number
local ucreated = getField(user, "created")
--    ^ hover: (local) ucreated: string

-- Invalid key on child class
getField(user, "missing")
--              ^ diag: generic-constraint-mismatch

-- Completions include inherited fields
getField(user, "")
--              ^ comp: created, email, id, username  diag: generic-constraint-mismatch

-- ═══════════════════════════════════════════════════════════════════════════════
-- Optional fields: T[K] preserves nilability
-- ═══════════════════════════════════════════════════════════════════════════════

---@class OptionalFields
---@field required string
---@field optional? number

---@type OptionalFields
local opt = {}
local req = getField(opt, "required")
--    ^ hover: (local) req: string
local optVal = getField(opt, "optional")
--    ^ hover: (local) optVal: number?

-- ═══════════════════════════════════════════════════════════════════════════════
-- CallMethod pattern: keyof for method dispatch
-- ═══════════════════════════════════════════════════════════════════════════════

---@class Dispatcher
---@field greet fun(self: Dispatcher, name: string): string
---@field reset fun(self: Dispatcher): boolean

---@generic T, K: keyof T
---@param obj T
---@param method K
---@param ... any
local function callMethod(obj, method, ...)
    obj[method](obj, ...)
end

---@type Dispatcher
local d = {}

-- Valid method names pass constraint
callMethod(d, "greet", "world")
callMethod(d, "reset")

-- Invalid method name fails constraint
callMethod(d, "nonexistent")
--             ^ diag: generic-constraint-mismatch

-- Completions for method names
callMethod(d, "")
--             ^ comp: greet, reset  diag: generic-constraint-mismatch

-- ═══════════════════════════════════════════════════════════════════════════════
-- CallMethod: T[K] resolves to callable function, return type propagates
-- ═══════════════════════════════════════════════════════════════════════════════

---@class Service
---@field getName fun(self: Service): string
---@field getCount fun(self: Service): number
---@field process fun(self: Service, input: string): boolean

---@generic T, K: keyof T
---@param obj T
---@param method K
---@return T[K]
local function getMethod(obj, method)
    return obj[method]
end

---@type Service
local svc = {}

-- T[K] resolves to the function type
local getFn = getMethod(svc, "getName")
--    ^ hover: (local) function getFn(self: Service)
local procFn = getMethod(svc, "process")
--    ^ hover: (local) function procFn(self: Service, input: string)

-- Calling the resolved function propagates return type
local name = getFn(svc)
--    ^ hover: (local) name: string
local ok = procFn(svc, "hello")
--    ^ hover: (local) ok: boolean

-- ═══════════════════════════════════════════════════════════════════════════════
-- Multiple keyof-constrained generics
-- ═══════════════════════════════════════════════════════════════════════════════

---@class SourceObj
---@field alpha number
---@field beta string

---@class TargetObj
---@field gamma boolean

---@generic S, SK: keyof S, T, TK: keyof T
---@param src S
---@param srcKey SK
---@param dst T
---@param dstKey TK
local function copyField(src, srcKey, dst, dstKey)
    dst[dstKey] = src[srcKey]
end

---@type SourceObj
local src = {}
---@type TargetObj
local dst = {}

-- Valid keys for both
copyField(src, "alpha", dst, "gamma")

-- Invalid source key
copyField(src, "invalid", dst, "gamma")
--              ^ diag: generic-constraint-mismatch

-- Invalid target key
copyField(src, "alpha", dst, "invalid")
--                            ^ diag: generic-constraint-mismatch

-- ═══════════════════════════════════════════════════════════════════════════════
-- T[K] with complex field types
-- ═══════════════════════════════════════════════════════════════════════════════

---@class ComplexFields
---@field callback fun(x: number): string
---@field items string[]
---@field nested Config

---@type ComplexFields
local cplx = {}
local cb = getField(cplx, "callback")
--    ^ hover: (local) function cb(x: number)
local items = getField(cplx, "items")
--    ^ hover: (local) items: string[]
local nested = getField(cplx, "nested")
--    ^ hover: (local) nested: Config

-- ═══════════════════════════════════════════════════════════════════════════════
-- keyof on a different generic (not the first parameter)
-- ═══════════════════════════════════════════════════════════════════════════════

---@class Lookup
---@field handlers table
---@field count number

---@generic T, K: keyof T
---@param container T
---@param key K
---@return T[K]
local function lookup(container, key)
    return container[key]
end

---@type Lookup
local reg = {}
local c = lookup(reg, "count")
--    ^ hover: (local) c: number

lookup(reg, "invalid")
--           ^ diag: generic-constraint-mismatch

-- ═══════════════════════════════════════════════════════════════════════════════
-- Non-literal keys: graceful degradation
-- ═══════════════════════════════════════════════════════════════════════════════

---@type Config
local cfg2 = {}
-- Variable key — keyof constraint doesn't fire (can't validate at analysis time)
local key = "name"
local r = getField(cfg2, key)
--    ^ hover: (local) r: Config

-- ═══════════════════════════════════════════════════════════════════════════════
-- Deep inheritance chain (grandparent fields)
-- ═══════════════════════════════════════════════════════════════════════════════

---@class GrandParent
---@field origin string

---@class MiddleClass : GrandParent
---@field level number

---@class LeafClass : MiddleClass
---@field tag boolean

---@type LeafClass
local leaf = {}
local leafOrigin = getField(leaf, "origin")
--    ^ hover: (local) leafOrigin: string
local leafLevel = getField(leaf, "level")
--    ^ hover: (local) leafLevel: number
local leafTag = getField(leaf, "tag")
--    ^ hover: (local) leafTag: boolean

-- Completions include all ancestor fields
getField(leaf, "")
--              ^ comp: level, origin, tag  diag: generic-constraint-mismatch

-- ═══════════════════════════════════════════════════════════════════════════════
-- Union-typed fields resolve correctly
-- ═══════════════════════════════════════════════════════════════════════════════

---@class UnionFields
---@field status "active" | "inactive"
---@field data number | string

---@type UnionFields
local uf = {}
local status = getField(uf, "status")
--    ^ hover: (local) status: "active" | "inactive"
local data = getField(uf, "data")
--    ^ hover: (local) data: number | string

-- ═══════════════════════════════════════════════════════════════════════════════
-- T[K] return used in further expressions
-- ═══════════════════════════════════════════════════════════════════════════════

---@class MathConfig
---@field multiplier number
---@field offset number
---@field label string

---@type MathConfig
local mc = {}
-- T[K] result feeds into arithmetic
local doubled = getField(mc, "multiplier") * 2
--    ^ hover: (local) doubled: number

-- ═══════════════════════════════════════════════════════════════════════════════
-- Multiple valid calls: same function, different classes
-- ═══════════════════════════════════════════════════════════════════════════════

---@class PointXY
---@field x number
---@field y number

---@class NamedItem
---@field title string
---@field description string

---@type PointXY
local pt = {}
---@type NamedItem
local item = {}

-- Same getField, different T binding per call
local px = getField(pt, "x")
--    ^ hover: (local) px: number
local title = getField(item, "title")
--    ^ hover: (local) title: string

-- Each call validates against its own T
getField(pt, "title")
--            ^ diag: generic-constraint-mismatch
getField(item, "x")
--              ^ diag: generic-constraint-mismatch

-- ═══════════════════════════════════════════════════════════════════════════════
-- keyof on colon-defined methods (function Class:Method) — not just @field
-- ═══════════════════════════════════════════════════════════════════════════════

---@class ColonMethods
local ColonMethods = {}

function ColonMethods:Draw()
end

function ColonMethods:Reset()
end

---@type ColonMethods
local cm = {}

-- Colon-defined method names satisfy the keyof constraint
callMethod(cm, "Draw")
callMethod(cm, "Reset")

-- Invalid method name still fails
callMethod(cm, "Nope")
--              ^ diag: generic-constraint-mismatch

-- Completions offer colon-defined method names
callMethod(cm, "")
--              ^ comp: Draw, Reset  diag: generic-constraint-mismatch

-- ═══════════════════════════════════════════════════════════════════════════════
-- keyof string literals tracked as field references
-- ═══════════════════════════════════════════════════════════════════════════════

---@class Receiver
local Receiver = {}

function Receiver:Activate()
end

function Receiver:Deactivate()
end

---@generic T, K: keyof T
---@param obj T
---@param method K
local function invoke(obj, method)
    obj[method](obj)
end

---@type Receiver
local rv = {}

-- Direct method call: refs should include both syntactic and keyof string refs
rv:Activate()
-- ^ refs: 359:19, 376:4, 378:13
invoke(rv, "Activate")
invoke(rv, "Deactivate")

-- Refs on the other method
rv:Deactivate()
-- ^ refs: 362:19, 379:13, 382:4
