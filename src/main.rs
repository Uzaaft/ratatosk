mod config;

use config::FieldConfig;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Span,
    widgets::{BarChart, Block, BorderType, Borders, List, ListItem},
};

use std::{
    collections::{HashMap, VecDeque},
    io::{self, BufRead},
    sync::mpsc,
    thread,
};

const MAX_SAMPLES: usize = 128;

enum AppEvent {
    LogLine(String),
    Input(Event),
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum Section {
    Traces,
    Errors,
}



#[inline]
fn likely_json_object(s: &str) -> bool {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' => return true,
            _ => return false,
        }
    }
    false
}

fn crop_str(s: &str, start_cols: usize, max_cols: usize) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow;
    if start_cols == 0 && s.chars().count() <= max_cols {
        return Cow::Borrowed(s);
    }
    let mut it = s.chars();
    for _ in 0..start_cols {
        if it.next().is_none() {
            return Cow::Borrowed("");
        }
    }
    let mut out = String::with_capacity(max_cols.min(64));
    for _ in 0..max_cols {
        if let Some(ch) = it.next() {
            out.push(ch);
        } else {
            break;
        }
    }
    Cow::Owned(out)
}

struct RouteStats {
    samples: VecDeque<u64>,
    sum: u128,
}

impl RouteStats {
    fn with_capacity(cap: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(cap),
            sum: 0,
        }
    }

    fn add(&mut self, latency: u64) {
        if self.samples.len() == MAX_SAMPLES {
            if let Some(old) = self.samples.pop_front() {
                self.sum -= old as u128;
            }
        }
        self.samples.push_back(latency);
        self.sum += latency as u128;
    }

    fn average(&self) -> u64 {
        if self.samples.is_empty() {
            0
        } else {
            (self.sum / (self.samples.len() as u128)) as u64
        }
    }
}

struct App {
    cfg: FieldConfig,
    traces: VecDeque<String>,
    latencies: HashMap<String, RouteStats>,
    errors: VecDeque<String>,
    bar_cache_dirty: bool,
    cached_bar_data: Vec<(String, u64)>,
    traces_scroll: usize,
    traces_view_rows: usize,
    errors_scroll: usize,
    errors_view_rows: usize,
    selected_section: Section,
    traces_h_scroll: usize,
    traces_view_cols: usize,
    traces_max_line_width: usize,
    errors_h_scroll: usize,
    errors_view_cols: usize,
    errors_max_line_width: usize,
}

impl App {
    fn new(cfg: FieldConfig) -> Self {
        Self {
            cfg,
            traces: VecDeque::with_capacity(1000),
            latencies: HashMap::new(),
            errors: VecDeque::with_capacity(500),
            bar_cache_dirty: true,
            cached_bar_data: Vec::new(),
            traces_scroll: 0,
            traces_view_rows: 0,
            errors_scroll: 0,
            errors_view_rows: 0,
            selected_section: Section::Traces,
            traces_h_scroll: 0,
            traces_view_cols: 0,
            traces_max_line_width: 0,
            errors_h_scroll: 0,
            errors_view_cols: 0,
            errors_max_line_width: 0,
        }
    }

    fn max_traces_scroll(&self) -> usize {
        self.traces.len().saturating_sub(self.traces_view_rows)
    }

    fn clamp_traces_scroll(&mut self) {
        let max_off = self.max_traces_scroll();
        if self.traces_scroll > max_off {
            self.traces_scroll = max_off;
        }
    }

    fn max_errors_scroll(&self) -> usize {
        self.errors.len().saturating_sub(self.errors_view_rows)
    }

    fn clamp_errors_scroll(&mut self) {
        let max_off = self.max_errors_scroll();
        if self.errors_scroll > max_off {
            self.errors_scroll = max_off;
        }
    }

    fn scroll_up(&mut self) {
        match self.selected_section {
            Section::Traces => {
                let max_off = self.max_traces_scroll();
                self.traces_scroll = (self.traces_scroll + 1).min(max_off);
            }
            Section::Errors => {
                let max_off = self.max_errors_scroll();
                self.errors_scroll = (self.errors_scroll + 1).min(max_off);
            }
        }
    }

