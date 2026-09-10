//! Runs the bundled Java helpers (Claritas, PackXZExtract) and the Forge installer.
//!
//! Both jars are embedded in the binary and extracted to `<root>/work/java` on
//! demand, so Nebula does not depend on its working directory.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use log::Level;
use serde::Deserialize;

use crate::error::{Error, Result, io};
use crate::layout::Root;
use crate::version::MinecraftVersion;

const CLARITAS_JAR: &[u8] = include_bytes!("../../../libraries/java/Claritas.jar");
const PACK_XZ_EXTRACT_JAR: &[u8] = include_bytes!("../../../libraries/java/PackXZExtract.jar");

const POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JarKind {
    Claritas,
    PackXzExtract,
}

impl JarKind {
    fn file_name(self) -> &'static str {
        match self {
            JarKind::Claritas => "Claritas.jar",
            JarKind::PackXzExtract => "PackXZExtract.jar",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            JarKind::Claritas => CLARITAS_JAR,
            JarKind::PackXzExtract => PACK_XZ_EXTRACT_JAR,
        }
    }
}

/// Metadata Claritas reports for one jar. Every field is optional: which ones
/// are present depends on the mod loader and the mod type.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ClaritasMeta {
    pub id: Option<String>,
    pub group: Option<String>,
    pub version: Option<String>,
    pub name: Option<String>,
    #[serde(rename = "modType")]
    pub mod_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Java {
    exe: PathBuf,
    work_dir: PathBuf,
}

impl Java {
    pub fn new(exe: impl Into<PathBuf>, root: &Root) -> Java {
        Java {
            exe: exe.into(),
            work_dir: root.work_java_dir(),
        }
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    fn jar(&self, kind: JarKind) -> Result<PathBuf> {
        let path = self.work_dir.join(kind.file_name());
        let bytes = kind.bytes();
        let extracted = match fs::metadata(&path) {
            Ok(meta) => meta.len() == bytes.len() as u64,
            Err(_) => false,
        };
        if !extracted {
            fs::create_dir_all(&self.work_dir)
                .map_err(io(format!("create {}", self.work_dir.display())))?;
            fs::write(&path, bytes).map_err(io(format!("write {}", path.display())))?;
        }
        Ok(path)
    }

    pub fn run(
        &self,
        vm_opts: &[String],
        jar: &Path,
        args: &[String],
        cwd: Option<&Path>,
        timeout: Option<Duration>,
    ) -> Result<ExitStatus> {
        self.exec(vm_opts, jar, args, cwd, timeout, ("java", Level::Warn))
    }

    pub fn run_installer(
        &self,
        installer_jar: &Path,
        args: &[String],
        cwd: &Path,
        timeout: Duration,
    ) -> Result<ExitStatus> {
        self.exec(
            &[],
            installer_jar,
            args,
            Some(cwd),
            Some(timeout),
            ("forge-installer", Level::Error),
        )
    }

    fn exec(
        &self,
        vm_opts: &[String],
        jar: &Path,
        args: &[String],
        cwd: Option<&Path>,
        timeout: Option<Duration>,
        log_to: (&'static str, Level),
    ) -> Result<ExitStatus> {
        let (target, stderr_level) = log_to;
        let mut command = Command::new(&self.exe);
        command
            .args(vm_opts)
            .arg("-jar")
            .arg(jar)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }

        let mut child = command.spawn().map_err(io(format!(
            "failed to start java executable {}",
            self.exe.display()
        )))?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let out_reader = thread::spawn(move || pump(stdout, target, Level::Info));
        let err_reader = thread::spawn(move || pump(stderr, target, stderr_level));

        let status = wait_for(&mut child, timeout, jar, target);
        let _ = out_reader.join();
        let _ = err_reader.join();
        status
    }

    /// Runs Claritas over `jars` in a scratch directory that is always removed
    /// afterwards. A non-zero exit or a missing output file is non-fatal
    /// (upstream behaviour) and yields `Ok(None)`.
    pub fn run_claritas(
        &self,
        scratch: &Path,
        mc: &MinecraftVersion,
        jars: &[PathBuf],
    ) -> Result<Option<HashMap<PathBuf, ClaritasMeta>>> {
        if jars.is_empty() {
            return Ok(Some(HashMap::new()));
        }
        let result = self.claritas(scratch, mc, jars);
        let _ = fs::remove_dir_all(scratch);
        result
    }

    fn claritas(
        &self,
        scratch: &Path,
        mc: &MinecraftVersion,
        jars: &[PathBuf],
    ) -> Result<Option<HashMap<PathBuf, ClaritasMeta>>> {
        fs::create_dir_all(scratch).map_err(io(format!("create {}", scratch.display())))?;
        let arg_file = scratch.join("claritasArgFile.txt");
        let output_file = scratch.join("claritasOutput.json");
        fs::write(&arg_file, claritas_arg_file(mc, jars, &output_file))
            .map_err(io(format!("write {}", arg_file.display())))?;

        let jar = self.jar(JarKind::Claritas)?;
        let vm_opts = [format!("-Dclaritas.argFile={}", arg_file.display())];
        let status = self.run(&vm_opts, &jar, &[], None, None)?;
        if !status.success() {
            log::error!(target: "java", "Claritas finished with non-zero exit code, {:?}", status.code());
            return Ok(None);
        }
        if !output_file.exists() {
            log::error!(target: "java", "Claritas output file not found when exit code is 0, is this a bug?");
            return Ok(None);
        }

        let raw = fs::read(&output_file).map_err(io(format!("read {}", output_file.display())))?;
        let parsed: HashMap<String, Option<ClaritasMeta>> = serde_json::from_slice(&raw)?;
        Ok(Some(
            parsed
                .into_iter()
                .filter_map(|(jar_path, meta)| meta.map(|meta| (PathBuf::from(jar_path), meta)))
                .collect(),
        ))
    }

    pub fn packxz_extract_unpack(&self, files: &[PathBuf]) -> Result<()> {
        let jar = self.jar(JarKind::PackXzExtract)?;
        let args = ["-packxz".to_string(), join_paths(files)];
        let status = self.run(&[], &jar, &args, None, None)?;
        if !status.success() {
            return Err(Error::Tool {
                tool: jar_name(&jar),
                code: status.code(),
            });
        }
        Ok(())
    }
}

fn wait_for(
    child: &mut Child,
    timeout: Option<Duration>,
    jar: &Path,
    target: &'static str,
) -> Result<ExitStatus> {
    let context = format!("waiting for {}", jar.display());
    let Some(limit) = timeout else {
        return child.wait().map_err(io(context));
    };
    let start = Instant::now();
    loop {
        match child.try_wait().map_err(io(context.clone()))? {
            Some(status) => return Ok(status),
            None if start.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                let tool = jar_name(jar);
                let seconds = limit.as_secs();
                log::error!(target: target, "{tool} did not finish within {seconds}s and was killed");
                return Err(Error::Tool {
                    tool: format!("{tool} (timed out after {seconds}s, killed)"),
                    code: None,
                });
            }
            None => thread::sleep(POLL_INTERVAL),
        }
    }
}

