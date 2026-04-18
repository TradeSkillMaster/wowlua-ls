-- Deep cross-file test: negative case — chains NOT rooted at the addon namespace
-- must not fabricate sub-tables. The scanner should silently ignore 4+ part
-- writes on non-addon-ns roots.
---@class Alien
Alien = {}

-- 4-part writes below are rooted at `Alien` (not the addon ns), so the scanner
-- must NOT register `Ship`/`Engine`/`Fuel` as fields on the Alien class.
Alien.Ship.Engine.Fuel = "plasma"

function Alien.Ship.Engine:Ignite()
    return 0
end
