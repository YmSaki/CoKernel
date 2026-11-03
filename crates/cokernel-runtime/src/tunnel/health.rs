//! Narrow, bounded adapter for the pinned tunnel-client v0.0.14 health surface.
use std::{io, path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    time::{Instant, timeout},
};

#[derive(Default)]
pub(super) struct PollFreshness {
    timestamp: Option<u64>,
    observed: Option<Instant>,
}
pub(super) struct Observation {
    pub ready: bool,
    pub last_poll: u64,
}
impl PollFreshness {
    pub fn connected(&mut self, observation: Observation, now: Instant) -> bool {
        // Monotonic observation time bounds staleness, even if wall time moves.
        if observation.last_poll > 0 && self.timestamp != Some(observation.last_poll) {
            self.timestamp = Some(observation.last_poll);
            self.observed = Some(now);
        }
        observation.ready
            && observation.last_poll > 0
            && self
                .observed
                .is_some_and(|t| now.duration_since(t) <= Duration::from_secs(45))
    }
}
pub(super) async fn probe(socket: &Path) -> io::Result<Observation> {
    timeout(Duration::from_secs(1), async {
        let ready = get(socket, "/readyz", 8192).await?;
        // Only exact successful startup, not upstream's auth/timeout fallbacks.
        let ready = ready.0 == 200 && ready.1 == b"ready";
        let metrics = get(socket, "/metrics", 256 * 1024).await?;
        if metrics.0 != 200 {
            return Err(invalid());
        }
        let last_poll = parse_metrics(&metrics.1)?;
        Ok(Observation { ready, last_poll })
    })
    .await
    .map_err(|_| invalid())?
}
fn invalid() -> io::Error {
    io::Error::other("tunnel health unavailable or malformed")
}
async fn get(socket: &Path, path: &str, limit: usize) -> io::Result<(u16, Vec<u8>)> {
    let mut stream = UnixStream::connect(socket).await?;
    // HTTP/1.0 deliberately avoids chunked framing. No redirects or URLs from replies.
    stream
        .write_all(
            format!("GET {path} HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await?;
    let mut data = Vec::new();
    stream
        .take((limit + 1) as u64)
        .read_to_end(&mut data)
        .await?;
    if data.len() > limit {
        return Err(invalid());
    }
    let header_end = data
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .filter(|n| *n <= 4096)
        .ok_or_else(invalid)?;
    let headers = std::str::from_utf8(&data[..header_end]).map_err(|_| invalid())?;
    let mut lines = headers.split("\r\n");
    let mut status = lines.next().ok_or_else(invalid)?.split(' ');
    if !matches!(status.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
        return Err(invalid());
    }
    let code = status
        .next()
        .ok_or_else(invalid)?
        .parse()
        .map_err(|_| invalid())?;
    let body = &data[header_end + 4..];
    let mut content_length = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or_else(invalid)?;
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(invalid());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(invalid());
            }
            content_length = Some(value.trim().parse::<usize>().map_err(|_| invalid())?);
        }
    }
    if content_length.is_some_and(|n| n != body.len()) {
        return Err(invalid());
    }
    Ok((code, body.to_vec()))
}
fn parse_metrics(bytes: &[u8]) -> io::Result<u64> {
    const NAME: &str = "commands_poll_last_successful_timestamp_seconds";
    let mut result = None;
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    for line in text.lines() {
        let Some(rest) = line.strip_prefix(NAME) else {
            continue;
        };
        let rest = if rest.starts_with('{') {
            rest.split_once('}').ok_or_else(invalid)?.1
        } else if rest.starts_with(' ') {
            rest
        } else {
            continue;
        };
        let value: f64 = rest.trim().parse().map_err(|_| invalid())?;
        if result.is_some()
            || !value.is_finite()
            || !(0.0..=253402300799.0).contains(&value)
            || value.fract() != 0.0
        {
            return Err(invalid());
        }
        result = Some(value as u64);
    }
    result.ok_or_else(invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metric_contract_and_monotonic_staleness() {
        assert_eq!(parse_metrics(b"# metadata\ncommands_poll_last_successful_timestamp_seconds{otel_scope_name=\"controlplane\"} 1.79004749e+09\n").unwrap(),1790047490);
        for value in [
            "NaN",
            "inf",
            "-1",
            "1.5",
            "99999999999999999",
            "1\ncommands_poll_last_successful_timestamp_seconds 2",
        ] {
            assert!(
                parse_metrics(
                    format!("commands_poll_last_successful_timestamp_seconds {value}").as_bytes()
                )
                .is_err()
            );
        }
        assert!(parse_metrics(b"commands_poll_last_successful_timestamp_seconds_fake 1").is_err());
        let mut state = PollFreshness::default();
        let now = Instant::now();
        assert!(!state.connected(
            Observation {
                ready: true,
                last_poll: 0
            },
            now
        ));
        assert!(!state.connected(
            Observation {
                ready: false,
                last_poll: 1
            },
            now
        ));
        assert!(state.connected(
            Observation {
                ready: true,
                last_poll: 1
            },
            now
        ));
        assert!(!state.connected(
            Observation {
                ready: true,
                last_poll: 1
            },
            now + Duration::from_secs(46)
        ));
        assert!(state.connected(
            Observation {
                ready: true,
                last_poll: 2
            },
            now + Duration::from_secs(47)
        ));
    }
}
