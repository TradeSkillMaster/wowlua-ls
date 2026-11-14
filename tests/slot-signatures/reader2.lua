---@diagnostic disable: create-global, unused-local
-- Round 2 of signatures (`factory.lua`, `nested.lua`).
local _, ns = ...

local mod = ns.NewMod("a")
--    ^ hover: (local) mod: table | SigMod
local name = mod:GetName()
--    ^ hover: (local) name: string
local ordered = ns.OrderedTable()
--    ^ hover: (local) ordered: table
local link, quality = ns.Recover(nil, "item", 3)
--    ^ hover: (local) link: string
local info, isNew = ns.GetInfo(1)
--          ^ hover: (local) isNew: boolean
local frame = LibStub("SigLib-1.0"):WithWindow()
--    ^ hover: (local) frame: Frame & { button: number, label: string }
local open = ns.IsOpen()
--    ^ hover: (local) open: boolean
local setTip = ns.Toggle.SetTip
--    ^ hover: (local) function setTip(self: Button & UIPanelButtonTemplate, text)
local start = SigCore.Test.Warper.Start
--    ^ hover: (local) function start(self: SigWarper)\n-> table

-- A workspace function a value written to another table holds, or one compared.
SigExports = {}
SigExports.Recover = ns.Recover
--                      ^ hover: (field) function Recover(info, link: T1, quality: T2)\n-> T1 | string, T2 | number\ncases:\n(T1, T2)\n(string, number)
if SigExports.Recover == ns.OrderedTable then
--                          ^ hover: (field) function OrderedTable()\n-> table
end

---@type SigReg
local reg = SigRegistry
reg:Fire("y", 2)

local entry, isNewEntry = ns.GetEntry(1)
--           ^ hover: (local) isNewEntry: boolean
local optValue = ns.Entry.opt
--    ^ hover: (local) optValue: any
