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
    widgets::{BarChart, Block, Borders, List, ListItem},
};
use serde::Deserialize;
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    io::{self, BufRead},
    sync::mpsc,
    thread,
    time::Duration,
};

const MAX_SAMPLES: usize = 128;

#[derive(Deserialize)]
struct JsonLatencyEvent<'a> {
    #[serde(borrow)]
    route: Option<Cow<'a, str>>,
    latency: Option<u64>,
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
    traces: VecDeque<String>,
    latencies: HashMap<String, RouteStats>,
    errors: VecDeque<String>,
}

impl App {
    fn new() -> Self {
        Self {
            traces: VecDeque::with_capacity(1000),
            latencies: HashMap::new(),
            errors: VecDeque::with_capacity(500),
        }
    }

    fn add_trace(&mut self, line: String) {
        if self.traces.len() == 1000 {
            self.traces.pop_front();
        }
        self.traces.push_back(line);
    }

    fn add_latency(&mut self, route: &str, latency: u64) {
        if let Some(stats) = self.latencies.get_mut(route) {
            stats.add(latency);
        } else {
            let mut stats = RouteStats::with_capacity(MAX_SAMPLES);
            stats.add(latency);
            self.latencies.insert(route.to_owned(), stats);
        }
    }

    fn add_error(&mut self, error: String) {
        if self.errors.len() == 500 {
            self.errors.pop_front();
        }
        self.errors.push_back(error);
    }

    fn parse_log_line(&mut self, line: String) {
        self.add_trace(line.clone());

        if line.contains("ERROR") || line.contains("WARN") {
            self.add_error(line.clone());
        }

        if likely_json_object(&line) {
            if let Ok(evt) = serde_json::from_str::<JsonLatencyEvent>(&line) {
                if let (Some(route), Some(latency)) = (evt.route.as_deref(), evt.latency) {
                    self.add_latency(route, latency);
                    return;
                }
            }
        }

        if let Some(start) = line.find("route=") {
            let rest = &line[start + 6..];
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let route = &rest[..end];

            if let Some(lat) = line.find("latency=") {
                let lat_str = &line[lat + 8..];
                let end = lat_str
                    .find(|c: char| !c.is_numeric())
                    .unwrap_or(lat_str.len());
                if let Ok(latency) = lat_str[..end].parse() {
                    self.add_latency(route, latency);
                }
            }
        }
    }

    fn get_avg_latencies(&self) -> Vec<(&str, u64)> {
        let mut result: Vec<_> = self
            .latencies
            .iter()
            .map(|(route, stats)| (route.as_str(), stats.average()))
            .collect();
        result.sort_by_key(|&(_, avg)| std::cmp::Reverse(avg));
        result.truncate(10);
        result
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let stdin = io::stdin();
        let reader = stdin.lock();
        for line in reader.lines() {
            if let Ok(line) = line {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }
    });

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();

    loop {
        while let Ok(line) = rx.try_recv() {
            app.parse_log_line(line);
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
            let trace_capacity = max_trace_lines.min(app.traces.len());
            let mut trace_items: Vec<ListItem> = Vec::with_capacity(trace_capacity);
            for t in app.traces.iter().rev().take(trace_capacity) {
                trace_items.push(ListItem::new(t.as_str()));
            }

            let traces_widget = List::new(trace_items)
                .block(Block::default().borders(Borders::ALL).title("Traces"));
            f.render_widget(traces_widget, chunks[0]);

            let bar_data = app.get_avg_latencies();

            let barchart = BarChart::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("API Route Latencies (avg ms)"),
                )
                .data(&bar_data)
                .bar_width(9)
                .bar_style(Style::default().fg(Color::Cyan))
                .value_style(Style::default().fg(Color::White));
            f.render_widget(barchart, chunks[1]);

            let max_error_lines = chunks[2].height.saturating_sub(2) as usize;
            let error_capacity = max_error_lines.min(app.errors.len());
            let mut error_items: Vec<ListItem> = Vec::with_capacity(error_capacity);
            for e in app.errors.iter().rev().take(error_capacity) {
                let style = if e.contains("ERROR") {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                error_items.push(ListItem::new(Span::styled(e.as_str(), style)));
            }

            let errors_widget = List::new(error_items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Errors / Warnings"),
            );
            f.render_widget(errors_widget, chunks[2]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q') {
                    break;
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}
