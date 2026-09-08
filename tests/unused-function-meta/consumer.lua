---@diagnostic disable: unused-local
-- A NON-meta file in the same workspace. Its genuinely-unused function MUST
-- still be flagged, proving the cross-file unused-function pass actually runs
-- here and the meta exclusion is specific to `---@meta` files (not a blanket
-- disable that would make the meta assertion pass vacuously).

function ConsumerUnusedFunc()
    return 1
end
