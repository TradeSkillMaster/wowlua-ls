---@diagnostic disable: unused-local, unused-function, undefined-global
-- Member completion on a bracket-indexed receiver (`x[i]:m` / `x[i].f`).
-- Regression: completion after the `]` of a bracket-index expression produced
-- "No suggestions" even though the element type already resolves for hover and
-- go-to-definition. The receiver-token switch in `complete_member_access` only
-- handled `)` / strings / bare names, never `]` (a `BracketAccess`).

---@class BICItem
---@field label string
local BICItem = {}

function BICItem:Alpha() end
function BICItem:Beta() end

-- Array element (`BICItem[]`) → colon method completion.
---@type BICItem[]
local arr = {}

arr[1]:Alpha()
--     ^ comp: Alpha, Beta

-- A typed prefix after `]:` narrows the method set.
arr[1]:Alpha()
--      ^ comp: Alpha

-- Dictionary element (`table<K, V>`) → dot access lists methods and data fields.
---@type table<string, BICItem>
local map = {}

local a = map["k"].label
--                 ^ comp: Alpha, Beta, label

-- Trailing same-line `---@type T[]` comment (the exact reported form).
local trailing = {} ---@type BICItem[]

trailing[1]:Alpha()
--          ^ comp: Alpha, Beta
