use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

const UNIT_HEADER: &str = "id\ttrack\tstate\tbranch\tpr\tsha\tbrief";
const LEDGER_HEADER: &str = "pr\tsha\tverdict\tevidence\tverifier\tts";
const MAX_SAFE: u64 = 9_007_199_254_740_991;
pub type Result<T> = std::result::Result<T, Error>;
pub type Counts = BTreeMap<String, usize>;

#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub code: i32,
    pub output: Option<Value>,
    pub usage: bool,
}
impl Error {
    pub fn user(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code: 1,
            output: None,
            usage: false,
        }
    }
    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            usage: true,
            ..Self::user(message)
        }
    }
    fn missing(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            ..Self::user(message)
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::user(error.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::user(error.to_string())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Unit {
    pub id: String,
    pub track: String,
    pub state: String,
    pub branch: String,
    pub pr: String,
    pub sha: String,
    pub brief: String,
}
impl Unit {
    pub fn cells(&self) -> Vec<String> {
        vec![
            self.id.clone(),
            self.track.clone(),
            self.state.clone(),
            self.branch.clone(),
            self.pr.clone(),
            self.sha.clone(),
            self.brief.clone(),
        ]
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LedgerEntry {
    pub pr: String,
    pub sha: String,
    pub verdict: String,
    pub evidence: String,
    pub verifier: String,
    pub ts: String,
}
impl LedgerEntry {
    fn cells(&self) -> Vec<String> {
        vec![
            self.pr.clone(),
            self.sha.clone(),
            self.verdict.clone(),
            self.evidence.clone(),
            self.verifier.clone(),
            self.ts.clone(),
        ]
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pointer {
    pub ts: String,
    pub agent: String,
    pub unit: String,
    pub status: String,
    pub report: String,
}
impl Pointer {
    pub fn cells(&self) -> Vec<String> {
        vec![
            self.ts.clone(),
            self.agent.clone(),
            self.unit.clone(),
            self.status.clone(),
            self.report.clone(),
        ]
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Gate {
    pub kind: String,
    pub id: String,
    pub question: String,
    pub options: String,
    pub default_answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FrontierPr {
    pub pr: u64,
    pub branches: String,
    pub sha: String,
    pub state: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Frontier {
    pub generation: u64,
    pub prs: Vec<FrontierPr>,
    pub lowest_unmerged: Option<i64>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StandingLine {
    pub number: usize,
    pub line: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub unit_states: Counts,
    pub ledger_verdicts: Counts,
    pub frontier_generation: u64,
    pub open_gate_ids: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub units: Vec<Unit>,
    pub ledger: Vec<LedgerEntry>,
    pub frontier: Frontier,
    pub gates: Vec<Gate>,
    pub summary: Summary,
    pub changed: String,
}

pub fn parse_verdict(value: &str) -> Result<String> {
    match value {
        "live-ui-verified" | "unit-test-verified" | "type-check-only" | "verifier-blocked" | "verifier-failed" => Ok(value.into()),
        _ => Err(Error::user("verdict must be live-ui-verified, unit-test-verified, type-check-only, verifier-blocked, or verifier-failed")),
    }
}
pub fn parse_forge(value: &str) -> Result<&str> {
    if matches!(value, "gh" | "origin") {
        Ok(value)
    } else {
        Err(Error::user("forge must be gh or origin"))
    }
}
pub fn clean_cell(value: &str) -> String {
    let value = value.replace(['\t', '\n', '\r'], " ");
    if value.starts_with(['=', '+', '-', '@']) {
        format!("'{value}")
    } else {
        value
    }
}
fn required_cell(value: &str, label: &str) -> Result<String> {
    let value = clean_cell(value);
    if value.trim().is_empty() {
        Err(Error::user(format!("{label} must not be empty")))
    } else {
        Ok(value)
    }
}
fn required_line(value: &str, label: &str) -> Result<String> {
    let value = value.replace(['\n', '\r'], " ").trim().to_string();
    if value.is_empty() {
        Err(Error::user(format!("{label} must not be empty")))
    } else {
        Ok(value)
    }
}
fn optional_cell(value: Option<&str>, label: &str) -> Result<String> {
    value
        .map(|v| required_cell(v, label))
        .unwrap_or_else(|| Ok(String::new()))
}
fn positive_integer(value: u64) -> Result<u64> {
    if value == 0 || value > MAX_SAFE {
        Err(Error::user("PR must be a positive integer"))
    } else {
        Ok(value)
    }
}
fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn absolute(path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    Ok(result)
}
fn atomic_write(path: &Path, contents: &str) -> Result<()> {
    let temporary = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id(),
        Uuid::new_v4()
    ));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(contents.as_bytes())?;
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    match fs::remove_file(&temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    result
}
fn required_file(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(uninitialized(path.parent().unwrap_or(path)))
        }
        Err(error) => Err(error.into()),
    }
}
fn uninitialized(path: &Path) -> Error {
    Error::user(format!(
        "store is not initialized at {}; run orch init",
        path.display()
    ))
}
fn read_tsv(path: &Path, header: &str, width: usize) -> Result<Vec<Vec<String>>> {
    let raw = required_file(path)?.replace('\r', "");
    let mut lines = raw.split('\n');
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    if lines.next() != Some(header) {
        return Err(Error::user(format!("{name} has an invalid header")));
    }
    lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let cells: Vec<String> = line.split('\t').map(str::to_string).collect();
            if cells.len() != width {
                Err(Error::user(format!("{name} has a malformed row")))
            } else {
                Ok(cells)
            }
        })
        .collect()
}
fn write_tsv(path: &Path, header: &str, rows: impl Iterator<Item = Vec<String>>) -> Result<()> {
    let mut contents = format!("{header}\n");
    for row in rows {
        contents.push_str(
            &row.iter()
                .map(|v| clean_cell(v))
                .collect::<Vec<_>>()
                .join("\t"),
        );
        contents.push('\n');
    }
    atomic_write(path, &contents)
}
fn count_values<'a>(values: impl Iterator<Item = &'a str>) -> Counts {
    let mut counts = Counts::new();
    for value in values {
        *counts.entry(value.into()).or_default() += 1;
    }
    counts
}
pub fn count_line(counts: &Counts) -> String {
    if counts.is_empty() {
        "none".into()
    } else {
        counts
            .iter()
            .map(|(name, count)| format!("{name}={count}"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
fn holder_is_dead(holder: &str) -> bool {
    let Ok(pid) = holder.parse::<i32>() else {
        return false;
    };
    if pid <= 0 || pid.to_string() != holder {
        return false;
    }
    #[cfg(unix)]
    {
        (unsafe { libc::kill(pid, 0) }) == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

pub struct Store {
    pub directory: PathBuf,
    force: bool,
    locked: bool,
    closed: bool,
    pub warnings: Vec<String>,
}
impl Store {
    pub fn open(directory: impl AsRef<Path>, force: bool) -> Result<Self> {
        Ok(Self {
            directory: absolute(directory.as_ref())?,
            force,
            locked: false,
            closed: false,
            warnings: Vec::new(),
        })
    }
    fn ensure_open(&self) -> Result<()> {
        if self.closed {
            Err(Error::user("store is closed"))
        } else {
            Ok(())
        }
    }
    fn path(&self, file: &str) -> PathBuf {
        self.directory.join(file)
    }
    fn lock(&mut self) -> Result<()> {
        self.ensure_open()?;
        if self.locked {
            return Ok(());
        }
        let path = self.path(".orch.lock");
        let create = || -> std::io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            writeln!(file, "{}", std::process::id())
        };
        match create() {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = fs::read_to_string(&path)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "unknown".into());
                if holder_is_dead(&holder) {
                    self.warnings.push(format!(
                        "replacing stale store lock (pid {holder} is dead)\n"
                    ));
                } else if self.force {
                    self.warnings
                        .push(format!("stealing store lock held by pid {holder}\n"));
                } else {
                    return Err(Error::user(format!("store lock held by pid {holder}")));
                }
                fs::remove_file(&path)?;
                if let Err(error) = create() {
                    if error.kind() == std::io::ErrorKind::AlreadyExists {
                        let holder = fs::read_to_string(&path)?.trim().to_string();
                        return Err(Error::user(format!(
                            "store lock held by pid {}",
                            if holder.is_empty() {
                                "unknown"
                            } else {
                                &holder
                            }
                        )));
                    }
                    return Err(error.into());
                }
            }
            Err(error) => return Err(error.into()),
        }
        self.locked = true;
        Ok(())
    }
    fn begin_write(&mut self) -> Result<()> {
        self.ensure_open()?;
        if !self.directory.try_exists()? {
            return Err(uninitialized(&self.directory));
        }
        self.lock()
    }
    pub fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        if self.locked {
            self.locked = false;
            let path = self.path(".orch.lock");
            match fs::read_to_string(&path) {
                Ok(holder) if holder.trim() == std::process::id().to_string() => {
                    fs::remove_file(path)?
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
    pub fn init(&mut self) -> Result<Value> {
        self.ensure_open()?;
        fs::create_dir_all(&self.directory)?;
        self.lock()?;
        for (name, contents) in [
            ("units.tsv", format!("{UNIT_HEADER}\n")),
            ("ledger.tsv", format!("{LEDGER_HEADER}\n")),
            ("gates.md", String::new()),
            ("preferences.md", String::new()),
            ("frontier.json", "{}\n".into()),
        ] {
            if name == "gates.md" {
                fs::create_dir_all(self.path("inbox"))?;
            }
            let path = self.path(name);
            if !path.try_exists()? {
                atomic_write(&path, &contents)?;
            }
        }
        Ok(json!({"store": self.directory.to_string_lossy()}))
    }
    pub fn units(&self) -> Result<Vec<Unit>> {
        self.ensure_open()?;
        Ok(read_tsv(&self.path("units.tsv"), UNIT_HEADER, 7)?
            .into_iter()
            .map(|row| {
                let mut row = row.into_iter();
                Unit {
                    id: row.next().unwrap(),
                    track: row.next().unwrap(),
                    state: row.next().unwrap(),
                    branch: row.next().unwrap(),
                    pr: row.next().unwrap(),
                    sha: row.next().unwrap(),
                    brief: row.next().unwrap(),
                }
            })
            .collect())
    }
    pub fn unit_add(&mut self, id: &str, track: &str, brief: Option<&str>) -> Result<Unit> {
        self.begin_write()?;
        let unit = Unit {
            id: required_cell(id, "unit id")?,
            track: required_cell(track, "track")?,
            state: "pending".into(),
            branch: String::new(),
            pr: String::new(),
            sha: String::new(),
            brief: optional_cell(brief, "brief")?,
        };
        let mut units = self.units()?;
        if units.iter().any(|u| u.id == unit.id) {
            return Err(Error::user(format!("unit {} already exists", unit.id)));
        }
        units.push(unit.clone());
        write_tsv(
            &self.path("units.tsv"),
            UNIT_HEADER,
            units.iter().map(Unit::cells),
        )?;
        Ok(unit)
    }
    pub fn unit_set(
        &mut self,
        id: &str,
        state: &str,
        branch: Option<&str>,
        pr: Option<u64>,
        sha: Option<&str>,
    ) -> Result<Unit> {
        self.begin_write()?;
        let id = required_cell(id, "unit id")?;
        let state = required_cell(state, "state")?;
        let mut units = self.units()?;
        let unit = units
            .iter_mut()
            .find(|u| u.id == id)
            .ok_or_else(|| Error::missing(format!("unit {id} not found")))?;
        unit.state = state;
        if let Some(value) = branch {
            unit.branch = required_cell(value, "branch")?;
        }
        if let Some(value) = pr {
            unit.pr = positive_integer(value)?.to_string();
        }
        if let Some(value) = sha {
            unit.sha = required_cell(value, "SHA")?;
        }
        let result = unit.clone();
        write_tsv(
            &self.path("units.tsv"),
            UNIT_HEADER,
            units.iter().map(Unit::cells),
        )?;
        Ok(result)
    }
    pub fn unit_get(&self, id: &str) -> Result<Unit> {
        self.ensure_open()?;
        let id = required_cell(id, "unit id")?;
        self.units()?
            .into_iter()
            .find(|u| u.id == id)
            .ok_or_else(|| Error::missing(format!("unit {id} not found")))
    }
    pub fn unit_list(&self, state: Option<&str>, track: Option<&str>) -> Result<Vec<Unit>> {
        self.ensure_open()?;
        let state = state.map(|v| required_cell(v, "state")).transpose()?;
        let track = track.map(|v| required_cell(v, "track")).transpose()?;
        Ok(self
            .units()?
            .into_iter()
            .filter(|u| {
                state.as_ref().is_none_or(|s| *s == u.state)
                    && track.as_ref().is_none_or(|t| *t == u.track)
            })
            .collect())
    }
    pub fn unit_counts(&self) -> Result<Counts> {
        Ok(count_values(self.units()?.iter().map(|u| u.state.as_str())))
    }
    pub fn ledger(&self) -> Result<Vec<LedgerEntry>> {
        self.ensure_open()?;
        read_tsv(&self.path("ledger.tsv"), LEDGER_HEADER, 6)?
            .into_iter()
            .map(|row| {
                if parse_verdict(&row[2]).is_err() {
                    return Err(Error::user(format!(
                        "ledger.tsv has invalid verdict {}",
                        row[2]
                    )));
                }
                let mut row = row.into_iter();
                Ok(LedgerEntry {
                    pr: row.next().unwrap(),
                    sha: row.next().unwrap(),
                    verdict: row.next().unwrap(),
                    evidence: row.next().unwrap(),
                    verifier: row.next().unwrap(),
                    ts: row.next().unwrap(),
                })
            })
            .collect()
    }
    pub fn ledger_record(
        &mut self,
        pr: u64,
        sha: &str,
        verdict: &str,
        evidence: &str,
        verifier: Option<&str>,
    ) -> Result<LedgerEntry> {
        self.begin_write()?;
        let verdict = parse_verdict(verdict)?;
        let entry = LedgerEntry {
            pr: positive_integer(pr)?.to_string(),
            sha: required_cell(sha, "SHA")?,
            verdict,
            evidence: required_cell(evidence, "evidence")?,
            verifier: optional_cell(verifier, "verifier")?,
            ts: now(),
        };
        let mut rows = self.ledger()?;
        if let Some(old) = rows
            .iter_mut()
            .find(|old| old.pr == entry.pr && old.sha == entry.sha)
        {
            *old = entry.clone();
        } else {
            rows.push(entry.clone());
        }
        write_tsv(
            &self.path("ledger.tsv"),
            LEDGER_HEADER,
            rows.iter().map(LedgerEntry::cells),
        )?;
        Ok(entry)
    }
    pub fn ledger_check(&self, pr: u64, sha: &str) -> Result<LedgerEntry> {
        self.ensure_open()?;
        let pr = positive_integer(pr)?.to_string();
        let sha = required_cell(sha, "SHA")?;
        self.ledger()?
            .into_iter()
            .find(|row| row.pr == pr && row.sha == sha)
            .ok_or_else(|| Error {
                output: Some(json!({"pr": pr, "sha": sha, "verdict": "NOT-VERIFIED"})),
                ..Error::missing("NOT-VERIFIED")
            })
    }
    pub fn ledger_summary(&self) -> Result<Counts> {
        Ok(count_values(
            self.ledger()?.iter().map(|v| v.verdict.as_str()),
        ))
    }
    pub fn inbox_peek(&self) -> Result<Vec<Pointer>> {
        self.ensure_open()?;
        let directory = self.path("inbox");
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(uninitialized(&self.directory))
            }
            Err(error) => return Err(error.into()),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_file() && entry.file_name().to_string_lossy().ends_with(".tsv")
            {
                files.push(entry.path());
            }
        }
        files.sort();
        files
            .into_iter()
            .map(|path| {
                let raw = fs::read_to_string(&path)?;
                let raw = raw
                    .strip_suffix("\r\n")
                    .or_else(|| raw.strip_suffix('\n'))
                    .unwrap_or(&raw);
                let cells: Vec<_> = raw.split('\t').collect();
                if raw.contains(['\r', '\n']) || cells.len() != 5 {
                    return Err(Error::user(format!(
                        "inbox pointer {} is malformed",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )));
                }
                Ok(Pointer {
                    ts: cells[0].into(),
                    agent: cells[1].into(),
                    unit: cells[2].into(),
                    status: cells[3].into(),
                    report: cells[4].into(),
                })
            })
            .collect()
    }
    pub fn inbox_push(
        &mut self,
        agent: &str,
        unit: &str,
        status: &str,
        report: Option<&str>,
    ) -> Result<(Pointer, String)> {
        self.begin_write()?;
        let pointer = Pointer {
            ts: now(),
            agent: required_cell(agent, "agent")?,
            unit: required_cell(unit, "unit")?,
            status: required_cell(status, "status")?,
            report: optional_cell(report, "report")?,
        };
        let directory = self.path("inbox");
        if !directory.try_exists()? {
            return Err(uninitialized(&self.directory));
        }
        let filename = format!(
            "{}-{}-{}.tsv",
            pointer.ts.replace([':', '.'], "-"),
            std::process::id(),
            Uuid::new_v4()
        );
        atomic_write(
            &directory.join(&filename),
            &format!(
                "{}\n",
                pointer
                    .cells()
                    .iter()
                    .map(|v| clean_cell(v))
                    .collect::<Vec<_>>()
                    .join("\t")
            ),
        )?;
        Ok((pointer, filename))
    }
    pub fn inbox_drain(&mut self) -> Result<Vec<Pointer>> {
        self.begin_write()?;
        let rows = self.inbox_peek()?;
        let inbox = self.path("inbox");
        let drained = self.path(&format!(
            ".inbox-drain-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        fs::rename(&inbox, &drained)?;
        if let Err(error) = fs::create_dir(&inbox) {
            fs::rename(&drained, &inbox)?;
            return Err(error.into());
        }
        fs::remove_dir_all(drained)?;
        Ok(rows)
    }
    pub fn gates(&self) -> Result<Vec<Gate>> {
        self.ensure_open()?;
        parse_gates(&required_file(&self.path("gates.md"))?)
    }
    pub fn gate_park(
        &mut self,
        id: &str,
        question: &str,
        options: &str,
        default: &str,
    ) -> Result<Gate> {
        self.begin_write()?;
        let gate = Gate {
            kind: "open".into(),
            id: required_line(id, "gate id")?,
            question: required_line(question, "question")?,
            options: required_line(options, "options")?,
            default_answer: required_line(default, "default")?,
            answer: None,
        };
        let mut rows = self.gates()?;
        if let Some(old) = rows.iter_mut().find(|g| g.id == gate.id) {
            *old = gate.clone();
        } else {
            rows.push(gate.clone());
        }
        atomic_write(&self.path("gates.md"), &render_gates(&rows))?;
        Ok(gate)
    }
    pub fn gate_list(&self) -> Result<Vec<Gate>> {
        Ok(self
            .gates()?
            .into_iter()
            .filter(|g| g.kind == "open")
            .collect())
    }
    pub fn gate_resolve(&mut self, id: &str, answer: &str) -> Result<Gate> {
        self.begin_write()?;
        let id = required_line(id, "gate id")?;
        let mut rows = self.gates()?;
        let gate = rows
            .iter_mut()
            .find(|g| g.id == id)
            .ok_or_else(|| Error::missing(format!("gate {id} not found")))?;
        gate.kind = "resolved".into();
        gate.answer = Some(required_line(answer, "answer")?);
        let result = gate.clone();
        atomic_write(&self.path("gates.md"), &render_gates(&rows))?;
        Ok(result)
    }
    pub fn frontier_show(&self) -> Result<Frontier> {
        self.ensure_open()?;
        parse_frontier(&required_file(&self.path("frontier.json"))?)
    }
    pub fn frontier_set(
        &mut self,
        repo: &str,
        forge: &str,
        pin: Option<&[u64]>,
    ) -> Result<Frontier> {
        self.begin_write()?;
        let repo = absolute(Path::new(&required_line(repo, "repo directory")?))?;
        parse_forge(forge)?;
        if let Some(pin) = pin {
            for pr in pin {
                positive_integer(*pr)?;
            }
            if pin.iter().collect::<BTreeSet<_>>().len() != pin.len() {
                return Err(Error::user("--prs must not contain duplicates"));
            }
        }
        let old = self.frontier_show()?;
        let prs = resolve_frontier(&repo, forge)?;
        if let Some(pin) = pin {
            validate_pin(&prs.iter().map(|row| row.pr).collect::<Vec<_>>(), pin)?;
        }
        let value = Frontier {
            generation: old.generation + 1,
            lowest_unmerged: prs.iter().find(|r| r.state == "OPEN").map(|r| r.pr as i64),
            prs,
        };
        atomic_write(
            &self.path("frontier.json"),
            &format!("{}\n", serde_json::to_string_pretty(&value)?),
        )?;
        Ok(value)
    }
    pub fn standing_show(&self) -> Result<Vec<StandingLine>> {
        self.ensure_open()?;
        let raw = required_file(&self.path("preferences.md"))?.replace('\r', "");
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        for line in raw.split('\n').filter(|line| !line.is_empty()) {
            let Some((number, line)) = line.split_once(". ") else {
                return Err(Error::user("preferences.md has malformed numbering"));
            };
            if number.starts_with('0')
                || number.parse::<usize>().ok() != Some(result.len() + 1)
                || !number.bytes().all(|b| b.is_ascii_digit())
                || line.is_empty()
            {
                return Err(Error::user("preferences.md has malformed numbering"));
            }
            result.push(StandingLine {
                number: result.len() + 1,
                line: line.into(),
            });
        }
        Ok(result)
    }
    pub fn standing_add(&mut self, line: &str) -> Result<StandingLine> {
        self.begin_write()?;
        let mut rows = self.standing_show()?;
        let item = StandingLine {
            number: rows.len() + 1,
            line: required_line(line, "standing order")?,
        };
        rows.push(item.clone());
        atomic_write(
            &self.path("preferences.md"),
            &format!(
                "{}\n",
                rows.iter()
                    .map(|r| format!("{}. {}", r.number, r.line))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        )?;
        Ok(item)
    }
    pub fn status_render(&mut self) -> Result<StatusReport> {
        self.begin_write()?;
        let units = self.units()?;
        let ledger = self.ledger()?;
        let frontier = self.frontier_show()?;
        let gates = self.gates()?;
        let mut ids: Vec<_> = gates
            .iter()
            .filter(|g| g.kind == "open")
            .map(|g| g.id.clone())
            .collect();
        ids.sort();
        let summary = Summary {
            unit_states: count_values(units.iter().map(|u| u.state.as_str())),
            ledger_verdicts: count_values(ledger.iter().map(|r| r.verdict.as_str())),
            frontier_generation: frontier.generation,
            open_gate_ids: ids,
        };
        let path = self.path("status.md");
        let before = if path.try_exists()? {
            previous_summary(&fs::read_to_string(&path)?)
        } else {
            None
        };
        let changed = changed(before.as_ref(), &summary);
        let report = StatusReport {
            units,
            ledger,
            frontier,
            gates,
            summary,
            changed,
        };
        atomic_write(&path, &status_markdown(&report))?;
        Ok(report)
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn render_gates(rows: &[Gate]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let blocks = rows
        .iter()
        .map(|g| {
            let answer = g
                .answer
                .as_ref()
                .filter(|_| g.kind == "resolved")
                .map(|v| format!("\n- Answer: {v}"))
                .unwrap_or_default();
            format!(
                "## {}\n\n- Status: {}\n- Question: {}\n- Options: {}\n- Default: {}{}",
                g.id, g.kind, g.question, g.options, g.default_answer, answer
            )
        })
        .collect::<Vec<_>>();
    format!("# Gates\n\n{}\n", blocks.join("\n\n"))
}
fn parse_gates(raw: &str) -> Result<Vec<Gate>> {
    let raw = raw.replace('\r', "");
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let Some(raw) = raw.strip_prefix("# Gates\n\n## ") else {
        return Err(Error::user("gates.md has an invalid heading"));
    };
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    for block in raw.split("\n\n## ") {
        let mut lines = block.split('\n').filter(|v| !v.is_empty());
        let id = lines.next().unwrap_or("");
        let mut fields = BTreeMap::new();
        let malformed = || Error::user(format!("gates.md has a malformed gate {id}"));
        for line in lines {
            let Some((name, value)) = line.strip_prefix("- ").and_then(|v| v.split_once(": "))
            else {
                return Err(malformed());
            };
            if name.is_empty() || name.contains(':') {
                return Err(malformed());
            }
            fields.insert(name, value);
        }
        if id.is_empty()
            || !["Question", "Options", "Default"]
                .iter()
                .all(|k| fields.contains_key(k))
        {
            return Err(malformed());
        }
        let status = fields.get("Status").copied().unwrap_or("");
        if status != "open" && !(status == "resolved" && fields.contains_key("Answer")) {
            return Err(Error::user(format!("gates.md has invalid status {status}")));
        }
        result.push(Gate {
            kind: status.into(),
            id: id.into(),
            question: fields["Question"].into(),
            options: fields["Options"].into(),
            default_answer: fields["Default"].into(),
            answer: if status == "resolved" {
                Some(fields["Answer"].into())
            } else {
                None
            },
        });
        ids.insert(id);
    }
    if ids.len() != result.len() {
        return Err(Error::user("gates.md has duplicate gate ids"));
    }
    Ok(result)
}
fn safe_unsigned(value: &Value) -> Option<u64> {
    value.as_u64().filter(|v| *v <= MAX_SAFE).or_else(|| {
        value
            .as_f64()
            .filter(|v| v.is_finite() && v.fract() == 0.0 && *v >= 0.0 && *v <= MAX_SAFE as f64)
            .map(|v| v as u64)
    })
}
fn safe_signed(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .filter(|v| v.unsigned_abs() <= MAX_SAFE)
        .or_else(|| {
            value
                .as_f64()
                .filter(|v| v.is_finite() && v.fract() == 0.0 && v.abs() <= MAX_SAFE as f64)
                .map(|v| v as i64)
        })
}
fn parse_frontier(raw: &str) -> Result<Frontier> {
    let value: Value =
        serde_json::from_str(raw).map_err(|_| Error::user("frontier.json is not valid JSON"))?;
    let Some(object) = value.as_object() else {
        return Err(Error::user("frontier.json must contain an object"));
    };
    if object.is_empty() {
        return Ok(Frontier::default());
    }
    let invalid = || Error::user("frontier.json has an invalid shape");
    let generation = value
        .get("generation")
        .and_then(safe_unsigned)
        .ok_or_else(invalid)?;
    let rows = value
        .get("prs")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let lowest = value.get("lowestUnmerged").ok_or_else(invalid)?;
    let lowest_unmerged = if lowest.is_null() {
        None
    } else {
        Some(safe_signed(lowest).ok_or_else(invalid)?)
    };
    let mut prs = Vec::new();
    for row in rows {
        let invalid = || Error::user("frontier.json has an invalid PR row");
        let pr = row
            .get("pr")
            .and_then(safe_unsigned)
            .filter(|v| *v > 0)
            .ok_or_else(invalid)?;
        let branches = row
            .get("branches")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .ok_or_else(invalid)?;
        let sha = row.get("sha").and_then(Value::as_str).ok_or_else(invalid)?;
        let state = row
            .get("state")
            .and_then(Value::as_str)
            .filter(|v| matches!(*v, "OPEN" | "MERGED" | "CLOSED"))
            .ok_or_else(invalid)?;
        prs.push(FrontierPr {
            pr,
            branches: branches.into(),
            sha: sha.into(),
            state: state.into(),
        });
    }
    Ok(Frontier {
        generation,
        prs,
        lowest_unmerged,
    })
}
struct PreviousSummary {
    unit_states: Counts,
    ledger_verdicts: Counts,
    frontier_generation: f64,
    open_gate_ids: Vec<String>,
}
fn previous_summary(raw: &str) -> Option<PreviousSummary> {
    let raw = raw.split_once("<!-- orch-summary ")?.1;
    let raw = raw.split(['\r', '\n']).next()?.rsplit_once(" -->")?.0;
    let value: Value = serde_json::from_str(raw).ok()?;
    let counts = |name: &str| -> Option<Counts> {
        value
            .get(name)?
            .as_object()?
            .iter()
            .map(|(name, count)| Some((name.clone(), safe_unsigned(count)? as usize)))
            .collect()
    };
    Some(PreviousSummary {
        unit_states: counts("unitStates")?,
        ledger_verdicts: counts("ledgerVerdicts")?,
        frontier_generation: value.get("frontierGeneration")?.as_f64()?,
        open_gate_ids: value
            .get("openGateIds")?
            .as_array()?
            .iter()
            .map(|v| v.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()?,
    })
}
fn changed(before: Option<&PreviousSummary>, after: &Summary) -> String {
    let Some(before) = before else {
        return "first render".into();
    };
    let mut result = Vec::new();
    for (label, old, new) in [
        ("units", &before.unit_states, &after.unit_states),
        ("ledger", &before.ledger_verdicts, &after.ledger_verdicts),
    ] {
        for name in old.keys().chain(new.keys()).collect::<BTreeSet<_>>() {
            let old = old.get(name).copied().unwrap_or(0);
            let new = new.get(name).copied().unwrap_or(0);
            if old != new {
                result.push(format!("{label} {name} {old}->{new}"));
            }
        }
    }
    if before.frontier_generation != after.frontier_generation as f64 {
        result.push(format!(
            "frontier generation {}->{}",
            before.frontier_generation, after.frontier_generation
        ));
    }
    if before.open_gate_ids.join("\0") != after.open_gate_ids.join("\0") {
        result.push(format!(
            "open gates {}->{}",
            before.open_gate_ids.len(),
            after.open_gate_ids.len()
        ));
    }
    if result.is_empty() {
        "no derived changes".into()
    } else {
        result.join("; ")
    }
}
fn table(headers: &[&str], rows: impl Iterator<Item = Vec<String>>) -> String {
    let rows: Vec<_> = rows.collect();
    if rows.is_empty() {
        return "(none)".into();
    }
    let mut lines = vec![
        format!("| {} |", headers.join(" | ")),
        format!("| {} |", vec!["---"; headers.len()].join(" | ")),
    ];
    for row in rows {
        lines.push(format!(
            "| {} |",
            row.iter()
                .map(|v| v.replace('\\', "\\\\").replace('|', "\\|"))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    lines.join("\n")
}
fn status_markdown(report: &StatusReport) -> String {
    format!("# Orchestrate status\n\nGenerated: {}\n\n## Units\n\nStates: {}\n\n{}\n\n## Verification ledger\n\nVerdicts: {}\n\n{}\n\n## Frontier\n\nGeneration: {}\nLowest unmerged: {}\n\n{}\n\n## Gates\n\n{}\n\n<!-- orch-summary {} -->\n",
        now(), count_line(&report.summary.unit_states), table(&["ID", "Track", "State", "Branch", "PR", "SHA", "Brief"], report.units.iter().map(Unit::cells)),
        count_line(&report.summary.ledger_verdicts), table(&["PR", "SHA", "Verdict", "Evidence", "Verifier", "Timestamp"], report.ledger.iter().map(LedgerEntry::cells)),
        report.frontier.generation, report.frontier.lowest_unmerged.map(|v| v.to_string()).unwrap_or_else(|| "none".into()), table(&["Branch", "PR", "SHA", "State"], report.frontier.prs.iter().map(|r| vec![r.branches.clone(), r.pr.to_string(), r.sha.clone(), r.state.clone()])),
        table(&["ID", "Status", "Question", "Options", "Default", "Answer"], report.gates.iter().map(|g| vec![g.id.clone(), g.kind.clone(), g.question.clone(), g.options.clone(), g.default_answer.clone(), g.answer.clone().unwrap_or_default()])), serde_json::to_string(&report.summary).expect("summary is serializable"))
}

fn run_tool(command: &str, args: &[&str], repo: &Path) -> Result<String> {
    let output = Command::new(command)
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| Error::user(format!("{command} {} failed: {error}", args.join(" "))))?;
    if !output.status.success() {
        return Err(Error::user(format!(
            "{command} {} failed: {}{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
trait ForgeQuery {
    fn git(&self, args: &[&str]) -> Result<String>;
    fn list(&self, filter: &str, branch: &str) -> Result<Vec<PullRequest>>;
}
struct NativeForge<'a> {
    repo: &'a Path,
    forge: &'a str,
}
#[derive(Clone, Debug)]
struct PullRequest {
    pr: u64,
    state: String,
    branch: String,
    base: String,
}
fn parse_pull_requests(
    raw: &str,
    forge: &str,
    filter: &str,
    branch: &str,
) -> Result<Vec<PullRequest>> {
    let command = format!("{forge} pr list {filter} {branch}");
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| Error::user(format!("{command} output is not valid JSON")))?;
    let rows = value
        .as_array()
        .ok_or_else(|| Error::user(format!("{command} output is not a JSON array")))?;
    let (state_field, head_field, base_field) = if forge == "gh" {
        ("state", "headRefName", "baseRefName")
    } else {
        ("status", "headRef", "baseRef")
    };
    rows.iter()
        .map(|row| {
            let invalid = || Error::user(format!("{command} output has an invalid PR row"));
            let status = row
                .get(state_field)
                .and_then(Value::as_str)
                .ok_or_else(invalid)?;
            let state = match (forge, status) {
                ("gh", "OPEN") | ("origin", "open" | "draft") => "OPEN",
                ("gh", "MERGED") | ("origin", "merged") => "MERGED",
                ("gh", "CLOSED") | ("origin", "closed") => "CLOSED",
                _ => return Err(invalid()),
            };
            Ok(PullRequest {
                pr: row
                    .get("number")
                    .and_then(safe_unsigned)
                    .filter(|v| *v > 0)
                    .ok_or_else(invalid)?,
                state: state.into(),
                branch: row
                    .get(head_field)
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(invalid)?
                    .into(),
                base: row
                    .get(base_field)
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(invalid)?
                    .into(),
            })
        })
        .collect()
}
impl ForgeQuery for NativeForge<'_> {
    fn git(&self, args: &[&str]) -> Result<String> {
        Ok(run_tool("git", args, self.repo)?.trim().into())
    }
    fn list(&self, filter: &str, branch: &str) -> Result<Vec<PullRequest>> {
        let fields = if self.forge == "gh" {
            "number,state,headRefName,baseRefName"
        } else {
            "number,status,headRef,baseRef"
        };
        parse_pull_requests(
            &run_tool(
                self.forge,
                &[
                    "pr", "list", filter, branch, "--state", "all", "--json", fields,
                ],
                self.repo,
            )?,
            self.forge,
            filter,
            branch,
        )
    }
}
fn branch_pr(query: &impl ForgeQuery, forge: &str, branch: &str) -> Result<PullRequest> {
    let rows = query.list("--head", branch)?;
    let active: Vec<_> = rows.iter().filter(|r| r.state == "OPEN").cloned().collect();
    let rows = if active.is_empty() { rows } else { active };
    if rows.is_empty() {
        return Err(Error::user(format!("branch {branch} has no pull request; open one with {forge} pr create, or resolve the frontier from a branch whose chain is fully submitted")));
    }
    if rows.len() > 1 {
        return Err(Error::user(format!(
            "branch {branch} has multiple pull requests ({}); close the stale ones first",
            rows.iter()
                .map(|r| format!("#{}", r.pr))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    Ok(rows.into_iter().next().unwrap())
}
fn resolve_frontier(repo: &Path, forge: &str) -> Result<Vec<FrontierPr>> {
    resolve_with(&NativeForge { repo, forge }, forge)
}
fn resolve_with(query: &impl ForgeQuery, forge: &str) -> Result<Vec<FrontierPr>> {
    let trunk = query
        .git(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .map(|s| s.strip_prefix("origin/").unwrap_or(&s).to_string())
        .unwrap_or_else(|_| "main".into());
    let current = query.git(&["symbolic-ref", "--short", "HEAD"])?;
    if current == trunk {
        return Ok(Vec::new());
    }
    let mut seen = BTreeSet::from([current.clone()]);
    let mut bottom = branch_pr(query, forge, &current)?;
    let mut chain = vec![bottom.clone()];
    while bottom.base != trunk {
        if !seen.insert(bottom.base.clone()) {
            return Err(Error::user(format!(
                "pull request base chain loops at branch {}",
                bottom.base
            )));
        }
        bottom = branch_pr(query, forge, &bottom.base)?;
        chain.insert(0, bottom.clone());
    }
    let mut tip = current;
    loop {
        let children: Vec<_> = query
            .list("--base", &tip)?
            .into_iter()
            .filter(|r| r.state == "OPEN")
            .collect();
        if children.is_empty() {
            break;
        }
        if children.len() > 1 {
            return Err(Error::user(format!(
                "stack is not linear: branches {} all target {tip}",
                children
                    .iter()
                    .map(|r| r.branch.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
        let child = children.into_iter().next().unwrap();
        if !seen.insert(child.branch.clone()) {
            return Err(Error::user(format!(
                "pull request base chain loops at branch {}",
                child.branch
            )));
        }
        tip = child.branch.clone();
        chain.push(child);
    }
    if chain.iter().map(|r| r.pr).collect::<BTreeSet<_>>().len() != chain.len() {
        return Err(Error::user(
            "pull request base chain contains duplicate pull requests",
        ));
    }
    chain
        .into_iter()
        .map(|row| {
            let sha = query.git(&["rev-parse", &row.branch])?;
            if !(40..=64).contains(&sha.len()) || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Error::user(format!(
                    "git rev-parse {} returned an invalid SHA",
                    row.branch
                )));
            }
            Ok(FrontierPr {
                pr: row.pr,
                branches: row.branch,
                sha,
                state: row.state,
            })
        })
        .collect()
}
fn validate_pin(actual: &[u64], expected: &[u64]) -> Result<()> {
    if actual == expected {
        return Ok(());
    }
    let join = |values: Vec<u64>| {
        values
            .iter()
            .map(u64::to_string)
            .collect::<Vec<_>>()
            .join(",")
    };
    let missing: Vec<_> = expected
        .iter()
        .filter(|v| !actual.contains(v))
        .copied()
        .collect();
    let extra: Vec<_> = actual
        .iter()
        .filter(|v| !expected.contains(v))
        .copied()
        .collect();
    let mut drift = Vec::new();
    if !missing.is_empty() {
        drift.push(format!(
            "missing from resolved chain: {}",
            join(missing.clone())
        ));
    }
    if !extra.is_empty() {
        drift.push(format!("extra in resolved chain: {}", join(extra.clone())));
    }
    if missing.is_empty() && extra.is_empty() {
        drift.push(format!(
            "order differs: expected {}; resolved {}",
            join(expected.to_vec()),
            join(actual.to_vec())
        ));
    }
    Err(Error::user(format!(
        "frontier pin mismatch: {}",
        drift.join("; ")
    )))
}

#[cfg(test)]
#[path = "forge_tests.rs"]
mod forge_tests;
