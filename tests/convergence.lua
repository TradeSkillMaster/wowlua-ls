-- Regression test for fixpoint convergence:
-- 60 functions defined in reverse dependency order.
-- Without the inner fixpoint loop, this would need 60 outer
-- iterations (exceeding the 50-iteration cap) to resolve all types.

local M = {}

function M.f60() return M.f59() end
function M.f59() return M.f58() end
function M.f58() return M.f57() end
function M.f57() return M.f56() end
function M.f56() return M.f55() end
function M.f55() return M.f54() end
function M.f54() return M.f53() end
function M.f53() return M.f52() end
function M.f52() return M.f51() end
function M.f51() return M.f50() end
function M.f50() return M.f49() end
function M.f49() return M.f48() end
function M.f48() return M.f47() end
function M.f47() return M.f46() end
function M.f46() return M.f45() end
function M.f45() return M.f44() end
function M.f44() return M.f43() end
function M.f43() return M.f42() end
function M.f42() return M.f41() end
function M.f41() return M.f40() end
function M.f40() return M.f39() end
function M.f39() return M.f38() end
function M.f38() return M.f37() end
function M.f37() return M.f36() end
function M.f36() return M.f35() end
function M.f35() return M.f34() end
function M.f34() return M.f33() end
function M.f33() return M.f32() end
function M.f32() return M.f31() end
function M.f31() return M.f30() end
function M.f30() return M.f29() end
function M.f29() return M.f28() end
function M.f28() return M.f27() end
function M.f27() return M.f26() end
function M.f26() return M.f25() end
function M.f25() return M.f24() end
function M.f24() return M.f23() end
function M.f23() return M.f22() end
function M.f22() return M.f21() end
function M.f21() return M.f20() end
function M.f20() return M.f19() end
function M.f19() return M.f18() end
function M.f18() return M.f17() end
function M.f17() return M.f16() end
function M.f16() return M.f15() end
function M.f15() return M.f14() end
function M.f14() return M.f13() end
function M.f13() return M.f12() end
function M.f12() return M.f11() end
function M.f11() return M.f10() end
function M.f10() return M.f9() end
function M.f9() return M.f8() end
function M.f8() return M.f7() end
function M.f7() return M.f6() end
function M.f6() return M.f5() end
function M.f5() return M.f4() end
function M.f4() return M.f3() end
function M.f3() return M.f2() end
function M.f2() return M.f1() end
function M.f1() return 42 end

local result = M.f60()
--       ^ hover: (local) result: number

local mid = M.f30()
--    ^ hover: (local) mid: number
