AuctionLib = {}

-- Tuple-union whose failure case is discriminated purely by a numeric
-- literal in slot 0. The slot-0 union is `number | 0`, which collapses to
-- `number` on hover.
---@return (number total, string topAddon, number topTime) | (0, nil, nil)
function AuctionLib.GetTopHookedTime()
	return 0, nil, nil
end
