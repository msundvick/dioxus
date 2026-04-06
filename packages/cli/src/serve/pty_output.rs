use crate::{
    serve::{ServeUpdate, WebServer},
    BuildId, BuildStage, BuilderUpdate, BundleFormat, TraceMsg, TraceSrc,
};
use anyhow::{Context, Result};
use crossterm::{
    cursor::Show,
    event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use futures_util::StreamExt;
use portable_pty::PtySize;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Borders, Paragraph},
};
use std::{
    collections::VecDeque,
    io::{self, stdout, Read, Write},
    rc::Rc,
    cell::RefCell,
    time::Duration,
};
use tokio::sync::mpsc;
use tracing::Level;

use super::AppServer;

pub(crate) const SIDEBAR_WIDTH: u16 = 28;
const TICK_RATE_MS: u64 = 100;

/// A TUI output mode that shows a dx sidebar alongside an embedded PTY terminal.
///
/// The child process runs inside a real PTY, so it sees a terminal (isatty()=true) and can use
/// raw mode, cursor positioning, alternate screen, etc. The vt100 crate parses the PTY output
/// into a screen buffer which ratatui renders in the right pane.
///
/// Input routing: All keystrokes go to the PTY by default. Ctrl+B is a prefix key (tmux-style)
/// that captures the next keystroke as a dx serve command. Ctrl+C always exits.
pub(crate) struct PtyOutput {
    // Rc<RefCell> to avoid borrow conflicts between term.draw() and self methods
    term: Rc<RefCell<Terminal<CrosstermBackend<io::Stdout>>>>,
    events: Option<EventStream>,

    // PTY state
    pty_master: Box<dyn portable_pty::MasterPty + Send>,
    pty_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    pty_writer: Box<dyn Write + Send>,
    screen: vt100::Parser,

    // UI state
    prefix_active: bool,
    sidebar_width: u16,

    // Build info display
    dx_version: String,
    verbose: bool,
    tick_animation: bool,
    tick_interval: tokio::time::Interval,
    throbber: RefCell<throbber_widgets_tui::ThrobberState>,
    pending_logs: VecDeque<TraceMsg>,
}

impl PtyOutput {
    pub(crate) fn start(
        pty_master: Box<dyn portable_pty::MasterPty + Send>,
        pty_size: PtySize,
    ) -> Result<Self> {
        enable_raw_mode().context("Failed to enable raw mode for app terminal")?;
        stdout()
            .execute(EnterAlternateScreen)?
            .execute(crossterm::cursor::Hide)?;

        let term = Terminal::new(CrosstermBackend::new(stdout()))?;
        let events = EventStream::new();

        let reader = pty_master
            .try_clone_reader()
            .context("Failed to clone PTY reader")?;
        let writer = pty_master
            .take_writer()
            .context("Failed to take PTY writer")?;

        let (pty_tx, pty_rx) = mpsc::unbounded_channel();
        std::thread::spawn(move || {
            pty_read_loop(reader, pty_tx);
        });

        let screen = vt100::Parser::new(pty_size.rows, pty_size.cols, 1000);

        Ok(Self {
            term: Rc::new(RefCell::new(term)),
            events: Some(events),
            pty_master,
            pty_rx,
            pty_writer: writer,
            screen,
            prefix_active: false,
            sidebar_width: SIDEBAR_WIDTH,
            dx_version: format!(
                "{}-{}",
                env!("CARGO_PKG_VERSION"),
                crate::dx_build_info::GIT_COMMIT_HASH_SHORT.unwrap_or("main")
            ),
            verbose: false,
            tick_animation: false,
            tick_interval: {
                let mut interval = tokio::time::interval(Duration::from_millis(TICK_RATE_MS));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                interval
            },
            throbber: RefCell::new(throbber_widgets_tui::ThrobberState::default()),
            pending_logs: VecDeque::new(),
        })
    }

