use std::fmt::{Display, Formatter};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),

    #[error(transparent)]
    TomlDe(#[from] toml::de::Error),

    #[error(transparent)]
    Url(#[from] url::ParseError),

    #[error("{0}")]
    Auth(String),

    #[error("{0}")]
    Config(String),

    #[error("{0}")]
    Jira(JiraApiError),

    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone)]
pub struct JiraApiError {
    pub status: u16,
    pub messages: Vec<String>,
    pub field_errors: Vec<(String, String)>,
}

impl Display for JiraApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if !self.messages.is_empty() {
            write!(f, "{}", self.messages.join("; "))?;
        } else if !self.field_errors.is_empty() {
            let rendered: Vec<String> = self
                .field_errors
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect();
            write!(f, "{}", rendered.join("; "))?;
        } else {
            write!(f, "Jira API error ({})", self.status)?;
        }
        Ok(())
    }
}

impl From<JiraApiError> for Error {
    fn from(value: JiraApiError) -> Self {
        Self::Jira(value)
    }
}

impl Error {
    pub fn auth(msg: impl Into<String>) -> Self {
        Self::Auth(msg.into())
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    pub fn message(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}
