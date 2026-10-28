mod annotator;
mod profile;
mod sso;
mod token;

pub(crate) use annotator::{annotator_token_ok, ensure_annotator_token, renew_annotator};
pub(crate) use profile::ensure_profile;
pub(crate) use sso::{Credentials, login, refresh};
pub(crate) use token::{jwt_fresh_enough, jwt_open_id, jwt_username};

use crate::api::session::Session;
use anyhow::{Result, bail};

pub(crate) async fn ensure_login(session: &Session) -> Result<bool> {
    let changed = session.refresh_auth().await?;
    if !session.is_authenticated() || (!changed && !jwt_fresh_enough(&session.jwt())) {
        bail!("登录凭证已失效且无法自动登录：请向宿主配置账号密码或有效 refresh_token");
    }
    Ok(changed)
}

pub(crate) fn ensure_course(session: &Session) -> Result<()> {
    if session.course_id().is_empty() {
        bail!("未选择课程：请先查询课程列表并在配置中设置 course_id");
    }
    if session.open_id().is_empty() {
        bail!("open_id 为空：请提供账号密码以自动登录，或在配置中设置 open_id");
    }
    Ok(())
}

pub(super) fn account_switched(cfg: &crate::config::AuthConfig, jwt: &str) -> bool {
    jwt_username(jwt).is_some_and(|username| !cfg.username.is_empty() && username != cfg.username)
}

#[cfg(test)]
mod tests {
    use super::annotator::{
        AnnotatorParams, build_annotator_token, extract_script_srcs, generate_annotator_token,
        parse_annotator_params,
    };
    use super::sso::{encrypt_field, parse_credentials};
    use super::token::jwt_exp;
    use super::*;
    use crate::config::AuthConfig;

    #[test]
    fn encrypt_matches_frontend_vector() {
        assert_eq!(
            encrypt_field("testuser"),
            "0BEE28AAD749A902C115ABE749B97F61"
        );
    }

    #[test]
    fn annotator_token_matches_captured_vector() {
        let params = AnnotatorParams::from_config(&AuthConfig::default());

        let token =
            build_annotator_token("ad01959edee04ce89baebd12f5d3236e", 1823157201685, &params);
        assert_eq!(
            token,
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
             eyJvcGVuX2lkIjoiYWQwMTk1OWVkZWUwNGNlODliYWViZDEyZjVkMzIzNmUiLCJuYW1lIjoiIiwiZW1haWwiOiIiLCJhZG1pbmlzdHJhdG9yIjpmYWxzZSwiZXhwIjoxODIzMTU3MjAxNjg1LCJpc3MiOiJjNGY3NzIwNjNkY2ZhOThlOWM1MCIsImF1ZCI6ImVkeC51bmlwdXMuY24ifQ.\
             oIZR0OePtxO5XArHD6z16KIbbNJqpXUx2S32tsWNcqY"
                .replace(' ', "")
        );

        let fresh = generate_annotator_token("ad01959edee04ce89baebd12f5d3236e", &params);
        assert!(annotator_token_ok(&fresh));
        assert!(!annotator_token_ok("bad.token.here"));

        let mut cfg = AuthConfig::default();
        assert!(!ensure_annotator_token(&mut cfg));
        cfg.open_id = "u1".into();
        assert!(ensure_annotator_token(&mut cfg));
        assert!(annotator_token_ok(&cfg.x_annotator_auth_token));

        let kept = cfg.x_annotator_auth_token.clone();
        assert!(!ensure_annotator_token(&mut cfg));
        assert_eq!(cfg.x_annotator_auth_token, kept);
    }

