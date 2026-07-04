-- Backtick-generic completion tests: a `@param p `T`` parameter receives the
-- *name* of a type, so a string literal in that position completes to class
-- names. The suggestions are scoped to T's constraint when one is declared
-- (`@generic T: Base` → only subclasses of Base), and span every known class
-- for an unconstrained `@generic T`.

---@diagnostic disable: unused-local, unused-function, missing-return

---@class BGElement
---@class BGButton : BGElement
---@class BGSlider : BGElement
---@class BGUnrelated

-- ── Constrained generic: only subclasses of the constraint ──────────────────

---@generic T: BGElement
---@param elementType `T` The element type
---@param id string
---@return T
local function bgNew(elementType, id) end

bgNew("", "x")
--     ^ comp: BGButton, BGElement, BGSlider

-- A partial prefix still yields the full constrained set (the client
-- fuzzy-matches against it), and BGUnrelated is never offered.
bgNew("BGB", "x")
--      ^ comp: BGButton, BGElement, BGSlider

-- ── Unconstrained generic: every known class ────────────────────────────────

---@generic T
---@param elementType `T` The element type
---@param id string
---@return T
local function bgAny(elementType, id) end

bgAny("", "x")
--     ^ comp: BGButton, BGElement, BGRegistry, BGSlider, BGUnrelated

-- ── Colon-method call: param index accounts for the implicit self ───────────

---@class BGRegistry
local BGRegistry = {}

---@generic T: BGElement
---@param elementType `T` The element type
---@return T
function BGRegistry:Make(elementType) end

---@type BGRegistry
local reg

reg:Make("")
--        ^ comp: BGButton, BGElement, BGSlider

-- ── Union-wrapped backtick (`T`|nil): recursive extraction through Union ────

---@generic T: BGElement
---@param elementType `T`|nil
---@return T
local function bgOptional(elementType) end

bgOptional("")
--          ^ comp: BGButton, BGElement, BGSlider

-- ── Malformed: backtick names a non-declared generic (no @generic Foo) ──────
-- No class-name suggestions, and no scope fall-through inside the string.

---@param elementType `Foo`
---@return any
local function bgMalformed(elementType) end
-- ^ diag: undefined-doc-name

bgMalformed("")
--           ^ comp: none
