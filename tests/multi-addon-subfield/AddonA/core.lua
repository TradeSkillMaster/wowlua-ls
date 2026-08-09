---@diagnostic disable: unused-local, unused-function
-- AddonA and AddonB are two separate addon roots (each `.wowluarc.json` sets
-- `addonRoot: true`). Both bind a same-named `ns.Widget` to a DIFFERENTLY-typed
-- class and scatter sub-field writes onto a shared-named `ns.db`. Even with the
-- roots isolated, the combined namespace table keys sub-tables by field NAME, so
-- `ns.db.*` writes from every addon used to pile into one shared `db` sub-table.
-- Each addon's namespace must stay its own: A's `ns.Widget` is `A_Widget` (not
-- B's `B_Widget`), A's `ns.db` surfaces only `aOnly` (not B's `bOnly` — the
-- reported nested-field leak), and the `@type Foo[]` field keeps its array type.
local _, ns = ...

--- @class A_Widget
--- @field a number
local Widget = {}
ns.Widget = Widget

--- @class A_Item
--- @field id number
local A_Item = {}

--- @type A_Item[]
ns.items = {}

ns.db = {}
ns.db.aOnly = true

function ns:Use()
    -- Regression: `ns.Widget` must be THIS addon's `A_Widget`, never AddonB's
    -- same-named `B_Widget` leaking through the shared combined namespace table.
    local w = ns.Widget
    --    ^ hover: (local) w: A_Widget
    -- The `@type Foo[]` field written `ns.x = {}` keeps its array type through the
    -- per-addon table (regression: the isolation pass degraded it to bare `table`).
    local its = ns.items
    --    ^ hover: (local) its: A_Item[]
    -- Cross-addon sub-field isolation: AddonB's `ns.db.bOnly` must not appear here.
    local a = ns.db.aOnly
    --              ^ comp: aOnly
end
