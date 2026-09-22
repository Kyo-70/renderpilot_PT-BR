use renderpilot_domain::PathRef;

mod config;
mod document;
mod reconcile;
mod removal;
mod strategies;

fn ini_path() -> PathRef {
    PathRef::new("C:/Game/ReShade.ini").expect("path")
}
