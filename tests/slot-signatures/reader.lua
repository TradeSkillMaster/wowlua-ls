---@diagnostic disable: create-global, unused-local
local _, ns = ...

local margin = ns.SigWidget.SetMargin
--    ^ hover: (local) function margin(self: SigWidget, left: number, right: string | number)\n-> SigWidget
local owner = ns.SigWidget.Owner
--    ^ hover: (local) function owner(self: SigWidget | SigPanel)\n-> SigWidget
local format = ns.Format
--    ^ hover: (local) function format(value: string | number)\n-> string
ns.Format(true)
--        ^ diag: type-mismatch
local state = ns.SigSchema.CreateState
--    ^ hover: (local) function state(self: SigSchema)\n-> SigState

SigChildMixin = CreateFromMixins(SigProviderMixin)
function SigChildMixin:Run()
    local update = self.onUpdate
    --    ^ hover: (local) update: fun()
    local done = self.onDone
    --    ^ hover: (local) done: fun(count: number): number
    update()
    done(1)
end

local compose = SigApp.Helpers.Mail.Compose
--    ^ hover: (local) compose: fun(self: table, text: string | number): string

local thing = ns.MakeThing(true)
--    ^ hover: (local) thing: Frame & { extra: number }?
local other = ns.MakeOther(nil, true)
--    ^ hover: (local) other: Frame & any?
