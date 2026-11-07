---@diagnostic disable: create-global, undefined-global
-- A class declared several times in one file, written through those tables and
-- through a value of it another file gives, and cleared by a file the scan can't
-- see writing it.
---@class ClassWriterMod
local proto = ClassWriterGetProto("ClassWriterMod")
_G.ClassWriterMod = proto

function ClassWriterStartCombat(mod)
    ---@class ClassWriterMod
    mod = mod
    if mod.lastKill and GetTime() - mod.lastKill < 5 then return end
    --         ^ hover: (field) lastKill: number?
end

function ClassWriterEndCombat(mod)
    ---@class ClassWriterMod
    mod = mod
    mod.lastKill = GetTime()
end

function ClassWriterResetAll()
    local m = ClassWriterCurrentMod()
    m.lastKill = nil
end
