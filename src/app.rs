use std::collections::HashMap;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use ratatui::style::{Color, Modifier, Style};
use tokio::sync::mpsc;
use tui_textarea::TextArea;

use crate::auth::oauth::{
    AccessibleResource, AuthorizeRequest, OAuthClient, authorize_url, listen_for_callback,
    token_service_url,
};
use crate::auth::store::{StoredTokens, TokenStore};
use crate::config::{Config, ViewConfig};
use crate::error::Result;
use crate::jira::agile::AgileFacade;
use crate::jira::client::JiraClient;
use crate::jira::issues::{IssueDraft, IssueFacade};
use crate::jira::models::{
    Board, CreateMeta, Issue, IssueType, Priority, Sprint, Transition, User,
};
use crate::jira::search::{AssigneeFilter, SearchBuilder, SearchFacade, SortField, SprintRef};
use crate::ui;

#[derive(Debug, Clone, Default)]
pub struct LoginState {
    pub client_id: String,
    pub auth_url: Option<String>,
    pub oauth_state: Option<String>,
    pub pkce_verifier: Option<String>,
    pub error: Option<String>,
}

impl LoginState {
    pub fn ready(&self) -> bool {
        !self.client_id.is_empty()
            && self.auth_url.is_some()
            && self.oauth_state.is_some()
            && self.pkce_verifier.is_some()
    }
}

pub enum Screen {
    Login(LoginState),
    OauthWait {
        url: String,
    },
    SitePicker {
        sites: Vec<AccessibleResource>,
        selected: usize,
    },
    BoardPicker {
        boards: Vec<Board>,
        selected: usize,
    },
    Main,
}

pub enum Overlay {
    None,
    Sort {
        selected: usize,
    },
    Filter(FilterForm),
    Search {
        textarea: TextArea<'static>,
    },
    Command {
        input: String,
    },
    IssueDetail {
        issue: Option<Issue>,
        scroll: u16,
    },
    Create(IssueForm),
    Edit(IssueForm),
    DeleteConfirm {
        key: String,
    },
    Assign(AssignForm),
    Transition {
        items: Vec<Transition>,
        selected: usize,
    },
    MoveSprint {
        items: Vec<SprintChoice>,
        selected: usize,
    },
}

#[derive(Debug, Clone)]
pub struct FilterForm {
    pub statuses: Vec<String>,
    pub types: Vec<String>,
    pub assignee: AssigneeFilter,
    pub focus: usize,
    pub status_options: Vec<String>,
    pub type_options: Vec<String>,
    pub users: Vec<User>,
    pub self_account_id: Option<String>,
    pub loaded: bool,
    pub pane: FilterPane,
}

#[derive(Debug, Clone)]
pub enum FilterPane {
    Menu,
    Status {
        cursor: usize,
        draft: Vec<String>,
    },
    Type {
        cursor: usize,
        draft: Vec<String>,
    },
    Assignee {
        cursor: usize,
        draft: AssigneeFilter,
    },
}

pub struct IssueForm {
    pub key: Option<String>,
    pub type_idx: usize,
    pub types: Vec<IssueType>,
    pub summary: String,
    pub description: TextArea<'static>,
    pub priority_idx: usize,
    pub priorities: Vec<Priority>,
    pub sprint_idx: usize,
    pub sprints: Vec<SprintChoice>,
    pub assignee_idx: usize,
    pub assignees: Vec<AssigneeChoice>,
    pub story_points: String,
    pub focus: usize,
}

#[derive(Debug, Clone)]
pub struct AssigneeChoice {
    pub account_id: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct SprintChoice {
    pub id: Option<i64>,
    pub name: String,
}

pub struct AssignForm {
    pub query: String,
    pub users: Vec<User>,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TabKind {
    Sprint(i64),
    Backlog,
}

#[derive(Clone)]
pub struct Tab {
    pub kind: TabKind,
    pub title: String,
    pub issues: Vec<Issue>,
    pub selected: usize,
    pub next_page: Option<String>,
    pub loaded: bool,
    pub view: ViewConfig,
}

enum AppMsg {
    LoginReady {
        client_id: String,
        auth: AuthorizeRequest,
    },
    OauthStarted {
        url: String,
    },
    LoggedIn {
        tokens: StoredTokens,
        sites: Vec<AccessibleResource>,
    },
    Boards {
        boards: Vec<Board>,
        choose: bool,
    },
    Sprints(Vec<Sprint>),
    Issues {
        tab_idx: usize,
        issues: Vec<Issue>,
        next_page: Option<String>,
        append: bool,
    },
    Issue(Issue),
    CreateMeta(CreateMeta),
    Users(Vec<User>),
    FormUsers(Vec<User>),
    FilterOptions {
        statuses: Vec<String>,
        types: Vec<String>,
        users: Vec<User>,
        myself: Option<User>,
    },
    CurrentUser(User),
    Transitions(Vec<Transition>),
    Done {
        message: String,
        refresh: bool,
    },
    LoggedOut,
    Error(String),
}

pub struct App {
    pub config: Config,
    pub screen: Screen,
    pub overlay: Overlay,
    pub show_help: bool,
    pub tabs: Vec<Tab>,
    pub tab_idx: usize,
    pub status: String,
    pub status_is_error: bool,
    pub loading: bool,
    pub search_query: String,
    pub self_account_id: Option<String>,
    /// account_id → display name for footer/filter labels.
    user_names: HashMap<String, String>,
    pub should_quit: bool,
    /// Visible issue rows in the board table (updated each draw).
    pub list_rows: usize,
    /// First visible issue row offset (updated each draw).
    pub list_offset: usize,
    client: Option<JiraClient>,
    tx: mpsc::UnboundedSender<AppMsg>,
}

pub async fn run(mut config: Config) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let tokens = TokenStore::load().unwrap_or_default();
    let mut app = App::new(config.clone(), tokens, tx.clone());

    let mut terminal = ratatui::init();
    let result = app_loop(&mut terminal, &mut app, &mut config, &mut rx).await;
    ratatui::restore();
    result
}

async fn app_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    config: &mut Config,
    rx: &mut mpsc::UnboundedReceiver<AppMsg>,
) -> Result<()> {
    let mut events = EventStream::new();
    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, app))?;
        tokio::select! {
            event = events.next() => {
                let Some(event) = event else { break };
                if let Ok(Event::Key(key)) = event {
                    if key.kind == KeyEventKind::Press {
                        app.handle_key(key, config);
                    }
                }
            }
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                app.handle_msg(msg, config);
            }
        }
    }
    Ok(())
}

impl App {
    fn new(config: Config, tokens: StoredTokens, tx: mpsc::UnboundedSender<AppMsg>) -> Self {
        let mut app = Self {
            config: config.clone(),
            screen: Screen::Login(LoginState::default()),
            overlay: Overlay::None,
            show_help: false,
            tabs: Vec::new(),
            tab_idx: 0,
            status: String::new(),
            status_is_error: false,
            loading: false,
            search_query: String::new(),
            self_account_id: None,
            user_names: HashMap::new(),
            should_quit: false,
            list_rows: 20,
            list_offset: 0,
            client: None,
            tx,
        };

        if tokens.has_refresh()
            && let Some(cloud_id) = config.cloud_id.clone()
        {
            match JiraClient::new(cloud_id, tokens, config.story_points_field_cloned()) {
                Ok(client) => {
                    app.client = Some(client);
                    if config.board_id.is_some() {
                        app.screen = Screen::Main;
                        app.refresh_board();
                        return app;
                    }
                    app.status = "Select a board".into();
                    app.fetch_boards(false);
                    return app;
                }
                Err(err) => {
                    tracing::warn!("failed to restore session: {err}");
                }
            }
        }
        app.prepare_login();
        app
    }

