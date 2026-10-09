"""Synthetic protocol/admission controls. These are not native study samples."""
import copy
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_profile_matrix as profile
import p8_cold_matrix as cold
import p8_scale_matrix as full
from test_p8_scale_matrix import raw_fixture, build_report

STUDY = profile.study_identity("12345", 1)


def environment():
    return dict(seed_cache_max_symbols=dict(raw=None, read_status="absent", parsed_usize=None, effective=500000),
                runtime_environment={key: None for key in profile.ENVIRONMENT_KEYS[1:]})


def fixture(name="batch_1", fanout=None):
    _, old = raw_fixture(capacity_profile=full.CAPACITY_PROFILE)
    plan = profile.registered_plan(1000, 0, study=STUDY, mutation_profile=name, fanout=fanout)
    if name == "fanout":
        events = [copy.deepcopy(old[0])] + [copy.deepcopy(event) for event in old
            if event.get("event") in ("fanout_started", "fanout_finished") and event["fanout"] == fanout]
        events[1]["case"] = profile.expected_fanout_case(fanout, plan["seed"])
        facts = copy.deepcopy(events[2]["result"]["checkpoints"][0]["incremental"])
        facts["files"] = [dict(file_path=p) for p in sorted(events[1]["case"]["initial"])]
        events[2]["result"]["initial_evidence"] = dict(equal=True, tables=sorted(full.TABLES),
            incremental=facts, full=copy.deepcopy(facts), incremental_report=build_report(full=True),
            full_report=build_report(full=True), process_snapshot=None, resource_scope="synthetic snapshot unavailable",
            runtime_config={"auto_index":{"enabled":False},"indexing":{"dirty_propagation_max_files":8,"db_read_pool_size":1}})
        events[2]["result"]["checkpoints"][0].update(full_report=build_report(full=True), process_snapshot=None)
    else:
        end = next(i for i, event in enumerate(old) if event["event"] == "cold_parity")
        label = "scale-1000/repetition-0/" + name
        events = copy.deepcopy(old[:end + 1]) + [copy.deepcopy(event) for event in old[end + 1:]
            if event.get("label") == label or event.get("label", "").startswith(label + "/")]
        if name.startswith("batch_"):
            witness = events[-1]["independent_config_fact"]
            witness.update(expected_target="f0000.ts", actual_targets=["f0000.ts"])
        for event in events:
            if event["event"] == "build_finished":
                event.update(process_snapshot=None, resource_scope="synthetic snapshot unavailable")
    events[0]["plan"] = plan
    events[0]["profile_environment"] = environment()
    return plan, events


class ProfileRawControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "raw.jsonl"

    def replay(self, plan, events):
        self.path.write_text("".join(json.dumps(event) + "\n" for event in events))
        return profile.inspect_raw(self.path, plan)

    def test_all_eight_profiles_and_actual_fanouts_retain_original_setup_and_checks(self):
        for name, fanout in [(p, None) for p in profile.PROFILES] + [("fanout", n) for n in full.FANOUTS]:
            with self.subTest(profile=name, fanout=fanout):
                plan, events = fixture(name, fanout)
                result = self.replay(plan, events)
                self.assertEqual(len(result["measurements"]), 1 if fanout else 2)
                with self.assertRaisesRegex(ValueError, "raw stage protocol"):
                    full.inspect_raw(self.path, plan)
                with self.assertRaisesRegex(ValueError, "raw stage protocol"):
                    cold.inspect_raw(self.path, plan)

    def test_batch_fresh_target_zero_is_required_not_deleted(self):
        plan, events = fixture()
        self.replay(plan, events)
        events[-1]["independent_config_fact"].update(expected_target="f0001.ts", actual_targets=["f0001.ts"])
        with self.assertRaises(ValueError): self.replay(plan, events)
        del events[-1]["independent_config_fact"]
        with self.assertRaises(ValueError): self.replay(plan, events)

    def test_missing_setup_final_parity_extra_profile_and_resume_after_complete_are_rejected(self):
        plan, original = fixture("body")
        changes = [lambda e: e.pop(5), lambda e: e[-1]["parity"]["tables"].pop(),
                   lambda e: e.append(dict(event="mutation", label="scale-1000/repetition-0/api")),
                   lambda e: e[-1].update(incremental_builds=2),
                   lambda e: e[-2]["report"].update(parse_errors=["synthetic failure"])]
        for change in changes:
            events = copy.deepcopy(original); change(events)
            with self.subTest(change=change), self.assertRaises(ValueError): self.replay(plan, events)
        self.replay(plan, original)
        self.path.write_bytes(self.path.read_bytes().rstrip(b"\n"))
        with self.assertRaisesRegex(ValueError, "truncated"): profile.inspect_raw(self.path, plan)

    def test_fanout_initial_full_final_report_truth_and_actual_file_count_cannot_be_dropped(self):
        plan, original = fixture("fanout", 16)
        changes = [lambda e: e[-1]["result"]["initial_evidence"].pop("full"),
            lambda e: e[-1]["result"]["initial_evidence"]["incremental"]["files"].pop(),
            lambda e: e[-1]["result"]["checkpoints"][0].pop("full_report"),
            lambda e: e[-1]["result"]["checkpoints"][0]["truth"].pop(),
            lambda e: e[1]["case"]["initial"].update({"api.ts":"wrong body"})]
        for change in changes:
            events=copy.deepcopy(original);change(events)
            with self.subTest(change=change), self.assertRaises(ValueError): self.replay(plan, events)

    def test_wrong_study_profile_budget_and_scope_are_rejected(self):
        for kwargs in (dict(repetitions=29),dict(deadline_ms=18000001),dict(mutation_profile="cold"),
                       dict(fanout=1),dict(seed=1),dict(shard_index=True)):
            args=dict(scale=1000,shard_index=0,study=STUDY,mutation_profile="body");args.update(kwargs)
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):profile.registered_plan(**args)
        plan, events=fixture()
        events[0]["plan"]=copy.deepcopy(plan);events[0]["plan"]["profile_study"]["run_id"]="54321"
        with self.assertRaisesRegex(ValueError,"header/plan"):self.replay(plan,events)