    fn scroll_down(&mut self) {
        match self.selected_section {
            Section::Traces => {
                self.traces_scroll = self.traces_scroll.saturating_sub(1);
            }
            Section::Errors => {
                self.errors_scroll = self.errors_scroll.saturating_sub(1);
            }
        }
    }

    fn page_up(&mut self) {
        let page = match self.selected_section {
            Section::Traces => self.traces_view_rows.saturating_sub(1).max(1),
            Section::Errors => self.errors_view_rows.saturating_sub(1).max(1),
        };
        match self.selected_section {
            Section::Traces => {
                let max_off = self.max_traces_scroll();
                self.traces_scroll = (self.traces_scroll + page).min(max_off);
            }
            Section::Errors => {
                let max_off = self.max_errors_scroll();
                self.errors_scroll = (self.errors_scroll + page).min(max_off);
            }
        }
    }

    fn page_down(&mut self) {
        let page = match self.selected_section {
            Section::Traces => self.traces_view_rows.saturating_sub(1).max(1),
            Section::Errors => self.errors_view_rows.saturating_sub(1).max(1),
        };
        match self.selected_section {
            Section::Traces => {
                self.traces_scroll = self.traces_scroll.saturating_sub(page);
            }
            Section::Errors => {
                self.errors_scroll = self.errors_scroll.saturating_sub(page);
            }
        }
    }

    fn to_home(&mut self) {
        match self.selected_section {
            Section::Traces => self.traces_scroll = 0,
            Section::Errors => self.errors_scroll = 0,
        }
    }

    fn to_end(&mut self) {
        match self.selected_section {
            Section::Traces => self.traces_scroll = self.max_traces_scroll(),
            Section::Errors => self.errors_scroll = self.max_errors_scroll(),
        }
    }

    fn max_traces_h_scroll(&self) -> usize {
        self.traces_max_line_width
            .saturating_sub(self.traces_view_cols)
    }

    fn clamp_traces_h_scroll(&mut self) {
        let max = self.max_traces_h_scroll();
        if self.traces_h_scroll > max {
            self.traces_h_scroll = max;
        }
    }

    fn max_errors_h_scroll(&self) -> usize {
        self.errors_max_line_width
            .saturating_sub(self.errors_view_cols)
    }

    fn clamp_errors_h_scroll(&mut self) {
        let max = self.max_errors_h_scroll();
        if self.errors_h_scroll > max {
            self.errors_h_scroll = max;
        }
    }

    fn hscroll_left(&mut self) {
        match self.selected_section {
            Section::Traces => {
                self.traces_h_scroll = self.traces_h_scroll.saturating_sub(1);
            }
            Section::Errors => {
                self.errors_h_scroll = self.errors_h_scroll.saturating_sub(1);
            }
        }
    }

    fn hscroll_right(&mut self) {
        match self.selected_section {
            Section::Traces => {
                let max_off = self.max_traces_h_scroll();
                self.traces_h_scroll = (self.traces_h_scroll + 1).min(max_off);
            }
            Section::Errors => {
                let max_off = self.max_errors_h_scroll();
                self.errors_h_scroll = (self.errors_h_scroll + 1).min(max_off);
            }
        }
    }

    fn toggle_section(&mut self) {
        self.selected_section = match self.selected_section {
            Section::Traces => Section::Errors,
            Section::Errors => Section::Traces,
        };
    }

