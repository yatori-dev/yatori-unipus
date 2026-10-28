mod catalog;
mod labels;

use crate::api::session::{Session, progress_url, unit_progress_url};
use anyhow::Result;
use serde::Deserialize;

pub(crate) use crate::course::{
    CourseProgress as CourseProgressRt, GroupTask, UnitProgress as ProgressRt,
};
pub(crate) use catalog::{apply_course_selection, fetch_home_courses, fetch_user_school};
pub(crate) use labels::{course_display_name, unit_label};

#[derive(Debug, Clone, Deserialize)]
struct CourseProgressResponse {
    rt: CourseProgressRt,
}
#[derive(Debug, Clone, Deserialize)]
struct ProgressResponse {
    rt: ProgressRt,
}

pub async fn fetch_unit(session: &Session, unit_id: &str) -> Result<ProgressRt> {
    let course_id = session.course_id();
    let url = unit_progress_url(&course_id, unit_id, &session.open_id());
    let resp: ProgressResponse = session.get_json(&url).await?;
    Ok(resp.rt)
}

pub async fn fetch_course_progress(session: &Session) -> Result<CourseProgressRt> {
    let course_id = session.course_id();
    let url = progress_url(&course_id, &session.open_id());
    let resp: CourseProgressResponse = session.get_json(&url).await?;
    Ok(resp.rt)
}

pub async fn fetch_course_units(session: &Session) -> Result<Vec<String>> {
    let rt = fetch_course_progress(session).await?;
    let mut units: Vec<String> = rt.units.keys().cloned().collect();
    units.sort();
    Ok(units)
}

pub fn build_tasks(unit_id: &str, rt: &ProgressRt) -> Vec<GroupTask> {
    let mut tasks = Vec::new();
    for (gid, leaf) in &rt.leafs {
        let strategies = leaf.strategies.clone().unwrap_or_default();
        let state = leaf.state.unwrap_or_default();
        tasks.push(GroupTask {
            group_id: gid.clone(),
            unit_id: unit_id.to_string(),
            tab_type: leaf.tab_type.clone(),
            required: strategies.required,
            passed: state.pass >= 1,
            min_score_pct: strategies.min_score_pct.unwrap_or(0),
            start_time: strategies.start_time.unwrap_or(0),
            end_time: strategies.end_time.unwrap_or(0),
        });
    }
    tasks.sort_by(|a, b| a.group_id.cmp(&b.group_id));
    tasks
}

