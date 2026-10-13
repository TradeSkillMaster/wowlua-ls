-- A constructor's table with named fields, filled with untyped elements: its
-- elements read `any`, as through the scan's `table` for the field, not as its
-- named fields.
local StackProc = DefineCrossStack("CrossStackProc")

function StackProc:__init()
    self._stack = {
        queuedAction = nil, ---@type string?
    }
end

function StackProc:Push(tokenIndex)
    tinsert(self._stack, tokenIndex)
end

---@param n number
local function useNumber(n) end

function StackProc:Top()
    local top = self._stack[#self._stack]
    --    ^ hover: (local) top: any
    useNumber(top)
    return top
end