    pub(crate) async fn wait(&mut self) -> ServeUpdate {
        use futures_util::future::OptionFuture;
        loop {
            let next_event =
                OptionFuture::from(self.events.as_mut().map(|e| e.next()));
            tokio::select! {
                biased;
                Some(data) = self.pty_rx.recv() => {
                    self.screen.process(&data);
                    return ServeUpdate::Redraw;
                }
                Some(Some(Ok(event))) = next_event => {
                    match self.handle_input(event) {
                        Ok(Some(update)) => return update,
                        Ok(None) => continue,
                        Err(e) => return ServeUpdate::Exit { error: Some(e) },
                    }
                }
                _ = self.tick_interval.tick(), if self.tick_animation => {
                    self.throbber.borrow_mut().calc_next();
                    return ServeUpdate::Redraw;
                }
            }
        }
    }

    fn handle_input(&mut self, event: Event) -> Result<Option<ServeUpdate>> {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_keypress(key),
            Event::Resize(cols, rows) => {
                let pty_cols = cols.saturating_sub(self.sidebar_width + 1);
                let pty_rows = rows;
                let _ = self.pty_master.resize(PtySize {
                    rows: pty_rows,
                    cols: pty_cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
                self.screen.set_size(pty_rows, pty_cols);
                Ok(Some(ServeUpdate::Redraw))
            }
            _ => Ok(None),
        }
    }

    fn handle_keypress(&mut self, key: KeyEvent) -> Result<Option<ServeUpdate>> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(Some(ServeUpdate::Exit { error: None }));
        }

        if self.prefix_active {
            self.prefix_active = false;
            return self.handle_prefix_command(key);
        }

        if key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.prefix_active = true;
            return Ok(Some(ServeUpdate::Redraw));
        }

        self.forward_key_to_pty(key)?;
        Ok(None)
    }

    fn handle_prefix_command(&mut self, key: KeyEvent) -> Result<Option<ServeUpdate>> {
        match key.code {
            KeyCode::Char('r') => Ok(Some(ServeUpdate::RequestRebuild)),
            KeyCode::Char('o') => Ok(Some(ServeUpdate::OpenApp)),
            KeyCode::Char('p') => Ok(Some(ServeUpdate::ToggleShouldRebuild)),
            KeyCode::Char('v') => {
                self.verbose = !self.verbose;
                tracing::info!(
                    "Verbose logging is now {}",
                    if self.verbose { "on" } else { "off" }
                );
                Ok(Some(ServeUpdate::Redraw))
            }
            KeyCode::Char('d') => Ok(Some(ServeUpdate::OpenDebugger {
                id: BuildId::PRIMARY,
            })),
            KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.pty_writer.write_all(&[0x02])?;
                self.pty_writer.flush()?;
                Ok(None)
            }
            _ => Ok(Some(ServeUpdate::Redraw)),
        }
    }

    fn forward_key_to_pty(&mut self, key: KeyEvent) -> Result<()> {
        let bytes: Vec<u8> = match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                vec![(c as u8) & 0x1f]
            }
            KeyCode::Char(c) => {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf);
                buf[..c.len_utf8()].to_vec()
            }
            KeyCode::Enter => vec![b'\r'],
            KeyCode::Backspace => vec![0x7f],
            KeyCode::Tab => vec![b'\t'],
            KeyCode::Esc => vec![0x1b],
            KeyCode::Up => b"\x1b[A".to_vec(),
            KeyCode::Down => b"\x1b[B".to_vec(),
            KeyCode::Right => b"\x1b[C".to_vec(),
            KeyCode::Left => b"\x1b[D".to_vec(),
            KeyCode::Home => b"\x1b[H".to_vec(),
            KeyCode::End => b"\x1b[F".to_vec(),
            KeyCode::Delete => b"\x1b[3~".to_vec(),
            KeyCode::PageUp => b"\x1b[5~".to_vec(),
            KeyCode::PageDown => b"\x1b[6~".to_vec(),
            _ => return Ok(()),
        };
        self.pty_writer.write_all(&bytes)?;
        self.pty_writer.flush()?;
        Ok(())
    }

    // ---- Rendering ----

    pub(crate) fn render(&mut self, runner: &AppServer, server: &WebServer) {
        let owned_term = self.term.clone();
        let mut term = owned_term.borrow_mut();

        let _ = term.draw(|frame| {
            let area = frame.area();
            let [sidebar_area, pty_area]: [Rect; 2] = Layout::horizontal([
                Constraint::Length(self.sidebar_width),
                Constraint::Fill(1),
            ])
            .areas(area);

            render_sidebar(
                frame,
                sidebar_area,
                runner,
                server,
                &self.dx_version,
                self.prefix_active,
                &self.pending_logs,
            );
            render_pty_screen(frame, pty_area, self.screen.screen());
        });
    }

    // ---- State updates from the build engine ----

    pub(crate) fn push_log(&mut self, message: TraceMsg) {
        self.pending_logs.push_front(message);
        while self.pending_logs.len() > 100 {
            self.pending_logs.pop_back();
        }
    }

    pub fn push_stdio(&mut self, bundle: BundleFormat, msg: String, level: Level) {
        self.push_log(TraceMsg::text(TraceSrc::App(bundle), level, msg));
    }

    pub fn push_cargo_log(&mut self, message: cargo_metadata::diagnostic::Diagnostic) {
        self.push_log(TraceMsg::cargo(message));
    }

    pub fn push_ws_message(&mut self, bundle: BundleFormat, message: &axum::extract::ws::Message) {
        use dioxus_devtools_types::ClientMsg;
        let axum::extract::ws::Message::Text(text) = message else {
            return;
        };
        let Ok(ClientMsg::Log { level, messages }) = serde_json::from_str::<ClientMsg>(text.as_str()) else {
            return;
        };
        let content = messages.first().cloned().unwrap_or_default();
        let level = match level.as_str() {
            "trace" => Level::TRACE,
            "debug" => Level::DEBUG,
            "warn" => Level::WARN,
            "error" => Level::ERROR,
            _ => Level::INFO,
        };
        self.push_log(TraceMsg::text(TraceSrc::App(bundle), level, content));
    }

    pub(crate) fn new_build_update(&mut self, update: &BuilderUpdate) {
        match update {
            BuilderUpdate::Progress {
                stage: BuildStage::Starting { .. },
            } => self.tick_animation = true,
            BuilderUpdate::BuildReady { .. } => self.tick_animation = false,
            BuilderUpdate::BuildFailed { .. } => self.tick_animation = false,
            _ => {}
        }
    }

    pub(crate) fn shutdown(&mut self) {
        let _ = stdout().execute(Show);
        let _ = stdout().execute(LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }

}

