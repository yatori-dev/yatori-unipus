use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ArchiveEventKind {
    Generated,

    ViewOnly,

    Refreshed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ReportEvent {
    UnitStart {
        unit_id: String,
        label: Option<String>,
        total: usize,
        required_only: bool,
    },

    TaskStart {
        group_id: String,
        tab_type: String,
    },

    /// 提交接口接受了请求，远端是否通过仍须查询。
    TaskSubmitted {
        group_id: String,
        tab_type: String,
    },

    TaskSkipped {
        group_id: String,
        reason: String,
    },

    TaskFailed {
        group_id: String,
        tab_type: String,
        error: String,
    },

    RateLimited {
        attempt: u32,
        max: u32,
        wait_secs: u64,
        message: String,
    },

    Archive {
        unit_id: String,
        group_id: String,
        kind: ArchiveEventKind,
        path: String,
        modules: usize,
        questions: usize,
        media: usize,
    },

    RunFinished {
        submitted: u32,
        skipped: u32,
        failed: u32,
    },

    CourseInfo {
        course_id: String,
        name: String,
    },
    UnitInfo {
        unit_id: String,
        label: String,
    },
}

pub trait Reporter: Send + Sync {
    fn report(&self, ev: ReportEvent);
}

/// 不需要运行事件时可以直接传入 `&()`。
impl Reporter for () {
    fn report(&self, _: ReportEvent) {}
}

pub struct FnReporter<F>(pub F);

impl<F: Fn(ReportEvent) + Send + Sync> Reporter for FnReporter<F> {
    fn report(&self, ev: ReportEvent) {
        (self.0)(ev)
    }
}
