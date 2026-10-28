use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub owner: String,
    pub repo: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
    pub owner: String,
    pub repo: String,
    pub number: u64,
}
impl Context {
    pub fn repository(&self) -> Repository {
        Repository {
            owner: self.owner.clone(),
            repo: self.repo.clone(),
        }
    }
    pub fn with_number(&self, number: u64) -> Self {
        Self {
            number,
            ..self.clone()
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub context: Context,
    pub mergeable: String,
    pub merge_state_status: String,
    pub review_decision: Option<String>,
    pub head_ref_oid: Option<String>,
    pub head_ref_name: String,
    pub base_ref_name: String,
    pub state: String,
    pub merged_at: Option<String>,
    pub is_draft: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPr {
    pub number: u64,
    pub is_cross_repository: bool,
    pub head_ref_name: String,
    pub base_ref_name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub author_login: Option<String>,
    pub body: String,
    pub path: Option<String>,
    pub line: Option<i64>,
    pub created_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub first_comment: Option<Comment>,
    pub is_bugbot: bool,
    pub bugbot_review_passes: usize,
}
#[derive(Clone, Debug)]
pub struct RawThread {
    pub id: String,
    pub first_comment: Option<Comment>,
    pub resolved: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckKind {
    Passed,
    Skipped,
    Failed,
    Pending,
    CodeReviewGate,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub reported_state: String,
    pub description: String,
    pub link: String,
    pub workflow: String,
    pub kind: CheckKind,
}
#[derive(Clone, Debug)]
pub enum FastChecks {
    Checks(Vec<Check>),
    Unusable { exit_code: i32, stderr: String },
}
#[derive(Clone, Debug)]
pub struct RollupPage {
    pub checks: Vec<Check>,
    pub end_cursor: Option<String>,
}
#[derive(Clone, Debug)]
pub struct ThreadPage {
    pub threads: Vec<RawThread>,
    pub end_cursor: Option<String>,
}
#[derive(Clone, Debug)]
pub struct CommitRollup {
    pub oid: String,
    pub state: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: String,
    pub retryable: bool,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
}
impl Failure {
    pub fn new(kind: &str, detail: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            retryable: true,
            detail: detail.into(),
            raw_value: None,
            code: None,
        }
    }
}
pub type QueryResult<T> = Result<T, Failure>;
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ci {
    pub kind: String,
    pub source: String,
    pub all: Vec<Check>,
    pub had_previous_passing_ci: bool,
    pub failed: Vec<Check>,
    pub pending: Vec<Check>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github: Option<Value>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Snapshot {
    Merged {
        context: Context,
        facts: Facts,
    },
    Closed {
        context: Context,
        facts: Facts,
    },
    Open {
        context: Context,
        facts: Facts,
        threads: Vec<Thread>,
        ci: Ci,
        #[serde(rename = "reviewAutomationRunning")]
        review_automation_running: bool,
    },
}
impl Snapshot {
    pub fn context(&self) -> &Context {
        match self {
            Self::Merged { context, .. }
            | Self::Closed { context, .. }
            | Self::Open { context, .. } => context,
        }
    }
    pub fn facts(&self) -> &Facts {
        match self {
            Self::Merged { facts, .. } | Self::Closed { facts, .. } | Self::Open { facts, .. } => {
                facts
            }
        }
    }
    pub fn merged(&self) -> bool {
        matches!(self, Self::Merged { .. })
    }
}
pub trait Reader {
    fn origin_repo(&mut self) -> QueryResult<Option<Repository>>;
    fn current_pr(&mut self, pr: Option<u64>) -> QueryResult<Context>;
    fn pull_request(&mut self, context: &Context) -> QueryResult<Facts>;
    fn open_pull_requests(&mut self, repository: &Repository) -> QueryResult<Vec<OpenPr>>;
    fn checks_fast_path(&mut self, context: &Context) -> QueryResult<FastChecks>;
    fn check_rollup_page(
        &mut self,
        context: &Context,
        after: Option<&str>,
    ) -> QueryResult<RollupPage>;
    fn review_thread_page(
        &mut self,
        context: &Context,
        after: Option<&str>,
    ) -> QueryResult<ThreadPage>;
    fn commit_rollups(&mut self, context: &Context) -> QueryResult<Vec<CommitRollup>>;
}
#[derive(Clone, Debug)]
pub struct PollingOptions {
    pub interval: f64,
    pub sweep_interval: f64,
    pub timeout: f64,
    pub max_query_errors: u64,
    pub allow_draft: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Single,
    Stack,
    QueuedStack,
}
impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Stack => "stack",
            Self::QueuedStack => "queued-stack",
        }
    }
}
pub trait Clock {
    fn now(&self) -> f64;
    fn observed_at(&self) -> String;
    fn sleep(&mut self, seconds: f64);
}
