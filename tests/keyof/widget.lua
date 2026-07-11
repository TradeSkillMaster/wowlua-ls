---@diagnostic disable: unused-local

-- `keyof self` as a first-class type: a string argument that names a field/method
-- of the receiver. Go-to-definition, hover, and completion resolve the member;
-- a non-key literal is a `type-mismatch`. The 1-arg overload puts the same
-- constraint on the event string, modelling AceEvent's `RegisterEvent`.

---@class Widget
local Widget = {}

function Widget:PLAYER_LOGIN() end

function Widget:doThing() end

---@param callback keyof self
function Widget:Register(callback) end

---@overload fun(self, event: keyof self)
---@param event string
---@param callback keyof self
function Widget:RegisterEvent(event, callback) end

function Widget:Setup()
    -- Explicit handler: nav + hover + completion resolve the receiver method.
    self:Register("doThing")
    --              ^ def: local 13:10  hover: (method) function Widget:doThing()  comp: PLAYER_LOGIN, Register, RegisterEvent, Setup, doThing

    -- A non-key is flagged (closed set, unlike an open string enum).
    self:Register("nope")
    --              ^ diag: type-mismatch

    -- 1-arg overload: the event string doubles as the handler method.
    self:RegisterEvent("PLAYER_LOGIN")
    --                  ^ def: local 11:10

    -- 2-arg form: the explicit handler names the method; the event is a plain string.
    self:RegisterEvent("SOME_EVENT", "doThing")
    --                                ^ def: local 13:10
end
