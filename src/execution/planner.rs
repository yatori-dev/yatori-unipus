use crate::api::course::{build_tasks, fetch_course_units, fetch_unit, select_tasks};
use crate::course::GroupTask;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct Plan {
    pub units: Vec<UnitPlan>,
    pub total: usize,
    pub todo: usize,
}

#[derive(Debug, Serialize)]
pub struct UnitPlan {
    pub unit_id: String,
    pub tasks: Vec<GroupTask>,
    pub todo: usize,
}

pub(crate) async fn plan_course(session: &crate::api::session::Session) -> Result<Plan> {
    let compulsory_only = session.cfg().execution.compulsory_only();
    let units = fetch_course_units(session).await?;
    let mut plan = Plan::default();
    for uid in &units {
        let rt = fetch_unit(session, uid).await?;
        let tasks = select_tasks(&build_tasks(uid, &rt), compulsory_only);
        let total = tasks.len();
        let todo = tasks.iter().filter(|t| !t.passed).count();
        plan.units.push(UnitPlan {
            unit_id: uid.clone(),
            tasks,
            todo,
        });
        plan.total += total;
        plan.todo += todo;
    }
    Ok(plan)
}
