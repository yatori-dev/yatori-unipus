use super::*;
use crate::config::AuthConfig;

fn credentials(account: &str, refresh_token: &str) -> Credentials {
    Credentials {
        jwt: format!("access-{account}"),
        open_id: account.into(),
        refresh_token: refresh_token.into(),
        jwt_expire: 1_900_000_000,
        rt_expire: 2_000_000_000,
    }
}

#[test]
fn cloned_session_updates_merge_fields_without_losing_credentials() {
    let mut session = Session::from_config(Config::default()).unwrap();
    let clone = session.clone();
    let baseline = session.cfg();
    let mut profile = baseline.clone();
    profile.auth.u_school = "school-1".into();
    clone
        .update_credentials(&credentials("account-1", "latest-refresh"), 0)
        .unwrap();
    session.merge_profile(&baseline, &profile, 0).unwrap();
    session.set_publish_version("version-1").unwrap();
    let config = clone.cfg();
    assert_eq!(config.auth.refresh_token, "latest-refresh");
    assert_eq!(config.auth.u_school, "school-1");
    assert_eq!(config.course.publish_version, "version-1");
    assert!(clone.is_authenticated());
    assert!(session.config_path.is_none());
}

#[test]
fn changing_account_discards_inherited_identity_and_tokens() {
    let mut config = Config::default();
    config.auth.username = "first-user".into();
    config.auth.password = "first-password".into();
    let mut session = Session::from_config(config).unwrap();
    session
        .update_credentials(&credentials("first-account", "first-refresh"), 0)
        .unwrap();
    let mut config = session.cfg();
    config.auth.username = "second-user".into();
    session.update_config(config).unwrap();
    let config = session.cfg();
    assert!(!session.is_authenticated());
    assert!(config.auth.open_id.is_empty());
    assert!(config.auth.refresh_token.is_empty());
    assert!(config.auth.x_annotator_auth_token.is_empty());
}

#[test]
fn rotating_refresh_token_preserves_current_jwt_and_course_cache() {
    let mut session = Session::from_config(Config::default()).unwrap();
    session
        .update_credentials(&credentials("account-1", "first-refresh"), 0)
        .unwrap();
    let scope = session.cache_scope("course-1");
    session.cache_course_name(&scope, "course-name");
    let mut config = session.cfg();
    config.auth.refresh_token = "rotated-refresh".into();
    session.update_config(config).unwrap();
    assert!(session.is_authenticated());
    assert_eq!(session.open_id(), "account-1");
    assert_eq!(
        session.cached_course_name(&scope).as_deref(),
        Some("course-name")
    );
    assert_eq!(session.cfg().auth.refresh_token, "rotated-refresh");
}

#[test]
fn changing_open_id_discards_current_jwt() {
    let mut session = Session::from_config(Config::default()).unwrap();
    session
        .update_credentials(&credentials("account-1", "first-refresh"), 0)
        .unwrap();
    let mut config = session.cfg();
    config.auth.refresh_token = "new-refresh".into();
    config.auth.open_id = "account-2".into();
    session.update_config(config).unwrap();
    assert_eq!(session.open_id(), "account-2");
    assert_eq!(session.cfg().auth.refresh_token, "new-refresh");
    assert!(!session.is_authenticated());
}

#[test]
fn responses_for_previous_account_cannot_overwrite_new_account() {
    let mut session = Session::from_config(Config::default()).unwrap();
    let mut config = session.cfg();
    config.auth = AuthConfig {
        username: "new-user".into(),
        password: "new-password".into(),
        ..AuthConfig::default()
    };
    session.update_config(config).unwrap();
    assert!(
        session
            .update_credentials(&credentials("old-account", "old-refresh"), 0)
            .is_err()
    );
    assert_eq!(session.cfg().auth.username, "new-user");
    assert!(session.cfg().auth.refresh_token.is_empty());
}

#[test]
fn metadata_caches_are_isolated_between_sessions_accounts_and_courses() {
    let mut first = Session::from_config(Config::default()).unwrap();
    let second = Session::from_config(Config::default()).unwrap();
    let scope = first.cache_scope("course-1");
    first.cache_course_name(&scope, "first-course");
    first.cache_unit_label(&scope, "unit-1", "first-unit");
    assert_eq!(
        first.cached_course_name(&scope).as_deref(),
        Some("first-course")
    );
    assert!(
        second
            .cached_course_name(&second.cache_scope("course-1"))
            .is_none()
    );
    assert!(
        first
            .cached_unit_label(&first.cache_scope("course-2"), "unit-1")
            .is_none()
    );
    let mut config = first.cfg();
    config.auth.username = "new-user".into();
    first.update_config(config).unwrap();
    first.cache_unit_label(&scope, "unit-1", "stale-unit");
    assert!(
        first
            .cached_unit_label(&first.cache_scope("course-1"), "unit-1")
            .is_none()
    );
}

#[tokio::test]
async fn authentication_rejects_stale_jwt_without_refresh_credentials() {
    let session = Session::from_config(Config::default()).unwrap();
    session
        .update_credentials(&credentials("account-1", ""), 0)
        .unwrap();
    assert!(crate::api::login::ensure_login(&session).await.is_err());
}

#[tokio::test]
async fn rejected_request_uses_credentials_already_refreshed_by_another_clone() {
    let session = Session::from_config(Config::default()).unwrap();
    session
        .update_credentials(&credentials("account-1", "latest-refresh"), 0)
        .unwrap();
    // This path must return before any SSO request: another clone already changed the JWT.
    assert!(
        session
            .refresh_rejected_auth("previous-access")
            .await
            .unwrap()
    );
    assert_eq!(session.cfg().auth.refresh_token, "latest-refresh");
}

#[test]
fn persistence_failure_does_not_publish_partial_state() {
    let path = std::env::temp_dir().join(format!(
        "yatori-unipus-nonexistent-parent-{}/config.json",
        std::process::id()
    ));
    let session = Session::new(Config::default(), path).unwrap();
    assert!(
        session
            .update_credentials(&credentials("account-1", "refresh-1"), 0)
            .is_err()
    );
    assert!(!session.is_authenticated());
    assert!(session.cfg().auth.refresh_token.is_empty());
}

#[test]
fn file_session_persists_refresh_token_without_access_token() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "yatori-unipus-auth-{}-{nonce}.json",
        std::process::id()
    ));
    let session = Session::new(Config::default(), path.clone()).unwrap();
    session
        .update_credentials(&credentials("account-1", "refresh-1"), 0)
        .unwrap();
    let persisted = Config::load(&path).unwrap();
    assert_eq!(persisted.auth.refresh_token, "refresh-1");
    assert_eq!(persisted.auth.open_id, "account-1");
    let value = serde_json::to_value(persisted).unwrap();
    assert!(value.get("cookie").is_none());
    assert!(value.get("authorization").is_none());
    std::fs::remove_file(path).unwrap();
}
