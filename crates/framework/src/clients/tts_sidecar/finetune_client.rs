//! `VoiceFinetuner` `HTTP` implementation (`POST /finetune` and `GET /finetune/{job_id}`) for [`HttpTtsSidecarClient`].

use std::time::Duration;

use domain::ports::{ExternalPortError, FinetuneJobProgress, FinetuneStartRequest, VoiceFinetuner};

use super::{HttpTtsSidecarClient, MAX_ATTEMPTS, RETRY_BACKOFF_MS, is_transient_status};

#[async_trait::async_trait]
impl VoiceFinetuner for HttpTtsSidecarClient {
    async fn start_finetune(
        &self,
        req: &FinetuneStartRequest,
    ) -> Result<FinetuneJobProgress, ExternalPortError> {
        let url = format!("{}/finetune", self.base_url);
        let mut last_err = String::from("TTS finetune start exhausted retries");

        for (attempt, &backoff_ms) in RETRY_BACKOFF_MS.iter().enumerate() {
            let send_res = self.http.post(&url).json(req).send().await;
            match send_res {
                Ok(resp) if resp.status().is_success() => {
                    return resp.json::<FinetuneJobProgress>().await.map_err(|err| {
                        ExternalPortError::Tts(format!(
                            "invalid finetune start JSON response: {err}"
                        ))
                    });
                }
                Ok(resp) if is_transient_status(resp.status()) && attempt + 1 < MAX_ATTEMPTS => {
                    last_err = format!("transient HTTP {} from TTS finetune start", resp.status());
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ExternalPortError::Tts(format!(
                        "TTS finetune start HTTP {status}: {body}"
                    )));
                }
                Err(err)
                    if (err.is_timeout() || err.is_connect()) && attempt + 1 < MAX_ATTEMPTS =>
                {
                    last_err = format!("transient TTS finetune start transport error: {err}");
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => {
                    return Err(ExternalPortError::Tts(format!(
                        "TTS finetune start request error: {err}"
                    )));
                }
            }
        }

        Err(ExternalPortError::Tts(last_err))
    }

    async fn poll_finetune(&self, job_id: &str) -> Result<FinetuneJobProgress, ExternalPortError> {
        let url = format!("{}/finetune/{job_id}", self.base_url);
        let mut last_err = String::from("TTS finetune poll exhausted retries");

        for (attempt, &backoff_ms) in RETRY_BACKOFF_MS.iter().enumerate() {
            let send_res = self.http.get(&url).send().await;
            match send_res {
                Ok(resp) if resp.status().is_success() => {
                    return resp.json::<FinetuneJobProgress>().await.map_err(|err| {
                        ExternalPortError::Tts(format!(
                            "invalid finetune poll JSON response: {err}"
                        ))
                    });
                }
                Ok(resp) if is_transient_status(resp.status()) && attempt + 1 < MAX_ATTEMPTS => {
                    last_err = format!("transient HTTP {} from TTS finetune poll", resp.status());
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ExternalPortError::Tts(format!(
                        "TTS finetune poll HTTP {status}: {body}"
                    )));
                }
                Err(err)
                    if (err.is_timeout() || err.is_connect()) && attempt + 1 < MAX_ATTEMPTS =>
                {
                    last_err = format!("transient TTS finetune poll transport error: {err}");
                    tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                }
                Err(err) => {
                    return Err(ExternalPortError::Tts(format!(
                        "TTS finetune poll request error: {err}"
                    )));
                }
            }
        }

        Err(ExternalPortError::Tts(last_err))
    }
}
