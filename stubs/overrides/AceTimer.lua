---@meta _

---@class AceTimer-3.0
local AceTimer = {}

---@param func keyof self | function Callback function for the timer pulse (funcref or method name).
---@param delay number Delay for the timer, in seconds.
---@param ... any An optional, unlimited amount of arguments to pass to the callback function.
---@return AceTimerObj
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-timer-3-0#title-3)
function AceTimer:ScheduleRepeatingTimer(func, delay, ...) end

---@param func keyof self | function Callback function for the timer pulse (funcref or method name).
---@param delay number Delay for the timer, in seconds.
---@param ... any An optional, unlimited amount of arguments to pass to the callback function.
---@return AceTimerObj
---[Documentation](https://www.wowace.com/projects/ace3/pages/api/ace-timer-3-0#title-4)
function AceTimer:ScheduleTimer(func, delay, ...) end
