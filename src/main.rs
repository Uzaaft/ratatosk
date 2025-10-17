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
    text::{Line, Span},
    widgets::{BarChart, Block, Borders, List, ListItem},
};
use std::{
    collections::{HashMap, VecDeque},
    io::{self, BufRead},
    sync::mpsc,
    thread,
    time::Duration,
};

struct App {
    traces: VecDeque<String>,
    latencies: HashMap<String, Vec<u64>>,
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

    fn add_latency(&mut self, route: String, latency: u64) {
        self.latencies
            .entry(route)
            .or_insert_with(Vec::new)
            .push(latency);
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

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
            if let (Some(route), Some(latency)) = (
                json.get("route").and_then(|r| r.as_str()),
                json.get("latency").and_then(|l| l.as_u64()),
            ) {
                self.add_latency(route.to_string(), latency);
                return;
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
                    self.add_latency(route.to_string(), latency);
                }
            }
        }
    }

    fn get_avg_latencies(&self) -> Vec<(&str, u64)> {
        let mut result: Vec<_> = self
            .latencies
            .iter()
            .map(|(route, lats)| {
                let avg = lats.iter().sum::<u64>() / lats.len() as u64;
                (route.as_str(), avg)
            })
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

            let trace_items: Vec<ListItem> = app
                .traces
                .iter()
                .rev()
                .take(chunks[0].height as usize - 2)
                .map(|t| ListItem::new(Line::from(t.clone())))
                .collect();

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

            let error_items: Vec<ListItem> = app
                .errors
                .iter()
                .rev()
                .take(chunks[2].height as usize - 2)
                .map(|e| {
                    let color = if e.contains("ERROR") {
                        Color::Red
                    } else {
                        Color::Yellow
                    };
                    ListItem::new(Line::from(Span::styled(
                        e.clone(),
                        Style::default().fg(color),
                    )))
                })
                .collect();

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
