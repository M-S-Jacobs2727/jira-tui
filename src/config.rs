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
}

/// Per-tab filter/sort rules for the current session (not persisted).
#[derive(Debug, Clone)]
pub struct ViewConfig {
    pub sort_field: SortField,
    pub sort_dir: SortDir,
    pub filter_statuses: Vec<String>,
    pub filter_types: Vec<String>,
    pub filter_assignee: AssigneeFilter,
}

impl Default for ViewConfig {
    fn default() -> Self {
        Self {
            sort_field: SortField::Default,
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
        }
    }
}

fn default_columns() -> Vec<String> {
    vec![
        "key".into(),
        "summary".into(),
        "issuetype".into(),
        "priority".into(),
        "status".into(),
        "assignee".into(),
        "story_points".into(),
    ]
}

fn legacy_default_columns() -> Vec<String> {
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

pub(crate) fn migrate_columns(columns: &mut Vec<String>) -> bool {
    if *columns == legacy_default_columns() {
        *columns = default_columns();
        true
    } else {
        false
    }
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
        let mut cfg: Self = toml::from_str(&raw)?;
        if migrate_columns(&mut cfg.columns) {
            cfg.save()?;
        }
        Ok(cfg)
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
        assert_eq!(decoded.columns, default_columns());
        assert!(decoded.board_id.is_none());
    }

    #[test]
    fn ignores_legacy_view_section_on_load() {
        let decoded: Config = toml::from_str(
            r#"
            board_id = 1
            [view]
            sort_field = "status"
            filter_assignee = "me"
            "#,
        )
        .unwrap();
        assert_eq!(decoded.board_id, Some(1));
    }

    #[test]
    fn view_config_defaults() {
        let view = ViewConfig::default();
        assert_eq!(view.sort_field, SortField::Default);
        assert_eq!(view.sort_dir, SortDir::Desc);
        assert!(view.filter_statuses.is_empty());
        assert!(view.filter_types.is_empty());
        assert!(view.filter_assignee.is_empty());
    }

    #[test]
    fn legacy_columns_migrate() {
        let mut columns = legacy_default_columns();
        assert!(migrate_columns(&mut columns));
        assert_eq!(columns, default_columns());

        let custom = vec!["key".into(), "summary".into()];
        let mut custom_cols = custom.clone();
        assert!(!migrate_columns(&mut custom_cols));
        assert_eq!(custom_cols, custom);
    }
}
