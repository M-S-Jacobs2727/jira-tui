use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::DefaultTerminal;
use ratatui::style::{Color, Modifier, Style};
use tokio::sync::mpsc;
use tui_textarea::TextArea;

use crate::auth::oauth::{AccessibleResource, OAuthClient, authorize_url, listen_for_callback};
use crate::auth::store::{StoredTokens, TokenStore};
use crate::config::Config;
use crate::error::Result;
use crate::jira::agile::AgileFacade;
use crate::jira::client::JiraClient;
use crate::jira::issues::{IssueDraft, IssueFacade};
use crate::jira::models::{
    Board, CreateMeta, Issue, IssueType, Priority, Sprint, Transition, User,
};
use crate::jira::search::{AssigneeFilter, SearchBuilder, SearchFacade, SortField, SprintRef};
use crate::ui;

#[derive(Debug, Clone)]
pub struct LoginState {
    pub client_id: String,
    pub client_secret: String,
    pub focus: usize,
    pub auth_url: Option<String>,
    pub oauth_state: Option<String>,
}

impl LoginState {
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        let client_id = client_id.into();
        let focus = usize::from(!client_id.is_empty());
        let mut state = Self {
            client_id,
            client_secret: client_secret.into(),
            focus,
            auth_url: None,
            oauth_state: None,
        };
        state.refresh_auth_link();
        state
    }

    pub fn refresh_auth_link(&mut self) {
        let client_id = self.client_id.trim();
        if client_id.is_empty() {
            self.auth_url = None;
            self.oauth_state = None;
            return;
        }
        match authorize_url(client_id) {
            Ok((url, state)) => {
                self.auth_url = Some(url);
                self.oauth_state = Some(state);
            }
            Err(_) => {
                self.auth_url = None;
                self.oauth_state = None;
            }
        }
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
    Help,
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
}

#[derive(Debug, Clone)]
pub struct FilterForm {
    pub statuses: String,
    pub types: String,
    pub assignee: AssigneeFilter,
    pub named: String,
    pub focus: usize,
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
    pub story_points: String,
    pub focus: usize,
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

#[derive(Debug, Clone)]
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
}

enum AppMsg {
    OauthStarted {
        url: String,
    },
    LoggedIn {
        tokens: StoredTokens,
        sites: Vec<AccessibleResource>,
    },
    Boards(Vec<Board>),
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
    pub tabs: Vec<Tab>,
    pub tab_idx: usize,
    pub status: String,
    pub status_is_error: bool,
    pub loading: bool,
    pub search_query: String,
    pub should_quit: bool,
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
        let login = LoginState::new(config.client_id.clone(), tokens.client_secret.clone());
        let mut app = Self {
            config: config.clone(),
            screen: Screen::Login(login.clone()),
            overlay: Overlay::None,
            tabs: Vec::new(),
            tab_idx: 0,
            status: String::new(),
            status_is_error: false,
            loading: false,
            search_query: String::new(),
            should_quit: false,
            client: None,
            tx,
        };

        if !config.client_id.is_empty() && tokens.has_refresh() && config.cloud_id.is_some() {
            if let Ok(client) = JiraClient::new(
                config.client_id.clone(),
                config.cloud_id.clone().unwrap_or_default(),
                tokens,
                config.story_points_field.clone(),
            ) {
                app.client = Some(client);
                if config.board_id.is_some() {
                    app.screen = Screen::Main;
                    app.refresh_board();
                } else {
                    app.status = "Select a board".into();
                    app.fetch_boards();
                }
            }
        }
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

    pub fn selected_issue(&self) -> Option<&Issue> {
        let tab = self.current_tab()?;
        tab.issues.get(tab.selected)
    }

    fn persist_view(&mut self, config: &mut Config) {
        config.view = self.config.view.clone();
        if let Err(err) = config.save() {
            self.set_status(format!("failed to save config: {err}"), true);
        } else {
            self.config.view = config.view.clone();
        }
    }

