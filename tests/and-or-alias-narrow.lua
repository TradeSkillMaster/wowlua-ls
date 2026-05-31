---@diagnostic disable: undefined-global
local function _consume(...) end

-- Regression: a local typed `number?` from `cond and deferredCall() or nil`
-- is used in an `and`-chain (which creates an alias version restoring the
-- pre-narrowing state), then referenced via `not x` in a later elseif chain.
--
-- The price call resolves only after several fixpoint iterations (its return
-- type is inferred from a body that chains through another deferred call), so
-- the `or nil` expression transiently resolves to plain `nil`. The alias
-- version reads the base version's type live; it must keep tracking the base
-- as the call resolves, rather than getting permanently stuck on the partial
-- `nil`. Before the fix, the hover at `not thr` showed `nil` instead of
-- `number?`.

---@class AOSettings
---@field flag boolean

local _aoPriv = {}
local _aoUtil = {}

---@param settings AOSettings
---@param lowest boolean
local function andOrAliasHover(settings, lowest)
	local normalPrice = _aoPriv.GetPrice("normal")
	local thr = settings.flag and _aoPriv.GetPrice("thr") or nil
	if not lowest and normalPrice and thr and normalPrice > thr then
		return 1
	elseif not lowest then
		return 2
	end
	local minPrice = _aoPriv.GetPrice("min")
	if not minPrice then
		return 3
	elseif not normalPrice then
		return 4
	elseif settings.flag and not thr then
--                            ^ hover: (local) thr: number?
		return 5
	end
	return 6
end
_consume(andOrAliasHover)

-- Defined after the consumer with an inferred return that chains through
-- another deferred call, so resolution spans multiple fixpoint iterations.
function _aoPriv.GetPrice(key)
	return _aoUtil.GetPrice(key)
end

---@param key string
---@return number
function _aoUtil.GetPrice(key) return 1 end
