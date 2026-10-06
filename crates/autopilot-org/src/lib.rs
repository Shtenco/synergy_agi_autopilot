use std::collections::{BTreeMap, BTreeSet};

use autopilot_domain::RiskClass;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DepartmentSpec {
    pub id: String,
    pub name: String,
    pub mission: String,
    pub manager_employee_id: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub memory_namespace: String,
    #[serde(default)]
    pub monthly_budget_microusd: i64,
    #[serde(default)]
    pub allowed_capabilities: BTreeSet<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmployeeSpec {
    pub id: String,
    pub name: String,
    pub title: String,
    pub department_id: String,
    #[serde(default)]
    pub supervisor_id: Option<String>,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub capabilities: BTreeSet<String>,
    #[serde(default)]
    pub tools: BTreeSet<String>,
    pub max_risk: RiskClass,
    #[serde(default)]
    pub model_policy: String,
    #[serde(default)]
    pub device_policy: String,
    #[serde(default)]
    pub workspace: String,
    #[serde(default)]
    pub memory_namespace: String,
    #[serde(default)]
    pub autonomous: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrganizationSpec {
    pub schema: String,
    pub company_id: String,
    pub company_name: String,
    pub departments: Vec<DepartmentSpec>,
    pub employees: Vec<EmployeeSpec>,
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum OrgError {
    #[error("unsupported organization schema: {0}")]
    Schema(String),
    #[error("duplicate department id: {0}")]
    DuplicateDepartment(String),
    #[error("duplicate employee id: {0}")]
    DuplicateEmployee(String),
    #[error("department {department} references unknown manager {manager}")]
    UnknownManager { department: String, manager: String },
    #[error("employee {employee} references unknown department {department}")]
    UnknownDepartment { employee: String, department: String },
    #[error("employee {employee} references unknown supervisor {supervisor}")]
    UnknownSupervisor { employee: String, supervisor: String },
    #[error("supervision graph contains a cycle")]
    SupervisorCycle,
    #[error("employee {employee} capability {capability} is outside department policy")]
    CapabilityOutsideDepartment { employee: String, capability: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmployeeAgentDescriptor {
    pub agent_id: String,
    pub role: String,
    pub department_id: String,
    pub supervisor_id: Option<String>,
    pub capabilities: BTreeSet<String>,
    pub tools: BTreeSet<String>,
    pub workspace: String,
    pub memory_namespace: String,
    pub model_policy: String,
    pub device_policy: String,
    pub remote_eligible: bool,
    pub autonomous: bool,
}

#[derive(Clone, Debug)]
pub struct Organization {
    spec: OrganizationSpec,
    departments: BTreeMap<String, DepartmentSpec>,
    employees: BTreeMap<String, EmployeeSpec>,
}

impl Organization {
    pub fn validate(spec: OrganizationSpec) -> Result<Self, OrgError> {
        if spec.schema != "synergy.organization.v1" {
            return Err(OrgError::Schema(spec.schema));
        }
        let mut departments = BTreeMap::new();
        for department in &spec.departments {
            if departments.insert(department.id.clone(), department.clone()).is_some() {
                return Err(OrgError::DuplicateDepartment(department.id.clone()));
            }
        }
        let mut employees = BTreeMap::new();
        for employee in &spec.employees {
            if employees.insert(employee.id.clone(), employee.clone()).is_some() {
                return Err(OrgError::DuplicateEmployee(employee.id.clone()));
            }
        }
        for department in departments.values() {
            let Some(manager) = employees.get(&department.manager_employee_id) else {
                return Err(OrgError::UnknownManager {
                    department: department.id.clone(),
                    manager: department.manager_employee_id.clone(),
                });
            };
            if manager.department_id != department.id {
                return Err(OrgError::UnknownManager {
                    department: department.id.clone(),
                    manager: department.manager_employee_id.clone(),
                });
            }
        }
        for employee in employees.values() {
            let department = departments.get(&employee.department_id)
                .ok_or_else(|| OrgError::UnknownDepartment {
                    employee: employee.id.clone(),
                    department: employee.department_id.clone(),
                })?;
            if let Some(supervisor) = &employee.supervisor_id {
                if !employees.contains_key(supervisor) {
                    return Err(OrgError::UnknownSupervisor {
                        employee: employee.id.clone(), supervisor: supervisor.clone()
                    });
                }
            }
            if !department.allowed_capabilities.is_empty() {
                for capability in &employee.capabilities {
                    if !department.allowed_capabilities.contains(capability) {
                        return Err(OrgError::CapabilityOutsideDepartment {
                            employee: employee.id.clone(), capability: capability.clone()
                        });
                    }
                }
            }
        }
        if supervision_cycle(&employees) { return Err(OrgError::SupervisorCycle); }
        Ok(Self { spec, departments, employees })
    }

    pub fn spec(&self) -> &OrganizationSpec { &self.spec }
    pub fn employee(&self, id: &str) -> Option<&EmployeeSpec> { self.employees.get(id) }
    pub fn department(&self, id: &str) -> Option<&DepartmentSpec> { self.departments.get(id) }

    pub fn eligible_employees(&self, capability: &str, risk: RiskClass) -> Vec<&EmployeeSpec> {
        let mut values = self.employees.values()
            .filter(|employee| employee.autonomous)
            .filter(|employee| employee.capabilities.contains(capability))
            .filter(|employee| risk <= employee.max_risk)
            .collect::<Vec<_>>();
        values.sort_by(|a,b| {
            let manager_bias_a = self.departments.get(&a.department_id)
                .is_some_and(|department| department.manager_employee_id == a.id);
            let manager_bias_b = self.departments.get(&b.department_id)
                .is_some_and(|department| department.manager_employee_id == b.id);
            manager_bias_a.cmp(&manager_bias_b).then_with(|| a.id.cmp(&b.id))
        });
        values
    }
    pub fn agent_fabric_descriptors(&self) -> Vec<EmployeeAgentDescriptor> {
        self.employees.values().map(|employee| EmployeeAgentDescriptor {
            agent_id: employee.id.clone(),
            role: employee.title.clone(),
            department_id: employee.department_id.clone(),
            supervisor_id: employee.supervisor_id.clone(),
            capabilities: employee.capabilities.clone(),
            tools: employee.tools.clone(),
            workspace: if employee.workspace.is_empty() {
                self.departments.get(&employee.department_id)
                    .map(|department| department.workspace.clone())
                    .unwrap_or_default()
            } else {
                employee.workspace.clone()
            },
            memory_namespace: if employee.memory_namespace.is_empty() {
                self.departments.get(&employee.department_id)
                    .map(|department| department.memory_namespace.clone())
                    .unwrap_or_default()
            } else {
                employee.memory_namespace.clone()
            },
            model_policy: employee.model_policy.clone(),
            device_policy: employee.device_policy.clone(),
            remote_eligible: employee.device_policy.to_ascii_lowercase().contains("remote")
                || employee.device_policy.to_ascii_lowercase().contains("fabric"),
            autonomous: employee.autonomous,
        }).collect()
    }

}

fn supervision_cycle(employees: &BTreeMap<String, EmployeeSpec>) -> bool {
    for start in employees.keys() {
        let mut seen = BTreeSet::new();
        let mut current = Some(start.as_str());
        while let Some(id) = current {
            if !seen.insert(id.to_string()) { return true; }
            current = employees.get(id).and_then(|employee| employee.supervisor_id.as_deref());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> OrganizationSpec {
        OrganizationSpec {
            schema:"synergy.organization.v1".into(),
            company_id:"synergy".into(),
            company_name:"SYNERGY AI Company".into(),
            departments:vec![
                DepartmentSpec {
                    id:"sales".into(), name:"AI Sales".into(), mission:"Revenue".into(),
                    manager_employee_id:"sales-director".into(), workspace:"sales".into(),
                    memory_namespace:"sales".into(), monthly_budget_microusd:10_000_000,
                    allowed_capabilities:["business.sales.plan","business.sales.compose_offer"].into_iter().map(str::to_string).collect(),
                }
            ],
            employees:vec![
                EmployeeSpec {
                    id:"sales-director".into(), name:"Alex".into(), title:"AI Sales Director".into(),
                    department_id:"sales".into(), supervisor_id:None, instructions:"manage pipeline".into(),
                    capabilities:["business.sales.plan"].into_iter().map(str::to_string).collect(),
                    tools:BTreeSet::new(), max_risk:RiskClass::Medium, model_policy:"balanced".into(),
                    device_policy:"local-first".into(), workspace:"sales".into(), memory_namespace:"sales".into(),
                    autonomous:true,
                },
                EmployeeSpec {
                    id:"sales-copy".into(), name:"Maya".into(), title:"AI Sales Copywriter".into(),
                    department_id:"sales".into(), supervisor_id:Some("sales-director".into()),
                    instructions:"write offers".into(),
                    capabilities:["business.sales.compose_offer"].into_iter().map(str::to_string).collect(),
                    tools:BTreeSet::new(), max_risk:RiskClass::Low, model_policy:"quality".into(),
                    device_policy:"local-first".into(), workspace:"sales".into(), memory_namespace:"sales".into(),
                    autonomous:true,
                }
            ],
        }
    }

    #[test]
    fn default_organization_compiles_to_agent_descriptors() {
        let raw = include_str!("../../../config/organization.default.json");
        let spec: OrganizationSpec = serde_json::from_str(raw).unwrap();
        let org = Organization::validate(spec).unwrap();
        let agents = org.agent_fabric_descriptors();
        assert_eq!(agents.len(), org.spec().employees.len());
        assert!(agents.iter().any(|agent| agent.agent_id == "software-engineer"));
        assert!(agents.iter().all(|agent| !agent.department_id.is_empty()));
    }

    #[test]
    fn default_organization_file_is_valid() {
        let raw = include_str!("../../../config/organization.default.json");
        let spec: OrganizationSpec = serde_json::from_str(raw).unwrap();
        let org = Organization::validate(spec).unwrap();
        assert!(org.spec().departments.len() >= 10);
        assert!(org.spec().employees.len() >= 20);
    }

    #[test]
    fn organization_validates_and_routes_employee() {
        let org=Organization::validate(sample()).unwrap();
        let eligible=org.eligible_employees("business.sales.compose_offer",RiskClass::Low);
        assert_eq!(eligible[0].id,"sales-copy");
    }

    #[test]
    fn employee_cannot_escape_department_capability_policy() {
        let mut spec=sample();
        spec.employees[1].capabilities.insert("computer.run_command".into());
        assert!(matches!(
            Organization::validate(spec),
            Err(OrgError::CapabilityOutsideDepartment { .. })
        ));
    }

    #[test]
    fn supervision_cycle_is_rejected() {
        let mut spec=sample();
        spec.employees[0].supervisor_id=Some("sales-copy".into());
        assert_eq!(Organization::validate(spec).unwrap_err(),OrgError::SupervisorCycle);
    }
}
