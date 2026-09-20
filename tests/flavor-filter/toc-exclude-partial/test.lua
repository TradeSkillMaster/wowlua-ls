-- `tbc`, `wrath`, `cata` and `mists` all collapse into our single Classic flavor
-- bit, so an exclusion must be resolved from the game type *names*: excluding
-- `mists` leaves Cata, and this _Cata.toc file still loads.
--
-- Subtracting a parsed mask instead would clear the Classic bit outright, taking
-- the TOC's own flavor to zero — the file then drops out of the TOC map entirely
-- and falls back to the project's breadth (here: no config, so flavor filtering
-- switches off and this retail-only call goes unreported).
PlayerGetTimerunningSeasonID()
-- ^ diag: wrong-flavor-api
