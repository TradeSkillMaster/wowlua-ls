---@param code StatusCode
local function consume(code)
end

-- Both siblings are directly guarded: `ok` by a truthy early-exit and
-- `errCode` by a nil early-exit. After both guards the only remaining
-- tuple-union case is `(false, StatusCode)`, so `errCode` must narrow from
-- `number | StatusCode` to `StatusCode`. Passing it to a `StatusCode`-typed
-- parameter must NOT raise type-mismatch (cross-file regression).
local ok, errCode = Processor.Run()
if ok then
	return
elseif errCode == nil then
	return
end
local _ = errCode
--        ^ hover: (local) errCode: StatusCode
consume(errCode)

-- StripFalsy guard: `if not ok2 then return end` narrows ok2 to truthy.
-- The only compatible tuple-union case is `(true, string)`, so `detail`
-- should narrow from `string | StatusCode` to `string`.
local ok2, detail = Processor.Check()
if not ok2 then
	return
end
local _ = detail
--        ^ hover: (local) detail: string
