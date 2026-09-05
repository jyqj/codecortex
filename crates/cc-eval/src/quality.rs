//! Independent, identity-aware evaluation. Never consumes production scores.
use std::collections::{BTreeMap, BTreeSet};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub id: String,
    pub revision: String,
    /// Only these files are indexed. Labels and task descriptions stay outside.
    pub files: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub id: String,
    pub file_path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub grade: u32,
    #[serde(default)]
    pub symbol: Option<String>,
    /// A source anchor must actually occur in the returned text. None means
    /// a locator label, not proof that the implementation was read.
    #[serde(default)]
    pub anchor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub id: String,
    pub repo: String,
    pub category: String,
    pub tool: String,
    pub params: Value,
    pub result_pointer: String,
    pub labels: Vec<Label>,
    #[serde(default)]
    pub required_groups: Vec<Vec<String>>,
    #[serde(default)]
    pub no_answer: bool,
    #[serde(default)]
    pub min_recall_at_5: Option<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub dataset_id: String,
    pub purpose: String,
    pub repositories: Vec<Repository>,
    pub tasks: Vec<Task>,
}

pub fn valid_relative_path(path: &str) -> bool {
    !path.is_empty() && !path.contains('\\') && !path.contains(':')
        && path.split('/').all(|s| !matches!(s, "" | "." | ".." | ".git" | ".codecortex" | ".codecortex.json"))
}
impl Manifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.dataset_id.is_empty()
            || !matches!(self.purpose.as_str(), "regression" | "held_out")
            || self.repositories.is_empty() || self.tasks.is_empty() {
            return Err("invalid manifest header or empty dataset".into());
        }
        let mut repos = BTreeMap::new();
        for repo in &self.repositories {
            if repo.id.is_empty() || repo.revision.is_empty() || repo.files.is_empty()
                || repos.insert(&repo.id, repo).is_some()
                || repo.files.keys().any(|p| !valid_relative_path(p)) {
                return Err(format!("invalid/duplicate repository {}", repo.id));
            }
        }
        let mut task_ids = BTreeSet::new();
        for task in &self.tasks {
            if task.id.is_empty() || !task_ids.insert(&task.id)
                || !matches!(task.tool.as_str(), "search" | "context")
                || !task.params.is_object() || task.params.get("project_path").is_some()
                || (!task.result_pointer.is_empty() && !task.result_pointer.starts_with('/'))
                || task.no_answer != task.labels.is_empty()
                || task.min_recall_at_5.is_some_and(|n| !n.is_finite() || !(0.0..=1.0).contains(&n)) {
                return Err(format!("invalid task {}", task.id));
            }
            let repo = repos.get(&task.repo).ok_or_else(|| format!("unknown repo {}", task.repo))?;
            let mut labels = BTreeSet::new();
            for label in &task.labels {
                let text = repo.files.get(&label.file_path).ok_or("label path absent from snapshot")?;
                if label.id.is_empty() || !labels.insert(&label.id) || label.start_line == 0
                    || label.end_line < label.start_line || label.end_line as usize > text.lines().count()
                    || !(1..=5).contains(&label.grade)
                    || label.symbol.as_ref().is_some_and(|s| s.is_empty()) {
                    return Err(format!("invalid label {} in {}", label.id, task.id));
                }
                if let Some(anchor) = &label.anchor {
                    let region = text.lines().skip(label.start_line as usize - 1)
                        .take((label.end_line - label.start_line + 1) as usize).collect::<Vec<_>>().join("\n");
                    if anchor.is_empty() || !region.contains(anchor) {
                        return Err(format!("anchor not in labeled source: {}", label.id));
                    }
                }
            }
            for group in &task.required_groups {
                if group.is_empty() || group.iter().any(|id| !labels.contains(id)) {
                    return Err(format!("invalid evidence group in {}", task.id));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    pub task_id: String,
    pub mode: String,
    pub iteration: usize,
    pub elapsed_us: u64,
    /// Entire unwrapped handler JSON bytes, not just the source fragments.
    pub output_bytes: usize,
    pub output: Option<Value>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub schema_version: u32,
    pub implementation_commit: String,
    pub rustc: String,
    pub manifest_git_blob: String,
    pub repetitions: usize,
    pub variant: String,
    pub effective_config: Value,
    pub manifest: Manifest,
}
#[derive(Debug, Clone, Serialize)]
pub struct Metrics {
    pub recall_at_5: f64,
    pub reciprocal_rank: f64,
    pub ndcg_at_5: f64,
    pub evidence_sufficient: bool,
    pub correct_abstention: bool,
    pub schema_error: bool,
    pub returned: usize,
}
fn matches_label(hit: &Value, label: &Label) -> bool {
    if hit.get("file_path").and_then(Value::as_str) != Some(label.file_path.as_str()) { return false; }
    let start = hit.get("start_line").and_then(Value::as_u64);
    let end = hit.get("end_line").and_then(Value::as_u64);
    if !start.is_some_and(|s| s <= label.start_line as u64)
        || !end.is_some_and(|e| e >= label.end_line as u64) { return false; }
    if let Some(symbol) = &label.symbol {
        let name = hit.get("symbol_name").or_else(|| hit.get("name")).and_then(Value::as_str);
        if name != Some(symbol.as_str()) { return false; }
    }
    label.anchor.as_ref().is_none_or(|anchor| hit.get("text").and_then(Value::as_str)
        .is_some_and(|text| text.contains(anchor)))
}
/// Original wire ranks: unnamed and duplicate entries occupy positions. Each
/// label earns novelty credit once, independently of production ranking scores.
pub fn score(task: &Task, sample: &Sample) -> Metrics {
    let mut m = Metrics { recall_at_5: 0.0, reciprocal_rank: 0.0, ndcg_at_5: 0.0,
        evidence_sufficient: false, correct_abstention: false, schema_error: false, returned: 0 };
    if sample.error.is_some() { return m; }
    let Some(hits) = sample.output.as_ref().and_then(|o| o.pointer(&task.result_pointer)).and_then(Value::as_array) else {
        m.schema_error = true; return m;
    };
    m.returned = hits.len();
    if task.no_answer { m.correct_abstention = hits.is_empty(); return m; }
    let mut seen = BTreeSet::new();
    let mut top5 = BTreeSet::new();
    let mut dcg = 0.0;
    for (offset, hit) in hits.iter().enumerate() {
        let rank = offset + 1;
        let matching: Vec<_> = task.labels.iter().filter(|label| matches_label(hit, label)).collect();
        if m.reciprocal_rank == 0.0 && !matching.is_empty() { m.reciprocal_rank = 1.0 / rank as f64; }
        // One graded novelty gain per result; coverage may include several labels.
        let gain = matching.iter().filter(|label| !seen.contains(&label.id))
            .map(|label| (2.0f64).powi(label.grade as i32) - 1.0).fold(0.0, f64::max);
        for label in matching {
            seen.insert(label.id.clone());
            if rank <= 5 { top5.insert(label.id.clone()); }
        }
        if rank <= 5 { dcg += gain / ((rank + 1) as f64).log2(); }
    }
    let mut grades: Vec<_> = task.labels.iter().map(|l| l.grade).collect();
    grades.sort_unstable_by(|a,b| b.cmp(a));
    let ideal: f64 = grades.iter().take(5).enumerate().map(|(i,g)|
        ((2.0f64).powi(*g as i32)-1.0)/((i+2) as f64).log2()).sum();
    m.ndcg_at_5 = if ideal > 0.0 { dcg / ideal } else { 0.0 };
    m.recall_at_5 = top5.len() as f64 / task.labels.len() as f64;
    m.evidence_sufficient = !task.required_groups.is_empty() && task.required_groups.iter()
        .all(|group| group.iter().any(|id| seen.contains(id)));
    m
}
/// Nearest-rank percentile over every request, never per-case minima.
pub fn percentile(values: &[u64], fraction: f64) -> Option<u64> {
    if values.is_empty() || !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) { return None; }
    let mut sorted = values.to_vec(); sorted.sort_unstable();
    Some(sorted[((sorted.len() as f64 * fraction).ceil() as usize).saturating_sub(1)])
}
/// Reject incomplete/mixed/duplicate observations. Preserve per-task samples for
/// paired comparisons and keep errors in their original quality denominators.
pub fn report(header: &Header, samples: &[Sample]) -> Result<Value, String> {
    header.manifest.validate()?;
    if header.schema_version != 1 || header.repetitions == 0 { return Err("bad run header".into()); }
    let tasks: BTreeMap<_,_> = header.manifest.tasks.iter().map(|t| (t.id.as_str(),t)).collect();
    let mut keys = BTreeSet::new();
    let mut rows = Vec::new();
    let mut latency: BTreeMap<&str,Vec<u64>> = BTreeMap::new();
    let mut gates = Vec::new();
    let mut totals = Totals::default();
    let mut by_repo: BTreeMap<String, Totals> = BTreeMap::new();
    let mut by_category: BTreeMap<String, Totals> = BTreeMap::new();
    for sample in samples {
        let task = tasks.get(sample.task_id.as_str()).ok_or("unknown sample task")?;
        if !matches!(sample.mode.as_str(), "cold_session" | "warm_cache")
            || sample.iteration >= header.repetitions
            || !keys.insert((&sample.task_id,&sample.mode,sample.iteration))
            || sample.output.is_some() == sample.error.is_some() {
            return Err("invalid or duplicate observation".into());
        }
        if let Some(output) = &sample.output {
            if serde_json::to_vec(output).map_err(|e|e.to_string())?.len() != sample.output_bytes {
                return Err("serialized output byte count mismatch".into());
            }
        }
        let m = score(task,sample);
        totals.record(task, &m, sample.error.is_some());
        by_repo.entry(task.repo.clone()).or_default().record(task, &m, sample.error.is_some());
        by_category.entry(task.category.clone()).or_default().record(task, &m, sample.error.is_some());
        if m.schema_error || sample.error.is_some()
            || (task.no_answer && !m.correct_abstention)
            || task.min_recall_at_5.is_some_and(|min| m.recall_at_5 < min) {
            gates.push(format!("{}:{}:{}",task.id,sample.mode,sample.iteration));
        }
        latency.entry(&sample.mode).or_default().push(sample.elapsed_us);
        rows.push(serde_json::json!({"task_id":task.id,"repo":task.repo,"category":task.category,
            "mode":sample.mode,"iteration":sample.iteration,"no_answer":task.no_answer,
            "metrics":m,"error":sample.error,"elapsed_us":sample.elapsed_us,"output_bytes":sample.output_bytes}));
    }
    if keys.len() != tasks.len() * header.repetitions * 2 { return Err("incomplete sample grid".into()); }
    let latency: BTreeMap<_,_> = latency.iter().map(|(mode,v)| (*mode,serde_json::json!({
        "samples":v.len(),"p50_us":percentile(v,0.5),"p95_us":percentile(v,0.95),"max_us":v.iter().max()
    }))).collect();
    Ok(serde_json::json!({"schema_version":1,"dataset_id":header.manifest.dataset_id,
        "purpose":header.manifest.purpose,"manifest_git_blob":header.manifest_git_blob,
        "implementation_commit":header.implementation_commit,"variant":header.variant,
        "effective_config":header.effective_config,"latency":latency,"observations":rows,
        "summary":totals.finish(),
        "by_repo":by_repo.iter().map(|(k,v)|(k,v.finish())).collect::<BTreeMap<_,_>>(),
        "by_category":by_category.iter().map(|(k,v)|(k,v.finish())).collect::<BTreeMap<_,_>>(),
        "gate_failures":gates,"passed":gates.is_empty(),
        "notes":["All original ranks and failure observations retained.",
            "Bytes measure complete unwrapped handler JSON, not model tokens or JSON-RPC framing.",
            "Small regression fixtures do not establish cross-repository generalization."]}))
}

#[derive(Default)]
struct Totals {
    positive: usize, negative: usize, evidence_tasks: usize, errors: usize,
    recall: f64, rr: f64, ndcg: f64, abstained: usize, sufficient: usize,
}
impl Totals {
    fn record(&mut self, task: &Task, m: &Metrics, tool_error: bool) {
        self.errors += usize::from(tool_error || m.schema_error);
        if task.no_answer {
            self.negative += 1; self.abstained += usize::from(m.correct_abstention);
        } else {
            self.positive += 1; self.recall += m.recall_at_5;
            self.rr += m.reciprocal_rank; self.ndcg += m.ndcg_at_5;
            if !task.required_groups.is_empty() {
                self.evidence_tasks += 1; self.sufficient += usize::from(m.evidence_sufficient);
            }
        }
    }
    fn finish(&self) -> Value {
        fn ratio(sum: f64, n: usize) -> Option<f64> { (n > 0).then(||sum/n as f64) }
        serde_json::json!({"positive_observations":self.positive,"negative_observations":self.negative,
            "errors":self.errors,"recall_at_5":ratio(self.recall,self.positive),
            "mrr":ratio(self.rr,self.positive),"ndcg_at_5":ratio(self.ndcg,self.positive),
            "correct_abstention_rate":ratio(self.abstained as f64,self.negative),
            "evidence_sufficiency_rate":ratio(self.sufficient as f64,self.evidence_tasks)})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn task() -> Task {
        serde_json::from_value(json!({"id":"q","repo":"r","category":"test","tool":"search",
          "params":{},"result_pointer":"","labels":[{"id":"gold","file_path":"right.py",
          "start_line":1,"end_line":1,"grade":3,"symbol":"run","anchor":"answer"}],
          "required_groups":[["gold"]]})).unwrap()
    }
    fn sample(output: Value) -> Sample {
        Sample { task_id:"q".into(),mode:"warm_cache".into(),iteration:0,elapsed_us:1,
            output_bytes:serde_json::to_vec(&output).unwrap().len(),output:Some(output),error:None }
    }
    fn hit() -> Value { json!({"file_path":"right.py","start_line":1,"end_line":3,"symbol_name":"run","text":"answer"}) }
    #[test] fn raw_rank_six_is_not_rank_one() {
        let m=score(&task(), &sample(json!([{}, {}, {}, {}, {}, hit()])));
        assert_eq!(m.recall_at_5,0.0); assert_eq!(m.reciprocal_rank,1.0/6.0); assert!(m.evidence_sufficient);
    }
    #[test] fn wrong_file_same_name_is_not_relevant() {
        let mut h=hit(); h["file_path"]=json!("wrong.py");
        assert_eq!(score(&task(),&sample(json!([h]))).recall_at_5,0.0);
    }
    #[test] fn duplicate_results_do_not_gain_twice() {
        let m=score(&task(),&sample(json!([hit(),hit(),hit()])));
        assert_eq!(m.recall_at_5,1.0); assert_eq!(m.ndcg_at_5,1.0); assert_eq!(m.returned,3);
    }
    #[test] fn missing_source_anchor_is_not_evidence() {
        let mut h=hit(); h["text"]=json!("truncated");
        assert_eq!(score(&task(),&sample(json!([h]))).recall_at_5,0.0);
    }
    #[test] fn failures_are_not_correct_abstentions() {
        let mut t=task(); t.labels.clear(); t.required_groups.clear(); t.no_answer=true;
        let mut s=sample(json!([])); assert!(score(&t,&s).correct_abstention);
        s.output=None; s.error=Some("read failed".into()); assert!(!score(&t,&s).correct_abstention);
    }
    #[test] fn missing_result_array_is_a_schema_failure() { assert!(score(&task(),&sample(json!({}))).schema_error); }
    #[test] fn percentile_uses_every_request() {
        assert_eq!(percentile(&[1,100,2,200],0.95),Some(200)); assert_eq!(percentile(&[],0.95),None);
    }
    #[test] fn path_traversal_and_control_files_are_rejected() {
        for p in ["../x","/x","a/../x","a\\x",".codecortex.json",".git/config","C:/x"] { assert!(!valid_relative_path(p)); }
        assert!(valid_relative_path("src/支付.py"));
    }
    #[test] fn incomplete_and_duplicate_runs_fail_closed() {
        let header=Header { schema_version:1,implementation_commit:"test".into(),rustc:"test".into(),
          manifest_git_blob:"test".into(),repetitions:1,variant:"default".into(),effective_config:json!({}),
          manifest:Manifest {schema_version:1,dataset_id:"test".into(),purpose:"regression".into(),
            repositories:vec![Repository{id:"r".into(),revision:"fixture-v1".into(),files:BTreeMap::from([("right.py".into(),"answer\n".into())])}],tasks:vec![task()]}};
        let s=sample(json!([hit()]));
        assert!(report(&header,&[s.clone()]).is_err()); assert!(report(&header,&[s.clone(),s.clone()]).is_err());
        let mut cold=s.clone(); cold.mode="cold_session".into();
        assert_eq!(report(&header,&[s,cold]).unwrap()["passed"],true);
    }
}
