use super::Source;
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::{env, fs};
use winreg::{enums::*, RegKey};

mod catalogue;

// Each row describes one independently identifiable client. Repeated IDs
// represent clients with one source identity (or a rebrand), not new readers.
struct Probe {
    agent: &'static str,
    label: &'static str,
    variant: &'static str,
    install: &'static [&'static str],
    paths: &'static [&'static str],
    commands: &'static [&'static str],
    extensions: &'static [&'static str],
}

#[derive(Default)]
struct Evidence {
    roots: BTreeMap<&'static str, PathBuf>,
    installs: HashSet<String>,
    extensions: HashSet<String>,
    commands: Vec<PathBuf>,
    jetbrains: HashSet<String>,
    packages: HashSet<String>,
}

fn rooted(path: PathBuf) -> Option<PathBuf> {
    match path.components().next() {
        Some(Component::Prefix(prefix))
            if path.is_absolute()
                && matches!(
                    prefix.kind(),
                    std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)
                ) => Some(path),
        _ => None,
    }
}

impl Evidence {
    fn gather() -> Self {
        let mut result = Self {
            installs: installed_names(),
            ..Self::default()
        };
        for (name, value) in [
            ("home", dirs::home_dir()),
            ("roaming", dirs::config_dir()),
            ("local", dirs::data_local_dir()),
            ("programs", env::var_os("ProgramFiles").map(PathBuf::from)),
            ("programs-x86", env::var_os("ProgramFiles(x86)").map(PathBuf::from)),
            ("npm", env::var_os("APPDATA").map(|path| PathBuf::from(path).join("npm"))),
        ] {
            if let Some(path) = value.and_then(rooted) {
                result.roots.insert(name, path);
            }
        }
        let mut paths = HashSet::new();
        if let Some(value) = env::var_os("PATH") {
            for path in env::split_paths(&value).filter_map(rooted) {
                if paths.insert(path.to_string_lossy().to_lowercase()) {
                    result.commands.push(path);
                }
            }
        }
        for (root, tail) in [
            ("home", ".local/bin"),
            ("home", ".cargo/bin"),
            ("home", "scoop/shims"),
            ("local", "Microsoft/WindowsApps"),
            ("npm", ""),
        ] {
            if let Some(base) = result.roots.get(root) {
                let path = base.join(tail);
                if paths.insert(path.to_string_lossy().to_lowercase()) {
                    result.commands.push(path);
                }
            }
        }

        if let Some(home) = result.roots.get("home") {
            for folder in [
                ".vscode", ".vscode-insiders", ".vscode-oss", ".cursor", ".windsurf",
                ".windsurf-next", ".devin", ".antigravity", ".kiro", ".trae", ".trae-cn",
                ".qoder", ".qoder-cn", ".codebuddy", ".codebuddy-cn",
            ] {
                directory_names(&home.join(folder).join("extensions"), &mut result.extensions);
            }
        }
        if let Some(roaming) = result.roots.get("roaming") {
            let mut versions = HashSet::new();
            let jetbrains = roaming.join("JetBrains");
            directory_names(&jetbrains, &mut versions);
            for product_version in versions {
                directory_names(&jetbrains.join(product_version).join("plugins"), &mut result.jetbrains);
            }
        }
        if let Some(local) = result.roots.get("local") {
            directory_names(&local.join("Packages"), &mut result.packages);
        }
        result
    }

    fn known_path(&self, spec: &str) -> Option<PathBuf> {
        let (root, tail) = spec.split_once(':')?;
        let path = self.roots.get(root)?.join(tail);
        if path.is_dir() || path.is_file() {
            Some(path)
        } else {
            None
        }
    }

    fn has_extension(&self, id: &str) -> bool {
        if id == "windsurf" {
            return self.roots.get("home").is_some_and(|home| {
                [".windsurf", ".windsurf-next"].iter().any(|folder| home.join(folder).is_dir())
            });
        }
        if let Some(folder) = id.strip_prefix("jetbrains:") {
            return self.jetbrains.contains(folder);
        }
        if let Some(package) = id.strip_prefix("store:") {
            return self.packages.contains(package);
        }
        self.extensions.iter().any(|entry| {
            entry == id || entry.strip_prefix(id)
                .and_then(|tail| tail.strip_prefix('-'))
                .is_some_and(|version| version.starts_with(|ch: char| ch.is_ascii_digit()))
        })
    }

    fn has_command(&self, command: &str) -> bool {
        self.commands.iter().any(|dir| {
            ["exe", "cmd", "bat", "ps1"].iter().any(|ext| {
                dir.join(command).with_extension(ext).is_file()
            })
        })
    }
}