    fn handle_nav_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Tab | KeyCode::BackTab => self.toggle_section(),
            KeyCode::Up => self.scroll_up(),
            KeyCode::Down => self.scroll_down(),
            KeyCode::Left => self.hscroll_left(),
            KeyCode::Right => self.hscroll_right(),
            KeyCode::PageUp => self.page_up(),
            KeyCode::PageDown => self.page_down(),
            KeyCode::Home => self.to_home(),
            KeyCode::End => self.to_end(),
            KeyCode::Char('j') => self.scroll_down(),
            KeyCode::Char('k') => self.scroll_up(),
            KeyCode::Char('h') => self.hscroll_left(),
            KeyCode::Char('l') => self.hscroll_right(),
            _ => {}
        }
    }

    fn add_trace(&mut self, line: String) {
        if self.traces.len() == 1000 {
            self.traces.pop_front();
        }
        self.traces.push_back(line);
        self.clamp_traces_scroll();
    }

    fn add_latency(&mut self, route: &str, latency: u64) {
        if let Some(stats) = self.latencies.get_mut(route) {
            stats.add(latency);
        } else {
            let mut stats = RouteStats::with_capacity(MAX_SAMPLES);
            stats.add(latency);
            self.latencies.insert(route.to_owned(), stats);
        }
        self.bar_cache_dirty = true;
    }

    fn add_error(&mut self, error: String) {
        if self.errors.len() == 500 {
            self.errors.pop_front();
        }
        self.errors.push_back(error);
        self.clamp_errors_scroll();
    }

    fn parse_log_line(&mut self, line: String) {
        self.add_trace(line.clone());

        if line.contains("ERROR") || line.contains("WARN") {
            self.add_error(line.clone());
        }

        if likely_json_object(&line) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                if let (Some(route), Some(latency)) = (
                    self.cfg.extract_route(&json),
                    self.cfg.extract_latency(&json),
                ) {
                    self.add_latency(&route, latency);
                    return;
                }
            }
        }

        let route_key = format!("{}=", self.cfg.route_field_raw);
        let latency_key = format!("{}=", self.cfg.latency_field_raw);

        if let Some(start) = line.find(&route_key) {
            let rest = &line[start + route_key.len()..];
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let route = &rest[..end];

            if let Some(lat) = line.find(&latency_key) {
                let lat_str = &line[lat + latency_key.len()..];
                let end = lat_str
                    .find(|c: char| !c.is_numeric())
                    .unwrap_or(lat_str.len());
                if let Ok(latency) = lat_str[..end].parse::<u64>() {
                    let scaled = latency / self.cfg.latency_scale.max(1);
                    self.add_latency(route, scaled);
                }
            }
        }
    }

    fn recompute_bar_cache(&mut self) {
        let mut items: Vec<(&str, u64)> = self
            .latencies
            .iter()
            .map(|(route, stats)| (route.as_str(), stats.average()))
            .collect();

        if items.len() <= 10 {
            items.sort_unstable_by(|a, b| b.1.cmp(&a.1));
        } else {
            let (top, _, _) = items.select_nth_unstable_by_key(10, |x| std::cmp::Reverse(x.1));
            top.sort_unstable_by(|a, b| b.1.cmp(&a.1));
            items.truncate(10);
        }

        self.cached_bar_data = items
            .into_iter()
            .map(|(route, avg)| (route.to_owned(), avg))
            .collect();

        self.bar_cache_dirty = false;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel::<AppEvent>();

    {
        let tx_lines = tx.clone();
        thread::spawn(move || {
            let stdin = io::stdin();
            let reader = stdin.lock();
            for line in reader.lines() {
                match line {
                    Ok(l) => {
                        if tx_lines.send(AppEvent::LogLine(l)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    {
        let tx_input = tx.clone();
        thread::spawn(move || {
            loop {
                match event::read() {
                    Ok(ev) => {
                        if tx_input.send(AppEvent::Input(ev)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    let cfg = config::load();
    let mut app = App::new(cfg);

    'main: loop {
        let first = match rx.recv() {
            Ok(ev) => ev,
            Err(_) => break,
        };

        let mut quit = false;
        match first {
            AppEvent::LogLine(line) => app.parse_log_line(line),
            AppEvent::Input(Event::Key(key)) => match key.code {
                KeyCode::Char('q') => quit = true,
                other => app.handle_nav_key(other),
            },
            AppEvent::Input(_) => {}
        }
        if quit {
            break 'main;
        }

        while let Ok(ev) = rx.try_recv() {
            match ev {
                AppEvent::LogLine(line) => app.parse_log_line(line),
                AppEvent::Input(Event::Key(key)) => match key.code {
                    KeyCode::Char('q') => {
                        quit = true;
                        break;
                    }
                    other => app.handle_nav_key(other),
                },
                AppEvent::Input(_) => {}
            }
        }
        if quit {
            break 'main;
        }

        if app.bar_cache_dirty {
            app.recompute_bar_cache();
        }

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(40),
                    Constraint::Percentage(30),
                    Constraint::Percentage(30),
                ])
                .split(f.area());

            let max_trace_lines = chunks[0].height.saturating_sub(2) as usize;
            app.traces_view_rows = max_trace_lines;
            app.clamp_traces_scroll();

            app.traces_view_cols = chunks[0].width.saturating_sub(2) as usize;
            app.traces_max_line_width = app
                .traces
                .iter()
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(0);
            app.clamp_traces_h_scroll();

            let max_error_lines = chunks[2].height.saturating_sub(2) as usize;
            app.errors_view_rows = max_error_lines;
            app.clamp_errors_scroll();

            app.errors_view_cols = chunks[2].width.saturating_sub(2) as usize;
            app.errors_max_line_width = app
                .errors
                .iter()
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(0);
            app.clamp_errors_h_scroll();

            let mut trace_items: Vec<ListItem> =
                Vec::with_capacity(max_trace_lines.min(app.traces.len()));
            for t in app
                .traces
                .iter()
                .rev()
                .skip(app.traces_scroll)
                .take(app.traces_view_rows)
            {
                let cropped = crop_str(t, app.traces_h_scroll, app.traces_view_cols);
                trace_items.push(ListItem::new(cropped.to_string()));
            }

            let traces_focused = app.selected_section == Section::Traces;
            let traces_block = Block::default()
                .borders(Borders::ALL)
                .border_type(if traces_focused {
                    BorderType::Thick
                } else {
                    BorderType::Plain
                })
                .border_style(if traces_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                })
                .title(if traces_focused {
                    "Traces (focused) — Tab to switch (↑/↓ PgUp/PgDn Home/End, h/j/k/l)"
                } else {
                    "Traces — Tab to switch"
                });

            let traces_widget = List::new(trace_items).block(traces_block);
            f.render_widget(traces_widget, chunks[0]);

            let bar_data: Vec<(&str, u64)> = app
                .cached_bar_data
                .iter()
                .map(|(route, avg)| (route.as_str(), *avg))
                .collect();

            let bar_title = format!(
                "Route Latencies (avg) [{}={}, {}={}]",
                app.cfg.route_field_raw,
                if app.cfg.route_path.segments.len() > 1 { "nested" } else { "flat" },
                app.cfg.latency_field_raw,
                if app.cfg.latency_scale == 1 { "ms" } else { &format!("/{}", app.cfg.latency_scale) }
            );

            let barchart = BarChart::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(bar_title),
                )
                .data(&bar_data)
                .bar_width(9)
                .bar_style(Style::default().fg(Color::Cyan))
                .value_style(Style::default().fg(Color::White));
            f.render_widget(barchart, chunks[1]);

            let mut error_items: Vec<ListItem> =
                Vec::with_capacity(app.errors_view_rows.min(app.errors.len()));
            for e in app
                .errors
                .iter()
                .rev()
                .skip(app.errors_scroll)
                .take(app.errors_view_rows)
            {
                let style = if e.contains("ERROR") {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                let cropped = crop_str(e, app.errors_h_scroll, app.errors_view_cols);
                error_items.push(ListItem::new(Span::styled(cropped.to_string(), style)));
            }

            let errors_focused = app.selected_section == Section::Errors;
            let errors_block = Block::default()
                .borders(Borders::ALL)
                .border_type(if errors_focused {
                    BorderType::Thick
                } else {
                    BorderType::Plain
                })
                .border_style(if errors_focused {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                })
                .title(if errors_focused {
                    "Errors / Warnings (focused) — Tab to switch (↑/↓ PgUp/PgDn Home/End, h/j/k/l)"
                } else {
                    "Errors / Warnings — Tab to switch"
                });

            let errors_widget = List::new(error_items).block(errors_block);
            f.render_widget(errors_widget, chunks[2]);
        })?;
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}
