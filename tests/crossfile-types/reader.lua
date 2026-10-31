local _, ns = ...

print(ns.Settings.threshold, ns.Settings:GetThreshold())
local settings = ns.Settings
print(settings.threshold)
ns.Settings.Reset()

---@param ledger Ledger
local function total(ledger)
  return #ledger.entries + #ns.Ledger.entries
end

local db = FixtureAddonDB
print(db.profile.scale, FixtureAddonDB.profile.scale)

local function untyped(x)
  return x.anything
end

local frame = CreateFrame("Frame")
frame:Show()
Mixin(frame, FixtureMixin)
frame:Refresh()
local mixed = Mixin(CreateFrame("Frame"), FixtureMixin)
mixed.count = 5
print(mixed.count)
C_Timer.After(1, function() end)
print(FixtureFlags.verbose, _G.FixtureFlags)

---@param panel FixturePanel
local function reload(panel)
  panel:Reload()
  return panel.reloaded
end
print(total, untyped, reload, ns.Slots.first)
