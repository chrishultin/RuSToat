//! Finding a Stoat instance's API from what the user typed.
//!
//! People type a web address (`stoat.example.com`), not necessarily the API's.
//! The official instance serves its API on its own host (`api.stoat.chat`),
//! while self-hosted ones usually serve it under `/api` on the web app's host.
//! So: add `https://` if there's no scheme, try `GET {url}/`, then
//! `GET {url}/api`, and use whichever answers with Stoat's root config.

use super::{ApiError, RootConfig, StoatClient};

/// API versions this client is built for (the pinned `revolt-*` crates).
pub const SUPPORTED_VERSION_PREFIX: &str = "0.16.";

#[derive(Debug, thiserror::Error)]
pub enum InstanceError {
    #[error("Enter the address of a Stoat server, like stoat.example.com.")]
    Invalid,
    #[error("Couldn't reach {url}. Check the address and your connection.")]
    Unreachable {
        url: String,
        #[source]
        source: ApiError,
    },
    #[error(
        "There's no Stoat server at {url}. If its API is at another address, enter that instead."
    )]
    NotStoat { url: String },
}

/// Tidy up a typed address: `https://` added when there's no scheme (`http`
/// only when typed), host lowercased, query, fragment and trailing slash
/// removed.
pub fn normalize_instance(input: &str) -> Result<String, InstanceError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(InstanceError::Invalid);
    }
    let with_scheme = if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let mut url = url::Url::parse(&with_scheme).map_err(|_| InstanceError::Invalid)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return Err(InstanceError::Invalid);
    }
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// Find the API for `input` (see the module docs). Returns a client for it,
/// without a token, and its root config.
///
/// Must run on the Tokio IO runtime.
pub async fn resolve_instance(input: &str) -> Result<(StoatClient, RootConfig), InstanceError> {
    let base = normalize_instance(input)?;
    let client = StoatClient::new(base.clone());
    match client.root_config().await {
        Ok(root) => return Ok(checked(client, root)),
        // Nothing answered at all, so `/api` won't either.
        Err(source @ ApiError::Network(_)) => {
            return Err(InstanceError::Unreachable { url: base, source });
        }
        Err(err) => log::debug!("instance: no Stoat API at {base}/ ({err})"),
    }
    if !base.ends_with("/api") {
        let client = StoatClient::new(format!("{base}/api"));
        match client.root_config().await {
            Ok(root) => return Ok(checked(client, root)),
            Err(err) => log::debug!("instance: no Stoat API at {base}/api/ ({err})"),
        }
    }
    Err(InstanceError::NotStoat { url: base })
}

/// Log a warning for an API version the pinned crates may not parse well.
fn checked(client: StoatClient, root: RootConfig) -> (StoatClient, RootConfig) {
    if root.version.starts_with(SUPPORTED_VERSION_PREFIX) {
        log::info!("instance: {} (API {})", client.base_url(), root.version);
    } else {
        log::warn!(
            "instance: {} runs API {}, but this client is built for {SUPPORTED_VERSION_PREFIX}x; \
             some things may not work",
            client.base_url(),
            root.version
        );
    }
    (client, root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalized(input: &str) -> String {
        normalize_instance(input).unwrap()
    }

    #[test]
    fn adds_https_when_there_is_no_scheme() {
        assert_eq!(normalized("stoat.example.com"), "https://stoat.example.com");
        assert_eq!(normalized("  api.stoat.chat/ "), "https://api.stoat.chat");
    }

    #[test]
    fn keeps_an_explicit_http_and_port() {
        assert_eq!(
            normalized("http://localhost:14702/"),
            "http://localhost:14702"
        );
    }

    #[test]
    fn keeps_the_path_and_drops_the_rest() {
        assert_eq!(
            normalized("HTTPS://Stoat.Example.com/api/?x=1#top"),
            "https://stoat.example.com/api"
        );
    }

    #[test]
    fn rejects_junk() {
        for input in [
            "",
            "   ",
            "ftp://example.com",
            "https://",
            "http://exa mple.com",
        ] {
            assert!(
                matches!(normalize_instance(input), Err(InstanceError::Invalid)),
                "{input:?} should be rejected"
            );
        }
    }
}
