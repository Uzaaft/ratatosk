use serde::Deserialize;
use std::{env, fs};

#[derive(Debug, Clone)]
pub enum PathSeg {
    Key(String),
}

#[derive(Debug, Clone)]
pub struct FieldPath {
    pub segments: Vec<PathSeg>,
}

impl FieldPath {
    pub fn parse(s: &str) -> Self {
        let segments = s
            .split('.')
            .map(|part| PathSeg::Key(part.to_string()))
            .collect();
        Self { segments }
    }

    pub fn get<'a>(&self, v: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        let mut cur = v;
        for seg in &self.segments {
            match seg {
                PathSeg::Key(k) => {
                    cur = cur.get(k)?;
                }
            }
        }
        Some(cur)
    }
}

#[derive(Debug, Clone)]
pub struct FieldConfig {
    pub route_path: FieldPath,
    pub latency_path: FieldPath,
    pub latency_scale: u64,
    pub route_field_raw: String,
    pub latency_field_raw: String,
}

#[derive(Deserialize)]
struct FileConfig {
    route_field: Option<String>,
    latency_field: Option<String>,
    latency_scale: Option<u64>,
}

impl FieldConfig {
    pub fn extract_route(&self, v: &serde_json::Value) -> Option<String> {
        self.route_path.get(v).and_then(|x| match x {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => n.as_i64().map(|n| n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            _ => None,
        })
    }

    pub fn extract_latency(&self, v: &serde_json::Value) -> Option<u64> {
        self.latency_path.get(v).and_then(|x| match x {
            serde_json::Value::Number(n) => n.as_u64(),
            serde_json::Value::String(s) => s.parse::<u64>().ok(),
            _ => None,
        })
        .map(|lat| lat / self.latency_scale.max(1))
    }
}

pub fn load() -> FieldConfig {
    let args: Vec<String> = env::args().collect();
    
    let mut route_field: Option<String> = None;
    let mut latency_field: Option<String> = None;
    let mut latency_scale: Option<u64> = None;
    let mut config_file: Option<String> = None;

    for arg in args.iter().skip(1) {
        if let Some(value) = arg.strip_prefix("--route-field=") {
            route_field = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--latency-field=") {
            latency_field = Some(value.to_string());
        } else if let Some(value) = arg.strip_prefix("--latency-scale=") {
            latency_scale = value.parse().ok();
        } else if let Some(value) = arg.strip_prefix("--config=") {
            config_file = Some(value.to_string());
        }
    }

    if let Some(path) = config_file {
        if let Ok(content) = fs::read_to_string(&path) {
            if let Ok(file_cfg) = serde_json::from_str::<FileConfig>(&content) {
                route_field = route_field.or(file_cfg.route_field);
                latency_field = latency_field.or(file_cfg.latency_field);
                latency_scale = latency_scale.or(file_cfg.latency_scale);
            }
        }
    }

    route_field = route_field
        .or_else(|| env::var("RAT_ROUTE_FIELD").ok());
    latency_field = latency_field
        .or_else(|| env::var("RAT_LATENCY_FIELD").ok());
    latency_scale = latency_scale
        .or_else(|| env::var("RAT_LATENCY_SCALE").ok().and_then(|s| s.parse().ok()));

    let route_field_raw = route_field.unwrap_or_else(|| "route".to_string());
    let latency_field_raw = latency_field.unwrap_or_else(|| "latency".to_string());
    let latency_scale = latency_scale.unwrap_or(1);

    FieldConfig {
        route_path: FieldPath::parse(&route_field_raw),
        latency_path: FieldPath::parse(&latency_field_raw),
        latency_scale,
        route_field_raw,
        latency_field_raw,
    }
}
