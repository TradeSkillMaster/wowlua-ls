#!/usr/bin/lua
-- This file has a shebang line and should be skipped entirely.
-- No diagnostics should fire on it.

local io = require("io")
local os = require("os")

local function main()
    local f = io.open("input.txt", "r")
    if f then
        local content = f:read("*a")
        f:close()
        print(content)
    end
    os.exit(0)
end

main()