fn directory_names(path: &Path, names: &mut HashSet<String>) {
    if let Ok(entries) = fs::read_dir(path) {
        names.extend(entries.flatten().filter_map(|entry| {
            entry.file_type().ok()?.is_dir()
                .then(|| entry.file_name().to_string_lossy().to_lowercase())
        }));
    }
}

fn normalized_name(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn installed_names() -> HashSet<String> {
    const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
    let mut names = HashSet::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let hive = RegKey::predef(hive);
        for view in [KEY_WOW64_32KEY, KEY_WOW64_64KEY] {
            let Ok(uninstall) = hive.open_subkey_with_flags(UNINSTALL, KEY_READ | view) else {
                continue;
            };
            for name in uninstall.enum_keys().flatten() {
                let Ok(entry) = uninstall.open_subkey_with_flags(name, KEY_READ | view) else {
                    continue;
                };
                if let Ok(display) = entry.get_value::<String, _>("DisplayName") {
                    names.insert(normalized_name(&display));
                }
            }
        }
    }
    names
}

fn installed(installs: &HashSet<String>, alias: &str) -> bool {
    installs.contains(alias) || installs.contains(&format!("{alias} (user)"))
        || installs.contains(&format!("{alias} (system)"))
        || (alias == "windsurf" && installs.iter().any(|name| name.starts_with("windsurf")))
        || installs.iter().any(|display| {
        display.strip_prefix(alias)
            .and_then(|tail| tail.strip_prefix(" version ").or_else(|| tail.strip_prefix(' ')))
            .is_some_and(|tail| {
                tail.starts_with(|ch: char| ch.is_ascii_digit())
                    && tail.contains('.')
                    && tail.chars().all(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-'))
            })
    })
}

fn scan_catalogue(evidence: &Evidence, catalogue: &[Probe]) -> Vec<Source> {
    let mut index = BTreeMap::new();
    let mut results: Vec<Source> = Vec::new();
    for probe in catalogue {
        let position = *index.entry(probe.agent).or_insert_with(|| {
            let position = results.len();
            results.push(Source {
                agent: probe.agent,
                label: probe.label,
                detected: false,
                dirs: Vec::new(),
                variants: Vec::new(),
            });
            position
        });
        let row = &mut results[position];
        let mut paths: Vec<_> = probe.paths.iter().filter_map(|path| evidence.known_path(path)).collect();
        let found = !paths.is_empty()
            || probe.install.iter().any(|alias| installed(&evidence.installs, alias))
            || probe.extensions.iter().any(|id| evidence.has_extension(id))
            || probe.commands.iter().any(|name| evidence.has_command(name));
        if found {
            row.detected = true;
            if !row.variants.contains(&probe.variant) {
                row.variants.push(probe.variant);
            }
            row.dirs.append(&mut paths);
        }
    }
    for row in &mut results {
        row.dirs.sort();
        row.dirs.dedup();
    }
    results
}

pub fn scan() -> Vec<Source> {
    scan_catalogue(&Evidence::gather(), catalogue::PROBES)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_variants_without_treating_hints_as_usage_or_other_variants() {
        let evidence = Evidence {
            installs: ["qoder cn", "qoder cn version 0.4.0", "windsurf (user)", "devin"]
                .into_iter().map(str::to_owned).collect(),
            extensions: ["saoudrizwan.claude-dev-3.2.0", "github.copilot-chat-0.5.1",
                "google.gemini-cli-vscode-0.5.1", "kilocode.kilo-code-4.1.0"]
                .into_iter().map(str::to_owned).collect(),
            ..Evidence::default()
        };
        let rows = scan_catalogue(&evidence, catalogue::PROBES);
        let get = |id| rows.iter().find(|row| row.agent == id).unwrap();
        assert!(get("qoder-cn").detected);
        assert!(get("qoder-cn").dirs.is_empty());
        assert!(!get("qoder-ide").detected);
        assert!(!get("copilot").detected);
        assert!(get("copilot-ide").detected);
        assert!(!get("cline-cli").detected);
        assert!(get("cline").detected);
        assert!(get("gemini").detected);
        assert!(!get("gemini-code-assist").detected);
        assert!(!get("kilo").detected);
        assert!(get("kilo-ide").detected);
        assert_eq!(get("devin").variants, ["Desktop", "Windsurf"]);
        assert_eq!(rows.iter().filter(|row| row.agent == "devin").count(), 1);
    }

    #[test]
    fn matches_version_suffixes_without_matching_other_products() {
        let installs = ["qoder cn", "cursor version 1.2.33", "codebuddy 1.2.3"]
            .into_iter().map(str::to_owned).collect();
        assert!(!installed(&installs, "qoder"));
        assert!(installed(&installs, "cursor"));
        assert!(installed(&installs, "codebuddy"));
    }
}
