---@class StatusCode
StatusCode = {}

Processor = {}

---@return (true ok, number count, number remaining)
---|       (nil)
---|       (nil, StatusCode errCode)
---|       (false, StatusCode errCode)
function Processor.Run()
	return true, 5, 5
end

---@return (true ok, string detail)
---|       (false, StatusCode errCode)
function Processor.Check()
	return true, "ok"
end
