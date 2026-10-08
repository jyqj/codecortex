"""Execute four unchanged historical test methods in their fixed CI context.

Only their method-local import of the v12 selector receives a facade. Its
zero-argument call runs the real v12 verifier on the byte-checked historical
snapshot; explicit roots run that same verifier on the supplied fixture. The
v12 module, guard functions, test source bytes and assertion bodies are intact.
"""
import functools
import importlib
import sys
from types import ModuleType
from unittest.mock import patch

METHODS = (
    ("test_current_source_v2", "CaptureSourceTests",
     "test_ci_selects_explicit_reviewed_version_and_keeps_historical_p0"),
    ("test_current_source_v3", "OwnerSourceTests",
     "test_ci_selector_only_change_and_historical_p0_preserved"),
    ("test_reviewed_source_v10", "SequentialReviewTests",
     "test_current_workflows_have_only_the_explicit_migration"),
    ("test_reviewed_source_v11", "WorkerFixtureReviewTests",
     "test_current_ci_and_every_old_step_are_preserved"),
)


def facade(guard):
    # Verify the snapshot bytes and execute its real historical CI proof before
    # exposing a default root. No return value or approval predicate is mocked.
    guard.verify_snapshots(guard.ROOT)
    real = guard.previous
    selected = ModuleType(real.__name__)
    selected.__dict__.update(real.__dict__)
    missing = object()

    def verify_ci(root=missing):
        return real.verify_ci(root=guard.ROOT / guard.CI_SNAPSHOT if root is missing else root)

    selected.verify_ci = verify_ci
    return selected


def install(guard):
    # Discovery can import this adapter before setUpClass executes any proof.
    # A shallow CI checkout must fetch the immutable base before reading it.
    guard.git.ensure_refs([guard.BASE])
    originals = []
    for module_name, class_name, method_name in METHODS:
        module = importlib.import_module(module_name)
        path = "tests/source_integrity/" + module_name + ".py"
        guard.unchanged_file(guard.ROOT, path, guard.git.blob(guard.BASE, path),
                             "historical source-integrity test changed")
        cls = getattr(module, class_name)
        original = getattr(cls, method_name)
        if hasattr(original, "__v13_original__"):
            originals.append(original.__v13_original__)
            continue

        @functools.wraps(original)
        def adapted(self, _original=original):
            selected = facade(guard)
            # The original method's own import sees this temporary namespace.
            # All already imported guards keep their actual module references.
            with patch.dict(sys.modules, {guard.previous.__name__: selected}):
                return _original(self)

        adapted.__v13_original__ = original
        setattr(cls, method_name, adapted)
        originals.append(original)
    return originals
