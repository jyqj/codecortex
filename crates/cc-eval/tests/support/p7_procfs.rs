//! Resolve a spawned child's PID within the namespace exposed by procfs.
//! Missing or ambiguous identity remains an error, never isolation evidence.
use std::{collections::BTreeMap, fs, io, path::Path};

fn status(path: &Path) -> io::Result<BTreeMap<String, String>> {
    let mut rows = BTreeMap::new();
    for line in fs::read_to_string(path)?.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if rows
            .insert(key.to_owned(), value.trim().to_owned())
            .is_some()
        {
            return Err(io::Error::other("duplicate proc status field"));
        }
    }
    Ok(rows)
}

fn number(rows: &BTreeMap<String, String>, key: &str) -> io::Result<u32> {
    rows.get(key)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| io::Error::other(format!("missing or invalid proc {key}")))
}

fn namespace_pids(rows: &BTreeMap<String, String>) -> io::Result<Vec<u32>> {
    let ids: Result<Vec<u32>, _> = rows
        .get("NSpid")
        .ok_or_else(|| io::Error::other("proc NSpid is unavailable"))?
        .split_whitespace()
        .map(str::parse)
        .collect();
    let ids = ids.map_err(|_| io::Error::other("invalid proc NSpid"))?;
    if ids.is_empty() || ids.contains(&0) || ids[0] != number(rows, "Pid")? {
        return Err(io::Error::other("inconsistent proc Pid/NSpid"));
    }
    Ok(ids)
}

pub fn child_security(
    proc_root: &Path,
    observer_pid: u32,
    child_pid: u32,
) -> io::Result<BTreeMap<String, String>> {
    let observer = status(&proc_root.join("self/status"))?;
    let observer_ids = namespace_pids(&observer)?;
    if observer_ids.last() != Some(&observer_pid) {
        return Err(io::Error::other("proc self does not identify the observer"));
    }
    let observer_proc_pid = number(&observer, "Pid")?;
    let observer_namespace = fs::read_link(proc_root.join("self/ns/pid"))?;
    let mut matches = Vec::new();
    for entry in fs::read_dir(proc_root)? {
        let entry = entry?;
        let Ok(proc_pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        // Unrelated host processes may disappear or be inaccessible; they
        // cannot contribute positive evidence for this live direct child.
        let Ok(rows) = status(&entry.path().join("status")) else {
            continue;
        };
        let Ok(ids) = namespace_pids(&rows) else {
            continue;
        };
        if number(&rows, "Pid")? != proc_pid
            || number(&rows, "PPid")? != observer_proc_pid
            || ids.len() != observer_ids.len()
            || ids.last() != Some(&child_pid)
            || fs::read_link(entry.path().join("ns/pid")).ok().as_ref() != Some(&observer_namespace)
        {
            continue;
        }
        let mut observed = BTreeMap::new();
        for field in ["NoNewPrivs", "Seccomp", "Seccomp_filters"] {
            number(&rows, field)?;
            observed.insert(field.to_owned(), rows[field].clone());
        }
        observed.insert("ProcPid".into(), proc_pid.to_string());
        observed.insert("RequestedPid".into(), child_pid.to_string());
        observed.insert("ObserverProcPid".into(), observer_proc_pid.to_string());
        observed.insert("NSpid".into(), rows["NSpid"].clone());
        observed.insert(
            "PidNamespace".into(),
            observer_namespace.to_string_lossy().into_owned(),
        );
        observed.insert(
            "identity".into(),
            "same_pid_namespace_and_direct_parent".into(),
        );
        matches.push(observed);
    }
    if matches.len() != 1 {
        return Err(io::Error::other(format!(
            "expected one bound child in procfs, found {} for namespace PID {child_pid}",
            matches.len()
        )));
    }
    Ok(matches.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::symlink,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "p7-procfs-{}-{}",
                std::process::id(),
                SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            let fixture = Self(path);
            fixture.process("self", 90011, 1, "90011 27", "pid:[100]");
            fixture
        }
        fn process(&self, directory: &str, pid: u32, parent: u32, ids: &str, ns: &str) {
            let root = self.0.join(directory);
            fs::create_dir_all(root.join("ns")).unwrap();
            fs::write(root.join("status"), format!("Pid:\t{pid}\nPPid:\t{parent}\nNSpid:\t{ids}\nNoNewPrivs:\t1\nSeccomp:\t2\nSeccomp_filters:\t3\n")).unwrap();
            symlink(ns, root.join("ns/pid")).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn namespace_child_resolves_to_host_proc_pid() {
        let fixture = Fixture::new();
        fixture.process("90012", 90012, 90011, "90012 28", "pid:[100]");
        let result = child_security(&fixture.0, 27, 28).unwrap();
        assert_eq!(result["ProcPid"], "90012");
        assert_eq!(result["RequestedPid"], "28");
        assert_eq!(result["Seccomp_filters"], "3");
    }
    #[test]
    fn unrelated_numeric_pid_cannot_supply_security_evidence() {
        let fixture = Fixture::new();
        fixture.process("28", 28, 1, "28", "pid:[100]");
        assert!(child_security(&fixture.0, 27, 28).is_err());
    }
    #[test]
    fn wrong_namespace_or_parent_is_rejected() {
        for (parent, namespace) in [(90010, "pid:[100]"), (90011, "pid:[101]")] {
            let fixture = Fixture::new();
            fixture.process("90012", 90012, parent, "90012 28", namespace);
            assert!(child_security(&fixture.0, 27, 28).is_err());
        }
    }
    #[test]
    fn missing_proc_or_child_is_not_an_isolation_pass() {
        let fixture = Fixture::new();
        assert!(child_security(&fixture.0, 27, 28).is_err());
        fs::remove_file(fixture.0.join("self/status")).unwrap();
        assert!(child_security(&fixture.0, 27, 28).is_err());
    }
    #[test]
    fn wrong_observer_and_ambiguous_child_are_rejected() {
        let fixture = Fixture::new();
        fixture.process("90012", 90012, 90011, "90012 28", "pid:[100]");
        assert!(child_security(&fixture.0, 26, 28).is_err());
        fixture.process("90013", 90013, 90011, "90013 28", "pid:[100]");
        assert!(child_security(&fixture.0, 27, 28).is_err());
    }
    #[test]
    fn same_namespace_proc_mount_is_supported() {
        let fixture = Fixture::new();
        fs::write(
            fixture.0.join("self/status"),
            "Pid:\t27\nPPid:\t1\nNSpid:\t27\n",
        )
        .unwrap();
        fixture.process("28", 28, 27, "28", "pid:[100]");
        assert_eq!(child_security(&fixture.0, 27, 28).unwrap()["ProcPid"], "28");
    }
    #[test]
    fn actual_sleep_child_has_bound_proc_identity() {
        let mut child = std::process::Command::new("/usr/bin/sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let result = child_security(Path::new("/proc"), std::process::id(), child.id());
        child.kill().unwrap();
        child.wait().unwrap();
        let result = result.unwrap();
        eprintln!("P7_PROC_CHILD_IDENTITY {result:?}");
        assert_eq!(result["RequestedPid"], child.id().to_string());
        assert_eq!(result["identity"], "same_pid_namespace_and_direct_parent");
    }
}
