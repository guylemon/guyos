//! Architecture smoke tests — keep inner layers free of direct iroh adapter coupling
//! (see `docs/planning/phase-0-composition-root.md`).

use std::fs;
use std::path::{Path, PathBuf};

const LAYER_ROOTS: [&str; 2] = ["src/application", "src/ports"];

/// Substrings that must not appear in application/ports sources (`application/` depends on `domain` + `ports` only).
const FORBIDDEN: [&str; 3] = ["iroh::", "iroh_gossip::", "IrohGossipRelayBackend"];

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[test]
fn application_and_ports_avoid_direct_iroh_adapter_references() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for rel in LAYER_ROOTS {
        collect_rs_files(&manifest_dir.join(rel), &mut files);
    }
    files.sort();

    let mut violations = Vec::new();
    for path in files {
        let content = fs::read_to_string(&path).expect("read source");
        let rel = path.strip_prefix(&manifest_dir).unwrap_or(&path);
        for pat in FORBIDDEN {
            if content.contains(pat) {
                violations.push(format!("{} contains `{pat}`", rel.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "layer policy violations:\n{}",
        violations.join("\n")
    );
}
