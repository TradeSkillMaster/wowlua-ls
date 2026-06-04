---@diagnostic disable: unused-function
-- Cross-file backward inference: inferring param type from a typed
-- function defined in another file.

local function wrapTag(tag)
--                     ^ hover: (param) tag: string
    return BackwardLibConsumeTag(tag)
end

local function wrapNumber(n)
--                        ^ hover: (param) n: number
    BackwardLibTakeNumber(n)
end
