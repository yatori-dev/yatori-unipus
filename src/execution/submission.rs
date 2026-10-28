use crate::api::session::Session;
use crate::api::submit::{RateLimited, submit_raw};
use crate::reporter::{ReportEvent, Reporter};
use anyhow::{Result, bail};
use tokio_util::sync::CancellationToken;

pub(super) async fn submit_with_rate_retry(
    session: &Session,
    payload: &str,
    reporter: &dyn Reporter,
    cancel: &CancellationToken,
) -> Result<()> {
    const MAX_RETRIES: u32 = 5;
    const COOLDOWN_SECS: u64 = 180;
    let mut attempt = 0u32;
    loop {
        if cancel.is_cancelled() {
            bail!("已取消");
        }
        match submit_raw(session, payload).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                if !e.is::<RateLimited>() {
                    return Err(e);
                }
                attempt += 1;
                if attempt >= MAX_RETRIES {
                    return Err(e);
                }
                let secs = COOLDOWN_SECS * (attempt as u64);
                reporter.report(ReportEvent::RateLimited {
                    attempt,
                    max: MAX_RETRIES,
                    wait_secs: secs,
                    message: e.to_string(),
                });
                tokio::select! {
                    _ = cancel.cancelled() => bail!("已取消"),
                    _ = tokio::time::sleep(std::time::Duration::from_secs(secs)) => {}
                }
            }
        }
    }
}
