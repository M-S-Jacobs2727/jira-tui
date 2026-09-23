use std::fs;
use std::path::PathBuf;

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::jira::search::{AssigneeFilter, SortDir, SortField};

const QUALIFIER: &str = "dev";
const ORGANIZATION: &str = "jira-tui";
const APPLICATION: &str = "jira-tui";

/// Per-board preferences remembered across sessions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoardConfig {
    pub board_id: i64,
    #[serde(default)]
    pub project_key: Option<String>,
    #[serde(default)]
    pub story_points_field: Option<String>,
    #[serde(default = "default_columns")]
    pub columns: Vec<String>,
}

impl BoardConfig {
    pub fn new(board_id: i64, project_key: Option<String>) -> Self {
        Self {
            board_id,
            project_key,
            story_points_field: None,
            columns: default_columns(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub cloud_id: Option<String>,
    /// Currently selected board.
    #[serde(default)]
    pub board_id: Option<i64>,
    /// Boards the user has loaded, with per-board preferences.
    #[serde(default)]
    pub boards: Vec<BoardConfig>,
}

/// Wire format that also accepts the pre-multi-board flat fields.
#[derive(Debug, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    client_id: String,
    #[serde(default)]
    cloud_id: Option<String>,
    #[serde(default)]
    board_id: Option<i64>,
    #[serde(default)]
    boards: Vec<BoardConfig>,
    #[serde(default)]
    project_key: Option<String>,
    #[serde(default)]
    story_points_field: Option<String>,
    #[serde(default)]
    columns: Option<Vec<String>>,
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
            boards: Vec::new(),
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

    pub fn active_board(&self) -> Option<&BoardConfig> {
        let id = self.board_id?;
        self.boards.iter().find(|b| b.board_id == id)
    }

    pub fn active_board_mut(&mut self) -> Option<&mut BoardConfig> {
        let id = self.board_id?;
        self.boards.iter_mut().find(|b| b.board_id == id)
    }

    pub fn project_key(&self) -> Option<&str> {
        self.active_board().and_then(|b| b.project_key.as_deref())
    }

    pub fn project_key_cloned(&self) -> Option<String> {
        self.project_key().map(str::to_string)
    }

    pub fn story_points_field(&self) -> Option<&str> {
        self.active_board()
            .and_then(|b| b.story_points_field.as_deref())
    }

    pub fn story_points_field_cloned(&self) -> Option<String> {
        self.story_points_field().map(str::to_string)
    }

    pub fn columns(&self) -> Vec<String> {
        self.active_board()
            .map(|b| b.columns.clone())
            .unwrap_or_else(default_columns)
    }

    /// Select a board, creating a profile on first visit and restoring prefs later.
    pub fn select_board(&mut self, board_id: i64, project_key: Option<String>) {
        self.board_id = Some(board_id);
        if let Some(existing) = self.boards.iter_mut().find(|b| b.board_id == board_id) {
            if project_key.is_some() {
                existing.project_key = project_key;
            }
        } else {
            self.boards.push(BoardConfig::new(board_id, project_key));
        }
    }

    pub fn set_story_points_field(&mut self, field: Option<String>) {
        if let Some(board) = self.active_board_mut() {
            board.story_points_field = field;
        }
    }

    fn from_file(raw: ConfigFile) -> (Self, bool) {
        let mut changed = false;
        let mut boards = raw.boards;
        let legacy_present = raw.project_key.is_some()
            || raw.story_points_field.is_some()
            || raw.columns.is_some();

        // Lift legacy flat fields into a board profile when needed.
        if let Some(id) = raw.board_id
            && !boards.iter().any(|b| b.board_id == id)
        {
            let mut columns = raw.columns.unwrap_or_else(default_columns);
            let _ = migrate_columns(&mut columns);
            boards.push(BoardConfig {
                board_id: id,
                project_key: raw.project_key,
                story_points_field: raw.story_points_field,
                columns,
            });
            changed = true;
        } else if legacy_present {
            // Drop obsolete flat fields on next save.
            changed = true;
        }

        for board in &mut boards {
            if migrate_columns(&mut board.columns) {
                changed = true;
            }
        }

        (
            Self {
                client_id: raw.client_id,
                cloud_id: raw.cloud_id,
                board_id: raw.board_id,
                boards,
            },
            changed,
        )
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)?;
        let file: ConfigFile = toml::from_str(&raw)?;
        let (cfg, changed) = Self::from_file(file);
        if changed {
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
        assert!(decoded.boards.is_empty());
        assert!(decoded.board_id.is_none());
    }

    #[test]
    fn ignores_legacy_view_section_on_load() {
        let file: ConfigFile = toml::from_str(
            r#"
            board_id = 1
            [view]
            sort_field = "status"
            filter_assignee = "me"
            "#,
        )
        .unwrap();
        let (cfg, _) = Config::from_file(file);
        assert_eq!(cfg.board_id, Some(1));
        assert_eq!(cfg.boards.len(), 1);
        assert_eq!(cfg.boards[0].board_id, 1);
    }

    #[test]
    fn migrates_legacy_flat_board_fields() {
        let file: ConfigFile = toml::from_str(
            r#"
            cloud_id = "cloud"
            board_id = 42
            project_key = "ABC"
            story_points_field = "customfield_10016"
            columns = ["key", "summary"]
            "#,
        )
        .unwrap();
        let (cfg, changed) = Config::from_file(file);
        assert!(changed);
        assert_eq!(cfg.board_id, Some(42));
        assert_eq!(cfg.boards.len(), 1);
        let board = &cfg.boards[0];
        assert_eq!(board.board_id, 42);
        assert_eq!(board.project_key.as_deref(), Some("ABC"));
        assert_eq!(
            board.story_points_field.as_deref(),
            Some("customfield_10016")
        );
        assert_eq!(board.columns, vec!["key", "summary"]);
        assert_eq!(cfg.project_key(), Some("ABC"));
        assert_eq!(cfg.story_points_field(), Some("customfield_10016"));
    }

    #[test]
    fn multi_board_select_restores_prefs() {
        let mut cfg = Config::default();
        cfg.select_board(1, Some("AAA".into()));
        cfg.set_story_points_field(Some("customfield_1".into()));
        if let Some(board) = cfg.active_board_mut() {
            board.columns = vec!["key".into(), "summary".into()];
        }

        cfg.select_board(2, Some("BBB".into()));
        cfg.set_story_points_field(Some("customfield_2".into()));
        assert_eq!(cfg.project_key(), Some("BBB"));
        assert_eq!(cfg.story_points_field(), Some("customfield_2"));
        assert_eq!(cfg.columns(), default_columns());

        cfg.select_board(1, Some("AAA".into()));
        assert_eq!(cfg.project_key(), Some("AAA"));
        assert_eq!(cfg.story_points_field(), Some("customfield_1"));
        assert_eq!(
            cfg.columns(),
            vec!["key".to_string(), "summary".to_string()]
        );
        assert_eq!(cfg.boards.len(), 2);
    }

    #[test]
    fn serializes_boards_not_flat_fields() {
        let mut cfg = Config::default();
        cfg.cloud_id = Some("cloud".into());
        cfg.select_board(7, Some("PRJ".into()));
        cfg.set_story_points_field(Some("customfield_9".into()));
        let encoded = toml::to_string_pretty(&cfg).unwrap();
        assert!(encoded.contains("[[boards]]"));
        assert!(encoded.contains("board_id = 7"));
        assert!(encoded.contains("project_key = \"PRJ\""));
        let top = encoded.split("[[boards]]").next().unwrap();
        assert!(!top.contains("project_key"));
        assert!(!top.contains("story_points_field"));
        assert!(!top.contains("columns"));
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
