use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no API key configured")]
    MissingApiKey,
    #[error("could not reach Hackatime: {0}")]
    Http(reqwest::Error),
    #[error("server returned {0}")]
    Status(u16),
    #[error("That doesn't look like a Hackatime key")]
    InvalidKey,
    #[error("config error: {0}")]
    Config(String),
}

/// Drops the request URL from the error, so the server address (and anything in it) never reaches the UI.
impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Http(e.without_url())
    }
}

impl Error {
    /// Stable code the UI switches on, so it can tell "needs a key" from "try again".
    pub fn code(&self) -> &'static str {
        match self {
            Error::MissingApiKey => "no_key",
            Error::Status(401 | 403) => "unauthorized",
            Error::Http(_) => "network",
            Error::Status(_) => "server",
            Error::InvalidKey => "invalid_key",
            Error::Config(_) => "config",
        }
    }
}

// Tauri commands need errors serialised to the frontend; send `{ code, message }`.
impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("Error", 2)?;
        st.serialize_field("code", self.code())?;
        st.serialize_field("message", &self.to_string())?;
        st.end()
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_separate_auth_from_outages() {
        assert_eq!(Error::MissingApiKey.code(), "no_key");
        assert_eq!(Error::Status(401).code(), "unauthorized");
        assert_eq!(Error::Status(403).code(), "unauthorized");
        assert_eq!(Error::Status(500).code(), "server");
    }

    #[test]
    fn serialises_code_and_message() {
        let v = serde_json::to_value(Error::Status(401)).unwrap();
        assert_eq!(v["code"], "unauthorized");
        assert_eq!(v["message"], "server returned 401");
    }
}