def synthetic_population():
    parsed={}
    with tempfile.TemporaryDirectory() as tmp:
        for name,n in [(p,None) for p in profile.PROFILES]+[("fanout",n) for n in full.FANOUTS]:
            plan,events=fixture(name,n);path=Path(tmp)/"raw.jsonl"
            path.write_text("".join(json.dumps(e)+"\n" for e in events));parsed[(name,n)]=profile.inspect_raw(path,plan)
    shards=[]
    for name,amount,rep in sorted(profile.expected_slots()):
        fanout=name=="fanout";scale=1000 if fanout else amount
        item=copy.deepcopy(parsed[(name,amount if fanout else None)])
        plan=profile.registered_plan(scale,rep,study=STUDY,mutation_profile=name,fanout=amount if fanout else None)
        for sample in item["measurements"]:
            if fanout:sample["sample"]=f"fanout-{amount}/repetition-{rep}"
            else:
                sample["sample"]=f"scale-{amount}/repetition-{rep}"
                sample["group"]=f"scale-{amount}/"+sample["group"].split('/')[-1]
        item.update(plan=plan,study=STUDY,driver_source={"synthetic":True},build_receipt_sha256="a"*64,
            directory=f"synthetic/{name}/{amount}/{rep}",receipt_sha256="b"*64,
            environment={"host":"synthetic","kernel":"synthetic"},
            measurement_environment={key:None for key in profile.ENVIRONMENT_KEYS})
        shards.append(item)
    return shards


class ProfilePopulationControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):cls.population=synthetic_population()

    def test_exact_1200_mutations_plus_150_fanout_and_1200_setup(self):
        result=profile.combine(self.population)
        self.assertTrue(result["passed"]);self.assertEqual(result["sample_count"],1350)
        self.assertEqual(result["mutation_sample_count"],1200);self.assertEqual(result["setup_pair_count"],1200)
        self.assertEqual(len(result["groups"]),45);self.assertTrue(all(g["n"]==30 for g in result["groups"]))
        self.assertFalse(result["task_statuses_changed"])

    def test_missing_duplicate_mixed_source_attempt_and_missing_over_budget_observation(self):
        for population in (self.population[:-1],self.population+[self.population[0]]):
            with self.assertRaises(ValueError):profile.combine(population)
        for mutate in (lambda s:s[0]["engine"].update(binary_digest="f"*64),
                       lambda s:s[0]["study"].update(run_attempt=2),
                       lambda s:s[0]["plan"].update(deadline_ms=18000001)):
            rows=copy.deepcopy(self.population);mutate(rows)
            with self.assertRaises(ValueError):profile.combine(rows)
        rows=copy.deepcopy(self.population)
        for row in rows:
            for sample in row["measurements"]:
                if "first_build_incomplete" in sample:sample["first_build_incomplete"]=False
        with self.assertRaisesRegex(ValueError,"over-budget"):profile.combine(rows)

    def test_workflow_chain_has_exact_1350_cells_and_global_ten_not_ninety(self):
        # Parse only this fixed scalar subset without adding a PyYAML dependency to CI.
        text=(Path(__file__).resolve().parents[2]/'.github/workflows/p8-profile.yml').read_text()
        self.assertEqual(text.count('max-parallel: 10'),9)
        self.assertEqual(text.count('timeout-minutes: 350'),9)
        self.assertEqual(text.count('        shard: ['+', '.join(map(str,range(30)))+']'),9)
        self.assertIn("github.event.label.name == 'p8-profile-run'",text)
        previous='build'
        for name in (*profile.PROFILES,'fanout'):
            expected='    needs: [build'+('' if previous=='build' else ', '+previous)+']'
            section=text.split('  measure_'+name+':\n',1)[1].split('\n  ',1)[0]
            # The jobs themselves are checked by the distinct exact dependency text.
            self.assertIn('  measure_'+name+':\n'+expected,text)
            previous='measure_'+name
        self.assertNotIn('p8-scale-shard-',text)
        self.assertNotIn('p8-cold-shard-',text)


if __name__=='__main__':unittest.main()