    fn set_status(&mut self, message: impl Into<String>, error: bool) {
        self.status = message.into();
        self.status_is_error = error;
        self.loading = false;
    }

    pub fn current_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.tab_idx)
    }

    pub fn current_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.tab_idx)
    }

    pub fn current_view(&self) -> Option<&ViewConfig> {
        self.current_tab().map(|tab| &tab.view)
    }

    pub fn current_view_mut(&mut self) -> Option<&mut ViewConfig> {
        self.current_tab_mut().map(|tab| &mut tab.view)
    }

    pub fn selected_issue(&self) -> Option<&Issue> {
        let tab = self.current_tab()?;
        tab.issues.get(tab.selected)
    }

    fn handle_key(&mut self, key: KeyEvent, config: &mut Config) {
        match &mut self.screen {
            Screen::Login(state) => {
                let open_link =
                    key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL);
                let quit =
                    key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL);
                if quit {
                    self.should_quit = true;
                    return;
                }
                let start = key.code == KeyCode::Enter || open_link;
                if start {
                    if state.ready() {
                        let client_id = state.client_id.clone();
                        let url = state.auth_url.clone();
                        let oauth_state = state.oauth_state.clone();
                        let pkce_verifier = state.pkce_verifier.clone();
                        self.start_oauth(client_id, url, oauth_state, pkce_verifier, config);
                    } else if key.code == KeyCode::Enter {
                        self.prepare_login();
                    } else {
                        self.set_status("login link is not ready yet", true);
                    }
                }
                return;
            }
            Screen::OauthWait { url } => {
                let url = url.clone();
                if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    if let Err(err) = open::that(&url) {
                        self.set_status(format!("could not open browser: {err}"), true);
                    }
                    return;
                }
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.prepare_login();
                }
                return;
            }
            Screen::SitePicker { sites, selected } => {
                move_sel(selected, sites.len(), key);
                if key.code == KeyCode::Enter {
                    if let Some(site) = sites.get(*selected).cloned() {
                        self.select_site(site, config);
                    }
                }
                return;
            }
            Screen::BoardPicker { boards, selected } => {
                if key.code == KeyCode::Esc && self.config.board_id.is_some() {
                    self.screen = Screen::Main;
                    self.set_status(String::new(), false);
                    return;
                }
                move_sel(selected, boards.len(), key);
                if key.code == KeyCode::Enter {
                    if let Some(board) = boards.get(*selected).cloned() {
                        self.select_board(board, config);
                    }
                }
                return;
            }
            Screen::Main => {}
        }
        self.handle_main_key(key, config);
    }

    fn handle_main_key(&mut self, key: KeyEvent, config: &mut Config) {
        if self.show_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.show_help = false;
            }
            return;
        }
        if matches!(self.overlay, Overlay::None) {
            self.handle_board_key(key, config);
            return;
        }
        if let Overlay::Sort { selected } = &self.overlay {
            let selected = *selected;
            self.handle_sort_key(key, selected);
            return;
        }
        if let Overlay::Filter(form) = &self.overlay {
            let mut form = form.clone();
            if self.handle_filter_key(key, &mut form) {
                self.overlay = Overlay::None;
            } else {
                self.overlay = Overlay::Filter(form);
            }
            return;
        }
        if let Overlay::Assign(form) = &self.overlay {
            let form = AssignForm {
                query: form.query.clone(),
                users: form.users.clone(),
                selected: form.selected,
            };
            self.handle_assign_key(key, &form);
            return;
        }
        if matches!(self.overlay, Overlay::Create(_) | Overlay::Edit(_)) {
            self.handle_issue_form_key(key);
            return;
        }

        match &mut self.overlay {
            Overlay::Search { textarea } => match key.code {
                KeyCode::Esc => {
                    self.search_query.clear();
                    self.overlay = Overlay::None;
                    self.reload_current_tab();
                }
                KeyCode::Enter => {
                    self.search_query = textarea.lines().join(" ");
                    self.overlay = Overlay::None;
                    self.reload_current_tab();
                }
                _ => {
                    textarea.input(key);
                }
            },
            Overlay::Command { input } => match key.code {
                KeyCode::Esc => self.overlay = Overlay::None,
                KeyCode::Enter => {
                    let command = input.clone();
                    self.overlay = Overlay::None;
                    self.run_command(&command, config);
                }
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Char(c) => input.push(c),
                _ => {}
            },
            Overlay::IssueDetail { scroll, .. } => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => self.overlay = Overlay::None,
                KeyCode::Char('?') => self.show_help = true,
                KeyCode::Char('j') | KeyCode::Down => *scroll = scroll.saturating_add(1),
                KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
                KeyCode::Char('e') => {
                    if let Some(issue) = self.selected_issue().cloned() {
                        self.open_edit(issue);
                    }
                }
                KeyCode::Char('a') => self.open_assign(),
                KeyCode::Char('t') => self.open_transitions(),
                KeyCode::Char('m') => self.open_move_sprint(),
                KeyCode::Char('d') => {
                    if let Some(issue) = self.selected_issue() {
                        self.overlay = Overlay::DeleteConfirm {
                            key: issue.key.clone(),
                        };
                    }
                }
                _ => {}
            },
            Overlay::DeleteConfirm { key: issue_key } => {
                if matches!(key.code, KeyCode::Char('y') | KeyCode::Enter) {
                    let issue_key = issue_key.clone();
                    self.delete_issue(issue_key);
                } else if matches!(key.code, KeyCode::Esc | KeyCode::Char('n')) {
                    self.overlay = Overlay::None;
                }
            }
            Overlay::Transition { selected, items } => {
                move_sel(selected, items.len(), key);
                if key.code == KeyCode::Enter {
                    if let Some(item) = items.get(*selected).cloned() {
                        if let Some(issue) = self.selected_issue().cloned() {
                            self.do_transition(issue.key, item.id);
                        }
                    }
                } else if key.code == KeyCode::Esc {
                    self.overlay = Overlay::None;
                }
            }
            Overlay::MoveSprint { selected, items } => {
                move_sel(selected, items.len(), key);
                if key.code == KeyCode::Enter {
                    if let Some(choice) = items.get(*selected).cloned() {
                        if let Some(issue) = self.selected_issue().cloned() {
                            self.move_issue_sprint(issue.key, choice.id, choice.name);
                        }
                    }
                } else if key.code == KeyCode::Esc {
                    self.overlay = Overlay::None;
                }
            }
            Overlay::None
            | Overlay::Sort { .. }
            | Overlay::Filter(_)
            | Overlay::Assign(_)
            | Overlay::Create(_)
            | Overlay::Edit(_) => {}
        }
    }

    fn handle_board_key(&mut self, key: KeyEvent, config: &mut Config) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char(':') => {
                self.overlay = Overlay::Command {
                    input: String::new(),
                }
            }
            KeyCode::Char('s') => {
                let selected = self
                    .current_view()
                    .and_then(|view| SortField::ALL.iter().position(|f| *f == view.sort_field))
                    .unwrap_or(0);
                self.overlay = Overlay::Sort { selected };
            }
            KeyCode::Char('f') => self.open_filter(),
            KeyCode::Char('p') => self.open_project_picker(),
            KeyCode::Char('/') => {
                let mut textarea = styled_textarea("search summary or issue key");
                if !self.search_query.is_empty() {
                    textarea.insert_str(&self.search_query);
                }
                self.overlay = Overlay::Search { textarea };
            }
            KeyCode::Char('r') => self.refresh_board(),
            KeyCode::Enter => {
                if let Some(issue) = self.selected_issue().cloned() {
                    self.overlay = Overlay::IssueDetail {
                        issue: None,
                        scroll: 0,
                    };
                    self.fetch_issue(issue.key);
                }
            }
            KeyCode::Char('n') => self.open_create(),
            KeyCode::Char('e') => {
                if let Some(issue) = self.selected_issue().cloned() {
                    self.open_edit(issue);
                }
            }
            KeyCode::Char('d') => {
                if let Some(issue) = self.selected_issue() {
                    self.overlay = Overlay::DeleteConfirm {
                        key: issue.key.clone(),
                    };
                }
            }
            KeyCode::Char('a') => self.open_assign(),
            KeyCode::Char('t') => self.open_transitions(),
            KeyCode::Char('m') => self.open_move_sprint(),
            KeyCode::BackTab => self.switch_tab(-1),
            KeyCode::Left | KeyCode::Char('h') => self.switch_tab(-1),
            KeyCode::Right | KeyCode::Char('l') => self.switch_tab(1),
            KeyCode::Tab | KeyCode::Char('\t') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.switch_tab(-1);
                } else {
                    self.switch_tab(1);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_issue(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_issue(-1),
            KeyCode::PageDown => self.move_issue(10),
            KeyCode::PageUp => self.move_issue(-10),
            KeyCode::Char('g') => self.move_issue_abs(0),
            KeyCode::Char('G') => {
                if let Some(tab) = self.current_tab() {
                    let last = tab.issues.len().saturating_sub(1);
                    self.move_issue_abs(last);
                }
            }
            _ => {}
        }
        let _ = config;
    }

    fn handle_sort_key(&mut self, key: KeyEvent, selected: usize) {
        let mut selected = selected;
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                return;
            }
            KeyCode::Char('d') => {
                let selected_field = SortField::ALL.get(selected).copied();
                if selected_field.is_some_and(|f| f.has_direction()) {
                    if let Some(view) = self.current_view_mut() {
                        view.sort_dir = view.sort_dir.toggle();
                    }
                }
            }
            KeyCode::Enter => {
                if let Some(view) = self.current_view_mut() {
                    view.sort_field = SortField::ALL[selected];
                }
                self.overlay = Overlay::None;
                self.reload_current_tab();
                return;
            }
            _ => move_sel(&mut selected, SortField::ALL.len(), key),
        }
        self.overlay = Overlay::Sort { selected };
    }

    fn handle_filter_key(&mut self, key: KeyEvent, form: &mut FilterForm) -> bool {
        if matches!(form.pane, FilterPane::Menu) {
            match key.code {
                KeyCode::Esc => return true,
                KeyCode::Up | KeyCode::BackTab => form.focus = (form.focus + 4) % 5,
                KeyCode::Down | KeyCode::Tab => form.focus = (form.focus + 1) % 5,
                KeyCode::Enter if form.focus == 3 => {
                    self.apply_filter(form);
                    return true;
                }
                KeyCode::Enter if form.focus == 4 => {
                    form.clear();
                    self.apply_filter(form);
                    return true;
                }
                KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') if form.focus < 3 => {
                    form.open_sub();
                }
                _ => {}
            }
            return false;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                form.pane = FilterPane::Menu;
            }
            KeyCode::Enter => form.commit_sub(),
            KeyCode::Char(' ') => form.toggle_sub(),
            KeyCode::Char('a') => form.select_all_sub(),
            _ => form.move_sub(key),
        }
        false
    }

    fn note_self_user(&mut self, user: &User) {
        self.self_account_id = Some(user.account_id.clone());
        self.remember_user(user);
        if let Some(view) = self.current_view_mut() {
            view.filter_assignee.resolve_me(&user.account_id);
        }
        if let Overlay::Filter(form) = &mut self.overlay {
            form.self_account_id = Some(user.account_id.clone());
            form.assignee.resolve_me(&user.account_id);
        }
    }

    fn remember_user(&mut self, user: &User) {
        if user.account_id.is_empty() || user.display_name.is_empty() {
            return;
        }
        self.user_names
            .insert(user.account_id.clone(), user.display_name.clone());
    }

    fn remember_users<I>(&mut self, users: I)
    where
        I: IntoIterator<Item = User>,
    {
        for user in users {
            self.remember_user(&user);
        }
    }

    pub fn user_display_name(&self, account_id: &str) -> Option<&str> {
        self.user_names.get(account_id).map(String::as_str)
    }

    fn remember_issue_assignees(&mut self, issues: &[Issue]) {
        for issue in issues {
            if let Some(user) = &issue.assignee {
                self.remember_user(user);
            }
        }
    }

    fn apply_filter(&mut self, form: &FilterForm) {
        let mut assignee = form.assignee.clone();
        if let Some(id) = form
            .self_account_id
            .as_deref()
            .or(self.self_account_id.as_deref())
        {
            assignee.resolve_me(id);
        }
        self.remember_users(form.users.iter().cloned());
        if let Some(view) = self.current_view_mut() {
            view.filter_statuses = form.statuses.clone();
            view.filter_types = form.types.clone();
            view.filter_assignee = assignee;
        }
        self.reload_current_tab();
    }

    fn open_filter(&mut self) {
        let view = self.current_view().cloned().unwrap_or_default();
        let mut form = FilterForm::from_view(&view);
        form.self_account_id = self.self_account_id.clone();
        if let Some(id) = &form.self_account_id {
            form.assignee.resolve_me(id);
        }
        self.overlay = Overlay::Filter(form);
        self.fetch_filter_options();
    }

    fn open_project_picker(&mut self) {
        self.fetch_boards(true);
    }

    fn handle_issue_form_key(&mut self, key: KeyEvent) {
        let layout = FormFocus::new();
        let mut submit = false;
        let mut close = false;
        {
            let (Overlay::Create(form) | Overlay::Edit(form)) = &mut self.overlay else {
                return;
            };
            if form.focus == layout.description {
                let row = form.description.cursor().0;
                let last = form.description.lines().len().saturating_sub(1);
                match key.code {
                    KeyCode::Esc => close = true,
                    KeyCode::Tab => form.focus = (form.focus + 1) % layout.count,
                    KeyCode::BackTab => {
                        form.focus = (form.focus + layout.count - 1) % layout.count;
                    }
                    KeyCode::Up if row == 0 => form.focus = layout.points,
                    KeyCode::Down if row >= last => form.focus = layout.submit,
                    _ => {
                        form.description.input(key);
                    }
                }
            } else {
                match key.code {
                    KeyCode::Esc => close = true,
                    KeyCode::Tab => form.focus = (form.focus + 1) % layout.count,
                    KeyCode::BackTab => {
                        form.focus = (form.focus + layout.count - 1) % layout.count;
                    }
                    KeyCode::Down if form.focus < layout.cancel => form.focus += 1,
                    KeyCode::Up if form.focus > 0 => form.focus -= 1,
                    KeyCode::Enter if form.focus == layout.submit => submit = true,
                    KeyCode::Enter if form.focus == layout.cancel => close = true,
                    KeyCode::Left | KeyCode::Right => {
                        let delta = if key.code == KeyCode::Left { -1 } else { 1 };
                        nudge_picker(form, &layout, delta);
                    }
                    KeyCode::Char(c @ ('h' | 'l')) if picker_focused(form, &layout) => {
                        let delta = if c == 'h' { -1 } else { 1 };
                        nudge_picker(form, &layout, delta);
                    }
                    KeyCode::Backspace => {
                        if form.focus == layout.summary {
                            form.summary.pop();
                        } else if form.focus == layout.points {
                            form.story_points.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if form.focus == layout.summary {
                            form.summary.push(c);
                        } else if form.focus == layout.points && c.is_ascii_digit() {
                            form.story_points.push(c);
                        }
                    }
                    _ => {}
                }
            }
        }
        if close {
            self.overlay = Overlay::None;
        } else if submit {
            self.submit_issue_form();
        }
    }

    fn handle_assign_key(&mut self, key: KeyEvent, form: &AssignForm) {
        let mut form = AssignForm {
            query: form.query.clone(),
            users: form.users.clone(),
            selected: form.selected,
        };
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                return;
            }
            KeyCode::Enter => {
                if let Some(user) = form.users.get(form.selected).cloned() {
                    if let Some(issue) = self.selected_issue().cloned() {
                        self.assign_issue(issue.key, Some(user.account_id));
                    }
                }
                return;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(issue) = self.selected_issue().cloned() {
                    self.assign_issue(issue.key, None);
                }
                return;
            }
            KeyCode::Backspace => {
                form.query.pop();
                self.search_users(form.query.clone());
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.query.push(c);
                self.search_users(form.query.clone());
            }
            _ => move_sel(&mut form.selected, form.users.len(), key),
        }
        self.overlay = Overlay::Assign(form);
    }

    fn run_command(&mut self, command: &str, config: &mut Config) {
        match command.trim() {
            "q" | "quit" => self.should_quit = true,
            "logout" => self.logout(),
            "project" => self.open_project_picker(),
            "login" => {
                self.prepare_login();
                self.overlay = Overlay::None;
            }
            other => self.set_status(format!("unknown command: {other}"), true),
        }
        let _ = config;
    }

    fn prepare_login(&mut self) {
        tracing::info!("starting login");
        self.screen = Screen::Login(LoginState::default());
        self.set_status("contacting token service…", false);
        self.loading = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let oauth = OAuthClient::new()?;
                let config = oauth.fetch_config().await?;
                let auth = authorize_url(&config.client_id)?;
                Ok::<_, crate::error::Error>((config.client_id, auth))
            }
            .await;
            match result {
                Ok((client_id, auth)) => {
                    let _ = tx.send(AppMsg::LoginReady { client_id, auth });
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(format!(
                        "{err}. Start token-service or set JIRA_TUI_TOKEN_SERVICE (tried {})",
                        token_service_url()
                    )));
                }
            }
        });
    }

    fn start_oauth(
        &mut self,
        client_id: String,
        auth_url: Option<String>,
        oauth_state: Option<String>,
        pkce_verifier: Option<String>,
        config: &mut Config,
    ) {
        self.config.client_id = client_id.clone();
        config.client_id = client_id.clone();
        let _ = config.save();
        tracing::info!("opening Atlassian authorization");
        self.set_status("starting OAuth…", false);
        self.loading = true;
        let prepared = match (auth_url, oauth_state, pkce_verifier) {
            (Some(url), Some(state), Some(pkce_verifier)) => Ok(AuthorizeRequest {
                url,
                state,
                pkce_verifier,
            }),
            _ if !client_id.is_empty() => authorize_url(&client_id),
            _ => Err(crate::error::Error::auth(
                "login is not ready; press Enter to retry",
            )),
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match prepared {
                Ok(auth) => {
                    let _ = tx.send(AppMsg::OauthStarted {
                        url: auth.url.clone(),
                    });
                    if let Err(err) = open::that(&auth.url) {
                        tracing::warn!("failed to open browser: {err}");
                    }
                    let result = async {
                        let code = listen_for_callback(&auth.state).await?;
                        tracing::info!("received oauth callback");
                        let oauth = OAuthClient::new()?;
                        let tokens = oauth.exchange_code(&code, &auth.pkce_verifier).await?;
                        TokenStore::save(&tokens)?;
                        let sites = oauth.accessible_resources(&tokens.access_token).await?;
                        Ok::<_, crate::error::Error>((tokens, sites))
                    }
                    .await;
                    match result {
                        Ok((tokens, sites)) => {
                            let _ = tx.send(AppMsg::LoggedIn { tokens, sites });
                        }
                        Err(err) => {
                            let _ = tx.send(AppMsg::Error(err.to_string()));
                        }
                    }
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn select_site(&mut self, site: AccessibleResource, config: &mut Config) {
        tracing::info!(site = %site.name, "selected site");
        self.config.cloud_id = Some(site.id.clone());
        config.cloud_id = Some(site.id.clone());
        let _ = config.save();
        let tokens = TokenStore::load().unwrap_or_default();
        match JiraClient::new(site.id, tokens, self.config.story_points_field_cloned()) {
            Ok(client) => {
                self.client = Some(client);
                self.fetch_boards(false);
            }
            Err(err) => self.set_status(err.to_string(), true),
        }
    }

    fn select_board(&mut self, board: Board, config: &mut Config) {
        tracing::info!(
            board_id = board.id,
            project = board.project_key.as_deref().unwrap_or(""),
            "selected board"
        );
        config.select_board(board.id, board.project_key.clone());
        self.config.select_board(board.id, board.project_key.clone());
        let sp = self.config.story_points_field_cloned();
        if let Some(client) = &mut self.client {
            client.set_story_points_field(sp);
        }
        let _ = config.save();
        self.screen = Screen::Main;
        self.refresh_board();
    }

    fn logout(&mut self) {
        self.loading = true;
        let tokens = TokenStore::load().unwrap_or_default();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if !tokens.access_token.is_empty()
                && let Ok(oauth) = OAuthClient::new()
            {
                let _ = oauth.revoke(&tokens.access_token).await;
                if !tokens.refresh_token.is_empty() {
                    let _ = oauth.revoke(&tokens.refresh_token).await;
                }
            }
            let _ = TokenStore::clear();
            let _ = tx.send(AppMsg::LoggedOut);
        });
    }

    fn refresh_board(&mut self) {
        let Some(client) = self.client.clone() else {
            self.set_status("not authenticated", true);
            return;
        };
        let Some(board_id) = self.config.board_id else {
            self.fetch_boards(false);
            return;
        };
        self.fetch_myself();
        self.loading = true;
        self.set_status("loading sprints…", false);
        let tx = self.tx.clone();
        let project = self.config.project_key_cloned();
        tokio::spawn(async move {
            let agile = AgileFacade::new(&client);
            let sprints = match agile.open_sprints(board_id).await {
                Ok(sprints) if !sprints.is_empty() => Ok(sprints),
                Ok(_) => {
                    tracing::warn!("agile returned no sprints; discovering via JQL");
                    SearchFacade::new(&client)
                        .discover_sprints(project.as_deref())
                        .await
                }
                Err(err) => {
                    tracing::warn!("agile sprints unavailable ({err}); falling back to JQL");
                    SearchFacade::new(&client)
                        .discover_sprints(project.as_deref())
                        .await
                }
            };
            match sprints {
                Ok(sprints) => {
                    let _ = tx.send(AppMsg::Sprints(sprints));
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
        self.fetch_story_points_field(board_id);
    }

    fn fetch_story_points_field(&mut self, board_id: i64) {
        if self.config.story_points_field().is_some() {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let from_agile = AgileFacade::new(&client).story_points_field(board_id).await;
            let field = match from_agile {
                Ok(Some(field)) => Some(field),
                Ok(None) => {
                    tracing::warn!("board has no estimation field; looking up via field list");
                    SearchFacade::new(&client)
                        .story_points_field_id()
                        .await
                        .ok()
                        .flatten()
                }
                Err(err) => {
                    tracing::warn!(
                        "story points field via agile ({err}); looking up via field list"
                    );
                    SearchFacade::new(&client)
                        .story_points_field_id()
                        .await
                        .ok()
                        .flatten()
                }
            };
            if let Some(field) = field {
                let _ = tx.send(AppMsg::Done {
                    message: format!("__sp_field__:{field}"),
                    refresh: true,
                });
            }
        });
    }

    fn fetch_boards(&mut self, choose: bool) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.loading = true;
        self.set_status("loading boards…", false);
        let project = if choose {
            None
        } else {
            self.config.project_key_cloned()
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let agile = AgileFacade::new(&client);
            let boards = match agile.boards(project.as_deref()).await {
                Ok(boards) if !boards.is_empty() => Ok(boards),
                Ok(_) | Err(_) => {
                    tracing::warn!("agile boards unavailable; falling back to project list");
                    agile.projects().await
                }
            };
            match boards {
                Ok(boards) => {
                    let _ = tx.send(AppMsg::Boards { boards, choose });
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn fetch_myself(&mut self) {
        if self.self_account_id.is_some() {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client).myself().await {
                Ok(user) => {
                    let _ = tx.send(AppMsg::CurrentUser(user));
                }
                Err(err) => {
                    tracing::warn!("could not load current user: {err}");
                }
            }
        });
    }

    fn fetch_filter_options(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(project) = self.config.project_key_cloned() else {
            self.set_status("no project key configured", true);
            return;
        };
        let known = self.self_account_id.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let facade = IssueFacade::new(&client);
            let options = facade.project_filter_options(&project).await;
            let users = facade.assignable_users_limited(&project, "", 100).await;
            let myself = if known.is_some() {
                None
            } else {
                facade.myself().await.ok()
            };
            match (options, users) {
                (Ok((statuses, types)), Ok(users)) => {
                    let _ = tx.send(AppMsg::FilterOptions {
                        statuses,
                        types,
                        users,
                        myself,
                    });
                }
                (Err(err), _) | (_, Err(err)) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn switch_tab(&mut self, delta: isize) {
        if self.tabs.is_empty() {
            return;
        }
        let len = self.tabs.len() as isize;
        self.tab_idx = ((self.tab_idx as isize + delta).rem_euclid(len)) as usize;
        let title = self.tabs[self.tab_idx].title.clone();
        let loaded = self.tabs[self.tab_idx].loaded;
        self.status = format!("{}  {}/{}", title, self.tab_idx + 1, self.tabs.len());
        self.status_is_error = false;
        if !loaded {
            self.reload_current_tab();
        }
    }

    fn move_issue(&mut self, delta: isize) {
        let Some(tab) = self.current_tab_mut() else {
            return;
        };
        if tab.issues.is_empty() {
            return;
        }
        let next = tab.selected as isize + delta;
        tab.selected = next.clamp(0, tab.issues.len() as isize - 1) as usize;
        self.maybe_load_more_at_end();
    }

    fn move_issue_abs(&mut self, idx: usize) {
        if let Some(tab) = self.current_tab_mut() {
            if !tab.issues.is_empty() {
                tab.selected = idx.min(tab.issues.len() - 1);
            }
        }
        self.maybe_load_more_at_end();
    }

    fn maybe_load_more_at_end(&mut self) {
        let Some(tab) = self.current_tab() else {
            return;
        };
        if tab.issues.is_empty() {
            return;
        }
        if tab.selected + 1 >= tab.issues.len() && tab.next_page.is_some() {
            self.load_more();
        }
    }

    fn page_size(&self) -> u32 {
        (self.list_rows as u32).max(50)
    }

    fn builder_for_tab(&self, tab: &Tab) -> SearchBuilder {
        let mut assignee = tab.view.filter_assignee.clone();
        if let Some(id) = &self.self_account_id {
            assignee.resolve_me(id);
        }
        let mut builder = SearchBuilder::new()
            .status(tab.view.filter_statuses.clone())
            .issue_type(tab.view.filter_types.clone())
            .assignee(assignee)
            .order_by(tab.view.sort_field, tab.view.sort_dir)
            .story_points_field(self.config.story_points_field_cloned())
            .text(self.search_query.clone())
            .max_results(self.page_size());
        if let Some(project) = self.config.project_key() {
            builder = builder.project(project);
        }
        builder = match tab.kind {
            TabKind::Sprint(id) => builder.sprint(SprintRef::Id(id)),
            TabKind::Backlog => builder.sprint(SprintRef::Backlog),
        };
        // Backlog board hides epics/subtasks; keep them available via type filter or `/` search.
        if matches!(tab.kind, TabKind::Backlog)
            && tab.view.filter_types.is_empty()
            && self.search_query.is_empty()
        {
            builder = builder
                .clause("issuetype != Epic AND issuetype not in subTaskIssueTypes()");
        }
        builder
    }

    fn reload_current_tab(&mut self) {
        let Some(tab_idx) = self.tabs.get(self.tab_idx).map(|_| self.tab_idx) else {
            return;
        };
        self.load_issues(tab_idx, false);
    }

    fn load_more(&mut self) {
        let Some(tab_idx) = self.tabs.get(self.tab_idx).map(|_| self.tab_idx) else {
            return;
        };
        self.load_issues(tab_idx, true);
    }

    fn load_issues(&mut self, tab_idx: usize, append: bool) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(tab) = self.tabs.get(tab_idx) else {
            return;
        };
        if append {
            if tab.next_page.is_none() || self.loading {
                return;
            }
        }
        let mut builder = self.builder_for_tab(tab);
        if append {
            if let Some(token) = &tab.next_page {
                builder = builder.next_page(token);
            }
        }
        let request = builder.build();
        self.loading = true;
        if !append {
            self.set_status("loading issues…", false);
        }
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match SearchFacade::new(&client).search(request).await {
                Ok(page) => {
                    let _ = tx.send(AppMsg::Issues {
                        tab_idx,
                        issues: page.issues,
                        next_page: page.next_page_token,
                        append,
                    });
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn fetch_issue(&mut self, key: String) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.loading = true;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client).get(&key).await {
                Ok(issue) => {
                    let _ = tx.send(AppMsg::Issue(issue));
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn open_create(&mut self) {
        let mut form = IssueForm::blank();
        form.sprints = sprint_choices(&self.tabs);
        form.sprint_idx = 0;
        form.focus = FormFocus::new().summary;
        self.overlay = Overlay::Create(form);
        self.fetch_create_meta();
        self.fetch_form_users();
    }

    fn open_edit(&mut self, issue: Issue) {
        let tab_kind = self.current_tab().map(|tab| tab.kind.clone());
        let mut form = IssueForm::from_issue(&issue);
        form.sprints = sprint_choices(&self.tabs);
        form.sprint_idx = select_sprint_idx(&mut form.sprints, &issue, tab_kind.as_ref());
        form.focus = FormFocus::new().summary;
        self.overlay = Overlay::Edit(form);
        self.fetch_create_meta();
        self.fetch_form_users();
    }

    fn fetch_create_meta(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(project) = self.config.project_key_cloned() else {
            self.set_status("no project key configured", true);
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client).create_meta(&project).await {
                Ok(meta) => {
                    let _ = tx.send(AppMsg::CreateMeta(meta));
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn submit_issue_form(&mut self) {
        let (is_create, draft, key) = match &self.overlay {
            Overlay::Create(form) => (true, form.to_draft(), None),
            Overlay::Edit(form) => (false, form.to_draft(), form.key.clone()),
            _ => return,
        };
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(project) = self.config.project_key_cloned() else {
            self.set_status("no project key configured", true);
            return;
        };
        self.loading = true;
        self.set_status(
            if is_create {
                "creating issue…"
            } else {
                "updating issue…"
            },
            false,
        );
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let facade = IssueFacade::new(&client);
            let result = if is_create {
                facade.create(&project, &draft).await.map(|key| {
                    tracing::info!(key = %key, "created issue");
                    format!("created {key}")
                })
            } else if let Some(key) = key {
                facade.update(&key, &draft).await.map(|()| {
                    tracing::info!(key = %key, "updated issue");
                    format!("updated {key}")
                })
            } else {
                Err(crate::error::Error::message("missing issue key"))
            };
            match result {
                Ok(message) => {
                    let _ = tx.send(AppMsg::Done {
                        message,
                        refresh: true,
                    });
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn delete_issue(&mut self, key: String) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.loading = true;
        self.overlay = Overlay::None;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client).delete(&key).await {
                Ok(()) => {
                    tracing::info!(key = %key, "deleted issue");
                    let _ = tx.send(AppMsg::Done {
                        message: format!("deleted {key}"),
                        refresh: true,
                    });
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn open_assign(&mut self) {
        if self.selected_issue().is_none() {
            return;
        }
        self.overlay = Overlay::Assign(AssignForm {
            query: String::new(),
            users: Vec::new(),
            selected: 0,
        });
        self.search_users(String::new());
    }

    fn fetch_form_users(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(project) = self.config.project_key_cloned() else {
            self.set_status("no project key configured", true);
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client)
                .assignable_users_limited(&project, "", 100)
                .await
            {
                Ok(users) => {
                    let _ = tx.send(AppMsg::FormUsers(users));
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn search_users(&mut self, query: String) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(project) = self.config.project_key_cloned() else {
            return;
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client)
                .assignable_users(&project, &query)
                .await
            {
                Ok(users) => {
                    let _ = tx.send(AppMsg::Users(users));
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn assign_issue(&mut self, key: String, account_id: Option<String>) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.loading = true;
        self.overlay = Overlay::None;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client)
                .assign(&key, account_id.as_deref())
                .await
            {
                Ok(()) => {
                    tracing::info!(key = %key, "updated assignee");
                    let _ = tx.send(AppMsg::Done {
                        message: format!("updated assignee on {key}"),
                        refresh: true,
                    });
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn open_transitions(&mut self) {
        let Some(issue) = self.selected_issue().cloned() else {
            return;
        };
        let Some(client) = self.client.clone() else {
            return;
        };
        self.loading = true;
        self.set_status("loading transitions…", false);
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client).transitions(&issue.key).await {
                Ok(items) => {
                    let _ = tx.send(AppMsg::Transitions(items));
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn do_transition(&mut self, key: String, id: String) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.loading = true;
        self.overlay = Overlay::None;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client).transition(&key, &id).await {
                Ok(()) => {
                    tracing::info!(key = %key, "transitioned issue");
                    let _ = tx.send(AppMsg::Done {
                        message: format!("transitioned {key}"),
                        refresh: true,
                    });
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn open_move_sprint(&mut self) {
        let Some(issue) = self.selected_issue().cloned() else {
            return;
        };
        let tab_kind = self.current_tab().map(|tab| tab.kind.clone());
        let mut items = sprint_choices(&self.tabs);
        let selected = select_sprint_idx(&mut items, &issue, tab_kind.as_ref());
        self.overlay = Overlay::MoveSprint { items, selected };
    }

    fn move_issue_sprint(&mut self, key: String, sprint_id: Option<i64>, name: String) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.loading = true;
        self.overlay = Overlay::None;
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match IssueFacade::new(&client).set_sprint(&key, sprint_id).await {
                Ok(()) => {
                    tracing::info!(key = %key, sprint = %name, "moved issue");
                    let _ = tx.send(AppMsg::Done {
                        message: format!("moved {key} to {name}"),
                        refresh: true,
                    });
                }
                Err(err) => {
                    let _ = tx.send(AppMsg::Error(err.to_string()));
                }
            }
        });
    }

    fn handle_msg(&mut self, msg: AppMsg, config: &mut Config) {
        match msg {
            AppMsg::LoginReady { client_id, auth } => {
                if let Screen::Login(state) = &mut self.screen {
                    state.client_id = client_id.clone();
                    state.auth_url = Some(auth.url);
                    state.oauth_state = Some(auth.state);
                    state.pkce_verifier = Some(auth.pkce_verifier);
                    state.error = None;
                }
                self.config.client_id = client_id.clone();
                config.client_id = client_id;
                let _ = config.save();
                self.set_status("press Enter to log in", false);
            }
            AppMsg::OauthStarted { url, .. } => {
                self.screen = Screen::OauthWait { url };
                self.set_status("waiting for browser authorization…", false);
            }
            AppMsg::LoggedIn { tokens, sites } => {
                let _ = TokenStore::save(&tokens);
                if sites.is_empty() {
                    self.set_status("no Jira sites available for this account", true);
                    return;
                }
                if sites.len() == 1 {
                    self.select_site(sites[0].clone(), config);
                } else if let Some(existing) = &self.config.cloud_id {
                    if let Some(site) = sites.iter().find(|s| &s.id == existing).cloned() {
                        self.select_site(site, config);
                    } else {
                        self.screen = Screen::SitePicker { sites, selected: 0 };
                    }
                } else {
                    self.screen = Screen::SitePicker { sites, selected: 0 };
                }
            }
            AppMsg::Boards { boards, choose } => {
                if boards.is_empty() {
                    self.set_status("no boards found", true);
                    return;
                }
                if !choose
                    && let Some(id) = self.config.board_id
                    && let Some(board) = boards.iter().find(|b| b.id == id).cloned()
                {
                    self.select_board(board, config);
                    return;
                }
                let selected = self
                    .config
                    .board_id
                    .and_then(|id| boards.iter().position(|board| board.id == id))
                    .unwrap_or(0);
                self.screen = Screen::BoardPicker { boards, selected };
                self.set_status("select a board", false);
            }
            AppMsg::Sprints(sprints) => {
                let prev_views: std::collections::HashMap<TabKind, ViewConfig> = self
                    .tabs
                    .iter()
                    .map(|tab| (tab.kind.clone(), tab.view.clone()))
                    .collect();
                let mut tabs: Vec<Tab> = sprints
                    .into_iter()
                    .map(|sprint| {
                        let kind = TabKind::Sprint(sprint.id);
                        Tab {
                            view: prev_views
                                .get(&kind)
                                .cloned()
                                .unwrap_or_default(),
                            kind,
                            title: sprint_title(&sprint),
                            issues: Vec::new(),
                            selected: 0,
                            next_page: None,
                            loaded: false,
                        }
                    })
                    .collect();
                tabs.push(Tab {
                    kind: TabKind::Backlog,
                    title: "Backlog".into(),
                    issues: Vec::new(),
                    selected: 0,
                    next_page: None,
                    loaded: false,
                    view: prev_views
                        .get(&TabKind::Backlog)
                        .cloned()
                        .unwrap_or_default(),
                });
                self.tabs = tabs;
                self.tab_idx = 0;
                self.screen = Screen::Main;
                self.reload_current_tab();
            }
            AppMsg::Issues {
                tab_idx,
                issues,
                next_page,
                append,
            } => {
                let received = issues.len();
                self.remember_issue_assignees(&issues);
                if let Some(tab) = self.tabs.get_mut(tab_idx) {
                    if append {
                        tab.issues.extend(issues);
                    } else {
                        tab.issues = issues;
                        tab.selected = 0;
                    }
                    tab.next_page = next_page;
                    tab.loaded = true;
                }
                self.set_status(String::new(), false);
                if received > 0 {
                    let need = self.list_rows.max(1);
                    if let Some(tab) = self.tabs.get(tab_idx) {
                        if tab.issues.len() < need && tab.next_page.is_some() {
                            self.load_more();
                        }
                    }
                }
            }
            AppMsg::Issue(issue) => {
                if let Some(user) = &issue.assignee {
                    self.remember_user(user);
                }
                if let Overlay::IssueDetail { issue: slot, .. } = &mut self.overlay {
                    *slot = Some(issue);
                }
                self.set_status(String::new(), false);
            }
            AppMsg::CreateMeta(meta) => match &mut self.overlay {
                Overlay::Create(form) => {
                    apply_create_meta(form, meta, Some(("Story", "Medium")));
                }
                Overlay::Edit(form) => {
                    apply_create_meta(form, meta, None);
                }
                _ => {}
            },
            AppMsg::Users(users) => {
                self.remember_users(users.iter().cloned());
                if let Overlay::Assign(form) = &mut self.overlay {
                    form.users = users;
                    form.selected = 0;
                }
            }
            AppMsg::FormUsers(users) => {
                self.remember_users(users.iter().cloned());
                let self_id = self.self_account_id.clone();
                match &mut self.overlay {
                    Overlay::Create(form) | Overlay::Edit(form) => {
                        apply_assignees(form, users, self_id.as_deref());
                    }
                    _ => {}
                }
            }
            AppMsg::CurrentUser(user) => {
                self.note_self_user(&user);
            }
            AppMsg::FilterOptions {
                statuses,
                types,
                users,
                myself,
            } => {
                if let Some(user) = myself {
                    self.note_self_user(&user);
                }
                self.remember_users(users.iter().cloned());
                if let Overlay::Filter(form) = &mut self.overlay {
                    form.status_options = statuses;
                    form.type_options = types;
                    form.users = users;
                    form.loaded = true;
                    form.self_account_id = self.self_account_id.clone();
                    if let Some(id) = &form.self_account_id {
                        form.assignee.resolve_me(id);
                    }
                }
            }
            AppMsg::Transitions(items) => {
                self.overlay = Overlay::Transition { items, selected: 0 };
                self.set_status(String::new(), false);
            }
            AppMsg::Done { message, refresh } => {
                if let Some(field) = message.strip_prefix("__sp_field__:") {
                    self.config.set_story_points_field(Some(field.to_string()));
                    config.set_story_points_field(Some(field.to_string()));
                    let _ = config.save();
                    if let Some(client) = &mut self.client {
                        client.set_story_points_field(Some(field.to_string()));
                    }
                    if refresh {
                        self.reload_current_tab();
                    }
                    return;
                }
                self.overlay = Overlay::None;
                self.set_status(message, false);
                if refresh {
                    self.reload_current_tab();
                }
            }
            AppMsg::LoggedOut => {
                self.client = None;
                self.tabs.clear();
                self.overlay = Overlay::None;
                self.prepare_login();
                self.set_status("logged out", false);
            }
            AppMsg::Error(err) => {
                if let Screen::Login(state) = &mut self.screen {
                    state.error = Some(err.clone());
                }
                self.set_status(err, true);
            }
        }
    }
}

impl FilterForm {
    fn from_view(view: &crate::config::ViewConfig) -> Self {
        Self {
            statuses: view.filter_statuses.clone(),
            types: view.filter_types.clone(),
            assignee: view.filter_assignee.clone(),
            focus: 0,
            status_options: Vec::new(),
            type_options: Vec::new(),
            users: Vec::new(),
            self_account_id: None,
            loaded: false,
            pane: FilterPane::Menu,
        }
    }

    fn clear(&mut self) {
        self.statuses.clear();
        self.types.clear();
        self.assignee = AssigneeFilter::default();
    }

    fn commit_sub(&mut self) {
        match &self.pane {
            FilterPane::Status { draft, .. } => {
                self.statuses = draft.clone();
            }
            FilterPane::Type { draft, .. } => {
                self.types = draft.clone();
            }
            FilterPane::Assignee { draft, .. } => {
                let mut draft = draft.clone();
                if let Some(id) = &self.self_account_id {
                    draft.resolve_me(id);
                }
                self.assignee = draft;
            }
            FilterPane::Menu => return,
        }
        self.pane = FilterPane::Menu;
    }

    fn toggle_sub(&mut self) {
        let statuses = self.status_options.clone();
        let types = self.type_options.clone();
        let accounts: Vec<String> = self
            .users
            .iter()
            .map(|user| user.account_id.clone())
            .collect();
        match &mut self.pane {
            FilterPane::Status { cursor, draft } => {
                if let Some(name) = statuses.get(*cursor) {
                    toggle_name(draft, name);
                }
            }
            FilterPane::Type { cursor, draft } => {
                if let Some(name) = types.get(*cursor) {
                    toggle_name(draft, name);
                }
            }
            FilterPane::Assignee { cursor, draft } => {
                if *cursor == 0 {
                    draft.unassigned = !draft.unassigned;
                } else if let Some(id) = accounts.get(*cursor - 1) {
                    draft.toggle_account(id);
                }
            }
            FilterPane::Menu => {}
        }
    }

    fn select_all_sub(&mut self) {
        match &mut self.pane {
            FilterPane::Status { draft, .. } => {
                *draft = self.status_options.clone();
            }
            FilterPane::Type { draft, .. } => {
                *draft = self.type_options.clone();
            }
            FilterPane::Assignee { draft, .. } => {
                draft.unassigned = true;
                draft.me = false;
                draft.accounts = self
                    .users
                    .iter()
                    .map(|user| user.account_id.clone())
                    .collect();
                draft.dedup_accounts();
            }
            FilterPane::Menu => {}
        }
    }

    fn move_sub(&mut self, key: KeyEvent) {
        let status_len = self.status_options.len();
        let type_len = self.type_options.len();
        let assignee_len = self.users.len() + 1;
        match &mut self.pane {
            FilterPane::Status { cursor, .. } => move_sel(cursor, status_len, key),
            FilterPane::Type { cursor, .. } => move_sel(cursor, type_len, key),
            FilterPane::Assignee { cursor, .. } => move_sel(cursor, assignee_len, key),
            FilterPane::Menu => {}
        }
    }

    fn open_sub(&mut self) {
        if let Some(id) = &self.self_account_id {
            self.assignee.resolve_me(id);
        }
        self.pane = match self.focus {
            0 => FilterPane::Status {
                cursor: 0,
                draft: self.statuses.clone(),
            },
            1 => FilterPane::Type {
                cursor: 0,
                draft: self.types.clone(),
            },
            2 => FilterPane::Assignee {
                cursor: 0,
                draft: self.assignee.clone(),
            },
            _ => FilterPane::Menu,
        };
    }

    pub fn status_label(&self) -> String {
        if self.statuses.is_empty() {
            "any".into()
        } else {
            self.statuses.join(", ")
        }
    }

    pub fn type_label(&self) -> String {
        if self.types.is_empty() {
            "any".into()
        } else {
            self.types.join(", ")
        }
    }

    pub fn assignee_label(&self) -> String {
        let mut assignee = self.assignee.clone();
        if let Some(id) = &self.self_account_id {
            assignee.resolve_me(id);
        }
        if assignee.is_empty() {
            return String::new();
        }
        let mut parts = Vec::new();
        if assignee.unassigned {
            parts.push("Unassigned".to_string());
        }
        if assignee.me {
            parts.push("you".to_string());
        }
        for id in &assignee.accounts {
            let name = self
                .users
                .iter()
                .find(|user| &user.account_id == id)
                .map(|user| user.display_name.as_str())
                .unwrap_or(id.as_str());
            if self.self_account_id.as_deref() == Some(id.as_str()) {
                parts.push(format!("{name} (you)"));
            } else {
                parts.push(name.to_string());
            }
        }
        parts.join(", ")
    }
}

impl IssueForm {
    fn blank() -> Self {
        Self {
            key: None,
            type_idx: 0,
            types: vec![IssueType {
                id: String::new(),
                name: "Story".into(),
                subtask: false,
            }],
            summary: String::new(),
            description: styled_textarea("issue description"),
            priority_idx: 0,
            priorities: vec![Priority {
                id: String::new(),
                name: "Medium".into(),
            }],
            sprint_idx: 0,
            sprints: vec![SprintChoice {
                id: None,
                name: "Backlog".into(),
            }],
            assignee_idx: 0,
            assignees: vec![unassigned_choice()],
            story_points: String::new(),
            focus: 0,
        }
    }

    fn from_issue(issue: &Issue) -> Self {
        let mut description = styled_textarea("issue description");
        if !issue.description_text.is_empty() {
            description.insert_str(&issue.description_text);
        }
        Self {
            key: Some(issue.key.clone()),
            type_idx: 0,
            types: vec![IssueType {
                id: String::new(),
                name: issue.issue_type.clone(),
                subtask: false,
            }],
            summary: issue.summary.clone(),
            description,
            priority_idx: 0,
            priorities: issue
                .priority
                .clone()
                .map(|name| {
                    vec![Priority {
                        id: String::new(),
                        name,
                    }]
                })
                .unwrap_or_default(),
            story_points: issue
                .story_points
                .map(|n| {
                    if n.fract() == 0.0 {
                        format!("{n:.0}")
                    } else {
                        format!("{n}")
                    }
                })
                .unwrap_or_default(),
            assignee_idx: usize::from(issue.assignee.is_some()),
            assignees: {
                let mut choices = vec![unassigned_choice()];
                if let Some(user) = &issue.assignee {
                    choices.push(AssigneeChoice {
                        account_id: Some(user.account_id.clone()),
                        label: user.display_name.clone(),
                    });
                }
                choices
            },
            sprint_idx: 0,
            sprints: vec![SprintChoice {
                id: None,
                name: "Backlog".into(),
            }],
            focus: 2,
        }
    }

    fn to_draft(&self) -> IssueDraft {
        IssueDraft {
            issue_type: self
                .types
                .get(self.type_idx)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| "Story".into()),
            summary: self.summary.clone(),
            description: self.description.lines().join("\n"),
            priority: self
                .priorities
                .get(self.priority_idx)
                .map(|p| p.name.clone()),
            assignee_account_id: self
                .assignees
                .get(self.assignee_idx)
                .and_then(|choice| choice.account_id.clone()),
            story_points: self.story_points.parse().ok(),
            sprint_id: self.sprints.get(self.sprint_idx).and_then(|s| s.id),
        }
    }
}

fn styled_textarea(placeholder: &str) -> TextArea<'static> {
    let mut textarea = TextArea::default();
    textarea.set_placeholder_text(placeholder);
    textarea.set_cursor_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    textarea.set_cursor_line_style(Style::default());
    textarea
}

fn move_sel(selected: &mut usize, len: usize, key: KeyEvent) {
    if len == 0 {
        return;
    }
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => *selected = (*selected + 1).min(len - 1),
        KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
        KeyCode::Home | KeyCode::Char('g') => *selected = 0,
        KeyCode::End | KeyCode::Char('G') => *selected = len - 1,
        _ => {}
    }
}

fn apply_create_meta(form: &mut IssueForm, meta: CreateMeta, defaults: Option<(&str, &str)>) {
    if !meta.issue_types.is_empty() {
        let preferred = defaults
            .map(|(ty, _)| ty.to_string())
            .or_else(|| form.types.get(form.type_idx).map(|t| t.name.clone()));
        form.types = meta.issue_types;
        form.type_idx = preferred
            .and_then(|name| {
                form.types
                    .iter()
                    .position(|t| t.name.eq_ignore_ascii_case(&name))
            })
            .unwrap_or(0);
    }
    if !meta.priorities.is_empty() {
        let preferred = defaults.map(|(_, pri)| pri.to_string()).or_else(|| {
            form.priorities
                .get(form.priority_idx)
                .map(|p| p.name.clone())
        });
        form.priorities = meta.priorities;
        form.priority_idx = preferred
            .and_then(|name| {
                form.priorities
                    .iter()
                    .position(|p| p.name.eq_ignore_ascii_case(&name))
            })
            .unwrap_or(0);
    }
}

fn select_sprint_idx(
    choices: &mut Vec<SprintChoice>,
    issue: &Issue,
    tab_kind: Option<&TabKind>,
) -> usize {
    let found = crate::jira::search::sprints_from_issue(&issue.raw);
    let current = found
        .iter()
        .find(|sprint| sprint.state == "active")
        .or_else(|| found.first());
    if let Some(current) = current {
        if let Some(idx) = choices.iter().position(|c| c.id == Some(current.id)) {
            return idx;
        }
        choices.push(SprintChoice {
            id: Some(current.id),
            name: current.name.clone(),
        });
        return choices.len() - 1;
    }
    match tab_kind {
        Some(TabKind::Sprint(id)) => choices.iter().position(|c| c.id == Some(*id)).unwrap_or(0),
        _ => 0,
    }
}

fn sprint_choices(tabs: &[Tab]) -> Vec<SprintChoice> {
    let mut choices = vec![SprintChoice {
        id: None,
        name: "Backlog".into(),
    }];
    for tab in tabs {
        if let TabKind::Sprint(id) = tab.kind {
            choices.push(SprintChoice {
                id: Some(id),
                name: tab.title.clone(),
            });
        }
    }
    choices
}

struct FormFocus {
    type_: usize,
    priority: usize,
    sprint: Option<usize>,
    assignee: usize,
    summary: usize,
    points: usize,
    description: usize,
    submit: usize,
    cancel: usize,
    count: usize,
}

impl FormFocus {
    fn new() -> Self {
        Self {
            type_: 0,
            priority: 1,
            sprint: Some(2),
            assignee: 3,
            summary: 4,
            points: 5,
            description: 6,
            submit: 7,
            cancel: 8,
            count: 9,
        }
    }
}

fn cycle_idx(idx: &mut usize, len: usize, delta: isize) {
    if len == 0 {
        return;
    }
    *idx = ((*idx as isize + delta).rem_euclid(len as isize)) as usize;
}

fn picker_focused(form: &IssueForm, layout: &FormFocus) -> bool {
    form.focus == layout.type_
        || form.focus == layout.priority
        || layout.sprint == Some(form.focus)
        || form.focus == layout.assignee
}

fn nudge_picker(form: &mut IssueForm, layout: &FormFocus, delta: isize) {
    if form.focus == layout.type_ {
        cycle_idx(&mut form.type_idx, form.types.len(), delta);
    } else if form.focus == layout.priority {
        cycle_idx(&mut form.priority_idx, form.priorities.len(), -delta);
    } else if layout.sprint == Some(form.focus) {
        cycle_idx(&mut form.sprint_idx, form.sprints.len(), delta);
    } else if form.focus == layout.assignee {
        cycle_idx(&mut form.assignee_idx, form.assignees.len(), delta);
    }
}

fn unassigned_choice() -> AssigneeChoice {
    AssigneeChoice {
        account_id: None,
        label: "Unassigned".into(),
    }
}

fn apply_assignees(form: &mut IssueForm, users: Vec<User>, self_id: Option<&str>) {
    let selected = form
        .assignees
        .get(form.assignee_idx)
        .and_then(|choice| choice.account_id.clone());
    let mut choices = vec![unassigned_choice()];
    for user in users {
        let you = self_id == Some(user.account_id.as_str());
        let label = if you {
            format!("{} (you)", user.display_name)
        } else {
            user.display_name.clone()
        };
        choices.push(AssigneeChoice {
            account_id: Some(user.account_id),
            label,
        });
    }
    if let Some(id) = &selected {
        if !choices
            .iter()
            .any(|choice| choice.account_id.as_deref() == Some(id.as_str()))
        {
            let label = form
                .assignees
                .iter()
                .find(|choice| choice.account_id.as_deref() == Some(id.as_str()))
                .map(|choice| choice.label.clone())
                .unwrap_or_else(|| id.clone());
            choices.push(AssigneeChoice {
                account_id: Some(id.clone()),
                label,
            });
        }
    }
    form.assignee_idx = selected
        .as_ref()
        .and_then(|id| {
            choices
                .iter()
                .position(|choice| choice.account_id.as_deref() == Some(id.as_str()))
        })
        .unwrap_or(0);
    form.assignees = choices;
}

fn toggle_name(selected: &mut Vec<String>, name: &str) {
    if let Some(idx) = selected.iter().position(|item| item == name) {
        selected.remove(idx);
    } else {
        selected.push(name.to_string());
    }
}

fn sprint_title(sprint: &Sprint) -> String {
    let tag = match sprint.state.as_str() {
        "active" => "active",
        "future" => "future",
        other => other,
    };
    format!("{} ({tag})", sprint.name)
}
