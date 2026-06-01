-- Cross-file numeric-literal tuple-union narrowing, mirroring a real addon:
-- `if total > 1 and topTime then` drops the `(0, nil, nil)` case. The `> 1`
-- term eliminates it via the slot-0 `0` literal, and the truthy `topTime`
-- term independently eliminates it via the slot-2 nil. Both surviving
-- siblings narrow to their success-case types.
local total, topAddon, topTime = AuctionLib.GetTopHookedTime()

-- Slot 0 union `number | 0` collapses to plain `number` on hover.
local _ = total
--        ^ hover: (local) total: number

if total > 1 and topAddon ~= "Other" and topTime then
	local _ = topAddon
	--        ^ hover: (local) topAddon: string
	local _ = topTime
	--        ^ hover: (local) topTime: number
end
