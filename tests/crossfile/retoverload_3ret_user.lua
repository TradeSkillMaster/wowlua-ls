-- Basic 3-return narrowing: third return survives overload filtering
local ok, errCode, detail = CrossFileValidate3()
if not ok then
    local _ = errCode
    --        ^ hover: (local) errCode: string
    local _ = detail
    --        ^ hover: (local) detail: string | nil
end

-- Method-call 3-return narrowing: deferred sibling narrowing must not
-- be clobbered by the parent-scope continuation narrowing.
local function wrapMethodValidate()
    local obj = GetCrossFileValidator3()
    local isValid, errType, errTokenStr = obj:Validate()
    if not isValid then
        local _ = errType
        --        ^ hover: (local) errType: string
        local _ = errTokenStr
        --        ^ hover: (local) errTokenStr: string | nil
        return nil, errType, errTokenStr
    end
    return true, nil, nil
end

-- Method-call 3-return narrowing with variable reassignment from a second call
local function wrapMethodValidateReassign()
    local obj = GetCrossFileValidator3()
    local isValid, errType, errTokenStr = obj:Validate()
    if not isValid then
        return nil, errType, errTokenStr
    end
    errType, errTokenStr = obj:Check()
    if errType then
        return nil, errType, errTokenStr
    end
    return true, nil, nil
end
