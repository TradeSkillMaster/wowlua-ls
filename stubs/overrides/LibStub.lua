---@meta _
---[Documentation](https://www.wowace.com/projects/libstub)

---@class LibStub
---@generic T
---@overload fun(major: `T`, silent?: boolean): T, number?
LibStub = {}

---@generic T
---@param major `T`
---@param silent? boolean
---@return T? library
function LibStub:GetLibrary(major, silent) end

---@generic T
---@param major `T`
---@param minor number
---@return T library
---@return number? oldMinor
function LibStub:NewLibrary(major, minor) end

---@return fun(): string, table
function LibStub:IterateLibraries() end
