---@class CrossFileValidator3
local Validator3 = {}

---@return (false result, string errType, string? errTokenStr)|(true, nil, nil)
function Validator3:Validate()
    return true, nil, nil
end

---@return (string errType, string source)|(nil, nil)
function Validator3:Check()
    return nil, nil
end

---@return CrossFileValidator3
function GetCrossFileValidator3()
    return Validator3
end

---@return (false result, string errType, string? errTokenStr)|(true, nil, nil)
function CrossFileValidate3()
    return true, nil, nil
end

---@return (string errType, string source)|(nil, nil)
function CrossFileCheck3()
    return nil, nil
end
