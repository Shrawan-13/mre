-- Run with: mre test
-- Tests execute after src/main.lua has mounted, in the same Lua runtime.
-- Keep them deterministic: use mre.now(), never os.time().
assert(type(mre.stats) == "function", "mre.stats is missing")
assert(mre.__tree ~= nil, "app did not mount a view")
