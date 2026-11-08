//! `cargo xtask <command>`: build tasks written in Rust (no npm, no shell scripts).
//!
//! Commands:
//!   bump patch|minor|major   bump the workspace version, refresh Cargo.lock, roll the changelog
//!   release [--no-push]      pre-push checks, then `git push` (CI tags and publishes)
//!   icons                    render assets/icon.svg to PNG 16–1024, icon.ico and icon.icns
//!   installer-images         render installer/wizard-large.bmp and wizard-small.bmp
//!   samples                  regenerate the test sample files (crates/ffr-core/tests/samples)
//!   dist                     build this OS's release file into dist/ with its release name
//!   check                    run fmt, clippy, tests and cross-target checks

mod images;
mod samples;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

type Result<T> = std::result::Result<T, String>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    let rest = &args[1.min(args.len())..];
    let r = match cmd {
        "bump" => bump(rest.first().map(String::as_str).unwrap_or("")),
        "release" => release(rest.iter().any(|a| a == "--no-push")),
        "icons" => images::icons(&root()),
        "installer-images" => images::installer_images(&root()),
        "samples" => samples::generate(&root().join("crates/ffr-core/tests/samples")),
        "dist" => dist(),
        "check" => check(),
        _ => {
            eprintln!(
                "usage: cargo xtask <bump patch|minor|major | release [--no-push] | icons | installer-images | samples | dist | check>"
            );
            return ExitCode::from(2);
        }
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(cmd: &mut Command) -> Result<()> {
    eprintln!("> {cmd:?}");
    let st = cmd
        .current_dir(root())
        .status()
        .map_err(|e| format!("couldn't run {cmd:?}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("{cmd:?} failed ({st})"))
    }
}

fn output(cmd: &mut Command) -> Result<String> {
    let o = cmd
        .current_dir(root())
        .output()
        .map_err(|e| format!("couldn't run {cmd:?}: {e}"))?;
    if !o.status.success() {
        return Err(format!(
            "{cmd:?} failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

// ---------------------------------------------------------------- versions

fn read(p: &Path) -> Result<String> {
    std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn write(p: &Path, s: &str) -> Result<()> {
    std::fs::write(p, s).map_err(|e| format!("{}: {e}", p.display()))
}

/// Line index and value of `version = "…"` inside `[workspace.package]`.
fn workspace_version(toml: &str) -> Result<(usize, String)> {
    let mut in_section = false;
    for (i, line) in toml.lines().enumerate() {
        let t = line.trim();
        if t.starts_with('[') {
            in_section = t == "[workspace.package]";
            continue;
        }
        if in_section && t.starts_with("version") {
            if let Some(v) = t.split('"').nth(1) {
                return Ok((i, v.to_string()));
            }
        }
    }
    Err("no version in [workspace.package]".into())
}

pub fn next_version(v: &str, part: &str) -> Result<String> {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let n: Vec<u64> = core
        .split('.')
        .map(|x| x.parse().map_err(|_| format!("bad version {v}")))
        .collect::<Result<_>>()?;
    let [ma, mi, pa] = n[..] else {
        return Err(format!("bad version {v}"));
    };
    Ok(match (part, pre) {
        ("patch", Some(_)) => format!("{ma}.{mi}.{pa}"),
        ("patch", None) => format!("{ma}.{mi}.{}", pa + 1),
        ("minor", _) => format!("{ma}.{}.0", mi + 1),
        ("major", _) => format!("{}.0.0", ma + 1),
        _ => return Err("use: cargo xtask bump patch|minor|major".into()),
    })
}

fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Turn `## [Unreleased]` into `## [X.Y.Z] - date` with a fresh empty Unreleased above.
pub fn roll_changelog(text: &str, version: &str, date: &str) -> Result<String> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim() == "## [Unreleased]")
        .ok_or("CHANGELOG.md has no '## [Unreleased]' section")?;
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.starts_with("## ["))
        .map(|p| start + 1 + p)
        .unwrap_or(lines.len());
    if !lines[start + 1..end]
        .iter()
        .any(|l| l.trim_start().starts_with("- "))
    {
        return Err("the [Unreleased] section is empty; add changelog bullets first".into());
    }
    let mut out: Vec<String> = lines[..=start].iter().map(|s| s.to_string()).collect();
    out.push(String::new());
    out.push(format!("## [{version}] - {date}"));
    out.extend(lines[start + 1..].iter().map(|s| s.to_string()));
    let mut s = out.join("\n");
    s.push('\n');
    Ok(s)
}

fn bump(part: &str) -> Result<()> {
    let root = root();
    let cargo = root.join("Cargo.toml");
    let text = read(&cargo)?;
    let (line, old) = workspace_version(&text)?;
    let new = next_version(&old, part)?;
    let changelog = root.join("CHANGELOG.md");
    let rolled = roll_changelog(&read(&changelog)?, &new, &today())?;
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines[line] = lines[line].replace(&format!("\"{old}\""), &format!("\"{new}\""));
    let mut t = lines.join("\n");
    t.push('\n');
    write(&cargo, &t)?;
    write(&changelog, &rolled)?;
    run(Command::new("cargo").args(["update", "-w"]))?;
    println!("{old} -> {new}");
    Ok(())
}

fn lock_versions(lock: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut name = None;
    for l in lock.lines() {
        if let Some(n) = l.strip_prefix("name = ") {
            name = Some(n.trim_matches('"').to_string());
        } else if let Some(v) = l.strip_prefix("version = ") {
            if let Some(n) = name.take() {
                out.push((n, v.trim_matches('"').to_string()));
            }
        }
    }
    out
}

fn changelog_section(text: &str, version: &str) -> Option<String> {
    let head = format!("## [{version}]");
    let mut lines = text.lines().skip_while(|l| !l.starts_with(&head));
    lines.next()?;
    let body: Vec<&str> = lines.take_while(|l| !l.starts_with("## [")).collect();
    let s = body.join("\n").trim().to_string();
    (!s.is_empty()).then_some(s)
}

fn release(no_push: bool) -> Result<()> {
    let root = root();
    let (_, version) = workspace_version(&read(&root.join("Cargo.toml"))?)?;
    let lock = read(&root.join("Cargo.lock"))?;
    for (n, v) in lock_versions(&lock) {
        if ["ffr-core", "ffr-app", "xtask"].contains(&n.as_str()) && v != version {
            return Err(format!(
                "Cargo.lock has {n} {v}, expected {version}; run cargo update -w"
            ));
        }
    }
    let notes = changelog_section(&read(&root.join("CHANGELOG.md"))?, &version)
        .ok_or_else(|| format!("CHANGELOG.md has no non-empty section for {version}"))?;
    let status = output(Command::new("git").args(["status", "--porcelain"]))?;
    if !status.trim().is_empty() {
        return Err("the working tree is not clean; commit first".into());
    }
    let tag = format!("v{version}");
    let remote = output(Command::new("git").args([
        "ls-remote",
        "--tags",
        "origin",
        &format!("refs/tags/{tag}"),
    ]))?;
    if !remote.trim().is_empty() {
        return Err(format!(
            "tag {tag} already exists on origin; bump the version"
        ));
    }
    println!("ok: {tag} ready ({} lines of notes)", notes.lines().count());
    if no_push {
        return Ok(());
    }
    run(Command::new("git").arg("push"))
}

// ---------------------------------------------------------------- dist / check

fn dist() -> Result<()> {
    let root = root();
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    run(Command::new("cargo").args(["build", "--release", "--locked", "-p", "ffr-app"]))?;
    let (_, version) = workspace_version(&read(&root.join("Cargo.toml"))?)?;
    if cfg!(windows) {
        let exe = root.join("target/release/fast-file-recovery.exe");
        let iscc = find_iscc().ok_or("Inno Setup (iscc) not found; install Inno Setup 6")?;
        run(Command::new(iscc)
            .arg(format!("/DAppVersion={version}"))
            .arg(format!("/DSourceExe={}", exe.display()))
            .arg(format!("/O{}", dist.display()))
            .arg(root.join("installer/ffr.iss")))?;
        println!("dist/FastFileRecovery-setup.exe");
    } else {
        let name = if cfg!(target_os = "macos") {
            "fast-file-recovery-macos-arm64"
        } else {
            "fast-file-recovery-linux-x64"
        };
        std::fs::copy(
            root.join("target/release/fast-file-recovery"),
            dist.join(name),
        )
        .map_err(|e| e.to_string())?;
        println!("dist/{name}");
    }
    Ok(())
}

pub fn find_iscc() -> Option<PathBuf> {
    if Command::new("iscc").arg("/?").output().is_ok() {
        return Some(PathBuf::from("iscc"));
    }
    let mut c = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            let base = PathBuf::from(base);
            c.push(base.join("Inno Setup 6/ISCC.exe"));
            c.push(base.join("Programs/Inno Setup 6/ISCC.exe"));
        }
    }
    c.into_iter().find(|p| p.exists())
}

fn check() -> Result<()> {
    run(Command::new("cargo").args(["fmt", "--all", "--check"]))?;
    run(Command::new("cargo").args([
        "clippy",
        "--workspace",
        "--all-targets",
        "--locked",
        "--",
        "-D",
        "warnings",
    ]))?;
    run(Command::new("cargo").args(["test", "--workspace", "--locked"]))?;
    for t in ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin"] {
        if let Err(e) = run(Command::new("cargo").args(["check", "-p", "ffr-core", "--target", t]))
        {
            eprintln!("warning: cross check for {t} failed: {e}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(next_version("0.1.0", "patch").unwrap(), "0.1.1");
        assert_eq!(next_version("0.1.9", "minor").unwrap(), "0.2.0");
        assert_eq!(next_version("1.2.3", "major").unwrap(), "2.0.0");
        assert_eq!(next_version("0.2.0-beta.1", "patch").unwrap(), "0.2.0");
        assert!(next_version("0.1.0", "huge").is_err());
    }

    #[test]
    fn changelog_roll() {
        let t = "# Changelog\n\n## [Unreleased]\n\n### Added\n- Thing\n\n## [0.1.0] - 2025-01-01\n- Old\n";
        let r = roll_changelog(t, "0.2.0", "2025-02-02").unwrap();
        assert!(r.contains("## [Unreleased]\n\n## [0.2.0] - 2025-02-02\n\n### Added\n- Thing"));
        assert_eq!(
            changelog_section(&r, "0.2.0").unwrap(),
            "### Added\n- Thing"
        );
        assert!(roll_changelog("## [Unreleased]\n\n## [0.1.0]\n", "1", "d").is_err());
    }
}
