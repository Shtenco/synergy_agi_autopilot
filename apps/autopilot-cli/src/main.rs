use std::sync::Arc;
use async_trait::async_trait;
use clap::{Parser, Subcommand};
use autopilot_audit::{AuditStore, InMemoryAuditStore};
use autopilot_core::{AgentExecutor, AgentOutcome, Director, DirectorConfig, GoalRequest, PlannedTask, Planner, VerificationOutcome, Verifier};
use autopilot_domain::{AgentRole, Capability, Evidence, RiskClass};
use autopilot_policy::PolicyEngine;

#[derive(Parser)]
#[command(name = "open-autopilot", version, about = "OPEN AUTOPILOT core harness")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Demo {
        #[arg(long)]
        goal: String,
    },
}

struct DemoPlanner;
#[async_trait]
impl Planner for DemoPlanner {
    async fn plan(&self, request: &GoalRequest) -> Result<Vec<PlannedTask>, String> {
        Ok(vec![PlannedTask {
            role: AgentRole::Coder,
            objective: request.objective.clone(),
            capability: Capability::WriteWorkspace,
            risk: RiskClass::Medium,
        }])
    }
}

struct DemoExecutor;
#[async_trait]
impl AgentExecutor for DemoExecutor {
    async fn execute(&self, task: &PlannedTask) -> Result<AgentOutcome, String> {
        let role = match &task.role {
            AgentRole::Coder => "Coder",
            _ => "Specialist",
        };
        Ok(AgentOutcome {
            objective: task.objective.clone(),
            success: true,
            summary: format!("{role} agent completed demo execution"),
        })
    }
}

struct DemoVerifier;
#[async_trait]
impl Verifier for DemoVerifier {
    async fn verify(&self, _request: &GoalRequest, outcomes: &[AgentOutcome]) -> Result<VerificationOutcome, String> {
        let passed = !outcomes.is_empty() && outcomes.iter().all(|outcome| outcome.success);
        let evidence = vec![if passed {
            Evidence::passed("demo-verification", "all demo agent outcomes succeeded")
        } else {
            Evidence::failed("demo-verification", "one or more demo outcomes failed")
        }];
        Ok(VerificationOutcome { passed, evidence })
    }
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Demo { goal } => {
            let audit = Arc::new(InMemoryAuditStore::default());
            let director = Director::new(
                DirectorConfig::default(),
                PolicyEngine::default(),
                audit.clone(),
                Arc::new(DemoPlanner),
                Arc::new(DemoExecutor),
                Arc::new(DemoVerifier),
            );
            let request = GoalRequest::new(goal).require(Capability::WriteWorkspace, RiskClass::Medium);
            let summary = director.run_goal(request).await.map_err(|error| error.to_string())?;
            let events = audit.list_by_correlation(summary.correlation_id).await?;
            println!("OPEN AUTOPILOT demo");
            println!("final_state={:?}", summary.final_state);
            println!("repairs={}", summary.repairs);
            println!("evidence={}", summary.evidence.len());
            println!("audit_events={}", events.len());
        }
    }
    Ok(())
}
