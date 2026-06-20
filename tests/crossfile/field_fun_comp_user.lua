---@diagnostic disable: undefined-global, type-mismatch
-- Cross-file test: string literal completions and call resolution for
-- @field fun() types defined on workspace-scanned classes.
-- (Partial strings like "O" are completion placeholders, not valid values,
--  so type-mismatch is suppressed here.)

---@class FFCCallbackLib
local _, FFCCallbackLib = ...

-- Colon call: first visible arg is eventName, completions should show event names
FFCCallbackLib:RegisterCallback("O", myHandler)
--                               ^ comp: OnReady, OnComplete, OnError

-- Dot call: second arg is eventName
FFCCallbackLib.RegisterCallback({}, "O", myHandler)
--                                   ^ comp: OnReady, OnComplete, OnError

-- Signature help should show proper parameter types
FFCCallbackLib:RegisterCallback("O", myHandler)
--                               ^ sig: fun(target: table, eventName: FFCEventName, handler: string | function)

-- UnregisterCallback also has @field fun() with eventName
FFCCallbackLib:UnregisterCallback("O")
--                                 ^ comp: OnReady, OnComplete, OnError

-- fun()|nil union: Fun inside Union should be materialized
FFCCallbackLib.OptionalCallback("O")
--                               ^ comp: OnReady, OnComplete, OnError

-- fun()! lateinit: Fun inside NonNil should be materialized
FFCCallbackLib.LateinitCallback("O")
--                               ^ comp: OnReady, OnComplete, OnError
