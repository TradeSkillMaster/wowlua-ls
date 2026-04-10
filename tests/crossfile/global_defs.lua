-- Cross-file global function and variable test: definitions

-- Global variables with literal values
MY_VERSION = "2.0.1"
MY_COUNT = 42
MY_ENABLED = true

-- Global table with methods
UtilLib = {}

---@param text string
---@return number
function UtilLib.GetLength(text)
    return #text
end

---@param a number
---@param b number
---@return number
function UtilLib:Add(a, b)
    return a + b
end

-- Global function with @return
---@return string
function GetAppName()
    return "TestApp"
end

-- Global function with multiple returns
---@return number, string
function GetInfo()
    return 1, "info"
end

-- Global function with class return type
---@class GlobalConfig
---@field debug boolean
---@field level number

---@return GlobalConfig
function GetConfig()
    return {}
end
