local _, ns = ...

local Settings = { threshold = 5 }
ns.Settings = Settings

function Settings:GetThreshold()
  return self.threshold
end

function ns.Settings.Reset()
  Settings.threshold = 5
end

---@class Ledger
---@field entries number[]
ns.Ledger = {}

function ns.Ledger:Count()
  return #self.entries
end

ns.items = { { label = "first" } }
for _, item in ipairs(ns.items) do
  print(item.label)
end

FixtureAddonDB = { profile = { scale = 1 } }

local scratch = { size = 2 }
print(scratch.size)

FixtureMixin = { count = 0 }

function FixtureMixin:Refresh()
  self.count = 1
end

_G.FixtureFlags = { verbose = true }

---@class FixtureBase
FixtureBase = {}

function FixtureBase:Reload()
  self.reloaded = true
end

---@class FixturePanel : FixtureBase
FixturePanel = {}
