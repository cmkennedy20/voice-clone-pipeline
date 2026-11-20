#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    fs,
    path::{Path, PathBuf},
};

fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| manifest_dir.to_path_buf(), Path::to_path_buf)
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
}

fn strip_line_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

#[test]
fn forbids_legacy_mod_rs_files_across_workspace() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);
    collect_rs_files(&root.join("bins"), &mut files);

    let violations: Vec<PathBuf> = files
        .into_iter()
        .filter(|path| path.file_name().and_then(|name| name.to_str()) == Some("mod.rs"))
        .collect();
    assert!(
        violations.is_empty(),
        "Forbidden mod.rs files detected (use named module roots foo.rs + foo/bar.rs): {violations:#?}"
    );
}

#[test]
fn enforces_pure_dependencies_in_common_and_domain() {
    let root = workspace_root();
    let forbidden = [
        "tokio",
        "sqlx",
        "axum",
        "reqwest",
        "tower",
        "hyper",
        "serenity",
        "poise",
        "songbird",
        "whisper-rs",
    ];

    for crate_name in ["common", "domain"] {
        let manifest_path = root.join("crates").join(crate_name).join("Cargo.toml");
        let cargo_toml = fs::read_to_string(&manifest_path).unwrap();
        for banned in forbidden {
            assert!(
                !cargo_toml.contains(banned),
                "crates/{crate_name}/Cargo.toml must stay 100% pure and cannot depend on `{banned}`"
            );
        }
    }
}

#[test]
fn enforces_one_way_crate_dependency_dag() {
    let root = workspace_root();
    let forbidden_edges: &[(&str, &[&str])] = &[
        ("common", &["domain", "framework", "features", "api"]),
        ("domain", &["framework", "features", "api"]),
        ("framework", &["features", "api"]),
        ("features", &["api"]),
    ];

    for (crate_name, disallowed_upstreams) in forbidden_edges {
        let manifest_path = root.join("crates").join(crate_name).join("Cargo.toml");
        let cargo_toml = fs::read_to_string(&manifest_path).unwrap();
        for upstream in *disallowed_upstreams {
            let workspace_dep = format!("{upstream}.workspace");
            let table_dep = format!("{upstream} =");
            let has_violation = cargo_toml.lines().any(|line| {
                let trimmed = line.split('#').next().unwrap_or("").trim_start();
                trimmed.starts_with(&workspace_dep) || trimmed.starts_with(&table_dep)
            });
            assert!(
                !has_violation,
                "Crate `{crate_name}` cannot depend upward on `{upstream}` in {manifest_path:?}"
            );
        }
    }
}

#[test]
fn forbids_horizontal_imports_between_sibling_feature_capabilities() {
    let features_src = workspace_root().join("crates/features/src");
    let capabilities = ["voice_jobs", "profiles", "benchmarks", "desktop"];

    for cap in capabilities {
        let mut files = Vec::new();
        let root_module = features_src.join(format!("{cap}.rs"));
        if root_module.is_file() {
            files.push(root_module);
        }
        collect_rs_files(&features_src.join(cap), &mut files);

        for file in files {
            let content = fs::read_to_string(&file).unwrap();
            for sibling in capabilities {
                if sibling != cap {
                    let banned_patterns = [
                        format!("crate::{sibling}"),
                        format!("super::{sibling}"),
                        format!("features::{sibling}"),
                    ];
                    for (line_idx, raw_line) in content.lines().enumerate() {
                        let code_line = strip_line_comment(raw_line);
                        for banned in &banned_patterns {
                            assert!(
                                !code_line.contains(banned),
                                "Horizontal sibling feature import `{banned}` forbidden at {}:{}; move shared SQL to `shared_queries/` or pure logic to `crates/domain`.",
                                file.display(),
                                line_idx + 1
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn enforces_production_source_file_line_ceiling() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);

    let mut oversized = Vec::new();
    for file in files {
        if file.components().any(|comp| comp.as_os_str() == "src") {
            let content = fs::read_to_string(&file).unwrap();
            let prod_code = content.split("#[cfg(test)]").next().unwrap_or(&content);
            let line_count = prod_code.lines().count();
            if line_count > 350 {
                oversized.push((file, line_count));
            }
        }
    }

    assert!(
        oversized.is_empty(),
        "Production crates/**/src/** files exceeded 350 non-test LOC ceiling: {oversized:#?}"
    );
}

#[test]
fn enforces_binary_entrypoint_line_ceiling() {
    let bins_dir = workspace_root().join("bins");
    let mut main_files = Vec::new();
    if let Ok(entries) = fs::read_dir(&bins_dir) {
        let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        paths.sort();
        for bin_pkg in paths {
            let candidate = bin_pkg.join("src/main.rs");
            if candidate.is_file() {
                main_files.push(candidate);
            }
        }
    }

    assert!(
        !main_files.is_empty(),
        "Expected container entrypoints under bins/*/src/main.rs"
    );

    let mut oversized = Vec::new();
    for main_rs in main_files {
        let content = fs::read_to_string(&main_rs).unwrap();
        let line_count = content.lines().count();
        if line_count >= 100 {
            oversized.push((main_rs, line_count));
        }
    }

    assert!(
        oversized.is_empty(),
        "Binary entrypoints bins/*/src/main.rs must stay < 100 lines: {oversized:#?}"
    );
}

#[test]
fn forbids_allow_attributes_in_src_trees() {
    let root = workspace_root();
    let mut src_files = Vec::new();
    for top in ["crates", "bins"] {
        let mut all_rs = Vec::new();
        collect_rs_files(&root.join(top), &mut all_rs);
        for path in all_rs {
            if path.components().any(|comp| comp.as_os_str() == "src") {
                src_files.push(path);
            }
        }
    }

    let outer_allow = concat!("#[", "allow(");
    let inner_allow = concat!("#![", "allow(");
    let mut violations = Vec::new();

    for file in src_files {
        let content = fs::read_to_string(&file).unwrap();
        for (idx, raw_line) in content.lines().enumerate() {
            let code_line = strip_line_comment(raw_line);
            if code_line.contains(outer_allow) || code_line.contains(inner_allow) {
                violations.push(format!("{}:{}", file.display(), idx + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Lint-silencing allow attributes are forbidden in crates/**/src/** and bins/**/src/** (clippy.toml already permits unwrap/expect/panic inside tests): {violations:#?}"
    );
}

#[test]
fn forbids_direct_env_var_calls_across_workspace() {
    let root = workspace_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files);
    collect_rs_files(&root.join("bins"), &mut files);

    let banned_calls = [
        concat!("std::env::", "var("),
        concat!("std::env::", "var_os("),
        concat!("env::", "var("),
        concat!("env::", "var_os("),
    ];
    let mut violations = Vec::new();

    for file in files {
        let content = fs::read_to_string(&file).unwrap();
        for (idx, raw_line) in content.lines().enumerate() {
            let code_line = strip_line_comment(raw_line);
            for call in banned_calls {
                if code_line.contains(call) {
                    violations.push(format!("{}:{} -> `{call}`", file.display(), idx + 1));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Direct process environment lookups are forbidden workspace-wide; read `std::env::vars()` solely inside `crates/framework/src/config.rs`: {violations:#?}"
    );
}
