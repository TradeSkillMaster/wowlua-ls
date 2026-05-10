-- Regression: a @class annotation that re-declares a defclass-created class
-- must not lose the constructor registration from @constructor __init on ObjBase.

---@class ReactiveSubject: ObjBase

local ReactiveOneShot = DefineClass("ReactiveOneShot")

---@class ReactiveOneShot: ReactiveSubject

function ReactiveOneShot.__private:__init()
--                       ^ hover: (private accessor) __private: ReactiveOneShot {
    self._value = nil ---@type any!
end

-- Constructor call inside the same file must not produce cannot-call
---@return ReactiveOneShot
function ReactiveOneShot.__static.Get(value)
--                       ^ hover: (accessor) __static: ReactiveOneShot {
    local obj = ReactiveOneShot()
    --    ^ hover: (local) obj: ReactiveOneShot  diag: none
    return obj
end

-- Cross-file constructor call via DefineClass
local inst = DefineClass("ReactiveOneShot")()
--    ^ hover: (local) inst: ReactiveOneShot  diag: none

-- Constructor field from __init must be visible
local v = inst._value
--             ^ hover: (field) _value: any!  diag: unused-local