    #[test]
    fn parse_annotator_params_from_bundle_fixture() {
        let fixture = r#"
        }],[{key:"_generateJwtToken",value:function(e){var t={open_id:e,name:"",email:"",administrator:!1,exp:Date.now()+31536e6,iss:"c4f772063dcfa98e9c50",aud:"edx.unipus.cn"},n=Qt()(t,"a824b379f126b8b7aa5e33dee83fb0a05aa7462c");return console.log("create token: ",n),n}}])}();
        "#;
        let p = parse_annotator_params(fixture).expect("应能解析");
        assert_eq!(p.iss, "c4f772063dcfa98e9c50");
        assert_eq!(p.aud, "edx.unipus.cn");
        assert_eq!(p.key, "a824b379f126b8b7aa5e33dee83fb0a05aa7462c");
        assert_eq!(p.ttl_ms, 31_536_000_000);
        assert!(p.is_builtin());

        let variant = r#"
        {key:"_generateJwtToken",value:function(x){var z={email:"",open_id:x,name:"",administrator:!1,exp:Date.now()+2592e6,iss:"1234567890abcdef1234",aud:"other.example.cn"},q=JJ()(z,"deadbeefdeadbeefdeadbeefdeadbeef");return q}};
        "#;
        let p = parse_annotator_params(variant).expect("变体应能解析");
        assert_eq!(p.iss, "1234567890abcdef1234");
        assert_eq!(p.aud, "other.example.cn");
        assert_eq!(p.key, "deadbeefdeadbeefdeadbeefdeadbeef");
        assert_eq!(p.ttl_ms, 2_592_000_000);
        assert!(!p.is_builtin());

        assert!(parse_annotator_params("var a=1;").is_none());
    }

    #[test]
    fn extract_script_srcs_from_html() {
        let html = r#"<script src="https://a/x.js"></script><script defer="defer" src="question-data-e421babb.js"></script><link href="a.css">"#;
        let srcs = extract_script_srcs(html);
        assert_eq!(
            srcs,
            vec![
                "https://a/x.js".to_string(),
                "question-data-e421babb.js".to_string()
            ]
        );
    }

    #[test]
    fn account_switch_detection() {
        use base64::Engine;
        let mk = |u: &str| {
            let p = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(serde_json::json!({"username": u, "openId": "o"}).to_string());
            format!("x.{}.y", p)
        };
        let cfg = AuthConfig {
            username: "111".into(),
            ..Default::default()
        };
        assert!(!account_switched(&cfg, &mk("111")));
        assert!(
            account_switched(&cfg, &mk("222")),
            "用户名与 jwt 不一致应判定为已切换"
        );
        let mut cfg = cfg;
        cfg.username.clear();
        assert!(!account_switched(&cfg, &mk("222")), "未配置用户名时不判定");
        cfg.username = "222".into();
        assert!(
            !account_switched(&cfg, "not-a-jwt"),
            "无法解析 jwt 时不判定"
        );
    }

    #[test]
    fn jwt_payload_decodes_open_id() {
        use base64::Engine;
        let payload = serde_json::json!({"openId": "abc123", "exp": 4102444800i64});
        let p = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&payload).unwrap());
        let token = format!("x.{}.y", p);
        assert_eq!(jwt_open_id(&token).as_deref(), Some("abc123"));
        assert_eq!(jwt_exp(&token), Some(4102444800));
        assert!(jwt_fresh_enough(&token));
    }

    #[test]
    fn parse_login_success_and_captcha() {
        use base64::Engine;
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({"openId": "u1", "exp": 4102444800i64})).unwrap(),
        );
        let jwt = format!("a.{}.c", payload);
        let ok = serde_json::json!({
            "code": 0,
            "rs": {
                "jwt": jwt,
                "rt": "rt-1",
                "openid": "u1",
                "rtExpire": 4102444800i64
            }
        });
        let c = parse_credentials(&ok, "登录").unwrap();
        assert_eq!(c.open_id, "u1");
        assert_eq!(c.refresh_token, "rt-1");
        assert_eq!(c.jwt_expire, 4102444800);
        assert_eq!(c.rt_expire, 4102444800);

        let captcha = serde_json::json!({"code": "1506", "msg": "need check"});
        let err = parse_credentials(&captcha, "登录").unwrap_err().to_string();
        assert!(err.contains("滑块验证"), "{}", err);
    }
}
