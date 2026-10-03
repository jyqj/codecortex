"""Recheck BSD lineage and notices without fetching, importing or ranking code."""
import ast
import copy
import json
from author import ROOT, digest

NAMES = {"parse_list_header", "parse_dict_header", "unquote_header_value"}


def functions(raw):
    lines = raw.decode().splitlines(keepends=True)
    found = {}
    for i, line in enumerate(lines):
        if not line.startswith("def ") or line.split("(")[0][4:] not in NAMES:
            continue
        end = i + 1
        while end < len(lines) and not lines[end].startswith("def "):
            end += 1
        text = "".join(lines[i:end])
        if "\n@" in text:
            text = text[:text.rfind("\n@")]
        node = ast.parse(text).body[0]
        found[node.name] = node
    assert set(found) == NAMES
    return found


class Normalize(ast.NodeTransformer):
    def visit_AnnAssign(self, node):
        return ast.copy_location(ast.Assign(targets=[node.target], value=node.value), node)

    def visit_FunctionDef(self, node):
        node = self.generic_visit(node)
        node.returns = None
        for arg in node.args.args:
            arg.annotation = None
        if node.body and isinstance(node.body[0], ast.Expr) and isinstance(node.body[0].value, ast.Constant) and isinstance(node.body[0].value.value, str):
            node.body = node.body[1:]
        return node


def main():
    folder = ROOT / "license/werkzeug-0.6.2"
    receipt = json.loads((folder / "receipt.json").read_text())
    for entry in receipt["retained"]:
        raw = (folder / entry["path"]).read_bytes()
        assert len(raw) == entry["bytes"] and digest(raw) == entry["sha256"]
    lineage = json.loads((folder / "lineage.json").read_text())
    for name, sha256 in lineage["historical_requests_notices_sha256"].items():
        assert digest((folder / ("requests-introduction-" + name)).read_bytes()) == sha256
    assert digest((folder / "introduction.patch").read_bytes()) == lineage["introduction_patch_sha256"]
    sources = {"werkzeug_0_6_2": folder / "werkzeug/http.py",
               "requests_introduction": folder / "requests-introduction-utils.py",
               "requests_locked": ROOT / "source/src/requests/utils.py"}
    parsed = {}
    for label, path in sources.items():
        raw = path.read_bytes()
        assert digest(raw) == lineage["source_file_sha256"][label]
        parsed[label] = functions(raw)
    for record in lineage["functions"]:
        hashes = []
        for label, nodes in parsed.items():
            normalized = Normalize().visit(copy.deepcopy(nodes[record["symbol"]]))
            value = digest(ast.dump(normalized, include_attributes=False).encode())
            assert value == record["implementation_ast_sha256"][label]
            hashes.append(value)
        assert len(set(hashes)) == 1
    assert (ROOT / "source/src/requests/utils.py").read_text().count("From mitsuhiko/werkzeug (used with permission).") == 3
    print(json.dumps(dict(status="license_lineage_and_notices_verified_not_independent_acceptance",
                         helper_count=3, license_sha256=digest((folder / "LICENSE").read_bytes()),
                         source_and_gold_changed=False)))


if __name__ == "__main__":
    main()