    fn handle_key(&mut self, key: KeyEvent, config: &mut Config) {
        match &mut self.screen {
            Screen::Login(state) => {
                let open_link =
                    key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL);
                let quit =
                    key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL);
                let auth_url = state.auth_url.clone();
                if !open_link && !quit {
                    handle_login_key(state, key);
                }
                let ready = !state.client_id.is_empty() && !state.client_secret.is_empty();
                let credentials = ready.then(|| {
                    (
                        state.client_id.clone(),
                        state.client_secret.clone(),
                        state.auth_url.clone(),
                        state.oauth_state.clone(),
                    )
                });
                if open_link {
                    if let Some((id, secret, url, oauth_state)) = credentials {
                        self.start_oauth(id, secret, url, oauth_state, config);
                    } else if let Some(url) = auth_url {
                        match open::that(&url) {
                            Ok(()) => self.set_status(
                                "opened Atlassian login; enter the client secret and press Enter to finish",
                                false,
                            ),
                            Err(err) => {
                                self.set_status(format!("could not open browser: {err}"), true)
                            }
                        }
                    } else {
                        self.set_status("enter a client id to generate the login link", true);
                    }
                    return;
                }
                if key.code == KeyCode::Enter {
                    if let Some((id, secret, url, oauth_state)) = credentials {
                        self.start_oauth(id, secret, url, oauth_state, config);
                    }
                } else if quit {
                    self.should_quit = true;
                }
                return;
            }
            Screen::OauthWait { url } => {
                let url = url.clone();
                let client_id = self.config.client_id.clone();
                if key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    if let Err(err) = open::that(&url) {
                        self.set_status(format!("could not open browser: {err}"), true);
                    }
                    return;
                }
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                    self.screen = Screen::Login(LoginState::new(client_id, String::new()));
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
        if matches!(self.overlay, Overlay::None) {
            self.handle_board_key(key, config);
            return;
        }
        if matches!(self.overlay, Overlay::Help) {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.overlay = Overlay::None;
            }
            return;
        }
        if let Overlay::Sort { selected } = &self.overlay {
            let selected = *selected;
            self.handle_sort_key(key, selected, config);
            return;
        }
        if let Overlay::Filter(form) = &self.overlay {
            let mut form = form.clone();
            if self.handle_filter_key(key, &mut form, config) {
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
                KeyCode::Char('j') | KeyCode::Down => *scroll = scroll.saturating_add(1),
                KeyCode::Char('k') | KeyCode::Up => *scroll = scroll.saturating_sub(1),
                KeyCode::Char('e') => {
                    if let Some(issue) = self.selected_issue().cloned() {
                        self.open_edit(issue);
                    }
                }
                KeyCode::Char('a') => self.open_assign(),
                KeyCode::Char('t') => self.open_transitions(),
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
            Overlay::None
            | Overlay::Help
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
            KeyCode::Char('?') => self.overlay = Overlay::Help,
            KeyCode::Char(':') => {
                self.overlay = Overlay::Command {
                    input: String::new(),
                }
            }
            KeyCode::Char('s') => {
                let selected = SortField::ALL
                    .iter()
                    .position(|f| *f == self.config.view.sort_field)
                    .unwrap_or(0);
                self.overlay = Overlay::Sort { selected };
            }
            KeyCode::Char('f') => {
                self.overlay = Overlay::Filter(FilterForm::from_view(&self.config.view));
            }
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
            KeyCode::Char('c') | KeyCode::Char('n') => self.open_create(),
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
            KeyCode::BackTab => self.switch_tab(-1),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('[') => self.switch_tab(-1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(']') => self.switch_tab(1),
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

    fn handle_sort_key(&mut self, key: KeyEvent, selected: usize, config: &mut Config) {
        let mut selected = selected;
        match key.code {
            KeyCode::Esc => {
                self.overlay = Overlay::None;
                return;
            }
            KeyCode::Char('d') => {
                self.config.view.sort_dir = self.config.view.sort_dir.toggle();
            }
            KeyCode::Enter => {
                self.config.view.sort_field = SortField::ALL[selected];
                self.persist_view(config);
                self.overlay = Overlay::None;
                self.reload_current_tab();
                return;
            }
            _ => move_sel(&mut selected, SortField::ALL.len(), key),
        }
        self.overlay = Overlay::Sort { selected };
    }

    fn handle_filter_key(
        &mut self,
        key: KeyEvent,
        form: &mut FilterForm,
        config: &mut Config,
    ) -> bool {
        match key.code {
            KeyCode::Esc => return true,
            KeyCode::Enter => {
                self.config.view.filter_statuses = split_csv(&form.statuses);
                self.config.view.filter_types = split_csv(&form.types);
                self.config.view.filter_assignee =
                    if matches!(form.assignee, AssigneeFilter::Account(_)) {
                        if form.named.is_empty() {
                            AssigneeFilter::Any
                        } else {
                            AssigneeFilter::Account(form.named.clone())
                        }
                    } else {
                        form.assignee.clone()
                    };
                self.persist_view(config);
                self.reload_current_tab();
                return true;
            }
            KeyCode::Tab | KeyCode::Down => form.focus = (form.focus + 1) % 4,
            KeyCode::BackTab | KeyCode::Up => form.focus = (form.focus + 3) % 4,
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.focus == 2 => {
                form.assignee = cycle_assignee(&form.assignee);
            }
            KeyCode::Backspace => match form.focus {
                0 => {
                    form.statuses.pop();
                }
                1 => {
                    form.types.pop();
                }
                3 => {
                    form.named.pop();
                }
                _ => {}
            },
            KeyCode::Char(c) => match form.focus {
                0 => form.statuses.push(c),
                1 => form.types.push(c),
                3 => form.named.push(c),
                _ => {}
            },
            _ => {}
        }
        false
    }

    fn handle_issue_form_key(&mut self, key: KeyEvent) {
        let layout = FormFocus::new();
        let mut submit = false;
        let mut close = false;
        {
            let (Overlay::Create(form) | Overlay::Edit(form)) = &mut self.overlay else {
                return;
            };
            if form.focus == layout.description
                && !matches!(key.code, KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab)
            {
                if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
                    submit = true;
                } else {
                    form.description.input(key);
                }
            } else {
                match key.code {
                    KeyCode::Esc => close = true,
                    KeyCode::Tab => form.focus = (form.focus + 1) % layout.count,
                    KeyCode::BackTab => {
                        form.focus = (form.focus + layout.count - 1) % layout.count;
                    }
                    KeyCode::Enter => submit = true,
                    KeyCode::Left | KeyCode::Right => {
                        let delta = if key.code == KeyCode::Left { -1 } else { 1 };
                        nudge_picker(form, &layout, delta);
                    }
                    KeyCode::Char(c @ ('h' | '[' | 'l' | ']')) if picker_focused(form, &layout) => {
                        let delta = if matches!(c, 'h' | '[') { -1 } else { 1 };
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
                        } else if form.focus == layout.points {
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
            "login" => {
                self.screen = Screen::Login(LoginState::new(
                    self.config.client_id.clone(),
                    String::new(),
                ));
                self.overlay = Overlay::None;
            }
            other => self.set_status(format!("unknown command: {other}"), true),
        }
        let _ = config;
    }

    fn start_oauth(
        &mut self,
        client_id: String,
        client_secret: String,
        auth_url: Option<String>,
        oauth_state: Option<String>,
        config: &mut Config,
    ) {
        self.config.client_id = client_id.clone();
        config.client_id = client_id.clone();
        let _ = config.save();
        self.loading = true;
        self.set_status("starting OAuth…", false);
        let prepared = match (auth_url, oauth_state) {
            (Some(url), Some(state)) => Ok((url, state)),
            _ => authorize_url(&client_id),
        };
        let tx = self.tx.clone();
        tokio::spawn(async move {
            match prepared {
                Ok((url, state)) => {
                    let _ = tx.send(AppMsg::OauthStarted { url: url.clone() });
                    if let Err(err) = open::that(&url) {
                        tracing::warn!("failed to open browser: {err}");
                    }
                    let result = async {
                        let code = listen_for_callback(&state).await?;
                        let oauth = OAuthClient::new(&client_id, &client_secret)?;
                        let tokens = oauth.exchange_code(&code).await?;
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
        self.config.cloud_id = Some(site.id.clone());
        config.cloud_id = Some(site.id.clone());
        let _ = config.save();
        let tokens = TokenStore::load().unwrap_or_default();
        match JiraClient::new(
            self.config.client_id.clone(),
            site.id,
            tokens,
            self.config.story_points_field.clone(),
        ) {
            Ok(client) => {
                self.client = Some(client);
                self.fetch_boards();
            }
            Err(err) => self.set_status(err.to_string(), true),
        }
    }

    fn select_board(&mut self, board: Board, config: &mut Config) {
        self.config.board_id = Some(board.id);
        self.config.project_key = board.project_key.clone();
        config.board_id = Some(board.id);
        config.project_key = board.project_key.clone();
        let _ = config.save();
        self.screen = Screen::Main;
        self.refresh_board();
    }

    fn logout(&mut self) {
        self.loading = true;
        let client_id = self.config.client_id.clone();
        let tokens = TokenStore::load().unwrap_or_default();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if !tokens.access_token.is_empty() {
                if let Ok(oauth) = OAuthClient::new(&client_id, &tokens.client_secret) {
                    let _ = oauth.revoke(&tokens.access_token).await;
                    if !tokens.refresh_token.is_empty() {
                        let _ = oauth.revoke(&tokens.refresh_token).await;
                    }
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
            self.fetch_boards();
            return;
        };
        self.loading = true;
        self.set_status("loading sprints…", false);
        let tx = self.tx.clone();
        let project = self.config.project_key.clone();
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
        if self.config.story_points_field.is_some() {
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

    fn fetch_boards(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        self.loading = true;
        self.set_status("loading boards…", false);
        let project = self.config.project_key.clone();
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
                    let _ = tx.send(AppMsg::Boards(boards));
                }
                Err(err) => {
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
        if tab.selected + 5 >= tab.issues.len() {
            if tab.next_page.is_some() {
                self.load_more();
            }
        }
    }

    fn move_issue_abs(&mut self, idx: usize) {
        if let Some(tab) = self.current_tab_mut() {
            if !tab.issues.is_empty() {
                tab.selected = idx.min(tab.issues.len() - 1);
            }
        }
    }

    fn builder_for_tab(&self, tab: &Tab) -> SearchBuilder {
        let mut builder = SearchBuilder::new()
            .status(self.config.view.filter_statuses.clone())
            .issue_type(self.config.view.filter_types.clone())
            .assignee(self.config.view.filter_assignee.clone())
            .order_by(self.config.view.sort_field, self.config.view.sort_dir)
            .story_points_field(self.config.story_points_field.clone())
            .text(self.search_query.clone());
        if let Some(project) = &self.config.project_key {
            builder = builder.project(project);
        }
        builder = match tab.kind {
            TabKind::Sprint(id) => builder.sprint(SprintRef::Id(id)),
            TabKind::Backlog => builder.sprint(SprintRef::Backlog),
        };
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
        if append && tab.next_page.is_none() {
            return;
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
    }

    fn open_edit(&mut self, issue: Issue) {
        let tab_kind = self.current_tab().map(|tab| tab.kind.clone());
        let mut form = IssueForm::from_issue(&issue);
        form.sprints = sprint_choices(&self.tabs);
        form.sprint_idx = select_sprint_idx(&mut form.sprints, &issue, tab_kind.as_ref());
        form.focus = FormFocus::new().summary;
        self.overlay = Overlay::Edit(form);
        self.fetch_create_meta();
    }

    fn fetch_create_meta(&mut self) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(project) = self.config.project_key.clone() else {
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
        let Some(project) = self.config.project_key.clone() else {
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
                facade
                    .create(&project, &draft)
                    .await
                    .map(|key| format!("created {key}"))
            } else if let Some(key) = key {
                facade
                    .update(&key, &draft)
                    .await
                    .map(|()| format!("updated {key}"))
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

    fn search_users(&mut self, query: String) {
        let Some(client) = self.client.clone() else {
            return;
        };
        let Some(project) = self.config.project_key.clone() else {
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

    fn handle_msg(&mut self, msg: AppMsg, config: &mut Config) {
        match msg {
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
            AppMsg::Boards(boards) => {
                if boards.is_empty() {
                    self.set_status("no boards found", true);
                    return;
                }
                if let Some(id) = self.config.board_id {
                    if let Some(board) = boards.iter().find(|b| b.id == id).cloned() {
                        self.select_board(board, config);
                        return;
                    }
                }
                self.screen = Screen::BoardPicker {
                    boards,
                    selected: 0,
                };
                self.set_status("select a board", false);
            }
            AppMsg::Sprints(sprints) => {
                let mut tabs: Vec<Tab> = sprints
                    .into_iter()
                    .map(|sprint| Tab {
                        kind: TabKind::Sprint(sprint.id),
                        title: sprint_title(&sprint),
                        issues: Vec::new(),
                        selected: 0,
                        next_page: None,
                        loaded: false,
                    })
                    .collect();
                tabs.push(Tab {
                    kind: TabKind::Backlog,
                    title: "Backlog".into(),
                    issues: Vec::new(),
                    selected: 0,
                    next_page: None,
                    loaded: false,
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
            }
            AppMsg::Issue(issue) => {
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
                if let Overlay::Assign(form) = &mut self.overlay {
                    form.users = users;
                    form.selected = 0;
                }
            }
            AppMsg::Transitions(items) => {
                self.overlay = Overlay::Transition { items, selected: 0 };
                self.set_status(String::new(), false);
            }
            AppMsg::Done { message, refresh } => {
                if let Some(field) = message.strip_prefix("__sp_field__:") {
                    self.config.story_points_field = Some(field.to_string());
                    config.story_points_field = Some(field.to_string());
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
                self.screen = Screen::Login(LoginState::new(
                    self.config.client_id.clone(),
                    String::new(),
                ));
                self.set_status("logged out", false);
            }
            AppMsg::Error(err) => self.set_status(err, true),
        }
    }
}

impl FilterForm {
    fn from_view(view: &crate::config::ViewConfig) -> Self {
        let named = match &view.filter_assignee {
            AssigneeFilter::Account(id) => id.clone(),
            _ => String::new(),
        };
        Self {
            statuses: view.filter_statuses.join(", "),
            types: view.filter_types.join(", "),
            assignee: view.filter_assignee.clone(),
            named,
            focus: 0,
        }
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
                .map(|n| n.to_string())
                .unwrap_or_default(),
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
            assignee_account_id: None,
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

fn handle_login_key(state: &mut LoginState, key: KeyEvent) {
    let client_id_changed = match key.code {
        KeyCode::Tab | KeyCode::Down => {
            state.focus = (state.focus + 1) % 2;
            false
        }
        KeyCode::BackTab | KeyCode::Up => {
            state.focus = (state.focus + 1) % 2;
            false
        }
        KeyCode::Backspace => {
            if state.focus == 0 {
                state.client_id.pop();
                true
            } else {
                state.client_secret.pop();
                false
            }
        }
        KeyCode::Char(c) => {
            if state.focus == 0 {
                state.client_id.push(c);
                true
            } else {
                state.client_secret.push(c);
                false
            }
        }
        _ => false,
    };
    if client_id_changed {
        state.refresh_auth_link();
    }
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
    summary: usize,
    points: usize,
    description: usize,
    count: usize,
}

impl FormFocus {
    fn new() -> Self {
        Self {
            type_: 0,
            priority: 1,
            sprint: Some(2),
            summary: 3,
            points: 4,
            description: 5,
            count: 6,
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
    form.focus == layout.type_ || form.focus == layout.priority || layout.sprint == Some(form.focus)
}

fn nudge_picker(form: &mut IssueForm, layout: &FormFocus, delta: isize) {
    if form.focus == layout.type_ {
        cycle_idx(&mut form.type_idx, form.types.len(), delta);
    } else if form.focus == layout.priority {
        cycle_idx(&mut form.priority_idx, form.priorities.len(), -delta);
    } else if layout.sprint == Some(form.focus) {
        cycle_idx(&mut form.sprint_idx, form.sprints.len(), delta);
    }
}

fn split_csv(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn cycle_assignee(current: &AssigneeFilter) -> AssigneeFilter {
    match current {
        AssigneeFilter::Any => AssigneeFilter::Me,
        AssigneeFilter::Me => AssigneeFilter::Unassigned,
        AssigneeFilter::Unassigned => AssigneeFilter::Account(String::new()),
        AssigneeFilter::Account(_) => AssigneeFilter::Any,
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

impl FilterForm {
    pub fn assignee_label(&self) -> String {
        match &self.assignee {
            AssigneeFilter::Any => "any".into(),
            AssigneeFilter::Me => "me".into(),
            AssigneeFilter::Unassigned => "unassigned".into(),
            AssigneeFilter::Account(_) => "named".into(),
        }
    }
}
