---@meta basic_wow

---Calls the function `func` in protected mode with a temporary replacement
---environment.  The original environment is restored after the call completes.
---Returns `true` plus all values returned by `func` on success, or `false`
---plus the error object on failure.
---[View documents](https://warcraft.wiki.gg/wiki/API_pcallwithenv)
---@generic F
---@param func F
---@param env table
---@param ... params<F>
---@return (true, returns<F>) | (false, string)
function pcallwithenv(func, env, ...) end
