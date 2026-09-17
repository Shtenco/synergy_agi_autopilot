use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use autopilot_runtime::{CommandResult, CommandSpec, RuntimeAdapter, RuntimeError};
use autopilot_verification::{VerificationCheck, VerificationPipeline};

struct SequenceRuntime { results: Mutex<VecDeque<CommandResult>> }
#[async_trait]
impl RuntimeAdapter for SequenceRuntime {
    async fn probe(&self) -> bool { true }
    async fn execute(&self, _spec: CommandSpec) -> Result<CommandResult, RuntimeError> { Ok(self.results.lock().unwrap().pop_front().unwrap()) }
    async fn cancel(&self) -> Result<(), RuntimeError> { Ok(()) }
}
fn result(code: i32) -> CommandResult { CommandResult { exit_code: code, stdout: String::new(), stderr: String::new(), duration_ms: 1 } }

#[tokio::test]
async fn reports_pass_when_all_checks_pass() {
    let runtime = Arc::new(SequenceRuntime { results: Mutex::new(vec![result(0), result(0)].into()) });
    let pipeline = VerificationPipeline::new(runtime, false, vec![VerificationCheck::new("test", CommandSpec::new("test")), VerificationCheck::new("lint", CommandSpec::new("lint"))]);
    let report = pipeline.run().await.unwrap();
    assert!(report.passed);
    assert_eq!(report.evidence.len(), 2);
}

#[tokio::test]
async fn fail_fast_stops_after_first_failure() {
    let runtime = Arc::new(SequenceRuntime { results: Mutex::new(vec![result(1), result(0)].into()) });
    let pipeline = VerificationPipeline::new(runtime, true, vec![VerificationCheck::new("test", CommandSpec::new("test")), VerificationCheck::new("lint", CommandSpec::new("lint"))]);
    let report = pipeline.run().await.unwrap();
    assert!(!report.passed);
    assert_eq!(report.evidence.len(), 1);
}

#[tokio::test]
async fn collect_all_records_multiple_failures() {
    let runtime = Arc::new(SequenceRuntime { results: Mutex::new(vec![result(1), result(2)].into()) });
    let pipeline = VerificationPipeline::new(runtime, false, vec![VerificationCheck::new("a", CommandSpec::new("a")), VerificationCheck::new("b", CommandSpec::new("b"))]);
    let report = pipeline.run().await.unwrap();
    assert!(!report.passed);
    assert_eq!(report.evidence.iter().filter(|e| !e.passed).count(), 2);
}
