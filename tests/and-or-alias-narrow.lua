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

-- Regression: a local typed from a call whose inferred return only settles
-- after a fixpoint stall (the callee is defined later and returns a narrowed
-- local). The truthiness narrowings of the local — `not x`, the alias version
-- restoring it after the `or` chain — must follow the initializer's final
-- `number?` instead of keeping the `any` it had before that.
local _arPriv = {}

---@return number?
local function _arRead() return nil end

---@param have number
local function restockNeeded(have)
	local minRestock = _arPriv.GetMin()
	local maxRestock = _arPriv.GetMax()
	if not minRestock or not maxRestock or minRestock > maxRestock then
--                                      ^ hover: (local) minRestock: number
		return 0
	end
	return maxRestock - have
--      ^ hover: (local) maxRestock: number
end
_consume(restockNeeded)

function _arPriv.GetMin()
	local value = _arRead()
	if not value then
		return nil, "invalid"
	end
	return value
end

function _arPriv.GetMax()
	local value = _arRead()
	if not value then
		return nil, "invalid"
	end
	return value
end