fn pump<R: Read>(reader: R, target: &'static str, level: Level) {
    for line in BufReader::new(reader).lines() {
        let Ok(line) = line else { return };
        let line = line.trim();
        if !line.is_empty() {
            log::log!(target: target, level, "{line}");
        }
    }
}

fn claritas_arg_file(mc: &MinecraftVersion, jars: &[PathBuf], output_file: &Path) -> String {
    format!(
        "--absoluteJarPaths\n{}\n--libraryType\nFORGE\n--mcVersion\n{mc}\n--outputFile\n{}\n--previewOutput\ntrue",
        join_paths(jars),
        output_file.display()
    )
}

fn join_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(",")
}

fn jar_name(jar: &Path) -> String {
    jar.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| jar.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn java_in(dir: &Path) -> Java {
        Java::new(
            "java",
            &Root::new(dir, Url::parse("https://host/dist/").unwrap()),
        )
    }

    #[test]
    fn extracts_embedded_jars_once() {
        let tmp = tempfile::tempdir().unwrap();
        let java = java_in(tmp.path());

        let jar = java.jar(JarKind::Claritas).unwrap();
        assert_eq!(
            jar,
            tmp.path().join("work").join("java").join("Claritas.jar")
        );
        assert_eq!(fs::read(&jar).unwrap(), CLARITAS_JAR);

        let placeholder = vec![0u8; CLARITAS_JAR.len()];
        fs::write(&jar, &placeholder).unwrap();
        assert_eq!(java.jar(JarKind::Claritas).unwrap(), jar);
        assert_eq!(fs::read(&jar).unwrap(), placeholder);

        fs::write(&jar, b"truncated").unwrap();
        assert_eq!(java.jar(JarKind::Claritas).unwrap(), jar);
        assert_eq!(fs::read(&jar).unwrap(), CLARITAS_JAR);
    }

    #[test]
    fn extracts_pack_xz_extract() {
        let tmp = tempfile::tempdir().unwrap();
        let jar = java_in(tmp.path()).jar(JarKind::PackXzExtract).unwrap();
        assert!(jar.ends_with("PackXZExtract.jar"));
        assert_eq!(
            fs::metadata(&jar).unwrap().len(),
            PACK_XZ_EXTRACT_JAR.len() as u64
        );
    }

    #[test]
    fn writes_the_upstream_claritas_arg_file() {
        let jars = [PathBuf::from("/mods/a.jar"), PathBuf::from("/mods/b.jar")];
        let content = claritas_arg_file(
            &MinecraftVersion::new(1, 12, Some(2)),
            &jars,
            Path::new("/work/claritasOutput.json"),
        );
        assert_eq!(
            content,
            "--absoluteJarPaths\n/mods/a.jar,/mods/b.jar\n--libraryType\nFORGE\n--mcVersion\n1.12.2\n--outputFile\n/work/claritasOutput.json\n--previewOutput\ntrue"
        );
    }

    #[test]
    fn missing_java_executable_is_an_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let java = Java::new(
            tmp.path().join("nope").join("java"),
            &Root::new(tmp.path(), Url::parse("https://host/").unwrap()),
        );
        let err = java
            .run(&[], &tmp.path().join("any.jar"), &[], None, None)
            .unwrap_err();
        assert!(matches!(err, Error::Io { .. }), "{err}");
        assert!(err.to_string().contains("java"), "{err}");
    }

    #[test]
    fn claritas_without_jars_skips_java() {
        let tmp = tempfile::tempdir().unwrap();
        let scratch = tmp.path().join("claritasWork");
        let result = java_in(tmp.path())
            .run_claritas(&scratch, &MinecraftVersion::new(1, 20, Some(1)), &[])
            .unwrap();
        assert!(result.unwrap().is_empty());
        assert!(!scratch.exists());
    }

    #[test]
    #[ignore = "requires a JDK; set JAVA_EXECUTABLE"]
    fn runs_java() {
        let Ok(exe) = std::env::var("JAVA_EXECUTABLE") else {
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        let java = Java::new(
            exe,
            &Root::new(tmp.path(), Url::parse("https://host/").unwrap()),
        );
        let jar = java.jar(JarKind::PackXzExtract).unwrap();
        let status = java
            .run(
                &["-version".to_string()],
                &jar,
                &[],
                None,
                Some(Duration::from_secs(60)),
            )
            .unwrap();
        assert!(status.success());
    }
}
