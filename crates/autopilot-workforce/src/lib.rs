use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use autopilot_domain::RiskClass;
use autopilot_org::Organization;
use autopilot_plugin_runtime::{PluginCall, PluginExecutor};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkforcePolicy {
    pub lease_seconds: i64,
    pub offline_after_seconds: i64,
    pub max_attempts: u32,
    pub escalation_after_seconds: i64,
    pub min_skill_level: f64,
    pub department_cycle_max_items: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkforceConfig {
    pub schema: String,
    pub defaults: WorkforcePolicy,
    #[serde(default)]
    pub department_overrides: BTreeMap<String, PartialWorkforcePolicy>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PartialWorkforcePolicy {
    pub lease_seconds: Option<i64>,
    pub offline_after_seconds: Option<i64>,
    pub max_attempts: Option<u32>,
    pub escalation_after_seconds: Option<i64>,
    pub min_skill_level: Option<f64>,
    pub department_cycle_max_items: Option<usize>,
}

impl WorkforceConfig {
    pub fn policy_for(&self, department_id: &str) -> WorkforcePolicy {
        let mut p = self.defaults.clone();
        if let Some(o) = self.department_overrides.get(department_id) {
            if let Some(v) = o.lease_seconds { p.lease_seconds=v; }
            if let Some(v) = o.offline_after_seconds { p.offline_after_seconds=v; }
            if let Some(v) = o.max_attempts { p.max_attempts=v; }
            if let Some(v) = o.escalation_after_seconds { p.escalation_after_seconds=v; }
            if let Some(v) = o.min_skill_level { p.min_skill_level=v; }
            if let Some(v) = o.department_cycle_max_items { p.department_cycle_max_items=v; }
        }
        p
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmployeeRuntimeState {
    pub employee_id: String,
    pub department_id: String,
    pub status: String,
    pub current_work_item: Option<Uuid>,
    pub node_id: Option<String>,
    pub memory_namespace: String,
    pub workspace: String,
    pub model_profile: String,
    pub budget_id: String,
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SkillState {
    pub employee_id: String,
    pub skill: String,
    pub level: f64,
    pub evidence_count: u64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KpiState {
    pub employee_id: String,
    pub key: String,
    pub target: f64,
    pub current: f64,
    pub weight: f64,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkforceNode {
    pub node_id: String,
    pub kind: String,
    pub endpoint: String,
    pub capabilities: BTreeSet<String>,
    pub cpu_millis: u64,
    pub memory_mib: u64,
    pub online: bool,
    pub last_heartbeat: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: Uuid,
    pub department_id: String,
    pub assigned_employee_id: Option<String>,
    pub created_by: String,
    pub parent_id: Option<Uuid>,
    pub goal: String,
    pub plugin_id: String,
    pub capability: String,
    pub input: Value,
    pub risk: RiskClass,
    pub priority: i64,
    pub status: String,
    pub attempts: u32,
    pub max_attempts: u32,
    pub lease_owner: Option<String>,
    pub lease_until: Option<DateTime<Utc>>,
    pub required_skills: BTreeMap<String, f64>,
    pub required_tools: BTreeSet<String>,
    pub budget_id: Option<String>,
    pub budget_microusd: i64,
    pub approval_id: Option<String>,
    pub escalation_level: u32,
    pub next_escalation_at: Option<DateTime<Utc>>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkItem {
    pub fn new(
        department_id: impl Into<String>,
        created_by: impl Into<String>,
        goal: impl Into<String>,
        plugin_id: impl Into<String>,
        capability: impl Into<String>,
        input: Value,
        risk: RiskClass,
    ) -> Self {
        let now=Utc::now();
        Self {
            id:Uuid::new_v4(), department_id:department_id.into(), assigned_employee_id:None,
            created_by:created_by.into(), parent_id:None, goal:goal.into(),
            plugin_id:plugin_id.into(), capability:capability.into(), input, risk,
            priority:0, status:"pending".into(), attempts:0, max_attempts:3,
            lease_owner:None, lease_until:None, required_skills:BTreeMap::new(),
            required_tools:BTreeSet::new(), budget_id:None, budget_microusd:0,
            approval_id:None, escalation_level:0, next_escalation_at:None,
            result:None, error:None, created_at:now, updated_at:now,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Initiative {
    pub id: Uuid,
    pub owner_employee_id: String,
    pub department_id: String,
    pub title: String,
    pub objective: String,
    pub status: String,
    pub score: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerformanceRecord {
    pub id: Uuid,
    pub employee_id: String,
    pub work_item_id: Uuid,
    pub success: bool,
    pub verified: bool,
    pub latency_ms: u64,
    pub quality_score: f64,
    pub cost_microusd: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Error)]
pub enum WorkforceError {
    #[error(transparent)] Sqlite(#[from] rusqlite::Error),
    #[error(transparent)] Json(#[from] serde_json::Error),
    #[error("workforce store lock poisoned")] Poisoned,
    #[error("unknown employee: {0}")] UnknownEmployee(String),
    #[error("unknown work item: {0}")] UnknownWorkItem(Uuid),
    #[error("unknown node: {0}")] UnknownNode(String),
    #[error("employee {employee} is not eligible for capability {capability}")]
    Ineligible { employee:String, capability:String },
    #[error("employee {employee} skill {skill} below required level {required}")]
    SkillGap { employee:String, skill:String, required:f64 },
    #[error("node {node} is offline")] NodeOffline { node:String },
    #[error("work item cannot be leased: {0}")] Lease(String),
    #[error("execution failed: {0}")] Execution(String),
}

pub type Result<T> = std::result::Result<T, WorkforceError>;

#[derive(Clone)]
pub struct WorkforceStore { conn:Arc<Mutex<Connection>> }

impl WorkforceStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn=Connection::open(path)?;
        conn.pragma_update(None,"journal_mode","WAL")?;
        conn.pragma_update(None,"foreign_keys","ON")?;
        let store=Self{conn:Arc::new(Mutex::new(conn))};
        store.migrate()?;
        Ok(store)
    }
    pub fn open_in_memory() -> Result<Self> {
        let conn=Connection::open_in_memory()?;
        conn.pragma_update(None,"foreign_keys","ON")?;
        let store=Self{conn:Arc::new(Mutex::new(conn))};
        store.migrate()?;
        Ok(store)
    }
    fn conn(&self)->Result<MutexGuard<'_,Connection>> {
        self.conn.lock().map_err(|_|WorkforceError::Poisoned)
    }
    fn migrate(&self)->Result<()> {
        self.conn()?.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS employee_runtime(
          employee_id TEXT PRIMARY KEY, department_id TEXT NOT NULL, status TEXT NOT NULL,
          current_work_item TEXT, node_id TEXT, memory_namespace TEXT NOT NULL,
          workspace TEXT NOT NULL, model_profile TEXT NOT NULL, budget_id TEXT NOT NULL,
          last_heartbeat TEXT, updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS employee_skills(
          employee_id TEXT NOT NULL, skill TEXT NOT NULL, level REAL NOT NULL,
          evidence_count INTEGER NOT NULL DEFAULT 0, updated_at TEXT NOT NULL,
          PRIMARY KEY(employee_id,skill)
        );
        CREATE TABLE IF NOT EXISTS employee_kpis(
          employee_id TEXT NOT NULL, key TEXT NOT NULL, target REAL NOT NULL,
          current REAL NOT NULL, weight REAL NOT NULL, updated_at TEXT NOT NULL,
          PRIMARY KEY(employee_id,key)
        );
        CREATE TABLE IF NOT EXISTS workforce_nodes(
          node_id TEXT PRIMARY KEY, kind TEXT NOT NULL, endpoint TEXT NOT NULL,
          capabilities_json TEXT NOT NULL, cpu_millis INTEGER NOT NULL,
          memory_mib INTEGER NOT NULL, online INTEGER NOT NULL, last_heartbeat TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS employee_node_bindings(
          employee_id TEXT PRIMARY KEY, node_id TEXT NOT NULL, bound_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS workforce_work_items(
          id TEXT PRIMARY KEY, department_id TEXT NOT NULL, assigned_employee_id TEXT,
          created_by TEXT NOT NULL, parent_id TEXT, goal TEXT NOT NULL, plugin_id TEXT NOT NULL,
          capability TEXT NOT NULL, input_json TEXT NOT NULL, risk TEXT NOT NULL,
          priority INTEGER NOT NULL, status TEXT NOT NULL, attempts INTEGER NOT NULL,
          max_attempts INTEGER NOT NULL, lease_owner TEXT, lease_until TEXT,
          required_skills_json TEXT NOT NULL, required_tools_json TEXT NOT NULL,
          budget_id TEXT, budget_microusd INTEGER NOT NULL, approval_id TEXT,
          escalation_level INTEGER NOT NULL, next_escalation_at TEXT,
          result_json TEXT, error TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_workforce_inbox
          ON workforce_work_items(status,department_id,assigned_employee_id,priority DESC,created_at);
        CREATE TABLE IF NOT EXISTS workforce_initiatives(
          id TEXT PRIMARY KEY, owner_employee_id TEXT NOT NULL, department_id TEXT NOT NULL,
          title TEXT NOT NULL, objective TEXT NOT NULL, status TEXT NOT NULL, score REAL NOT NULL,
          created_at TEXT NOT NULL, updated_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS workforce_memory_checkpoints(
          employee_id TEXT NOT NULL, checkpoint_key TEXT NOT NULL, external_ref TEXT NOT NULL,
          summary TEXT, updated_at TEXT NOT NULL, PRIMARY KEY(employee_id,checkpoint_key)
        );
        CREATE TABLE IF NOT EXISTS workforce_schedule_links(
          employee_id TEXT NOT NULL, automation_id TEXT NOT NULL, purpose TEXT NOT NULL,
          created_at TEXT NOT NULL, PRIMARY KEY(employee_id,automation_id)
        );
        CREATE TABLE IF NOT EXISTS workforce_performance(
          id TEXT PRIMARY KEY, employee_id TEXT NOT NULL, work_item_id TEXT NOT NULL,
          success INTEGER NOT NULL, verified INTEGER NOT NULL, latency_ms INTEGER NOT NULL,
          quality_score REAL NOT NULL, cost_microusd INTEGER NOT NULL, created_at TEXT NOT NULL
        );
        "#)?;
        Ok(())
    }

    pub fn seed_organization(&self, org:&Organization)->Result<usize> {
        let mut count=0;
        let now=Utc::now().to_rfc3339();
        let conn=self.conn()?;
        for employee in &org.spec().employees {
            conn.execute(r#"INSERT INTO employee_runtime(
              employee_id,department_id,status,current_work_item,node_id,memory_namespace,workspace,
              model_profile,budget_id,last_heartbeat,updated_at)
              VALUES(?1,?2,'idle',NULL,NULL,?3,?4,?5,?6,NULL,?7)
              ON CONFLICT(employee_id) DO UPDATE SET
                department_id=excluded.department_id,memory_namespace=excluded.memory_namespace,
                workspace=excluded.workspace,model_profile=excluded.model_profile,budget_id=excluded.budget_id,
                updated_at=excluded.updated_at"#,
              params![employee.id,employee.department_id,employee.memory_namespace,employee.workspace,
                employee.model_policy,format!("employee:{}",employee.id),now])?;
            count+=1;
        }
        Ok(count)
    }

    pub fn heartbeat_employee(&self, employee_id:&str, at:DateTime<Utc>)->Result<()> {
        let changed=self.conn()?.execute(
          "UPDATE employee_runtime SET last_heartbeat=?2,updated_at=?2 WHERE employee_id=?1",
          params![employee_id,at.to_rfc3339()])?;
        if changed==0 { return Err(WorkforceError::UnknownEmployee(employee_id.into())); }
        Ok(())
    }

    pub fn put_skill(&self, employee_id:&str, skill:&str, level:f64, evidence_count:u64)->Result<()> {
        let now=Utc::now().to_rfc3339();
        self.conn()?.execute(r#"INSERT INTO employee_skills(employee_id,skill,level,evidence_count,updated_at)
          VALUES(?1,?2,?3,?4,?5)
          ON CONFLICT(employee_id,skill) DO UPDATE SET level=excluded.level,evidence_count=excluded.evidence_count,updated_at=excluded.updated_at"#,
          params![employee_id,skill,level.clamp(0.0,1.0),evidence_count as i64,now])?;
        Ok(())
    }

    pub fn put_kpi(&self, employee_id:&str,key:&str,target:f64,current:f64,weight:f64)->Result<()> {
        let now=Utc::now().to_rfc3339();
        self.conn()?.execute(r#"INSERT INTO employee_kpis(employee_id,key,target,current,weight,updated_at)
          VALUES(?1,?2,?3,?4,?5,?6)
          ON CONFLICT(employee_id,key) DO UPDATE SET target=excluded.target,current=excluded.current,weight=excluded.weight,updated_at=excluded.updated_at"#,
          params![employee_id,key,target,current,weight.max(0.0),now])?;
        Ok(())
    }

    pub fn register_node(&self,node:&WorkforceNode)->Result<()> {
        self.conn()?.execute(r#"INSERT INTO workforce_nodes(node_id,kind,endpoint,capabilities_json,cpu_millis,memory_mib,online,last_heartbeat)
          VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
          ON CONFLICT(node_id) DO UPDATE SET kind=excluded.kind,endpoint=excluded.endpoint,
          capabilities_json=excluded.capabilities_json,cpu_millis=excluded.cpu_millis,
          memory_mib=excluded.memory_mib,online=excluded.online,last_heartbeat=excluded.last_heartbeat"#,
          params![node.node_id,node.kind,node.endpoint,serde_json::to_string(&node.capabilities)?,
            node.cpu_millis as i64,node.memory_mib as i64,node.online as i64,node.last_heartbeat.to_rfc3339()])?;
        Ok(())
    }

    pub fn heartbeat_node(&self,node_id:&str,at:DateTime<Utc>)->Result<()> {
        let changed=self.conn()?.execute(
          "UPDATE workforce_nodes SET online=1,last_heartbeat=?2 WHERE node_id=?1",
          params![node_id,at.to_rfc3339()])?;
        if changed==0 { return Err(WorkforceError::UnknownNode(node_id.into())); }
        Ok(())
    }

    pub fn mark_stale_nodes_offline(&self,now:DateTime<Utc>,offline_after_seconds:i64)->Result<usize> {
        Ok(self.conn()?.execute(
          "UPDATE workforce_nodes SET online=0 WHERE online=1 AND last_heartbeat<?1",
          [(now-Duration::seconds(offline_after_seconds.max(1))).to_rfc3339()])?)
    }

    pub fn bind_employee_node(&self,employee_id:&str,node_id:&str)->Result<()> {
        let conn=self.conn()?;
        let exists:Option<i64>=conn.query_row("SELECT 1 FROM workforce_nodes WHERE node_id=?1",[node_id],|r|r.get(0)).optional()?;
        if exists.is_none(){return Err(WorkforceError::UnknownNode(node_id.into()));}
        conn.execute(r#"INSERT INTO employee_node_bindings(employee_id,node_id,bound_at) VALUES(?1,?2,?3)
          ON CONFLICT(employee_id) DO UPDATE SET node_id=excluded.node_id,bound_at=excluded.bound_at"#,
          params![employee_id,node_id,Utc::now().to_rfc3339()])?;
        conn.execute("UPDATE employee_runtime SET node_id=?2,updated_at=?3 WHERE employee_id=?1",
          params![employee_id,node_id,Utc::now().to_rfc3339()])?;
        Ok(())
    }

    pub fn enqueue(&self,item:&WorkItem)->Result<()> {
        self.conn()?.execute(r#"INSERT INTO workforce_work_items(
          id,department_id,assigned_employee_id,created_by,parent_id,goal,plugin_id,capability,input_json,risk,
          priority,status,attempts,max_attempts,lease_owner,lease_until,required_skills_json,required_tools_json,
          budget_id,budget_microusd,approval_id,escalation_level,next_escalation_at,result_json,error,created_at,updated_at)
          VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27)"#,
          params![item.id.to_string(),item.department_id,item.assigned_employee_id,item.created_by,
          item.parent_id.map(|v|v.to_string()),item.goal,item.plugin_id,item.capability,serde_json::to_string(&item.input)?,
          risk_text(item.risk),item.priority,item.status,item.attempts as i64,item.max_attempts as i64,item.lease_owner,
          item.lease_until.map(|v|v.to_rfc3339()),serde_json::to_string(&item.required_skills)?,
          serde_json::to_string(&item.required_tools)?,item.budget_id,item.budget_microusd,item.approval_id,
          item.escalation_level as i64,item.next_escalation_at.map(|v|v.to_rfc3339()),
          item.result.as_ref().map(serde_json::to_string).transpose()?,item.error,
          item.created_at.to_rfc3339(),item.updated_at.to_rfc3339()])?;
        Ok(())
    }

    pub fn get_work_item(&self,id:Uuid)->Result<Option<WorkItem>> {
        self.conn()?.query_row("SELECT * FROM workforce_work_items WHERE id=?1",[id.to_string()],work_item_from_row)
          .optional().map_err(WorkforceError::from)
    }

    pub fn list_inbox(&self,employee_id:&str,department_id:&str)->Result<Vec<WorkItem>> {
        let conn=self.conn()?;
        let mut stmt=conn.prepare(r#"SELECT * FROM workforce_work_items
          WHERE status='pending' AND department_id=?2 AND (assigned_employee_id IS NULL OR assigned_employee_id=?1)
          ORDER BY priority DESC,created_at,id"#)?;
        let rows=stmt.query_map(params![employee_id,department_id],work_item_from_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>,_>>()?)
    }

    pub fn lease_next(
        &self, org:&Organization, employee_id:&str, owner:&str, now:DateTime<Utc>, policy:&WorkforcePolicy
    )->Result<Option<WorkItem>> {
        let employee=org.employee(employee_id).ok_or_else(||WorkforceError::UnknownEmployee(employee_id.into()))?;
        let candidates=self.list_inbox(employee_id,&employee.department_id)?;
        for item in candidates {
            if !employee.capabilities.contains(&item.capability) || item.risk > employee.max_risk { continue; }
            let mut skill_ok=true;
            for (skill,required) in &item.required_skills {
                let level=self.skill_level(employee_id,skill)?.unwrap_or(0.0);
                if level < *required { skill_ok=false; break; }
            }
            if !skill_ok || !item.required_tools.is_subset(&employee.tools) { continue; }
            if let Some(node_id)=self.bound_node(employee_id)? {
                if !self.node_online(&node_id)? { continue; }
            }
            let lease_until=now+Duration::seconds(policy.lease_seconds.max(5));
            let changed=self.conn()?.execute(r#"UPDATE workforce_work_items
              SET status='leased',assigned_employee_id=?2,attempts=attempts+1,lease_owner=?3,lease_until=?4,updated_at=?5
              WHERE id=?1 AND status='pending'"#,
              params![item.id.to_string(),employee_id,owner,lease_until.to_rfc3339(),now.to_rfc3339()])?;
            if changed==0 { continue; }
            self.conn()?.execute(
              "UPDATE employee_runtime SET status='working',current_work_item=?2,updated_at=?3 WHERE employee_id=?1",
              params![employee_id,item.id.to_string(),now.to_rfc3339()])?;
            return self.get_work_item(item.id);
        }
        Ok(None)
    }

    pub fn complete(&self,item_id:Uuid,employee_id:&str,result:&Value,latency_ms:u64,cost_microusd:i64,quality:f64)->Result<()> {
        let now=Utc::now();
        self.conn()?.execute(r#"UPDATE workforce_work_items SET status='completed',result_json=?2,error=NULL,
          lease_owner=NULL,lease_until=NULL,updated_at=?3 WHERE id=?1"#,
          params![item_id.to_string(),serde_json::to_string(result)?,now.to_rfc3339()])?;
        self.conn()?.execute(
          "UPDATE employee_runtime SET status='idle',current_work_item=NULL,updated_at=?2 WHERE employee_id=?1",
          params![employee_id,now.to_rfc3339()])?;
        self.record_performance(&PerformanceRecord{
          id:Uuid::new_v4(),employee_id:employee_id.into(),work_item_id:item_id,success:true,verified:true,
          latency_ms,quality_score:quality.clamp(0.0,1.0),cost_microusd,created_at:now
        })?;
        Ok(())
    }

    pub fn fail_or_retry(
        &self,item:&WorkItem,employee_id:&str,error:&str,now:DateTime<Utc>,policy:&WorkforcePolicy
    )->Result<bool> {
        let dead=item.attempts>=policy.max_attempts || item.attempts>=item.max_attempts;
        let next_status=if dead{"failed"}else{"pending"};
        let next_escalation=Some((now+Duration::seconds(policy.escalation_after_seconds.max(30))).to_rfc3339());
        self.conn()?.execute(r#"UPDATE workforce_work_items SET status=?2,error=?3,lease_owner=NULL,lease_until=NULL,
          next_escalation_at=?4,updated_at=?5 WHERE id=?1"#,
          params![item.id.to_string(),next_status,error,next_escalation,now.to_rfc3339()])?;
        self.conn()?.execute(
          "UPDATE employee_runtime SET status='idle',current_work_item=NULL,updated_at=?2 WHERE employee_id=?1",
          params![employee_id,now.to_rfc3339()])?;
        self.record_performance(&PerformanceRecord{
          id:Uuid::new_v4(),employee_id:employee_id.into(),work_item_id:item.id,success:false,verified:false,
          latency_ms:0,quality_score:0.0,cost_microusd:0,created_at:now
        })?;
        Ok(!dead)
    }

    pub fn recover_expired_leases(&self,now:DateTime<Utc>)->Result<usize> {
        let conn=self.conn()?;
        let mut stmt=conn.prepare("SELECT id,assigned_employee_id FROM workforce_work_items WHERE status='leased' AND lease_until<?1")?;
        let rows=stmt.query_map([now.to_rfc3339()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?)))?
          .collect::<std::result::Result<Vec<_>,_>>()?;
        drop(stmt);
        let mut changed=0;
        for (id,employee) in rows {
            changed+=conn.execute("UPDATE workforce_work_items SET status='pending',lease_owner=NULL,lease_until=NULL,updated_at=?2 WHERE id=?1",
              params![id,now.to_rfc3339()])?;
            if let Some(employee)=employee {
                conn.execute("UPDATE employee_runtime SET status='idle',current_work_item=NULL,updated_at=?2 WHERE employee_id=?1",
                  params![employee,now.to_rfc3339()])?;
            }
        }
        Ok(changed)
    }

    pub fn due_escalations(&self,now:DateTime<Utc>)->Result<Vec<WorkItem>> {
        let conn=self.conn()?;
        let mut stmt=conn.prepare(r#"SELECT * FROM workforce_work_items
          WHERE status IN ('pending','failed') AND next_escalation_at IS NOT NULL AND next_escalation_at<=?1
          ORDER BY priority DESC,created_at"#)?;
        let rows=stmt.query_map([now.to_rfc3339()],work_item_from_row)?;
        Ok(rows.collect::<std::result::Result<Vec<_>,_>>()?)
    }

    pub fn escalate(&self,org:&Organization,item:&WorkItem,now:DateTime<Utc>,policy:&WorkforcePolicy)->Result<Option<String>> {
        let current=item.assigned_employee_id.as_deref()
          .and_then(|id|org.employee(id));
        let target=current.and_then(|e|e.supervisor_id.clone())
          .or_else(||org.department(&item.department_id).map(|d|d.manager_employee_id.clone()));
        let Some(target)=target else{return Ok(None)};
        self.conn()?.execute(r#"UPDATE workforce_work_items SET assigned_employee_id=?2,status='pending',
          escalation_level=escalation_level+1,next_escalation_at=?3,updated_at=?4 WHERE id=?1"#,
          params![item.id.to_string(),target,(now+Duration::seconds(policy.escalation_after_seconds.max(30))).to_rfc3339(),now.to_rfc3339()])?;
        Ok(Some(target))
    }

    pub fn get_initiative(&self,id:Uuid)->Result<Option<Initiative>> {
        self.conn()?.query_row(
          "SELECT id,owner_employee_id,department_id,title,objective,status,score,created_at,updated_at FROM workforce_initiatives WHERE id=?1",
          [id.to_string()],|r|{
            let id_text:String=r.get(0)?;
            Ok(Initiative{
              id:Uuid::parse_str(&id_text).map_err(|e|rusqlite::Error::FromSqlConversionFailure(id_text.len(),rusqlite::types::Type::Text,Box::new(e)))?,
              owner_employee_id:r.get(1)?,department_id:r.get(2)?,title:r.get(3)?,objective:r.get(4)?,
              status:r.get(5)?,score:r.get(6)?,created_at:parse_time(r.get(7)?)?,updated_at:parse_time(r.get(8)?)?
            })
          }).optional().map_err(WorkforceError::from)
    }

    pub fn promote_initiative_to_manager_work(
        &self,
        org:&Organization,
        initiative_id:Uuid,
        now:DateTime<Utc>,
        policy:&WorkforcePolicy,
    )->Result<WorkItem> {
        let initiative=self.get_initiative(initiative_id)?
          .ok_or(WorkforceError::UnknownWorkItem(initiative_id))?;
        let department=org.department(&initiative.department_id)
          .ok_or_else(||WorkforceError::UnknownEmployee(format!("department:{}",initiative.department_id)))?;
        let manager=org.employee(&department.manager_employee_id)
          .ok_or_else(||WorkforceError::UnknownEmployee(department.manager_employee_id.clone()))?;
        if !manager.capabilities.contains("intelligence.reason") {
            return Err(WorkforceError::Ineligible{employee:manager.id.clone(),capability:"intelligence.reason".into()});
        }
        let mut work=WorkItem::new(
          initiative.department_id.clone(),
          initiative.owner_employee_id.clone(),
          format!("Plan department initiative: {}",initiative.title),
          "intelligence.nexus",
          "intelligence.reason",
          serde_json::json!({
            "prompt":format!(
              "Act as {} for department {}. Convert this initiative into a bounded execution plan using only department-authorized capabilities. Initiative: {}. Objective: {}",
              manager.title,initiative.department_id,initiative.title,initiative.objective
            ),
            "context":{
              "initiative_id":initiative.id,
              "department_id":initiative.department_id,
              "initiative_score":initiative.score,
              "mode":"m12-department-initiative-planning"
            }
          }),
          RiskClass::Low
        );
        work.assigned_employee_id=Some(manager.id.clone());
        work.priority=(initiative.score.clamp(0.0,1.0)*1000.0) as i64;
        work.max_attempts=policy.max_attempts;
        work.next_escalation_at=Some(now+Duration::seconds(policy.escalation_after_seconds.max(30)));
        self.enqueue(&work)?;
        self.conn()?.execute(
          "UPDATE workforce_initiatives SET status='planning',updated_at=?2 WHERE id=?1",
          params![initiative.id.to_string(),now.to_rfc3339()])?;
        Ok(work)
    }

    pub fn add_initiative(&self,value:&Initiative)->Result<()> {
        self.conn()?.execute(r#"INSERT INTO workforce_initiatives(id,owner_employee_id,department_id,title,objective,status,score,created_at,updated_at)
          VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)"#,
          params![value.id.to_string(),value.owner_employee_id,value.department_id,value.title,value.objective,
          value.status,value.score,value.created_at.to_rfc3339(),value.updated_at.to_rfc3339()])?;
        Ok(())
    }

    pub fn link_schedule(&self,employee_id:&str,automation_id:&str,purpose:&str)->Result<()> {
        self.conn()?.execute(r#"INSERT OR REPLACE INTO workforce_schedule_links(employee_id,automation_id,purpose,created_at)
          VALUES(?1,?2,?3,?4)"#,params![employee_id,automation_id,purpose,Utc::now().to_rfc3339()])?;
        Ok(())
    }

    pub fn checkpoint_memory(&self,employee_id:&str,key:&str,external_ref:&str,summary:Option<&str>)->Result<()> {
        self.conn()?.execute(r#"INSERT INTO workforce_memory_checkpoints(employee_id,checkpoint_key,external_ref,summary,updated_at)
          VALUES(?1,?2,?3,?4,?5)
          ON CONFLICT(employee_id,checkpoint_key) DO UPDATE SET external_ref=excluded.external_ref,summary=excluded.summary,updated_at=excluded.updated_at"#,
          params![employee_id,key,external_ref,summary,Utc::now().to_rfc3339()])?;
        Ok(())
    }

    pub fn list_employee_states(&self)->Result<Vec<EmployeeRuntimeState>> {
        let conn=self.conn()?;
        let mut stmt=conn.prepare("SELECT employee_id,department_id,status,current_work_item,node_id,memory_namespace,workspace,model_profile,budget_id,last_heartbeat,updated_at FROM employee_runtime ORDER BY department_id,employee_id")?;
        let rows=stmt.query_map([],|r|Ok(EmployeeRuntimeState{
          employee_id:r.get(0)?,department_id:r.get(1)?,status:r.get(2)?,
          current_work_item:parse_uuid_opt(r.get(3)?)?,node_id:r.get(4)?,
          memory_namespace:r.get(5)?,workspace:r.get(6)?,model_profile:r.get(7)?,
          budget_id:r.get(8)?,last_heartbeat:r.get::<_,Option<String>>(9)?.map(parse_time).transpose()?,
          updated_at:parse_time(r.get(10)?)?
        }))?;
        Ok(rows.collect::<std::result::Result<Vec<_>,_>>()?)
    }

    pub fn employee_node(&self,employee_id:&str)->Result<Option<WorkforceNode>> {
        let Some(node_id)=self.bound_node(employee_id)? else{return Ok(None)};
        let conn=self.conn()?;
        conn.query_row("SELECT node_id,kind,endpoint,capabilities_json,cpu_millis,memory_mib,online,last_heartbeat FROM workforce_nodes WHERE node_id=?1",
          [node_id],|r|{
            let raw:String=r.get(3)?;
            Ok(WorkforceNode{
              node_id:r.get(0)?,kind:r.get(1)?,endpoint:r.get(2)?,
              capabilities:serde_json::from_str(&raw).map_err(|e|rusqlite::Error::FromSqlConversionFailure(raw.len(),rusqlite::types::Type::Text,Box::new(e)))?,
              cpu_millis:r.get::<_,i64>(4)? as u64,memory_mib:r.get::<_,i64>(5)? as u64,
              online:r.get::<_,i64>(6)?!=0,last_heartbeat:parse_time(r.get(7)?)?
            })
          }).optional().map_err(WorkforceError::from)
    }

    pub fn list_nodes(&self)->Result<Vec<WorkforceNode>> {
        let conn=self.conn()?;
        let mut stmt=conn.prepare("SELECT node_id,kind,endpoint,capabilities_json,cpu_millis,memory_mib,online,last_heartbeat FROM workforce_nodes ORDER BY node_id")?;
        let rows=stmt.query_map([],|r|{
          let raw:String=r.get(3)?;
          Ok(WorkforceNode{
            node_id:r.get(0)?,kind:r.get(1)?,endpoint:r.get(2)?,
            capabilities:serde_json::from_str(&raw).map_err(|e|rusqlite::Error::FromSqlConversionFailure(raw.len(),rusqlite::types::Type::Text,Box::new(e)))?,
            cpu_millis:r.get::<_,i64>(4)? as u64,memory_mib:r.get::<_,i64>(5)? as u64,
            online:r.get::<_,i64>(6)?!=0,last_heartbeat:parse_time(r.get(7)?)?
          })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>,_>>()?)
    }

    pub fn employee_score(&self,employee_id:&str)->Result<f64> {
        let conn=self.conn()?;
        let perf:Option<(f64,f64)>=conn.query_row(
          "SELECT AVG(CASE WHEN verified=1 THEN 1.0 ELSE 0.0 END),AVG(quality_score) FROM workforce_performance WHERE employee_id=?1",
          [employee_id],|r|Ok((r.get::<_,Option<f64>>(0)?.unwrap_or(0.0),r.get::<_,Option<f64>>(1)?.unwrap_or(0.0)))
        ).optional()?;
        let (verified,quality)=perf.unwrap_or((0.0,0.0));
        let mut stmt=conn.prepare("SELECT target,current,weight FROM employee_kpis WHERE employee_id=?1")?;
        let mut weighted=0.0; let mut total_w=0.0;
        for row in stmt.query_map([employee_id],|r|Ok((r.get::<_,f64>(0)?,r.get::<_,f64>(1)?,r.get::<_,f64>(2)?)))? {
            let (target,current,w)=row?;
            let ratio=if target.abs()<f64::EPSILON{1.0}else{(current/target).clamp(0.0,1.5)/1.5};
            weighted+=ratio*w; total_w+=w;
        }
        let kpi=if total_w>0.0{weighted/total_w}else{0.5};
        Ok((verified*0.5+quality*0.3+kpi*0.2).clamp(0.0,1.0))
    }

    fn skill_level(&self,employee_id:&str,skill:&str)->Result<Option<f64>> {
        self.conn()?.query_row("SELECT level FROM employee_skills WHERE employee_id=?1 AND skill=?2",
          params![employee_id,skill],|r|r.get(0)).optional().map_err(WorkforceError::from)
    }
    fn bound_node(&self,employee_id:&str)->Result<Option<String>> {
        self.conn()?.query_row("SELECT node_id FROM employee_node_bindings WHERE employee_id=?1",
          [employee_id],|r|r.get(0)).optional().map_err(WorkforceError::from)
    }
    fn node_online(&self,node_id:&str)->Result<bool> {
        self.conn()?.query_row("SELECT online FROM workforce_nodes WHERE node_id=?1",[node_id],|r|Ok(r.get::<_,i64>(0)?!=0))
          .optional()?.ok_or_else(||WorkforceError::UnknownNode(node_id.into()))
    }
    fn record_performance(&self,value:&PerformanceRecord)->Result<()> {
        self.conn()?.execute(r#"INSERT INTO workforce_performance(id,employee_id,work_item_id,success,verified,latency_ms,quality_score,cost_microusd,created_at)
          VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)"#,
          params![value.id.to_string(),value.employee_id,value.work_item_id.to_string(),value.success as i64,
          value.verified as i64,value.latency_ms as i64,value.quality_score,value.cost_microusd,value.created_at.to_rfc3339()])?;
        Ok(())
    }
}

#[derive(Clone)]
pub struct WorkforceExecutor {
    store: WorkforceStore,
    organization: Arc<Organization>,
    plugins: Arc<PluginExecutor>,
    config: WorkforceConfig,
}

impl WorkforceExecutor {
    pub fn new(store:WorkforceStore,organization:Arc<Organization>,plugins:Arc<PluginExecutor>,config:WorkforceConfig)->Self{
        Self{store,organization,plugins,config}
    }

    pub async fn run_employee_once(&self,employee_id:&str,owner:&str)->Result<Option<WorkItem>> {
        let employee=self.organization.employee(employee_id)
          .ok_or_else(||WorkforceError::UnknownEmployee(employee_id.into()))?;
        let policy=self.config.policy_for(&employee.department_id);
        if let Some(node)=self.store.employee_node(employee_id)? {
            if !node.online { return Err(WorkforceError::NodeOffline{node:node.node_id}); }
            if !node.endpoint.starts_with("local://") {
                return Err(WorkforceError::Execution(format!(
                    "remote employee node {} ({}) is registered but remote execution is fail-closed until SYNERGY P7.4 encrypted task delegation/result return is implemented",
                    node.node_id,node.endpoint
                )));
            }
        }
        let Some(item)=self.store.lease_next(&self.organization,employee_id,owner,Utc::now(),&policy)? else {
            return Ok(None);
        };

        let capability=self.plugins.registry().get(&item.plugin_id)
          .and_then(|manifest|manifest.capability(&item.capability))
          .ok_or_else(||WorkforceError::Execution(format!("unknown workforce capability {}/{}",item.plugin_id,item.capability)))?;
        if capability.requires_approval {
            if item.approval_id.is_none() {
                self.store.fail_or_retry(&item,employee_id,"persistent approval is required",Utc::now(),&policy)?;
                return Err(WorkforceError::Execution("persistent approval is required".into()));
            }
            if !self.plugins.persistent_control_plane_enabled() {
                self.store.fail_or_retry(&item,employee_id,"M12 approval requires control_plane_db",Utc::now(),&policy)?;
                return Err(WorkforceError::Execution("M12 approval requires persistent control_plane_db".into()));
            }
        }
        if item.budget_microusd>0 && !self.plugins.persistent_control_plane_enabled() {
            self.store.fail_or_retry(&item,employee_id,"M12 budget reservation requires control_plane_db",Utc::now(),&policy)?;
            return Err(WorkforceError::Execution("M12 budget reservation requires persistent control_plane_db".into()));
        }

        let memory = self.plugins.execute(PluginCall{
          plugin_id:"intelligence.nexus".into(),
          capability:"memory.search".into(),
          input:serde_json::json!({
            "query":format!("{} {}",employee.id,item.goal),
            "namespaces":[employee.memory_namespace],
            "limit":12
          }),
          approved:false,
        }).await.ok().map(|result|result.output).unwrap_or(Value::Null);

        let mut input=item.input.clone();
        if let Some(object)=input.as_object_mut() {
            object.insert("_workforce".into(),serde_json::json!({
              "employee_id":employee.id,
              "department_id":employee.department_id,
              "work_item_id":item.id,
              "workspace":employee.workspace,
              "memory_namespace":employee.memory_namespace,
              "model_policy":employee.model_policy,
              "device_policy":employee.device_policy,
              "memory":memory
            }));
            if let Some(budget_id)=&item.budget_id {
                object.insert("_budget".into(),serde_json::json!({
                  "id":budget_id,
                  "amount_microusd":item.budget_microusd
                }));
            }
            if let Some(approval_id)=&item.approval_id {
                object.insert("_approval_id".into(),Value::String(approval_id.clone()));
            }
        }
        let started=std::time::Instant::now();
        let result=self.plugins.execute(PluginCall{
          plugin_id:item.plugin_id.clone(),
          capability:item.capability.clone(),
          input,
          approved:item.approval_id.is_some(),
        }).await;
        let latency=started.elapsed().as_millis().min(u64::MAX as u128) as u64;
        match result {
            Ok(result)=>{
                let value=serde_json::to_value(&result).map_err(|e|WorkforceError::Execution(e.to_string()))?;
                self.store.complete(item.id,employee_id,&value,latency,0,1.0)?;
                let mut content=serde_json::to_string(&serde_json::json!({
                  "work_item_id":item.id,
                  "goal":item.goal,
                  "plugin_id":item.plugin_id,
                  "capability":item.capability,
                  "result":result.output
                })).map_err(|e|WorkforceError::Execution(e.to_string()))?;
                if content.len()>4000 { content.truncate(4000); }
                let key=format!("work:{}",item.id);
                if self.plugins.execute(PluginCall{
                  plugin_id:"intelligence.nexus".into(),
                  capability:"memory.upsert".into(),
                  input:serde_json::json!({
                    "key":key,
                    "content":content,
                    "namespace":employee.memory_namespace,
                    "metadata":{"employee_id":employee.id,"department_id":employee.department_id,"verified":true},
                    "source":"synergy-ai-autopilot-m12"
                  }),
                  approved:false,
                }).await.is_ok() {
                    self.store.checkpoint_memory(employee_id,&format!("work:{}",item.id),
                      &format!("nexus://{}/work:{}",employee.memory_namespace,item.id),Some("verified work outcome"))?;
                }
            }
            Err(error)=>{
                self.store.fail_or_retry(&item,employee_id,&error.to_string(),Utc::now(),&policy)?;
            }
        }
        self.store.get_work_item(item.id)?.ok_or(WorkforceError::UnknownWorkItem(item.id)).map(Some)
    }

    pub async fn run_department_once(
        &self,
        department_id:&str,
        owner_prefix:&str,
    )->Result<Vec<(String,Option<WorkItem>)>> {
        let mut jobs=tokio::task::JoinSet::new();
        for employee in self.organization.spec().employees.iter()
            .filter(|employee|employee.department_id==department_id && employee.autonomous)
        {
            let worker=self.clone();
            let employee_id=employee.id.clone();
            let owner=format!("{owner_prefix}:{}",employee.id);
            jobs.spawn(async move {
                let result=worker.run_employee_once(&employee_id,&owner).await;
                (employee_id,result)
            });
        }
        let mut results=Vec::new();
        while let Some(joined)=jobs.join_next().await {
            let (employee,result)=joined.map_err(|e|WorkforceError::Execution(format!("department worker join failure: {e}")))?;
            results.push((employee,result?));
        }
        results.sort_by(|a,b|a.0.cmp(&b.0));
        Ok(results)
    }

    pub fn maintain(&self,now:DateTime<Utc>)->Result<MaintenanceReport> {
        let mut report=MaintenanceReport::default();
        report.recovered_leases=self.store.recover_expired_leases(now)?;
        let mut seen=BTreeSet::new();
        for item in self.store.due_escalations(now)? {
            if !seen.insert(item.id){continue}
            let policy=self.config.policy_for(&item.department_id);
            if self.store.escalate(&self.organization,&item,now,&policy)?.is_some(){
                report.escalated+=1;
            }
        }
        let max_offline=self.config.defaults.offline_after_seconds;
        report.nodes_marked_offline=self.store.mark_stale_nodes_offline(now,max_offline)?;
        Ok(report)
    }
}

#[derive(Clone,Debug,Default,Serialize,Deserialize)]
pub struct MaintenanceReport {
    pub recovered_leases:usize,
    pub escalated:usize,
    pub nodes_marked_offline:usize,
}

fn risk_text(risk:RiskClass)->&'static str{
    match risk{RiskClass::Low=>"Low",RiskClass::Medium=>"Medium",RiskClass::High=>"High",RiskClass::Critical=>"Critical"}
}
fn parse_risk(value:&str)->rusqlite::Result<RiskClass>{
    match value{
      "Low"=>Ok(RiskClass::Low),"Medium"=>Ok(RiskClass::Medium),"High"=>Ok(RiskClass::High),"Critical"=>Ok(RiskClass::Critical),
      _=>Err(rusqlite::Error::InvalidQuery)
    }
}
fn parse_time(value:String)->rusqlite::Result<DateTime<Utc>>{
    DateTime::parse_from_rfc3339(&value).map(|v|v.with_timezone(&Utc))
      .map_err(|e|rusqlite::Error::FromSqlConversionFailure(value.len(),rusqlite::types::Type::Text,Box::new(e)))
}
fn parse_uuid_opt(value:Option<String>)->rusqlite::Result<Option<Uuid>>{
    value.map(|v|Uuid::parse_str(&v).map_err(|e|rusqlite::Error::FromSqlConversionFailure(v.len(),rusqlite::types::Type::Text,Box::new(e)))).transpose()
}
fn work_item_from_row(row:&rusqlite::Row<'_>)->rusqlite::Result<WorkItem>{
    let id_text:String=row.get("id")?;
    Ok(WorkItem{
      id:Uuid::parse_str(&id_text).map_err(|e|rusqlite::Error::FromSqlConversionFailure(id_text.len(),rusqlite::types::Type::Text,Box::new(e)))?,
      department_id:row.get("department_id")?,assigned_employee_id:row.get("assigned_employee_id")?,
      created_by:row.get("created_by")?,parent_id:parse_uuid_opt(row.get("parent_id")?)?,
      goal:row.get("goal")?,plugin_id:row.get("plugin_id")?,capability:row.get("capability")?,
      input:serde_json::from_str(&row.get::<_,String>("input_json")?).map_err(|e|rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(e)))?,
      risk:parse_risk(&row.get::<_,String>("risk")?)?,priority:row.get("priority")?,status:row.get("status")?,
      attempts:row.get::<_,i64>("attempts")? as u32,max_attempts:row.get::<_,i64>("max_attempts")? as u32,
      lease_owner:row.get("lease_owner")?,lease_until:row.get::<_,Option<String>>("lease_until")?.map(parse_time).transpose()?,
      required_skills:serde_json::from_str(&row.get::<_,String>("required_skills_json")?).map_err(|e|rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(e)))?,
      required_tools:serde_json::from_str(&row.get::<_,String>("required_tools_json")?).map_err(|e|rusqlite::Error::FromSqlConversionFailure(0,rusqlite::types::Type::Text,Box::new(e)))?,
      budget_id:row.get("budget_id")?,budget_microusd:row.get("budget_microusd")?,approval_id:row.get("approval_id")?,
      escalation_level:row.get::<_,i64>("escalation_level")? as u32,
      next_escalation_at:row.get::<_,Option<String>>("next_escalation_at")?.map(parse_time).transpose()?,
      result:row.get::<_,Option<String>>("result_json")?.map(|v|serde_json::from_str(&v)
        .map_err(|e|rusqlite::Error::FromSqlConversionFailure(v.len(),rusqlite::types::Type::Text,Box::new(e)))).transpose()?,
      error:row.get("error")?,created_at:parse_time(row.get("created_at")?)?,updated_at:parse_time(row.get("updated_at")?)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use autopilot_org::OrganizationSpec;

    fn org()->Organization{
        let raw=include_str!("../../../config/organization.default.json");
        Organization::validate(serde_json::from_str::<OrganizationSpec>(raw).unwrap()).unwrap()
    }
    fn config()->WorkforceConfig{
        serde_json::from_str(include_str!("../../../config/workforce.default.json")).unwrap()
    }

    #[test]
    fn organization_seeds_persistent_employee_runtime() {
        let store=WorkforceStore::open_in_memory().unwrap();
        let org=org();
        assert_eq!(store.seed_organization(&org).unwrap(),org.spec().employees.len());
        store.heartbeat_employee("sales-copy",Utc::now()).unwrap();
    }

    #[test]
    fn skill_and_node_constraints_gate_leases() {
        let store=WorkforceStore::open_in_memory().unwrap();
        let org=org();
        store.seed_organization(&org).unwrap();
        store.put_skill("software-engineer","rust",0.9,10).unwrap();
        let node=WorkforceNode{
          node_id:"dev-1".into(),kind:"desktop".into(),endpoint:"local://dev-1".into(),
          capabilities:["computer.read_file".to_string()].into_iter().collect(),
          cpu_millis:4000,memory_mib:8192,online:true,last_heartbeat:Utc::now()
        };
        store.register_node(&node).unwrap();
        store.bind_employee_node("software-engineer","dev-1").unwrap();

        let mut item=WorkItem::new("engineering","cto","review repo","computer.local.commander","computer.read_file",
          serde_json::json!({"path":"README.md"}),RiskClass::Low);
        item.assigned_employee_id=Some("software-engineer".into());
        item.required_skills.insert("rust".into(),0.8);
        store.enqueue(&item).unwrap();
        let p=config().policy_for("engineering");
        assert!(store.lease_next(&org,"software-engineer","test",Utc::now(),&p).unwrap().is_some());
    }

    #[test]
    fn failed_work_escalates_to_supervisor() {
        let store=WorkforceStore::open_in_memory().unwrap();
        let org=org();
        store.seed_organization(&org).unwrap();
        let mut item=WorkItem::new("sales","sales-director","compose","business.capability.runtime","business.sales.compose_offer",
          serde_json::json!({}),RiskClass::Low);
        item.assigned_employee_id=Some("sales-copy".into());
        item.next_escalation_at=Some(Utc::now()-Duration::seconds(1));
        store.enqueue(&item).unwrap();
        let p=config().policy_for("sales");
        let due=store.due_escalations(Utc::now()).unwrap();
        let target=store.escalate(&org,&due[0],Utc::now(),&p).unwrap();
        assert_eq!(target.as_deref(),Some("sales-director"));
    }

    #[test]
    fn employee_score_combines_verified_performance_and_kpis() {
        let store=WorkforceStore::open_in_memory().unwrap();
        let org=org();
        store.seed_organization(&org).unwrap();
        store.put_kpi("sales-copy","conversion",10.0,8.0,1.0).unwrap();
        let work=Uuid::new_v4();
        store.record_performance(&PerformanceRecord{
          id:Uuid::new_v4(),employee_id:"sales-copy".into(),work_item_id:work,success:true,verified:true,
          latency_ms:100,quality_score:0.9,cost_microusd:100,created_at:Utc::now()
        }).unwrap();
        assert!(store.employee_score("sales-copy").unwrap()>0.7);
    }
}