pub fn select_tasks(tasks: &[GroupTask], compulsory_only: bool) -> Vec<GroupTask> {
    tasks
        .iter()
        .filter(|t| !compulsory_only || t.required)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::catalog::{parse_bookshelf_courses, parse_home_courses};
    use super::labels::course_display_name_fallback;
    use super::*;
    use crate::course::HomeCourse;

    #[test]
    fn course_display_known_and_fallback() {
        assert_eq!(
            course_display_name_fallback("course-v2:75b7546ea002b72+nhce_v3_rw_4+20230116"),
            "新视野大学英语(第3版)读写教程 4"
        );
        assert_eq!(
            course_display_name_fallback("course-v2:75b7546ea002b72+nhce_v4_rw+20230116"),
            "新视野大学英语(第4版)读写教程"
        );
        assert_eq!(
            course_display_name_fallback("course-v2:x+yz_3+9"),
            "未知书本(yz)未知版本()未知课程() 3"
        );
    }

    #[test]
    fn course_selection_replaces_stale_identity_fields() {
        let mut cfg = crate::config::CourseConfig {
            class_id: "old-class".into(),
            curricula_id: "old-curricula".into(),
            ..crate::config::CourseConfig::default()
        };
        let course = HomeCourse {
            name: "New course".into(),
            course_id: "course-1".into(),
            class_id: "new-class".into(),
            curricula_id: String::new(),
            group_label: String::new(),
        };
        assert!(apply_course_selection(&mut cfg, &course));
        assert_eq!(cfg.course_id, "course-1");
        assert_eq!(cfg.class_id, "new-class");
        assert!(cfg.curricula_id.is_empty());

        cfg.curricula_id = "existing".into();
        let same_class = HomeCourse {
            curricula_id: String::new(),
            class_id: "new-class".into(),
            ..course
        };
        assert!(apply_course_selection(&mut cfg, &same_class));
        assert!(cfg.curricula_id.is_empty());
    }

    #[test]
    fn progress_tolerates_null_and_missing_fields() {
        let v = serde_json::json!({
            "rt": {
                "duration_time": null,
                "flag": null,
                "open_id": null,
                "publish_version": "139753",
                "tutorialId": null,
                "leafs": {
                    "g-null": {
                        "duration": null,
                        "state": {"pass": null, "pass2": 0, "perm": null},
                        "strategies": {"end_time": null, "min_score_pct": null, "required": null, "start_time": null, "statistic_mode_out": null},
                        "tab_type": "task"
                    },
                    "g-missing": {
                        "state": {},
                        "strategies": {},
                        "tab_type": "text"
                    },
                    "g-normal": {
                        "state": {"pass": 1, "pass2": 0, "perm": 1},
                        "strategies": {"end_time": 1791647999, "min_score_pct": 60, "required": true, "start_time": 1788710400, "statistic_mode_out": false},
                        "tab_type": "video",
                        "duration": 120
                    }
                }
            }
        });
        let resp: ProgressResponse = serde_json::from_value(v).expect("null/缺失字段应可正常解析");
        assert_eq!(resp.rt.duration_time, 0);
        assert_eq!(resp.rt.publish_version, "139753");

        let tasks = build_tasks("u1", &resp.rt);
        let t_null = tasks.iter().find(|t| t.group_id == "g-null").unwrap();
        assert_eq!(
            (t_null.min_score_pct, t_null.start_time, t_null.end_time),
            (0, 0, 0)
        );
        assert!(!t_null.required && !t_null.passed);
        let t_missing = tasks.iter().find(|t| t.group_id == "g-missing").unwrap();
        assert_eq!(
            (
                t_missing.min_score_pct,
                t_missing.start_time,
                t_missing.end_time
            ),
            (0, 0, 0)
        );
        let t_norm = tasks.iter().find(|t| t.group_id == "g-normal").unwrap();
        assert!(t_norm.required && t_norm.passed);
        assert_eq!(
            (t_norm.min_score_pct, t_norm.start_time, t_norm.end_time),
            (60, 1788710400, 1791647999)
        );

        let cv = serde_json::json!({"rt": {"units": {}, "publish_version": null}});
        let c: CourseProgressResponse =
            serde_json::from_value(cv).expect("null publish_version 应可解析");
        assert!(c.rt.publish_version.is_empty());
    }

    #[test]
    fn progress_tolerates_float_string_and_null_structs() {
        let v = serde_json::json!({
            "rt": {
                "leafs": {
                    "g-float": {
                        "duration": 120.5,
                        "state": {"pass": 1.0, "pass2": 0.0, "perm": null},
                        "strategies": {"end_time": 1791647999.6, "min_score_pct": 0.75, "required": true, "start_time": 1788710400.4},
                        "tab_type": "video"
                    },
                    "g-string": {
                        "state": {"pass": "1"},
                        "strategies": {"end_time": "1791647999", "min_score_pct": "60", "required": true, "start_time": "1788710400"},
                        "tab_type": "task"
                    },
                    "g-null-struct": {
                        "state": null,
                        "strategies": null,
                        "tab_type": "text"
                    }
                }
            }
        });
        let resp: ProgressResponse =
            serde_json::from_value(v).expect("浮点/字符串/null 结构应可正常解析");
        let tasks = build_tasks("u1", &resp.rt);

        let t_float = tasks.iter().find(|t| t.group_id == "g-float").unwrap();
        assert!(t_float.required && t_float.passed);
        assert_eq!(t_float.min_score_pct, 1);
        assert_eq!(t_float.start_time, 1788710400);
        assert_eq!(t_float.end_time, 1791648000);

        let t_string = tasks.iter().find(|t| t.group_id == "g-string").unwrap();
        assert!(t_string.required && t_string.passed);
        assert_eq!(
            (
                t_string.min_score_pct,
                t_string.start_time,
                t_string.end_time
            ),
            (60, 1788710400, 1791647999)
        );

        let t_null = tasks
            .iter()
            .find(|t| t.group_id == "g-null-struct")
            .unwrap();
        assert!(!t_null.required && !t_null.passed);
        assert_eq!(
            (t_null.min_score_pct, t_null.start_time, t_null.end_time),
            (0, 0, 0)
        );

        let cv = serde_json::json!({
            "rt": {
                "units": {
                    "u1": {"strategies": null},
                    "u2": {"strategies": {"min_score_pct": 0.75, "start_time": null}}
                },
                "publish_version": 139753
            }
        });
        let c: CourseProgressResponse =
            serde_json::from_value(cv).expect("课程级 null/浮点/数字版本号应可解析");

        assert_eq!(c.rt.publish_version, "139753");
        assert_eq!(c.rt.units.len(), 2);
    }

    #[test]
    fn parse_home_courses_extracts_fields() {
        let v = serde_json::json!({
            "code": 1,
            "value": {
                "courseList": [
                    {
                        "id": 369622,
                        "name": "教材课程",
                        "classId": "1840882666005127243",
                        "courseResourceList": [
                            {"instanceId": "course-v2:a+nhce_v4_rw_3+20230116", "name": "新视野3"},
                            {"instanceId": "course-v2:b+nhce_v4_rw_2+20230116", "name": ""}
                        ]
                    },
                    {
                        "id": "888",
                        "classId": "999",
                        "courseResourceList": [
                            {"instanceId": "course-v2:c+x+1", "name": ""}
                        ]
                    }
                ]
            }
        });
        let list = parse_home_courses(&v);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "新视野3");
        assert_eq!(list[0].course_id, "course-v2:a+nhce_v4_rw_3+20230116");
        assert_eq!(list[0].class_id, "1840882666005127243");
        assert_eq!(list[0].curricula_id, "369622");

        assert_eq!(list[1].name, "教材课程");
        assert_eq!(list[2].name, "");
        assert_eq!(list[2].class_id, "999");
        assert_eq!(list[2].curricula_id, "888");

        assert!(parse_home_courses(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn parse_bookshelf_courses_orders_and_dedupes() {
        let v = serde_json::json!({
            "code": 1,
            "value": {
                "groups": {
                    "CLASS_COURSE": [{
                        "resourceId": "course-v2:Unipus+nhce_v4_rw_3+20230116",
                        "instanceId": "course-v2:75b7546e9012b72+nhce_v4_rw_3+20230116",
                        "name": "新视野3(班级)",
                        "activation": 0,
                        "classCourses": [{"courseId": 369619, "classId": 1840882666005127252i64}]
                    }],
                    "PERSONAL": [
                        {"instanceId": "course-v2:75b7546e9002b72+nhce_v4_rw_1+20230116", "name": "新视野1"},
                        {"instanceId": "course-v2:75b7546ea002b72+nhce_v4_rw_2+20230116", "name": "新视野2"}
                    ],
                    "EXPIRED": [
                        {"instanceId": "course-v2:old+nhce_v4_rw_0+20230116", "name": "过期课"}
                    ]
                }
            }
        });
        let list = parse_bookshelf_courses(&v);
        assert_eq!(list.len(), 3, "过期课程应跳过");
        assert_eq!(list[0].group_label, "班级课程");
        assert_eq!(list[0].class_id, "1840882666005127252");
        assert_eq!(list[0].curricula_id, "369619");
        assert_eq!(list[1].group_label, "个人学习");
        assert!(list[1].class_id.is_empty());
        assert!(!list.iter().any(|c| c.name == "过期课"));

        let v2 = serde_json::json!({
            "value": {"groups": {
                "CLASS_COURSE": [{"instanceId": "course-v2:x", "name": "班级", "classCourses": {"classId": "c1", "courseId": "k1"}}],
                "PERSONAL": [{"instanceId": "course-v2:x", "name": "个人"}]
            }}
        });
        let list2 = parse_bookshelf_courses(&v2);
        assert_eq!(list2.len(), 1);
        assert_eq!(list2[0].group_label, "班级课程");
        assert_eq!(list2[0].name, "班级");

        assert!(parse_bookshelf_courses(&serde_json::json!({})).is_empty());
    }
}
