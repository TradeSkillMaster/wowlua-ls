local _, ns = ...
local sizes = ns.Module1.Size() + ns.Module2.Size() + ns.Module3.Size()
    + ns.Module4.Size() + ns.Module5.Size() + ns.Module6.Size()
local got = ns.CoreA.Total()
