//! Prints how many recent Claude Code sessions Peekaboo can see (counts only, no content).
use peekaboo_core::{model::now_secs, sessions_index::*};

fn main() {
    let mut idx = SessionIndex::default();
    let t = std::time::Instant::now();
    let s = idx.scan(&claude_projects_dir(), now_secs());
    let cold = t.elapsed();
    let t = std::time::Instant::now();
    idx.scan(&claude_projects_dir(), now_secs());
    println!("sessions={} titled={} with_context={} cold={cold:?} warm={:?}",
        s.len(), s.iter().filter(|x| x.title != "Untitled session").count(),
        s.iter().filter(|x| x.context_tokens.is_some()).count(), t.elapsed());
}
