---@diagnostic disable: unused-local, unused-function
local _, ns = ...

-- Cross-file @type-narrows 0 1: the receiver's type comes from a for-in loop
-- over a cross-file iterator. Phase 1 can't resolve the iterator chain, so the
-- fallback searches known classes for the @type-narrows method.

local TNChild = ns.TNChild

local function test_crossfile_isa()
    for _, task in ns.TaskIterator() do
        if task:__isa(TNChild) then
            local x = task.extra
            --              ^ hover: (field) extra: string
        end
    end
end

-- Early-exit variant
local function test_crossfile_isa_early_exit()
    for _, task in ns.TaskIterator() do
        if not task:__isa(TNChild) then return end
        local x = task.extra
        --              ^ hover: (field) extra: string
    end
end

-- Cross-file @type-narrows ClassName (method-style, no index args)
local function test_crossfile_type_narrows_class()
    for _, c in ns.CreatureIterator() do
        if c:IsFeline() then
            local x = c.purrs
            --          ^ hover: (field) purrs: boolean
        end
    end
end
