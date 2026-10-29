---@diagnostic disable: unused-local
-- need-check-nil on calls resolved against an @overload

---@class NcnItem
---@class NcnHost
local NcnHost = {}

-- Overloads that tie as the best fit: the call is checked against the first,
-- so the possibly-nil argument keeps its need-check-nil.
---@param item NcnItem
---@param b boolean
---@overload fun(self: NcnHost, item: NcnItem, b: number|string): number
---@overload fun(self: NcnHost, item: NcnItem, b: string|number|table): string
function NcnHost:Tie(item, b) end

---@type NcnItem?
local ncnMaybeItem
---@type number|string
local ncnNumOrStr
NcnHost:Tie(ncnMaybeItem, ncnNumOrStr)
--          ^ diag: need-check-nil

-- An optional overload param takes a possibly-nil argument.
---@param item string
---@param all? boolean
---@overload fun(self: NcnHost, item: string, other?: string, all?: boolean)
function NcnHost:Opt(item, all) end

---@type string?
local ncnMaybeName
NcnHost:Opt("x", ncnMaybeName, true)

local ncnFrame = CreateFrame("Frame")
---@type number?
local ncnMaybeOffset
ncnFrame:SetPoint("TOPLEFT", 0, ncnMaybeOffset)
