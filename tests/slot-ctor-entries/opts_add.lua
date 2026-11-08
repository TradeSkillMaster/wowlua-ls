---@class OptMod
local proto = GetPrototype("OptMod")

function proto:AddOption(name, default)
    self.Options[name] = default
end

function proto:SetDuration(name)
    self.Durations[name .. "Time"] = 5
end

---@param v number
function proto:AddItem(v)
    self.Items[#self.Items + 1] = v
end