impl Drop for PtyOutput {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---- Free rendering functions (avoid borrow conflicts with term.draw closure) ----

fn render_sidebar(
    frame: &mut Frame,
    area: Rect,
    runner: &AppServer,
    server: &WebServer,
    dx_version: &str,
    prefix_active: bool,
    pending_logs: &VecDeque<TraceMsg>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" dx serve ")
        .title_style(Style::default().fg(Color::Cyan).bold());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let client = runner.client();

    let [ver, status, platform, addr, sep1, keys, sep2, logs]: [Rect; 8] =
        Layout::vertical([
            Constraint::Length(1), // version
            Constraint::Length(1), // status
            Constraint::Length(1), // platform
            Constraint::Length(1), // address
            Constraint::Length(1), // separator
            Constraint::Length(7), // key reference
            Constraint::Length(1), // separator
            Constraint::Fill(1),  // logs
        ])
        .areas(inner);

    // Version
    frame.render_widget(
        Paragraph::new(format!("dx {dx_version}")).style(Style::default().fg(Color::DarkGray)),
        ver,
    );

    // Build status
    let status_text = match &client.stage {
        BuildStage::Success => "Ready".green(),
        BuildStage::Failed => "Failed".red(),
        BuildStage::Hotpatching => "Patching...".yellow(),
        BuildStage::Compiling { krate, .. } => format!("Building {krate}").yellow().into(),
        BuildStage::Linking => "Linking...".yellow(),
        _ => "Working...".yellow(),
    };
    frame.render_widget(
        Line::from(vec!["Status: ".dark_gray(), status_text]),
        status,
    );

