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

---@param text string
---@return boolean found
---@return number position
function UtilLib.Search(text)
    return true, 1
end

-- Global function with @return
---@return string
function GetAppName()
    return "TestApp"
end

-- Global function with multiple returns
---@return number
---@return string
function GetInfo()
    return 1, "info"
end

-- Global function with named return values
---@param slot number
---@return boolean hasItem
function HasSlotItem(slot)
    return true
end

---@param id number
---@return string itemName
---@return number itemCount
function GetItemDetails(id)
    return "item", 5
end

-- Global function with class return type
---@class GlobalConfig
---@field debug boolean
---@field level number

---@return GlobalConfig
function GetConfig()
    return {}
end

-- Globals created via _G.field assignment
_G.MyGlobalAPI = {}

---@param name string
---@return boolean
function _G.MyGlobalAPI:IsValid(name)
    return true
end

---@param x number
---@return number
function _G.GlobalHelper(x)
    return x + 1
end

_G.GLOBAL_CONST = "hello"

-- Global function that never returns a value (implicit nil return)
function DoAction()
    print("action")
end

-- Global function with only a bare return
function DoActionBare()
    print("action")
    return
end

-- Global table method that never returns a value
function UtilLib:RunTask()
    print("running")
end

-- Field name that collides with a global (regression test: field-position
-- tokens must NOT resolve to a same-named global when the chain walk fails).
MY_ENABLED_INFO = "some global"
DataLib = {}
DataLib.inner = {}
DataLib.inner.MY_ENABLED_INFO = true

-- Global non-class table with deep method chains (Auctionator-like pattern):
-- methods defined on sub-tables of a global table without @class.
GadgetTools = {}

---@param text string
---@return number
function GadgetTools.Search.Filter(text)
    return #text
end

---@return boolean
function GadgetTools.Search:IsActive()
    return true
end
