from pathlib import Path
p=Path('/workspace/review-owner-evidence')
s=(p/'archive/receiver_micro_driver.rs').read_text().replace('Harbor','Beacon').replace('harbor','beacon').replace('Seal','Commit').replace('Port','Sink').replace('CargoCode','SeedCode').replace('Load','Accept')
s=s.replace('let cases = [','let cases = [\n        ("Which Beacon API is deprecated?", Some(("Beacon", "class"))),\n        ("Which Beacon API rather than Sink accepts state?", None),\n        ("Which Beacon API calls Commit/Accept?", None),\n        ("Which Beacon API differs from Sink?", None),\n        ("name:Beacon Which Beacon API calls Commit?", Some(("Beacon", "class"))),\n        ("kind:class Which Beacon API calls Commit?", Some(("Beacon", "class"))),')
s=s.replace('println!("16 self-authored queries','println!("22 synthetic queries')
s=s.replace('for (query, expected) in cases {','for (query, expected) in cases {')
# Additional top-k membership probe, independent of the author\'s top_k=10 claim.
s=s.replace('std::fs::write(out.join("results.json")', '''let query = "Which Beacon API changes the ready state with Commit?";
    let small = serde_json::to_value(index.search().search_in_context(query, 1, None).unwrap()).unwrap();
    result["top_k_1"] = small;
    std::fs::write(out.join("results.json")''')
(p/'independent_driver.rs').write_text(s)
