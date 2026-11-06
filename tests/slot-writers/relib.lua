---@diagnostic disable: create-global
-- A local bound to a namespace field in a branch, then rebound: what is written
-- through it afterwards is no field of the namespace's.
local _, ns = ...
ns.Relib = {}

function LedgerStandalone() return {} end

local relib
if ns.Relib then
    relib = ns.Relib
else
    relib = LedgerStandalone()
end
relib = LedgerStandalone()
relib.mode = "fresh"
