---@diagnostic disable: unused-local
---@param n number?
local function useNumber(n) end

-- A read under a run-time key is one of the elements files write there.
---@param mod OptMod
local function readOptions(mod, key)
    local color = mod.Options[key .. "Color"]
    --    ^ hover: (local) color: any
    useNumber(color)
    local enabled = mod.Options.Enabled
    --    ^ hover: (local) enabled: boolean
end

-- A record holding elements under run-time keys iterates its fields and those;
-- a list's appends are no such elements.
---@param mod OptMod
local function readRecords(mod)
    for _, d in pairs(mod.Durations) do
        local duration = d
        --    ^ hover: (local) duration: number | boolean
    end
    local missing = mod.Items.missing
    --    ^ hover: (local) missing: ?
end
