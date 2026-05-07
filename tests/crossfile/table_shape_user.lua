-- Cross-file test: access table literal fields from namespace

local _, ns = ...

-- Table literal field should preserve its shape (not collapse to bare table)
local items = ns.ITEM_CLASSES
--    ^ hover: (local) items: {

-- Named fields should be accessible
local armor = ns.ITEM_CLASSES.ARMOR
--    ^ hover: (local) armor: number

local weapon = ns.ITEM_CLASSES.WEAPON
--     ^ hover: (local) weapon: number

-- Mixed-type table literal fields
local enabled = ns.CONFIG.enabled
--    ^ hover: (local) enabled: boolean

local name = ns.CONFIG.name
--    ^ hover: (local) name: string

local count = ns.CONFIG.count
--    ^ hover: (local) count: number

-- Nested table literal preserves inner shape
local inner = ns.NESTED.inner
--    ^ hover: (local) inner: {

local val = ns.NESTED.inner.value
--    ^ hover: (local) val: number

-- Empty table constructor produces bare table (no fields to extract)
local empty = ns.EMPTY
--    ^ hover: (local) empty: table

-- @type annotation takes precedence over inferred shape
local typed = ns.TYPED
--    ^ hover: (local) typed: ShapeOverrideClass {
local tx = ns.TYPED.x
--    ^ hover: (local) tx: number

-- Table with function-call values preserves field names (type is any)
local opaque = ns.OPAQUE_KEYS
--    ^ hover: (local) opaque: {

local foo = ns.OPAQUE_KEYS.FOO
--    ^ hover: (local) foo: any

local bar = ns.OPAQUE_KEYS.BAR
--    ^ hover: (local) bar: any
