use std::path::Path;

use super::check::http_client;

pub async fn download_file(url: &str, destination: &Path) -> Result<(), String> {
    if !url.starts_with("https://")
        && !url.starts_with("http://127.0.0.1")
        && !url.starts_with("http://localhost")
    {
        return Err("refusing non-https download url".to_string());
    }

    let client = http_client()?;
    let mut attempts = 0;
    let max_attempts = 3;

    loop {
        attempts += 1;
        match client.get(url).send().await {
            Ok(resp) => {
                if !resp.status().is_success() {
                    let status = resp.status();
                    if attempts < max_attempts
                        && (status.is_server_error()
                            || status == reqwest::StatusCode::REQUEST_TIMEOUT)
                    {
                        tokio::time::sleep(std::time::Duration::from_millis(500 * attempts as u64))
                            .await;
                        continue;
                    }
                    return Err(format!("download failed with status {status}"));
                }

                use futures::StreamExt;
                use tokio::io::AsyncWriteExt;

                let mut file = tokio::fs::File::create(destination)
                    .await
                    .map_err(|e| format!("failed to create temp download file: {e}"))?;

                let mut stream = resp.bytes_stream();
                let mut write_err = None;
                while let Some(chunk_res) = stream.next().await {
                    match chunk_res {
                        Ok(chunk) => {
                            if let Err(e) = file.write_all(&chunk).await {
                                write_err = Some(format!("failed to write download data: {e}"));
                                break;
                            }
                        }
                        Err(e) => {
                            write_err = Some(format!("download chunk stream error: {e}"));
                            break;
                        }
                    }
                }
                if let Some(err) = write_err {
                    let _ = tokio::fs::remove_file(destination).await;
                    return Err(err);
                }
                if let Err(e) = file.flush().await {
                    let _ = tokio::fs::remove_file(destination).await;
                    return Err(format!("failed to flush download file: {e}"));
                }
                return Ok(());
            }
            Err(e) => {
                if attempts < max_attempts {
                    tokio::time::sleep(std::time::Duration::from_millis(500 * attempts as u64))
                        .await;
                    continue;
                }
                return Err(format!("download network error: {e}"));
            }
        }
    }
}

pub async fn download_text(url: &str) -> Result<String, String> {
    if !url.starts_with("https://")
        && !url.starts_with("http://127.0.0.1")
        && !url.starts_with("http://localhost")
    {
        return Err("refusing non-https download url".to_string());
    }

    let client = http_client()?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("failed to download text: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "failed to download text, status: {}",
            resp.status()
        ));
    }

    resp.text()
        .await
        .map_err(|e| format!("failed to decode text response: {e}"))
}