    // Platform
    frame.render_widget(
        Paragraph::new(format!("Platform: {}", client.build.bundle))
            .style(Style::default().fg(Color::DarkGray)),
        platform,
    );

    // Address
    let address = format!("http://{}", server.devserver_address());
    frame.render_widget(
        Paragraph::new(address).style(Style::default().fg(Color::DarkGray)),
        addr,
    );

    // Separator
    let sep_line = "─".repeat(inner.width as usize);
    frame.render_widget(
        Paragraph::new(sep_line.clone()).style(Style::default().fg(Color::DarkGray)),
        sep1,
    );

    // Key reference
    let prefix_indicator = if prefix_active { " [^B]" } else { "" };
    let key_lines = vec![
        Line::from(vec![
            "Keys:".fg(Color::Cyan),
            prefix_indicator.yellow().into(),
        ]),
        Line::from(vec!["^B r".dark_gray(), " rebuild".white()]),
        Line::from(vec!["^B o".dark_gray(), " open".white()]),
        Line::from(vec!["^B p".dark_gray(), " pause".white()]),
        Line::from(vec!["^B v".dark_gray(), " verbose".white()]),
        Line::from(vec!["^B d".dark_gray(), " debugger".white()]),
        Line::from(vec!["^C  ".dark_gray(), " exit".white()]),
    ];
    frame.render_widget(Paragraph::new(key_lines), keys);

    // Separator
    frame.render_widget(
        Paragraph::new(sep_line).style(Style::default().fg(Color::DarkGray)),
        sep2,
    );

    // Recent logs
    let log_lines: Vec<Line> = pending_logs
        .iter()
        .rev()
        .take(logs.height as usize)
        .map(|log| {
            let content = match &log.content {
                crate::TraceContent::Text(t) => t.clone(),
                _ => String::new(),
            };
            let max_w = inner.width.saturating_sub(1) as usize;
            let truncated = if content.len() > max_w {
                format!("{}…", &content[..max_w.saturating_sub(1)])
            } else {
                content
            };
            Line::from(truncated.dark_gray())
        })
        .collect();
    frame.render_widget(Paragraph::new(log_lines), logs);
}

fn render_pty_screen(frame: &mut Frame, area: Rect, screen: &vt100::Screen) {
    let buf = frame.buffer_mut();
    let (screen_rows, screen_cols) = screen.size();

    for row in 0..area.height.min(screen_rows) {
        for col in 0..area.width.min(screen_cols) {
            if let Some(cell) = screen.cell(row, col) {
                if let Some(ratatui_cell) =
                    buf.cell_mut(Position::new(area.x + col, area.y + row))
                {
                    let contents = cell.contents();
                    let ch = contents.chars().next().unwrap_or(' ');
                    ratatui_cell.set_char(ch);
                    ratatui_cell.set_style(vt100_to_ratatui_style(cell));
                }
            }
        }
    }

    // Show cursor
    if !screen.hide_cursor() {
        let (cr, cc) = screen.cursor_position();
        if cr < area.height && cc < area.width {
            frame.set_cursor_position(Position::new(area.x + cc, area.y + cr));
        }
    }
}

fn vt100_to_ratatui_style(cell: &vt100::Cell) -> Style {
    let mut style = Style::default();
    style = style.fg(vt100_color_to_ratatui(cell.fgcolor()));
    style = style.bg(vt100_color_to_ratatui(cell.bgcolor()));
    if cell.bold() {
        style = style.bold();
    }
    if cell.italic() {
        style = style.italic();
    }
    if cell.underline() {
        style = style.underlined();
    }
    if cell.inverse() {
        style = style.reversed();
    }
    style
}

fn vt100_color_to_ratatui(color: vt100::Color) -> Color {
    match color {
        vt100::Color::Default => Color::Reset,
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Background thread that reads from the PTY master and sends data to the async runtime.
fn pty_read_loop(mut reader: Box<dyn Read + Send>, tx: mpsc::UnboundedSender<Vec<u8>>) {
    let mut buf = [0u8; 4096];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
}
