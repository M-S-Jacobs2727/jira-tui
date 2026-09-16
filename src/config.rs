use std::fs;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::jira::search::{AssigneeFilter, SortDir, SortField};

const QUALIFIER: &str = "dev";
const ORGANIZATION: &str = "jira-tui";
const APPLICATION: &str = "jira-tui";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub cloud_id: Option<String>,
    #[serde(default)]
    pub board_id: Option<i64>,
    #[serde(default)]
    pub project_key: Option<String>,
    #[serde(default)]
    pub story_points_field: Option<String>,
    #[serde(default = "default_columns")]
    pub columns: Vec<String>,
    #[serde(default)]
    pub view: ViewConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewConfig {
    #[serde(default = "default_sort_field")]
    pub sort_field: SortField,
    #[serde(default)]
    pub sort_dir: SortDir,
    #[serde(default)]
    pub filter_statuses: Vec<String>,
    #[serde(default)]
    pub filter_types: Vec<String>,
    #[serde(default)]
    pub filter_assignee: AssigneeFilter,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            sort_field: default_sort_field(),
            sort_dir: SortDir::default(),
            filter_statuses: Vec::new(),
            filter_types: Vec::new(),
            filter_assignee: AssigneeFilter::default(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            cloud_id: None,
            board_id: None,
            project_key: None,
            story_points_field: None,
            columns: default_columns(),
            view: ViewConfig::default(),
        }
    }
}

fn default_columns() -> Vec<String> {
    vec![
        "key".into(),
        "issuetype".into(),
        "priority".into(),
        "status".into(),
        "assignee".into(),
        "story_points".into(),
        "summary".into(),
    ]
}

fn default_sort_field() -> SortField {
    SortField::Priority
}

impl Config {
    pub fn project_dirs() -> Result<ProjectDirs> {
        ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .ok_or_else(|| Error::config("could not determine XDG project directories"))
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::project_dirs()?.config_dir().join("config.toml"))
    }

    pub fn credentials_path() -> Result<PathBuf> {
        Ok(Self::project_dirs()?.config_dir().join("credentials.toml"))
    }

    pub fn log_dir() -> Result<PathBuf> {
        let dirs = Self::project_dirs()?;
        Ok(dirs
            .state_dir()
            .unwrap_or_else(|| dirs.data_local_dir())
            .to_path_buf())
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)?;
        Ok(toml::from_str(&raw)?)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        fs::write(path, raw)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trips() {
        let cfg = Config::default();
        let encoded = toml::to_string(&cfg).unwrap();
        let decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.view.sort_field, SortField::Priority);
        assert_eq!(decoded.view.sort_dir, SortDir::Desc);
        assert_eq!(decoded.columns, default_columns());
        assert_eq!(decoded.view.filter_assignee, AssigneeFilter::Any);
    }

    #[test]
    fn view_persistence_fields() {
        let mut cfg = Config::default();
        cfg.view.sort_field = SortField::Status;
        cfg.view.sort_dir = SortDir::Asc;
        cfg.view.filter_statuses = vec!["In Progress".into()];
        cfg.view.filter_types = vec!["Bug".into()];
        cfg.view.filter_assignee = AssigneeFilter::Me;
        let encoded = toml::to_string(&cfg).unwrap();
        let decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.view.sort_field, SortField::Status);
        assert_eq!(decoded.view.sort_dir, SortDir::Asc);
        assert_eq!(decoded.view.filter_statuses, vec!["In Progress"]);
        assert_eq!(decoded.view.filter_types, vec!["Bug"]);
        assert_eq!(decoded.view.filter_assignee, AssigneeFilter::Me);
    }
}
