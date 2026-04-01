---@meta coroutine_wow

---
---
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-coroutine"])
---
---@class coroutinelib
coroutine = {}

---Creates a new coroutine, with body `f`. `f` must be a function.
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-coroutine.create"])
---
---@param f function
---@return thread
---@nodiscard
function coroutine.create(f) end

---Starts or continues the execution of coroutine `co`.
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-coroutine.resume"])
---
---@param co thread
---@param ... any
---@return boolean success
---@return any ...
function coroutine.resume(co, ...) end

---Suspends the execution of the calling coroutine.
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-coroutine.yield"])
---
---@param ... any
---@return any ...
function coroutine.yield(...) end

---Returns the status of the coroutine `co`.
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-coroutine.status"])
---
---@param co thread
---@return string status
---@nodiscard
function coroutine.status(co) end

---Creates a new coroutine, with body `f`, and returns a function that resumes the coroutine each time it is called.
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-coroutine.wrap"])
---
---@param f function
---@return function
---@nodiscard
function coroutine.wrap(f) end

---Returns the running coroutine plus a boolean, true when the running coroutine is the main one.
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-coroutine.running"])
---
---@return thread|nil running
---@return boolean ismain
---@nodiscard
function coroutine.running() end

---Returns true when the running coroutine can yield.
---
---[View documents](command:extension.lua.doc?["en-us/51/manual.html/pdf-coroutine.isyieldable"])
---
---@return boolean
---@nodiscard
function coroutine.isyieldable() end

return coroutine
