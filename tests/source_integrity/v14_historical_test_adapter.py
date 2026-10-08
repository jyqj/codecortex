"""Give the unchanged v13 tests their real historical default root.

The proxy forwards reads, writes and deletion of guard attributes to the actual
module. Patches of constants, proof functions and spies therefore still affect
the globals used by the original function bodies. Explicit fixture roots are
never redirected. This adapter does not install a success predicate.
"""

import atexit
import importlib
from pathlib import Path
import sys
from types import ModuleType

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import v14_historical_context as history

class HistoricalGuard(ModuleType):
    _LOCAL = frozenset({"_real", "_context", "_LOCAL", "_WRAPPED", "_wrap", "__class__"})
    _WRAPPED = {"approved_union": 1, "reconstruct": 2, "verify_snapshots": 0,
                "verify_ci": 0, "load_registry": 0}

    def __init__(self, real, context):
        ModuleType.__init__(self, real.__name__)
        ModuleType.__setattr__(self, "_real", real)
        ModuleType.__setattr__(self, "_context", context)

    def __getattribute__(self, name):
        if name in HistoricalGuard._LOCAL:
            return ModuleType.__getattribute__(self, name)
        context = ModuleType.__getattribute__(self, "_context")
        if name == "ROOT":
            return context.root
        if name == "REGISTRY":
            return context.root / "scripts/reviewed-source-registry-v13.json"
        value = getattr(ModuleType.__getattribute__(self, "_real"), name)
        if name in HistoricalGuard._WRAPPED:
            # Capture at attribute access, just like obtaining real.method.
            # A subsequently installed wraps spy can call this prior function
            # without recursively looking itself up through the real module.
            return self._wrap(name, value)
        return value

    def __setattr__(self, name, value):
        if name in {"_real", "_context"}:
            ModuleType.__setattr__(self, name, value)
        else:
            setattr(ModuleType.__getattribute__(self, "_real"), name, value)

    def __delattr__(self, name):
        delattr(ModuleType.__getattribute__(self, "_real"), name)

    def _wrap(self, method, function):
        position = self._WRAPPED[method]
        parameter = "path" if method == "load_registry" else "root"

        def call_original(*args, **kwargs):
            if len(args) > position or parameter in kwargs:
                return function(*args, **kwargs)
            self._context.before()
            target = self.REGISTRY if parameter == "path" else self._context.root
            result = function(*args, **kwargs, **{parameter: target})
            if method == "approved_union":
                self._real.verify_ci(root=self._context.root)
            self._context.after()
            return result

        return call_original


def install():
    original = importlib.import_module("test_reviewed_source_v13")
    real = importlib.import_module("verify_reviewed_source_v13")
    if isinstance(original.guard, HistoricalGuard):
        original.guard._context.before()
        return original.guard
    history.verify_current_history()
    history.require(original.guard is real, "unexpected historical v13 test guard")
    context = history.HistoricalContext()
    atexit.register(context.close)
    selected = HistoricalGuard(real, context)
    original.guard = selected
    return selected
