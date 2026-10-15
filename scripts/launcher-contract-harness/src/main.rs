//! Launcher-contract smoke harness.
//!
//! Usage: contract-harness <distribution.json> [--runtime <expected_version> <target>]
//! Targets: windows-x86_64 | macos-aarch64 | macos-x86_64
//!
//! Prints what `hambugui-distribution` sees in a Nebula distribution and exits 1 with the
//! launcher's own error code when either parser rejects it.

use hambugui_distribution::{
    ArtifactDigest, DistributionError, DistributionTarget, Module, parse_distribution,
    parse_runtime_distribution,
};
use std::collections::BTreeMap;
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (path, runtime) = match args.as_slice() {
        [path] => (path.as_str(), None),
        [path, flag, version, target] if flag == "--runtime" => {
            (path.as_str(), Some((version.as_str(), parse_target(target))))
        }
        _ => {
            eprintln!(
                "usage: contract-harness <distribution.json> [--runtime <expected_version> <target>]"
            );
            exit(2);
        }
    };

    let bytes = std::fs::read(path).unwrap_or_else(|err| {
        eprintln!("error: cannot read {path}: {err}");
        exit(2);
    });

    let distribution = parse_distribution(&bytes).unwrap_or_else(|err| fail("parse", &err));
    let mut modules = Vec::new();
    for server in &distribution.servers {
        collect(&server.modules, &mut modules);
    }
    let mut types: BTreeMap<String, usize> = BTreeMap::new();
    let mut digests: BTreeMap<&str, usize> = BTreeMap::new();
    for module in &modules {
        *types.entry(format!("{:?}", module.module_type)).or_default() += 1;
        let kind = match module.artifact.as_ref().map(|artifact| &artifact.digest) {
            Some(ArtifactDigest::Sha256(_)) => "SHA256",
            Some(ArtifactDigest::LegacyMd5(_)) => "MD5",
            None => "none",
        };
        *digests.entry(kind).or_default() += 1;
    }

    println!("file:     {path}");
    println!("version:  {}", distribution.version);
    println!("servers:  {}", distribution.servers.len());
    println!("modules:  {} (recursive)", modules.len());
    println!("types:    {}", histogram(types.iter().map(|(k, v)| (k.as_str(), *v))));
    println!("digests:  {}", histogram(digests.into_iter()));
    println!("parse:    OK");

    if let Some((expected_version, target)) = runtime {
        parse_runtime_distribution(&bytes, expected_version, target)
            .unwrap_or_else(|err| fail("runtime", &err));
        println!(
            "runtime:  OK (version {expected_version}, target {})",
            target.as_manifest_value()
        );
    }
}

fn collect<'a>(modules: &'a [Module], out: &mut Vec<&'a Module>) {
    for module in modules {
        out.push(module);
        collect(&module.submodules, out);
    }
}

fn histogram<'a>(counts: impl Iterator<Item = (&'a str, usize)>) -> String {
    let text = counts
        .map(|(key, count)| format!("{key}={count}"))
        .collect::<Vec<_>>()
        .join(", ");
    if text.is_empty() { "-".to_owned() } else { text }
}

fn parse_target(target: &str) -> DistributionTarget {
    match target {
        "windows-x86_64" => DistributionTarget::WindowsX86_64,
        "macos-aarch64" => DistributionTarget::MacOsAarch64,
        "macos-x86_64" => DistributionTarget::MacOsX86_64,
        other => {
            eprintln!("error: unknown target {other}");
            exit(2);
        }
    }
}

fn fail(stage: &str, err: &DistributionError) -> ! {
    eprintln!("{stage}: FAILED");
    eprintln!("code:     {}", err.code);
    eprintln!("key:      {}", err.message_key);
    eprintln!("detail:   {}", err.detail);
    exit(1);
}
