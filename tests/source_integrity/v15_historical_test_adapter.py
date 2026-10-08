"""Run the unchanged v14 test bodies with their byte-checked historical root.

Only the old test module's import of its guard is adapted. All proof functions,
spies and deliberate rejection predicates continue to call the actual module.
Explicit fixture roots are never redirected. This is not a success substitute.
"""
import atexit
import importlib
from pathlib import Path
import sys
from types import ModuleType, SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import v15_historical_context as history


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
        if name == "ROOT": return context.root
        if name == "REGISTRY": return context.root / "scripts/reviewed-source-registry-v14.json"
        value = getattr(ModuleType.__getattribute__(self, "_real"), name)
        return self._wrap(name, value) if name in HistoricalGuard._WRAPPED else value

    def __setattr__(self, name, value):
        if name in {"_real", "_context"}: ModuleType.__setattr__(self, name, value)
        else: setattr(ModuleType.__getattribute__(self, "_real"), name, value)

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
            if method == "approved_union": self._real.verify_ci(root=self._context.root)
            self._context.after()
            return result
        return call_original


def install():
    original = importlib.import_module("test_reviewed_source_v14")
    if hasattr(original, "_v15_historical_guard"):
        original._v15_historical_guard._context.before()
        return original._v15_historical_guard
    real = importlib.import_module("verify_reviewed_source_v14")
    history.verify_current_history()
    context = history.HistoricalContext()
    atexit.register(context.close)
    selected = HistoricalGuard(real, context)
    real_import = original.importlib.import_module
    def guarded_import(name, package=None):
        return selected if name == "verify_reviewed_source_v14" else real_import(name, package)
    original.importlib = SimpleNamespace(import_module=guarded_import)
    original._v15_historical_guard = selected
    return selected
