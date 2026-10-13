local PrivProc = DefineCrossStack("CrossPrivProc")

function PrivProc.__private:__init()
    self._count = 0
end

function PrivProc:Reset()
    self._cache = {}
end
