-- Regression: `self.field = GetLib(..):New()` — a chained-receiver call (a call
-- on a call's result) whose chain returns a *different* class than the receiver
-- — assigned inside a method of a local `@class`. The workspace self-field scan
-- can't resolve the chain, so the bare scanner parks `field` on the class as an
-- existence-only `any` placeholder. The query-time resolver (hover / completion /
-- go-to-definition) must still refine that placeholder from the in-file
-- assignment — matching the diagnostics engine — so it resolves to the chain's
-- concrete return class instead of `any`.
--
-- Reported symptom: adding `--- @class MuhAddon` on the receiver local made the
-- field hover as `any`, while removing it (a plain local table) resolved
-- correctly. The two paths are now consistent — the `@class` no longer degrades
-- the field to `any`.
---@diagnostic disable: unused-local, missing-return

---@class ChainRegistry
local ChainRegistry = {}

---@return ChainDB
function ChainRegistry:New() end

---@return ChainRegistry
local function GetLib(name) end

---@class ChainDB
---@field profile table
local ChainDB = {}

function ChainDB:Save() end

---@class ChainAddon
local ChainAddon = {}

function ChainAddon:OnInitialize()
    -- Chained receiver: GetLib(..) is itself a call, so the coarse scan parks
    -- `db` existence-only as `any`. New() returns ChainDB (not the receiver's
    -- own ChainAddon class).
    self.db = GetLib("Lib"):New()
end

function ChainAddon:Read()
    local d = self.db
    --              ^ hover: (field) db: ChainDB {
    local p = self.db.profile
    --                 ^ hover: (field) profile: table  def: local
end
