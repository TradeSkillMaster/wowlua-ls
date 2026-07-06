-- Regression: the defclass-local heuristic must bind a local only for the
-- *string-keyed navigation* idiom `Base:From("Lib"):ExtendClass("Class")` — every
-- hop is `:Method("name-literal")`, so the local genuinely becomes the named class
-- and methods defined on it route there. Suppressing that binding regressed every
-- `From():IncludeClassType()`-style local into false `undefined-field`.
--
-- It must NOT bind an instance transform, where a hop yields a runtime instance
-- (no string key) and the outer string is merely a parameter:
--   * plain-function root  `getReg():asType("Class")`      (indirect_chained_getter.lua)
--   * method-call root      `h:getReg():asType("Class")`    (section 2 below)
--   * parenthesized root    `(getReg()):asType("Class")`    (section 3 below)
-- Classifying by call *form* (is the innermost hop a colon call?) can't separate
-- the method-call-rooted transform from navigation — both are `name:m():m()`; the
-- string-key-on-every-hop check is what does.
---@diagnostic disable: unused-local, missing-return

-- == Section 1: navigation — MUST bind ======================================
---@class NavClass
local NavClass = {}

---@class NavComponentRef
local NavComponentRef = {}
---@generic T
---@param name `T`
---@return T
function NavComponentRef:From(name) end
---@generic T
---@param name `T`
---@return T
function NavComponentRef:ExtendClass(name) end

---@type NavComponentRef
local Base = nil

-- Bare-name root (`Base`), every hop `:Method("name")`: navigation. `Ext` binds to
-- `NavClass` so the method defined on it below is registered on `NavClass`.
local Ext = Base:From("SubLib"):ExtendClass("NavClass")
---@return string
function Ext:added() end

---@type NavClass
local inst = nil
local a = inst:added()
--    ^ hover: (local) a: string

-- == Section 2: method-call-rooted transform — MUST NOT bind ================
---@class XformTarget
local XformTarget = {}

---@class XformReg
local XformReg = {}
---@param className string
---@return XformReg
function XformReg:asType(className) end

---@class XformHolder
local XformHolder = {}
---@return XformReg
function XformHolder:getReg() end

---@type XformHolder
local h = nil

-- `h:getReg()` yields a runtime instance (no string key), so `:asType("XformTarget")`
-- must NOT bind `xr` to XformTarget — `leaked` must not leak onto that class.
local xr = h:getReg():asType("XformTarget")
function xr:leaked() end

---@type XformTarget
local xt = nil
local x2 = xt:leaked()
--            ^ diag: undefined-field

-- == Section 3: parenthesized-receiver transform — MUST NOT bind ============
---@class ParenTarget
local ParenTarget = {}

---@return XformReg
local function pGetReg() end

-- The receiver descent must see through `(...)` to reach `pGetReg()` (an instance
-- getter), so this must NOT bind `pr` to ParenTarget either.
local pr = (pGetReg()):asType("ParenTarget")
function pr:pleaked() end

---@type ParenTarget
local pt = nil
local x3 = pt:pleaked()
--            ^ diag: undefined-field
